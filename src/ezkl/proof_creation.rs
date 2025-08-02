use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use thiserror::Error;
use serde::{Serialize, Deserialize};
use sha2::{Sha256, Digest};
use chrono::Utc;

#[derive(Debug, Clone, PartialEq)]
pub enum ProofFormat {
    Standard,
    Compact,
    Aggregated,
    Recursive,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CompressionLevel {
    None,
    Fast,
    Balanced,
    Maximum,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProofStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct ModelInput {
    pub prompt: String,
    pub tokens: Vec<i32>,
    pub embeddings: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct ModelOutput {
    pub response: String,
    pub tokens: Vec<i32>,
    pub logits: Vec<f32>,
    pub attention_weights: Option<Vec<Vec<f32>>>,
    pub is_streaming: bool,
    pub partial_tokens: Vec<Vec<i32>>,
}

impl Default for ModelOutput {
    fn default() -> Self {
        Self {
            response: String::new(),
            tokens: Vec::new(),
            logits: Vec::new(),
            attention_weights: None,
            is_streaming: false,
            partial_tokens: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct InferenceData {
    pub model_id: String,
    pub model_hash: String,
    pub input: ModelInput,
    pub output: ModelOutput,
    pub timestamp: u64,
    pub node_id: String,
}

#[derive(Debug, Clone)]
pub struct ProofRequest {
    pub inference_data: InferenceData,
    pub proof_format: ProofFormat,
    pub compression: CompressionLevel,
    pub include_metadata: bool,
    pub custom_params: HashMap<String, String>,
}

impl ProofRequest {
    pub fn set_custom_param(&mut self, key: &str, value: &str) {
        self.custom_params.insert(key.to_string(), value.to_string());
    }
}


#[derive(Debug, Clone)]
pub struct ProofMetadata {
    pub circuit_hash: String,
    pub num_constraints: usize,
    pub num_public_inputs: usize,
    pub prover_id: String,
    pub proof_system_version: String,
    pub timestamp: u64,
    pub optimizations: Vec<String>,
    pub supports_batching: bool,
    pub recursion_depth: usize,
    pub is_incremental: bool,
    pub num_steps: usize,
    pub custom_params: HashMap<String, String>,
    pub handles_streaming: bool,
    pub stream_chunks_count: usize,
}

#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub witness_generation_ms: u64,
    pub proof_generation_ms: u64,
    pub total_time_ms: u64,
    pub overhead_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ProofResult {
    pub proof_data: Vec<u8>,
    pub status: ProofStatus,
    pub model_id: String,
    pub proof_hash: String,
    pub generation_time_ms: u64,
    pub proof_size_bytes: usize,
    pub format: ProofFormat,
    pub metadata: Option<ProofMetadata>,
    pub performance_metrics: Option<PerformanceMetrics>,
}

#[derive(Error, Debug)]
pub enum ProofError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Generation failed: {0}")]
    GenerationFailed(String),
    #[error("Proof cancelled")]
    Cancelled,
    #[error("Resource exhausted: {0}")]
    ResourceExhausted(String),
}

struct IncrementalProof {
    id: String,
    data: InferenceData,
    steps: Vec<Vec<u8>>,
    status: ProofStatus,
    started_at: u64,
}

pub struct ProofGenerator {
    mock_mode: bool,
    incremental_proofs: Arc<RwLock<HashMap<String, IncrementalProof>>>,
    proof_counter: Arc<RwLock<u64>>,
}

impl ProofGenerator {
    pub async fn new_mock() -> Result<Self> {
        Ok(Self {
            mock_mode: true,
            incremental_proofs: Arc::new(RwLock::new(HashMap::new())),
            proof_counter: Arc::new(RwLock::new(0)),
        })
    }

    pub async fn create_proof(&self, request: ProofRequest) -> Result<ProofResult> {
        // Validate input
        if request.inference_data.model_hash.is_empty() {
            return Err(ProofError::InvalidInput("model_hash cannot be empty".to_string()).into());
        }

        let start_time = std::time::Instant::now();

        // Simulate proof generation
        let (proof_data, compressed_size) = self.generate_mock_proof(&request).await?;

        let generation_time_ms = start_time.elapsed().as_millis() as u64;

        // Generate proof hash
        let mut hasher = Sha256::new();
        hasher.update(&proof_data);
        let proof_hash = format!("{:x}", hasher.finalize());

        // Create metadata if requested
        let metadata = if request.include_metadata {
            Some(self.create_metadata(&request, &proof_data))
        } else {
            None
        };

        // Create performance metrics
        let performance_metrics = Some(PerformanceMetrics {
            witness_generation_ms: generation_time_ms / 3,
            proof_generation_ms: generation_time_ms * 2 / 3,
            total_time_ms: generation_time_ms,
            overhead_ms: generation_time_ms / 10,
        });

        let mut counter = self.proof_counter.write().await;
        *counter += 1;

        Ok(ProofResult {
            proof_data,
            status: ProofStatus::Completed,
            model_id: request.inference_data.model_id.clone(),
            proof_hash,
            generation_time_ms,
            proof_size_bytes: compressed_size,
            format: request.proof_format.clone(),
            metadata,
            performance_metrics,
        })
    }

    async fn generate_mock_proof(&self, request: &ProofRequest) -> Result<(Vec<u8>, usize)> {
        // Simulate different proof sizes based on format
        let base_size = match request.proof_format {
            ProofFormat::Standard => 5000,
            ProofFormat::Compact => 3000,
            ProofFormat::Aggregated => 7000,
            ProofFormat::Recursive => 10000,
        };

        // Generate deterministic proof data
        let mut proof_data = Vec::with_capacity(base_size);
        let mut hasher = Sha256::new();
        hasher.update(request.inference_data.model_hash.as_bytes());
        hasher.update(request.inference_data.input.prompt.as_bytes());
        hasher.update(request.inference_data.output.response.as_bytes());
        let hash = hasher.finalize();

        for i in 0..base_size {
            proof_data.push(hash[i % 32]);
        }

        // Apply compression
        let compressed_size = match request.compression {
            CompressionLevel::None => base_size,
            CompressionLevel::Fast => base_size * 8 / 10,
            CompressionLevel::Balanced => base_size * 6 / 10,
            CompressionLevel::Maximum => base_size * 4 / 10,
        };

        // Simulate compression time
        let compression_delay = match request.compression {
            CompressionLevel::None => 0,
            CompressionLevel::Fast => 10,
            CompressionLevel::Balanced => 20,
            CompressionLevel::Maximum => 50,
        };
        tokio::time::sleep(tokio::time::Duration::from_millis(compression_delay)).await;

        proof_data.truncate(compressed_size);

        Ok((proof_data, compressed_size))
    }

    fn create_metadata(&self, request: &ProofRequest, _proof_data: &[u8]) -> ProofMetadata {
        let mut optimizations = Vec::new();
        if let ProofFormat::Compact = request.proof_format {
            optimizations.push("size".to_string());
        }

        ProofMetadata {
            circuit_hash: format!("circuit_{}", request.inference_data.model_hash),
            num_constraints: 100000,
            num_public_inputs: 10,
            prover_id: "mock_prover_001".to_string(),
            proof_system_version: crate::ezkl::PROOF_SYSTEM_VERSION.to_string(),
            timestamp: Utc::now().timestamp() as u64,
            optimizations,
            supports_batching: matches!(request.proof_format, ProofFormat::Aggregated),
            recursion_depth: if matches!(request.proof_format, ProofFormat::Recursive) { 1 } else { 0 },
            is_incremental: false,
            num_steps: 0,
            custom_params: request.custom_params.clone(),
            handles_streaming: request.inference_data.output.is_streaming,
            stream_chunks_count: request.inference_data.output.partial_tokens.len(),
        }
    }

    pub async fn start_incremental_proof(&self, inference_data: &InferenceData) -> Result<String> {
        let proof_id = format!("proof_{}", uuid::Uuid::new_v4());
        
        let incremental = IncrementalProof {
            id: proof_id.clone(),
            data: inference_data.clone(),
            steps: Vec::new(),
            status: ProofStatus::InProgress,
            started_at: Utc::now().timestamp() as u64,
        };

        let mut proofs = self.incremental_proofs.write().await;
        proofs.insert(proof_id.clone(), incremental);

        Ok(proof_id)
    }

    pub async fn add_proof_step(&self, proof_id: &str, step_data: &[u8]) -> Result<()> {
        let mut proofs = self.incremental_proofs.write().await;
        
        if let Some(proof) = proofs.get_mut(proof_id) {
            if proof.status != ProofStatus::InProgress {
                return Err(ProofError::GenerationFailed("Proof not in progress".to_string()).into());
            }
            proof.steps.push(step_data.to_vec());
            Ok(())
        } else {
            Err(ProofError::GenerationFailed("Proof not found".to_string()).into())
        }
    }

    pub async fn get_proof_status(&self, proof_id: &str) -> Result<ProofStatus> {
        let proofs = self.incremental_proofs.read().await;
        
        if let Some(proof) = proofs.get(proof_id) {
            Ok(proof.status.clone())
        } else {
            Err(ProofError::GenerationFailed("Proof not found".to_string()).into())
        }
    }

    pub async fn finalize_incremental_proof(&self, proof_id: &str) -> Result<ProofResult> {
        let mut proofs = self.incremental_proofs.write().await;
        
        if let Some(mut proof) = proofs.remove(proof_id) {
            if proof.status == ProofStatus::Cancelled {
                return Err(ProofError::Cancelled.into());
            }

            proof.status = ProofStatus::Completed;

            // Combine all steps into final proof
            let mut final_proof = Vec::new();
            for step in &proof.steps {
                final_proof.extend_from_slice(step);
            }

            // Generate hash
            let mut hasher = Sha256::new();
            hasher.update(&final_proof);
            let proof_hash = format!("{:x}", hasher.finalize());

            let generation_time_ms = (Utc::now().timestamp() as u64 - proof.started_at) * 1000;

            Ok(ProofResult {
                proof_data: final_proof,
                status: ProofStatus::Completed,
                model_id: proof.data.model_id,
                proof_hash,
                generation_time_ms,
                proof_size_bytes: proof.steps.iter().map(|s| s.len()).sum(),
                format: ProofFormat::Standard,
                metadata: Some(ProofMetadata {
                    circuit_hash: "incremental_circuit".to_string(),
                    num_constraints: 100000,
                    num_public_inputs: 10,
                    prover_id: "mock_prover_001".to_string(),
                    proof_system_version: crate::ezkl::PROOF_SYSTEM_VERSION.to_string(),
                    timestamp: Utc::now().timestamp() as u64,
                    optimizations: vec![],
                    supports_batching: false,
                    recursion_depth: 0,
                    is_incremental: true,
                    num_steps: proof.steps.len(),
                    custom_params: HashMap::new(),
                    handles_streaming: false,
                    stream_chunks_count: 0,
                }),
                performance_metrics: None,
            })
        } else {
            Err(ProofError::GenerationFailed("Proof not found".to_string()).into())
        }
    }

    pub async fn cancel_proof(&self, proof_id: &str) -> Result<()> {
        let mut proofs = self.incremental_proofs.write().await;
        
        if let Some(proof) = proofs.get_mut(proof_id) {
            proof.status = ProofStatus::Cancelled;
            Ok(())
        } else {
            Err(ProofError::GenerationFailed("Proof not found".to_string()).into())
        }
    }
}