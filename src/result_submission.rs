use ethers::prelude::*;
use ethers::types::{Address, H256, U256};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::{RwLock, mpsc, Semaphore};
use tokio::time::{sleep, Duration};
use anyhow::{Result, anyhow};
use serde::{Serialize, Deserialize};
use tracing::{info, warn, error, debug};
use flate2::Compression;
use flate2::write::GzEncoder;
use std::io::Write;
use sha2::Digest;

use crate::contracts::Web3Client;
use crate::job_processor::{JobResult, NodeConfig};

#[derive(Debug, Clone)]
pub enum SubmissionError {
    JobNotClaimedByNode,
    JobAlreadyCompleted,
    InvalidResult,
    StorageError(String),
    ContractError(String),
    Other(String),
}

impl std::fmt::Display for SubmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubmissionError::JobNotClaimedByNode => write!(f, "Job not claimed by node"),
            SubmissionError::JobAlreadyCompleted => write!(f, "Job already completed"),
            SubmissionError::InvalidResult => write!(f, "Invalid result"),
            SubmissionError::StorageError(e) => write!(f, "Storage error: {}", e),
            SubmissionError::ContractError(e) => write!(f, "Contract error: {}", e),
            SubmissionError::Other(e) => write!(f, "Other error: {}", e),
        }
    }
}

impl std::error::Error for SubmissionError {}

impl From<anyhow::Error> for SubmissionError {
    fn from(err: anyhow::Error) -> Self {
        SubmissionError::Other(err.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceResult {
    pub job_id: H256,
    pub model_id: String,
    pub output: String,
    pub tokens_used: u32,
    pub inference_time_ms: u64,
    pub timestamp: U256,
    pub metadata: serde_json::Value,
}

impl Default for InferenceResult {
    fn default() -> Self {
        Self {
            job_id: H256::zero(),
            model_id: String::new(),
            output: String::new(),
            tokens_used: 0,
            inference_time_ms: 0,
            timestamp: U256::zero(),
            metadata: serde_json::Value::Null,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SubmissionConfig {
    pub node_address: Address,
    pub max_result_size: usize,
    pub enable_compression: bool,
    pub compression_threshold: usize,
    pub batch_submission_size: usize,
    pub submission_retry_attempts: usize,
    pub submission_retry_delay: Duration,
    pub include_hardware_info: bool,
    pub result_expiry_time: Duration,
    pub max_concurrent_submissions: usize,
}

impl From<NodeConfig> for SubmissionConfig {
    fn from(config: NodeConfig) -> Self {
        Self {
            node_address: config.node_address,
            max_result_size: 10_000_000, // 10MB
            enable_compression: true,
            compression_threshold: 1000,
            batch_submission_size: 5,
            submission_retry_attempts: 3,
            submission_retry_delay: Duration::from_millis(100),
            include_hardware_info: false,
            result_expiry_time: Duration::from_secs(3600),
            max_concurrent_submissions: 10,
        }
    }
}

// Storage trait for IPFS/S5
#[async_trait::async_trait]
pub trait StorageClient: Send + Sync {
    async fn store(&self, data: Vec<u8>) -> Result<String, String>;
    async fn retrieve(&self, cid: &str) -> Result<Vec<u8>, String>;
}

// Contract interface trait
#[async_trait::async_trait]
pub trait JobMarketplaceTrait: Send + Sync {
    async fn is_job_claimed_by(&self, job_id: H256, node: Address) -> bool;
    async fn is_job_completed(&self, job_id: H256) -> bool;
    async fn submit_result(&self, job_id: H256, result: JobResult, node: Address) -> Result<H256, SubmissionError>;
}

#[derive(Clone)]
pub struct ResultSubmitter {
    config: SubmissionConfig,
    marketplace: Arc<dyn JobMarketplaceTrait>,
    storage: Arc<dyn StorageClient>,
    submission_semaphore: Arc<Semaphore>,
}

impl ResultSubmitter {
    pub fn new<C: Into<SubmissionConfig>>(
        config: C,
        marketplace: Arc<dyn JobMarketplaceTrait>,
        storage: Arc<dyn StorageClient>,
    ) -> Self {
        let config = config.into();
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_submissions));
        
        Self {
            config,
            marketplace,
            storage,
            submission_semaphore: semaphore,
        }
    }

    pub async fn submit_result(&self, result: InferenceResult) -> Result<H256, SubmissionError> {
        // Validate result
        self.validate_result(&result)?;

        // Check if job is claimed by this node
        if !self.marketplace.is_job_claimed_by(result.job_id, self.config.node_address).await {
            return Err(SubmissionError::JobNotClaimedByNode);
        }

        // Check if already completed
        if self.marketplace.is_job_completed(result.job_id).await {
            return Err(SubmissionError::JobAlreadyCompleted);
        }

        // Process and store the output
        let output_cid = self.store_output(&result).await?;

        // Store metadata if present
        let metadata_cid = if !result.metadata.is_null() {
            Some(self.store_metadata(&result).await?)
        } else {
            None
        };

        // Create job result
        let job_result = JobResult {
            job_id: result.job_id,
            output: result.output.clone(),
            tokens_used: result.tokens_used,
            inference_time_ms: result.inference_time_ms,
            output_cid,
            proof_cid: None,
            metadata_cid,
        };

        // Submit to blockchain
        self.marketplace.submit_result(result.job_id, job_result, self.config.node_address).await
    }

    pub async fn submit_result_with_proof(&self, result: InferenceResult, proof: ProofData) -> Result<H256, SubmissionError> {
        // Store proof
        let proof_bytes = bincode::serialize(&proof)
            .map_err(|e| SubmissionError::Other(e.to_string()))?;
        let proof_cid = self.storage.store(proof_bytes).await
            .map_err(|e| SubmissionError::StorageError(e))?;

        // Submit result with proof CID
        let mut job_result = self.prepare_result(&result).await?;
        job_result.proof_cid = Some(proof_cid);

        self.marketplace.submit_result(result.job_id, job_result, self.config.node_address).await
    }

    pub async fn submit_batch(&self, results: Vec<InferenceResult>) -> Vec<Result<H256, SubmissionError>> {
        let mut handles = Vec::new();

        for result in results {
            let submitter = self.clone();
            let permit = self.submission_semaphore.clone().acquire_owned().await.unwrap();
            
            let handle = tokio::spawn(async move {
                let res = submitter.submit_result(result).await;
                drop(permit);
                res
            });
            
            handles.push(handle);
        }

        let mut submission_results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(res) => submission_results.push(res),
                Err(e) => submission_results.push(Err(SubmissionError::Other(e.to_string()))),
            }
        }

        submission_results
    }

    pub async fn submit_with_retry(&self, result: InferenceResult) -> Result<H256, SubmissionError> {
        let mut attempts = 0;
        let mut last_error = None;

        while attempts < self.config.submission_retry_attempts {
            match self.submit_result(result.clone()).await {
                Ok(tx_hash) => return Ok(tx_hash),
                Err(e) => {
                    last_error = Some(e.clone());
                    
                    // Don't retry on certain errors
                    match &e {
                        SubmissionError::JobNotClaimedByNode |
                        SubmissionError::JobAlreadyCompleted |
                        SubmissionError::InvalidResult => return Err(e),
                        _ => {}
                    }

                    attempts += 1;
                    if attempts < self.config.submission_retry_attempts {
                        sleep(self.config.submission_retry_delay).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or(SubmissionError::Other("Unknown error".to_string())))
    }

    async fn store_output(&self, result: &InferenceResult) -> Result<String, SubmissionError> {
        let mut data = result.output.as_bytes().to_vec();

        // Compress if enabled and above threshold
        if self.config.enable_compression && data.len() > self.config.compression_threshold {
            data = self.compress_data(&data)?;
        }

        self.storage.store(data).await
            .map_err(|e| SubmissionError::StorageError(e))
    }

    async fn store_metadata(&self, result: &InferenceResult) -> Result<String, SubmissionError> {
        let metadata_bytes = serde_json::to_vec(&result.metadata)
            .map_err(|e| SubmissionError::Other(e.to_string()))?;
        
        self.storage.store(metadata_bytes).await
            .map_err(|e| SubmissionError::StorageError(e))
    }

    fn compress_data(&self, data: &[u8]) -> Result<Vec<u8>, SubmissionError> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data)
            .map_err(|e| SubmissionError::Other(e.to_string()))?;
        encoder.finish()
            .map_err(|e| SubmissionError::Other(e.to_string()))
    }

    fn validate_result(&self, result: &InferenceResult) -> Result<(), SubmissionError> {
        // Check empty output
        if result.output.is_empty() {
            return Err(SubmissionError::InvalidResult);
        }

        // Check zero tokens
        if result.tokens_used == 0 {
            return Err(SubmissionError::InvalidResult);
        }

        // Check inference time
        if result.inference_time_ms == 0 {
            return Err(SubmissionError::InvalidResult);
        }

        // Check result size
        if result.output.len() > self.config.max_result_size {
            return Err(SubmissionError::InvalidResult);
        }

        Ok(())
    }

    async fn prepare_result(&self, result: &InferenceResult) -> Result<JobResult, SubmissionError> {
        let output_cid = self.store_output(result).await?;
        let metadata_cid = if !result.metadata.is_null() {
            Some(self.store_metadata(result).await?)
        } else {
            None
        };

        Ok(JobResult {
            job_id: result.job_id,
            output: result.output.clone(),
            tokens_used: result.tokens_used,
            inference_time_ms: result.inference_time_ms,
            output_cid,
            proof_cid: None,
            metadata_cid,
        })
    }

    pub async fn would_result_expire(&self, result: &InferenceResult) -> bool {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let result_age = current_time - result.timestamp.as_u64();
        result_age > self.config.result_expiry_time.as_secs()
    }
}

// Proof data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofData {
    pub job_id: H256,
    pub model_hash: H256,
    pub input_hash: H256,
    pub output_hash: H256,
    pub computation_trace: Vec<u8>,
}

// Proof generator
pub struct ProofGenerator;

impl ProofGenerator {
    pub async fn generate_inference_proof(result: &InferenceResult) -> Result<ProofData> {
        // In a real implementation, this would generate a verifiable proof
        // For now, return mock proof
        Ok(ProofData {
            job_id: result.job_id,
            model_hash: H256::random(),
            input_hash: H256::random(),
            output_hash: H256::from_slice(&sha2::Sha256::digest(result.output.as_bytes())[..]),
            computation_trace: vec![0u8; 32],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockJobMarketplace {
        claimed_jobs: Arc<RwLock<HashMap<H256, Address>>>,
        completed_jobs: Arc<RwLock<Vec<H256>>>,
        results: Arc<RwLock<Vec<(H256, JobResult)>>>,
    }

    #[async_trait::async_trait]
    impl JobMarketplaceTrait for MockJobMarketplace {
        async fn is_job_claimed_by(&self, job_id: H256, node: Address) -> bool {
            self.claimed_jobs.read().await
                .get(&job_id)
                .map(|addr| *addr == node)
                .unwrap_or(false)
        }

        async fn is_job_completed(&self, job_id: H256) -> bool {
            self.completed_jobs.read().await.contains(&job_id)
        }

        async fn submit_result(&self, job_id: H256, result: JobResult, _node: Address) -> Result<H256, SubmissionError> {
            if self.completed_jobs.read().await.contains(&job_id) {
                return Err(SubmissionError::JobAlreadyCompleted);
            }
            
            self.results.write().await.push((job_id, result));
            self.completed_jobs.write().await.push(job_id);
            Ok(H256::random())
        }
    }

    struct MockStorageClient {
        stored_data: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    }

    #[async_trait::async_trait]
    impl StorageClient for MockStorageClient {
        async fn store(&self, data: Vec<u8>) -> Result<String, String> {
            let cid = format!("Qm{}", hex::encode(&data[..data.len().min(16)]));
            self.stored_data.write().await.insert(cid.clone(), data);
            Ok(cid)
        }

        async fn retrieve(&self, cid: &str) -> Result<Vec<u8>, String> {
            self.stored_data.read().await
                .get(cid)
                .cloned()
                .ok_or_else(|| "CID not found".to_string())
        }
    }
}