use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};
use thiserror::Error;
use serde::{Serialize, Deserialize};
use chrono::Utc;
use futures::stream::{Stream, StreamExt};

use crate::ezkl::{InferenceData, ProofFormat, CompressionLevel};

#[derive(Debug, Clone, PartialEq)]
pub enum BatchStrategy {
    Sequential,
    Parallel { max_concurrent: usize },
    Streaming { chunk_size: usize },
    Adaptive {
        target_latency_ms: u64,
        min_batch_size: usize,
        max_batch_size: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AggregationMethod {
    None,
    Recursive { depth: usize },
    Tree,
    Linear,
}

#[derive(Debug, Clone)]
pub struct ParallelismConfig {
    pub max_parallel_proofs: usize,
    pub worker_threads: usize,
    pub memory_limit_mb: usize,
    pub use_gpu: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BatchProofStatus {
    Pending,
    InProgress,
    Completed,
    PartialSuccess,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct BatchProofRequest {
    pub inferences: Vec<InferenceData>,
    pub strategy: BatchStrategy,
    pub aggregation: AggregationMethod,
    pub proof_format: ProofFormat,
    pub compression: CompressionLevel,
    pub priority: u8,
    pub enable_deduplication: bool,
}

impl BatchProofRequest {
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_deduplication(mut self, enabled: bool) -> Self {
        self.enable_deduplication = enabled;
        self
    }
}

#[derive(Debug, Clone)]
pub struct ProofEntry {
    inference_index: usize,
    proof_data: Vec<u8>,
    success: bool,
    error_message: Option<String>,
}

impl ProofEntry {
    pub fn is_success(&self) -> bool {
        self.success
    }

    pub fn is_failure(&self) -> bool {
        !self.success
    }

    pub fn inference_index(&self) -> usize {
        self.inference_index
    }

    pub fn proof_data(&self) -> &[u8] {
        &self.proof_data
    }
}

#[derive(Debug, Clone)]
pub struct BatchError {
    pub inference_index: usize,
    pub error_message: String,
}

#[derive(Debug, Clone)]
pub struct AggregatedProof {
    pub data: Vec<u8>,
    pub num_aggregated: usize,
    pub aggregation_tree_root: String,
    pub size_reduction_factor: f32,
}

#[derive(Debug, Clone)]
pub struct AdaptiveMetrics {
    pub avg_batch_size: f32,
    pub latency_compliance_rate: f32,
    pub total_batches: usize,
}

#[derive(Debug, Clone)]
pub struct ResourceMetrics {
    pub peak_memory_mb: usize,
    pub max_concurrent_proofs: usize,
}

#[derive(Debug, Clone)]
pub struct BatchProofResult {
    pub total_count: usize,
    pub successful_count: usize,
    pub failed_count: usize,
    pub unique_count: usize,
    pub duplicate_count: usize,
    pub proofs: Vec<ProofEntry>,
    pub status: BatchProofStatus,
    pub total_time_ms: u64,
    pub errors: Vec<BatchError>,
    pub aggregation_method: Option<AggregationMethod>,
    pub aggregated_proof: Option<AggregatedProof>,
    pub parallelism_speedup: f32,
    pub adaptive_metrics: Option<AdaptiveMetrics>,
    pub resource_metrics: Option<ResourceMetrics>,
    pub recovered_from_index: usize,
    pub is_recovered: bool,
    pub completion_timestamp: u64,
}

#[derive(Debug, Clone)]
pub struct ChunkResult {
    pub chunk_index: usize,
    pub total_chunks: usize,
    pub chunk_size: usize,
    pub proofs: Vec<ProofEntry>,
}

#[derive(Debug, Clone)]
pub struct BatchStatus {
    pub status: BatchProofStatus,
    pub processed_count: usize,
}

#[derive(Error, Debug)]
pub enum BatchProofError {
    #[error("Batch processing failed: {0}")]
    ProcessingFailed(String),
    #[error("Invalid batch configuration: {0}")]
    InvalidConfig(String),
    #[error("Resource limit exceeded: {0}")]
    ResourceExceeded(String),
    #[error("Batch cancelled")]
    Cancelled,
}

pub struct BatchProofStream {
    receiver: mpsc::Receiver<ChunkResult>,
}

impl BatchProofStream {
    pub async fn next_chunk(&mut self) -> Result<Option<ChunkResult>> {
        Ok(self.receiver.recv().await)
    }
}

struct BatchState {
    id: String,
    request: BatchProofRequest,
    status: BatchProofStatus,
    processed: usize,
    results: Vec<ProofEntry>,
    start_time: u64,
}

pub struct BatchProofGenerator {
    config: ParallelismConfig,
    batches: Arc<RwLock<HashMap<String, BatchState>>>,
    proof_counter: Arc<RwLock<u64>>,
}

impl BatchProofGenerator {
    pub async fn new_mock(config: ParallelismConfig) -> Result<Self> {
        Ok(Self {
            config,
            batches: Arc::new(RwLock::new(HashMap::new())),
            proof_counter: Arc::new(RwLock::new(0)),
        })
    }

    pub async fn create_batch_proof(&self, request: BatchProofRequest) -> Result<BatchProofResult> {
        let start_time = std::time::Instant::now();
        let total_count = request.inferences.len();

        // Handle deduplication if enabled
        let (unique_inferences, unique_count, duplicate_count) = if request.enable_deduplication {
            self.deduplicate_inferences(&request.inferences)
        } else {
            (request.inferences.clone(), total_count, 0)
        };

        // Process based on strategy
        let (proofs, errors) = match &request.strategy {
            BatchStrategy::Sequential => {
                self.process_sequential(&unique_inferences, &request).await?
            }
            BatchStrategy::Parallel { max_concurrent } => {
                self.process_parallel(&unique_inferences, &request, *max_concurrent).await?
            }
            BatchStrategy::Streaming { .. } => {
                self.process_sequential(&unique_inferences, &request).await?
            }
            BatchStrategy::Adaptive { .. } => {
                self.process_adaptive(&unique_inferences, &request).await?
            }
        };

        let successful_count = proofs.iter().filter(|p| p.success).count();
        let failed_count = proofs.iter().filter(|p| !p.success).count();

        // Handle aggregation if requested
        let aggregated_proof = if request.aggregation != AggregationMethod::None {
            Some(self.aggregate_proofs(&proofs, &request.aggregation).await?)
        } else {
            None
        };

        let total_time_ms = start_time.elapsed().as_millis() as u64;

        // Calculate parallelism speedup
        let parallelism_speedup = match &request.strategy {
            BatchStrategy::Parallel { .. } => 2.5, // Mock speedup
            _ => 1.0,
        };

        // Create adaptive metrics if applicable
        let adaptive_metrics = match &request.strategy {
            BatchStrategy::Adaptive { .. } => Some(AdaptiveMetrics {
                avg_batch_size: 5.0,
                latency_compliance_rate: 0.95,
                total_batches: (unique_count + 4) / 5,
            }),
            _ => None,
        };

        // Resource metrics
        let resource_metrics = Some(ResourceMetrics {
            peak_memory_mb: std::cmp::min(self.config.memory_limit_mb, 50 * unique_count),
            max_concurrent_proofs: match &request.strategy {
                BatchStrategy::Parallel { max_concurrent } => {
                    std::cmp::min(*max_concurrent, self.config.max_parallel_proofs)
                }
                _ => 1,
            },
        });

        Ok(BatchProofResult {
            total_count,
            successful_count,
            failed_count,
            unique_count,
            duplicate_count,
            proofs,
            status: if failed_count == 0 {
                BatchProofStatus::Completed
            } else if successful_count > 0 {
                BatchProofStatus::PartialSuccess
            } else {
                BatchProofStatus::Failed
            },
            total_time_ms,
            errors,
            aggregation_method: if request.aggregation != AggregationMethod::None {
                Some(request.aggregation.clone())
            } else {
                None
            },
            aggregated_proof,
            parallelism_speedup,
            adaptive_metrics,
            resource_metrics,
            recovered_from_index: 0,
            is_recovered: false,
            completion_timestamp: Utc::now().timestamp() as u64,
        })
    }

    fn deduplicate_inferences(&self, inferences: &[InferenceData]) -> (Vec<InferenceData>, usize, usize) {
        let mut seen = std::collections::HashSet::new();
        let mut unique = Vec::new();

        for inf in inferences {
            let key = format!("{}-{}", inf.model_hash, inf.input.prompt);
            if seen.insert(key) {
                unique.push(inf.clone());
            }
        }

        let unique_count = unique.len();
        let duplicate_count = inferences.len() - unique_count;
        (unique, unique_count, duplicate_count)
    }

    async fn process_sequential(
        &self,
        inferences: &[InferenceData],
        _request: &BatchProofRequest,
    ) -> Result<(Vec<ProofEntry>, Vec<BatchError>)> {
        let mut proofs = Vec::new();
        let mut errors = Vec::new();

        for (i, inference) in inferences.iter().enumerate() {
            // Simulate proof generation
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

            let (success, proof_data, error_msg) = self.generate_single_proof(inference).await;

            proofs.push(ProofEntry {
                inference_index: i,
                proof_data,
                success,
                error_message: error_msg.clone(),
            });

            if let Some(msg) = error_msg {
                errors.push(BatchError {
                    inference_index: i,
                    error_message: msg,
                });
            }
        }

        Ok((proofs, errors))
    }

    async fn process_parallel(
        &self,
        inferences: &[InferenceData],
        _request: &BatchProofRequest,
        max_concurrent: usize,
    ) -> Result<(Vec<ProofEntry>, Vec<BatchError>)> {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));
        let mut handles = Vec::new();

        for (i, inference) in inferences.iter().enumerate() {
            let inference = inference.clone();
            let sem = semaphore.clone();

            let handle = tokio::spawn(async move {
                let _permit = sem.acquire().await.unwrap();
                tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

                let (success, proof_data, error_msg) = Self::generate_single_proof_static(&inference).await;

                (i, ProofEntry {
                    inference_index: i,
                    proof_data,
                    success,
                    error_message: error_msg,
                })
            });

            handles.push(handle);
        }

        let mut proofs = vec![ProofEntry {
            inference_index: 0,
            proof_data: vec![],
            success: false,
            error_message: None,
        }; inferences.len()];
        let mut errors = Vec::new();

        for handle in handles {
            let (index, entry) = handle.await?;
            if let Some(ref msg) = entry.error_message {
                errors.push(BatchError {
                    inference_index: index,
                    error_message: msg.clone(),
                });
            }
            proofs[index] = entry;
        }

        Ok((proofs, errors))
    }

    async fn process_adaptive(
        &self,
        inferences: &[InferenceData],
        request: &BatchProofRequest,
    ) -> Result<(Vec<ProofEntry>, Vec<BatchError>)> {
        // For mock, just use parallel processing with adaptive batch size
        let optimal_batch_size = 5;
        self.process_parallel(inferences, request, optimal_batch_size).await
    }

    async fn generate_single_proof(&self, inference: &InferenceData) -> (bool, Vec<u8>, Option<String>) {
        Self::generate_single_proof_static(inference).await
    }

    async fn generate_single_proof_static(inference: &InferenceData) -> (bool, Vec<u8>, Option<String>) {
        // Check for invalid data that would cause failure
        if inference.model_hash.is_empty() {
            return (false, vec![], Some("Invalid model hash".to_string()));
        }
        if inference.output.tokens.is_empty() {
            return (false, vec![], Some("Invalid output tokens".to_string()));
        }
        if inference.input.embeddings.is_empty() {
            return (false, vec![], Some("Invalid input embeddings".to_string()));
        }

        // Generate mock proof
        let proof_data = vec![1, 2, 3, 4, 5]; // Simplified mock
        (true, proof_data, None)
    }

    async fn aggregate_proofs(
        &self,
        proofs: &[ProofEntry],
        method: &AggregationMethod,
    ) -> Result<AggregatedProof> {
        // Filter successful proofs
        let valid_proofs: Vec<_> = proofs.iter()
            .filter(|p| p.success)
            .collect();

        // Generate aggregated proof
        let aggregated_data = vec![99; 1000]; // Mock aggregated proof
        let num_aggregated = valid_proofs.len();

        // Calculate mock tree root
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        for proof in &valid_proofs {
            hasher.update(&proof.proof_data);
        }
        let tree_root = format!("{:x}", hasher.finalize());

        Ok(AggregatedProof {
            data: aggregated_data,
            num_aggregated,
            aggregation_tree_root: tree_root,
            size_reduction_factor: num_aggregated as f32 / 10.0,
        })
    }

    pub async fn create_batch_proof_stream(&self, request: BatchProofRequest) -> Result<BatchProofStream> {
        let (tx, rx) = mpsc::channel(10);
        let chunk_size = match &request.strategy {
            BatchStrategy::Streaming { chunk_size } => *chunk_size,
            _ => 5,
        };

        let inferences = request.inferences.clone();
        tokio::spawn(async move {
            let total_chunks = (inferences.len() + chunk_size - 1) / chunk_size;

            for (chunk_idx, chunk) in inferences.chunks(chunk_size).enumerate() {
                let mut proofs = Vec::new();

                for (i, inference) in chunk.iter().enumerate() {
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    let (success, proof_data, error_msg) = Self::generate_single_proof_static(inference).await;

                    proofs.push(ProofEntry {
                        inference_index: chunk_idx * chunk_size + i,
                        proof_data,
                        success,
                        error_message: error_msg,
                    });
                }

                let chunk_result = ChunkResult {
                    chunk_index: chunk_idx,
                    total_chunks,
                    chunk_size: proofs.len(),
                    proofs,
                };

                if tx.send(chunk_result).await.is_err() {
                    break;
                }
            }
        });

        Ok(BatchProofStream { receiver: rx })
    }

    pub async fn start_batch_proof(&self, request: BatchProofRequest) -> Result<String> {
        let batch_id = format!("batch_{}", uuid::Uuid::new_v4());

        let state = BatchState {
            id: batch_id.clone(),
            request,
            status: BatchProofStatus::InProgress,
            processed: 0,
            results: Vec::new(),
            start_time: Utc::now().timestamp() as u64,
        };

        let mut batches = self.batches.write().await;
        batches.insert(batch_id.clone(), state);

        // Start processing in background
        let batch_id_clone = batch_id.clone();
        let batches_clone = self.batches.clone();
        tokio::spawn(async move {
            // Simulate processing
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

            let mut batches = batches_clone.write().await;
            if let Some(batch) = batches.get_mut(&batch_id_clone) {
                if batch.status != BatchProofStatus::Cancelled {
                    batch.status = BatchProofStatus::Completed;
                    batch.processed = batch.request.inferences.len();
                }
            }
        });

        Ok(batch_id)
    }

    pub async fn cancel_batch_proof(&self, batch_id: &str) -> Result<()> {
        let mut batches = self.batches.write().await;
        
        if let Some(batch) = batches.get_mut(batch_id) {
            batch.status = BatchProofStatus::Cancelled;
            Ok(())
        } else {
            Err(BatchProofError::ProcessingFailed("Batch not found".to_string()).into())
        }
    }

    pub async fn get_batch_status(&self, batch_id: &str) -> Result<BatchStatus> {
        let batches = self.batches.read().await;
        
        if let Some(batch) = batches.get(batch_id) {
            Ok(BatchStatus {
                status: batch.status.clone(),
                processed_count: batch.processed,
            })
        } else {
            Err(BatchProofError::ProcessingFailed("Batch not found".to_string()).into())
        }
    }

    pub async fn simulate_interruption(&self, batch_id: &str, processed: usize) -> Result<()> {
        let mut batches = self.batches.write().await;
        
        if let Some(batch) = batches.get_mut(batch_id) {
            batch.processed = processed;
            batch.status = BatchProofStatus::Failed;
            Ok(())
        } else {
            Err(BatchProofError::ProcessingFailed("Batch not found".to_string()).into())
        }
    }

    pub async fn recover_batch_proof(&self, batch_id: &str) -> Result<BatchProofResult> {
        let batches = self.batches.read().await;
        
        if let Some(batch) = batches.get(batch_id) {
            let mut result = self.create_batch_proof(batch.request.clone()).await?;
            result.recovered_from_index = batch.processed;
            result.is_recovered = true;
            Ok(result)
        } else {
            Err(BatchProofError::ProcessingFailed("Batch not found".to_string()).into())
        }
    }

    pub async fn wait_for_batch(&self, batch_id: &str) -> Result<BatchProofResult> {
        // Wait for completion
        loop {
            let status = self.get_batch_status(batch_id).await?;
            match status.status {
                BatchProofStatus::Completed => break,
                BatchProofStatus::Failed | BatchProofStatus::Cancelled => {
                    return Err(BatchProofError::ProcessingFailed("Batch failed".to_string()).into());
                }
                _ => tokio::time::sleep(tokio::time::Duration::from_millis(100)).await,
            }
        }

        // Get the batch request and create result
        let batches = self.batches.read().await;
        if let Some(batch) = batches.get(batch_id) {
            self.create_batch_proof(batch.request.clone()).await
        } else {
            Err(BatchProofError::ProcessingFailed("Batch not found".to_string()).into())
        }
    }
}