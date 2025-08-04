// tests/test_result_submission.rs

use ethers::prelude::*;
use ethers::types::{Address, H256, U256};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};

use fabstir_llm_node::{
    ResultSubmitter,
    JobResult,
    InferenceResult,
    SubmissionError,
    JobNodeConfig,
    SubmissionConfig,
    ProofGenerator,
    StorageClient,
    SubmissionMarketplaceTrait,
};

#[derive(Debug, Clone)]
struct MockJobMarketplace {
    results: Arc<RwLock<Vec<(H256, JobResult)>>>,
    claimed_jobs: Arc<RwLock<Vec<(H256, Address)>>>,
    completed_jobs: Arc<RwLock<Vec<H256>>>,
}

impl MockJobMarketplace {
    fn new() -> Self {
        Self {
            results: Arc::new(RwLock::new(Vec::new())),
            claimed_jobs: Arc::new(RwLock::new(Vec::new())),
            completed_jobs: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    async fn is_job_claimed_by(&self, job_id: H256, node: Address) -> bool {
        self.claimed_jobs.read().await
            .iter()
            .any(|(id, addr)| *id == job_id && *addr == node)
    }
    
    async fn submit_result(
        &self, 
        job_id: H256, 
        result: JobResult,
        node: Address
    ) -> Result<H256, SubmissionError> {
        // Verify job is claimed by this node
        if !self.is_job_claimed_by(job_id, node).await {
            return Err(SubmissionError::JobNotClaimedByNode);
        }
        
        // Verify not already completed
        if self.completed_jobs.read().await.contains(&job_id) {
            return Err(SubmissionError::JobAlreadyCompleted);
        }
        
        // Store result
        self.results.write().await.push((job_id, result));
        self.completed_jobs.write().await.push(job_id);
        
        // Return transaction hash
        Ok(H256::random())
    }
    
    async fn is_job_completed(&self, job_id: H256) -> bool {
        self.completed_jobs.read().await.contains(&job_id)
    }
}

#[async_trait::async_trait]
impl SubmissionMarketplaceTrait for MockJobMarketplace {
    async fn is_job_claimed_by(&self, job_id: H256, node: Address) -> bool {
        self.is_job_claimed_by(job_id, node).await
    }
    
    async fn is_job_completed(&self, job_id: H256) -> bool {
        self.is_job_completed(job_id).await
    }
    
    async fn submit_result(&self, job_id: H256, result: JobResult, node: Address) -> Result<H256, SubmissionError> {
        self.submit_result(job_id, result, node).await
    }
}

#[derive(Clone)]
struct MockStorageClient {
    stored_data: Arc<RwLock<Vec<(String, Vec<u8>)>>>,
}

impl MockStorageClient {
    fn new() -> Self {
        Self {
            stored_data: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

#[async_trait::async_trait]
impl StorageClient for MockStorageClient {
    async fn store(&self, data: Vec<u8>) -> Result<String, String> {
        let cid = format!("Qm{}", hex::encode(&data[..data.len().min(16)]));
        self.stored_data.write().await.push((cid.clone(), data));
        Ok(cid)
    }
    
    async fn retrieve(&self, cid: &str) -> Result<Vec<u8>, String> {
        self.stored_data.read().await
            .iter()
            .find(|(stored_cid, _)| stored_cid == cid)
            .map(|(_, data)| data.clone())
            .ok_or_else(|| "CID not found".to_string())
    }
}

#[tokio::test]
async fn test_result_submission_success() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let storage = Arc::new(MockStorageClient::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    // Setup claimed job
    marketplace.claimed_jobs.write().await.push((job_id, node_address));
    
    let config = JobNodeConfig {
        node_address,
        ..Default::default()
    };
    
    let submitter = ResultSubmitter::new(
        config,
        marketplace.clone(),
        storage.clone(),
    );
    
    // Create inference result
    let inference_result = InferenceResult {
        job_id,
        model_id: "llama3-70b".to_string(),
        output: "The capital of France is Paris.".to_string(),
        tokens_used: 15,
        inference_time_ms: 250,
        timestamp: U256::from(1234567890),
        metadata: serde_json::Value::Null,
    };
    
    // Submit result
    let tx_hash = submitter.submit_result(inference_result).await.unwrap();
    
    // Verify submission
    assert!(marketplace.completed_jobs.read().await.contains(&job_id));
    assert_eq!(marketplace.results.read().await.len(), 1);
    
    let (stored_job_id, result) = &marketplace.results.read().await[0];
    assert_eq!(*stored_job_id, job_id);
    assert!(result.output_cid.starts_with("Qm"));
}

#[tokio::test]
async fn test_result_submission_not_claimed() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let storage = Arc::new(MockStorageClient::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    // Job not claimed by this node
    let other_node = Address::random();
    marketplace.claimed_jobs.write().await.push((job_id, other_node));
    
    let config = JobNodeConfig {
        node_address,
        ..Default::default()
    };
    
    let submitter = ResultSubmitter::new(
        config,
        marketplace.clone(),
        storage,
    );
    
    let inference_result = InferenceResult {
        job_id,
        output: "Test output".to_string(),
        tokens_used: 10,
        inference_time_ms: 100,
        ..Default::default()
    };
    
    // Should fail
    let result = submitter.submit_result(inference_result).await;
    assert!(matches!(result, Err(SubmissionError::JobNotClaimedByNode)));
}

#[tokio::test]
async fn test_large_result_storage() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let storage = Arc::new(MockStorageClient::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    marketplace.claimed_jobs.write().await.push((job_id, node_address));
    
    let config = SubmissionConfig {
        node_address,
        max_result_size: 1_000_000, // 1MB limit
        enable_compression: false, // Disable compression for this test
        compression_threshold: 1000,
        batch_submission_size: 5,
        submission_retry_attempts: 3,
        submission_retry_delay: Duration::from_millis(100),
        include_hardware_info: false,
        result_expiry_time: Duration::from_secs(3600),
        max_concurrent_submissions: 10,
    };
    
    let submitter = ResultSubmitter::new(
        config,
        marketplace.clone(),
        storage.clone(),
    );
    
    // Create large output
    let large_output = "x".repeat(500_000); // 500KB
    
    let inference_result = InferenceResult {
        job_id,
        output: large_output.clone(),
        tokens_used: 100_000,
        inference_time_ms: 5000,
        ..Default::default()
    };
    
    // Should succeed - under limit
    let tx_hash = submitter.submit_result(inference_result).await.unwrap();
    
    // Verify stored correctly
    let results = marketplace.results.read().await;
    let (_, result) = &results[0];
    
    // Retrieve from storage
    let stored_data = storage.retrieve(&result.output_cid).await.unwrap();
    let retrieved_output = String::from_utf8(stored_data).unwrap();
    assert_eq!(retrieved_output, large_output);
}

#[tokio::test]
async fn test_result_compression() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let storage = Arc::new(MockStorageClient::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    marketplace.claimed_jobs.write().await.push((job_id, node_address));
    
    let config = JobNodeConfig {
        node_address,
        enable_compression: true,
        compression_threshold: 1000, // Compress if > 1KB
        ..Default::default()
    };
    
    let submitter = ResultSubmitter::new(
        config,
        marketplace.clone(),
        storage.clone(),
    );
    
    // Create compressible output (repeated pattern)
    let output = "Hello World! ".repeat(200); // ~2.6KB
    
    let inference_result = InferenceResult {
        job_id,
        output: output.clone(),
        tokens_used: 10,
        inference_time_ms: 100,
        ..Default::default()
    };
    
    // Submit with compression
    submitter.submit_result(inference_result).await.unwrap();
    
    // Verify compression was applied
    let storage_data = storage.stored_data.read().await;
    let (_, compressed_data) = &storage_data[0];
    
    // Compressed size should be much smaller
    assert!(compressed_data.len() < output.len() / 2);
}

#[tokio::test]
async fn test_result_with_proof() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let storage = Arc::new(MockStorageClient::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    marketplace.claimed_jobs.write().await.push((job_id, node_address));
    
    let config = JobNodeConfig {
        node_address,
        enable_proofs: true,
        ..Default::default()
    };
    
    let submitter = ResultSubmitter::new(
        config,
        marketplace.clone(),
        storage,
    );
    
    let inference_result = InferenceResult {
        job_id,
        model_id: "llama3-70b".to_string(),
        output: "Test output".to_string(),
        tokens_used: 10,
        inference_time_ms: 100,
        ..Default::default()
    };
    
    // Generate proof
    let proof = ProofGenerator::generate_inference_proof(&inference_result).await.unwrap();
    
    // Submit with proof
    let tx_hash = submitter.submit_result_with_proof(inference_result, proof).await.unwrap();
    
    // Verify proof was included
    let results = marketplace.results.read().await;
    let (_, result) = &results[0];
    assert!(result.proof_cid.is_some());
}

#[tokio::test]
async fn test_batch_result_submission() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let storage = Arc::new(MockStorageClient::new());
    let node_address = Address::random();
    
    // Setup multiple claimed jobs
    let job_ids: Vec<H256> = (0..5).map(|_| H256::random()).collect();
    for job_id in &job_ids {
        marketplace.claimed_jobs.write().await.push((*job_id, node_address));
    }
    
    let config = JobNodeConfig {
        node_address,
        batch_submission_size: 3,
        ..Default::default()
    };
    
    let submitter = ResultSubmitter::new(
        config,
        marketplace.clone(),
        storage,
    );
    
    // Create results for all jobs
    let results: Vec<InferenceResult> = job_ids.iter()
        .enumerate()
        .map(|(i, job_id)| InferenceResult {
            job_id: *job_id,
            output: format!("Result {}", i),
            tokens_used: 10 * (i as u32 + 1),
            inference_time_ms: 1000 + (i as u64 * 100),
            ..Default::default()
        })
        .collect();
    
    // Submit batch
    let tx_hashes = submitter.submit_batch(results).await;
    
    // All should succeed
    assert_eq!(tx_hashes.len(), 5);
    assert!(tx_hashes.iter().all(|r| r.is_ok()));
    
    // Verify all completed
    let completed = marketplace.completed_jobs.read().await;
    assert_eq!(completed.len(), 5);
}

#[tokio::test]
async fn test_result_submission_retry() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let storage = Arc::new(MockStorageClient::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    marketplace.claimed_jobs.write().await.push((job_id, node_address));
    
    let config = JobNodeConfig {
        node_address,
        submission_retry_attempts: 3,
        submission_retry_delay: Duration::from_millis(100),
        ..Default::default()
    };
    
    let submitter = ResultSubmitter::new(
        config,
        marketplace.clone(),
        storage,
    );
    
    let inference_result = InferenceResult {
        job_id,
        output: "Test output".to_string(),
        tokens_used: 10,
        inference_time_ms: 100,
        ..Default::default()
    };
    
    // Submit with retry logic
    let result = submitter.submit_with_retry(inference_result).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_result_validation() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let storage = Arc::new(MockStorageClient::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    marketplace.claimed_jobs.write().await.push((job_id, node_address));
    
    let config = JobNodeConfig {
        node_address,
        ..Default::default()
    };
    
    let submitter = ResultSubmitter::new(
        config,
        marketplace.clone(),
        storage,
    );
    
    // Test various invalid results
    
    // Empty output
    let invalid_result1 = InferenceResult {
        job_id,
        output: "".to_string(),
        ..Default::default()
    };
    assert!(submitter.submit_result(invalid_result1).await.is_err());
    
    // Zero tokens used
    let invalid_result2 = InferenceResult {
        job_id,
        output: "Valid output".to_string(),
        tokens_used: 0,
        ..Default::default()
    };
    assert!(submitter.submit_result(invalid_result2).await.is_err());
    
    // Invalid inference time
    let invalid_result3 = InferenceResult {
        job_id,
        output: "Valid output".to_string(),
        tokens_used: 10,
        inference_time_ms: 0,
        ..Default::default()
    };
    assert!(submitter.submit_result(invalid_result3).await.is_err());
}

#[tokio::test]
async fn test_result_metadata() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let storage = Arc::new(MockStorageClient::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    marketplace.claimed_jobs.write().await.push((job_id, node_address));
    
    let config = JobNodeConfig {
        node_address,
        include_hardware_info: true,
        ..Default::default()
    };
    
    let submitter = ResultSubmitter::new(
        config,
        marketplace.clone(),
        storage,
    );
    
    let mut inference_result = InferenceResult {
        job_id,
        output: "Test output".to_string(),
        tokens_used: 10,
        inference_time_ms: 100,
        ..Default::default()
    };
    
    // Add metadata
    inference_result.metadata = serde_json::json!({
        "gpu": "NVIDIA RTX 4090",
        "cuda_version": "12.1",
        "driver_version": "535.104.05",
        "temperature_c": 65,
        "power_draw_w": 350,
        "memory_used_gb": 12.5,
        "quantization": "Q4_K_M"
    });
    
    // Submit with metadata
    submitter.submit_result(inference_result).await.unwrap();
    
    // Verify metadata was stored
    let results = marketplace.results.read().await;
    let (_, result) = &results[0];
    assert!(result.metadata_cid.is_some());
}

#[tokio::test]
async fn test_concurrent_submissions() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let storage = Arc::new(MockStorageClient::new());
    let node_address = Address::random();
    
    // Setup multiple jobs
    let job_count = 10;
    let job_ids: Vec<H256> = (0..job_count).map(|_| H256::random()).collect();
    
    for job_id in &job_ids {
        marketplace.claimed_jobs.write().await.push((*job_id, node_address));
    }
    
    let config = JobNodeConfig {
        node_address,
        max_concurrent_submissions: 5,
        ..Default::default()
    };
    
    // Submit all results concurrently
    let mut handles = vec![];
    for (i, job_id) in job_ids.iter().enumerate() {
        let submitter = ResultSubmitter::new(
            config.clone(),
            marketplace.clone(),
            storage.clone(),
        );
        
        let result = InferenceResult {
            job_id: *job_id,
            output: format!("Output {}", i),
            tokens_used: 10,
            inference_time_ms: 100,
            ..Default::default()
        };
        
        let handle = tokio::spawn(async move {
            submitter.submit_result(result).await
        });
        handles.push(handle);
    }
    
    // Wait for all
    let results: Vec<_> = futures::future::join_all(handles).await;
    
    // All should succeed
    assert!(results.iter().all(|r| r.as_ref().unwrap().is_ok()));
    
    // Verify all completed
    let completed = marketplace.completed_jobs.read().await;
    assert_eq!(completed.len(), job_count);
}

#[tokio::test]
async fn test_result_expiry_handling() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let storage = Arc::new(MockStorageClient::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    marketplace.claimed_jobs.write().await.push((job_id, node_address));
    
    let config = JobNodeConfig {
        node_address,
        result_expiry_time: Duration::from_secs(3600), // 1 hour
        ..Default::default()
    };
    
    let submitter = ResultSubmitter::new(
        config,
        marketplace.clone(),
        storage,
    );
    
    let inference_result = InferenceResult {
        job_id,
        output: "Test output".to_string(),
        tokens_used: 10,
        inference_time_ms: 100,
        timestamp: U256::from(1234567890),
        ..Default::default()
    };
    
    // Check if result would be expired
    let is_expired = submitter.would_result_expire(&inference_result).await;
    
    // Submit if not expired
    if !is_expired {
        let result = submitter.submit_result(inference_result).await;
        assert!(result.is_ok());
    }
}