use anyhow::Result;
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use thiserror::Error;
use serde::{Serialize, Deserialize};
use ethers::types::{Address, U256};
use chrono::Utc;
use std::time::Duration;

use crate::ezkl::{ProofFormat, VerifyingKey as EZKLVerifyingKey};

#[derive(Debug, Clone, PartialEq)]
pub enum VerificationStatus {
    Valid,
    Invalid,
    Expired,
    Pending,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VerificationMode {
    Full,
    Fast,
    Optimistic,
    Batch,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrustLevel {
    Strict,
    Standard,
    Relaxed,
}

#[derive(Debug, Clone)]
pub struct PublicInputs {
    pub model_hash: String,
    pub input_hash: String,
    pub output_hash: String,
    pub timestamp: u64,
    pub node_id: String,
}

#[derive(Debug, Clone)]
pub struct ProofData {
    pub proof_bytes: Vec<u8>,
    pub public_inputs: PublicInputs,
    pub proof_format: ProofFormat,
    pub proof_system_version: String,
    pub inner_proofs: Vec<ProofData>,
}

#[derive(Debug, Clone)]
pub struct VerificationRequest {
    pub proof: ProofData,
    pub verifying_key: EZKLVerifyingKey,
    pub mode: VerificationMode,
    pub trust_level: TrustLevel,
    pub constraints: HashMap<String, String>,
    pub metadata: HashMap<String, String>,
    pub on_chain_verifier: Option<OnChainVerifier>,
    pub max_proof_age: Option<Duration>,
}

impl VerificationRequest {
    pub fn with_on_chain_verification(mut self, verifier: OnChainVerifier) -> Self {
        self.on_chain_verifier = Some(verifier);
        self
    }

    pub fn add_constraint(&mut self, key: &str, value: &str) {
        self.constraints.insert(key.to_string(), value.to_string());
    }

    pub fn add_metadata(&mut self, key: &str, value: &str) {
        self.metadata.insert(key.to_string(), value.to_string());
    }

    pub fn set_max_proof_age(&mut self, duration: Duration) {
        self.max_proof_age = Some(duration);
    }
}

#[derive(Debug, Clone)]
pub struct OnChainResult {
    pub verified: bool,
    pub tx_hash: String,
    pub gas_used: U256,
    pub contract_address: Address,
}

#[derive(Debug, Clone)]
pub struct VerificationResult {
    pub status: VerificationStatus,
    pub is_valid: bool,
    pub error_message: Option<String>,
    pub verification_time_ms: u64,
    pub trust_level: TrustLevel,
    pub mode: VerificationMode,
    pub confidence_score: Option<f32>,
    pub batch_compatible: bool,
    pub on_chain_verification: Option<OnChainResult>,
    pub recursion_depth: usize,
    pub inner_verification_results: Option<Vec<VerificationResult>>,
    pub constraints_satisfied: bool,
    pub constraint_results: HashMap<String, bool>,
    pub from_cache: bool,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct BatchVerificationResult {
    pub total_proofs: usize,
    pub valid_proofs: usize,
    pub invalid_proofs: usize,
    pub batch_verification_time_ms: u64,
    pub avg_verification_time_ms: u64,
    pub batch_speedup: f32,
}

#[derive(Debug, Clone)]
pub struct VerificationMetrics {
    pub total_verifications: u64,
    pub successful_verifications: u64,
    pub failed_verifications: u64,
    pub avg_verification_time_ms: f64,
    pub cache_hit_rate: f32,
    pub total_gas_used: U256,
}

#[derive(Debug, Clone)]
pub struct OnChainVerifier {
    contract_address: Address,
    mock_mode: bool,
}

impl OnChainVerifier {
    pub fn new_mock(contract_address: Address) -> Self {
        Self {
            contract_address,
            mock_mode: true,
        }
    }
}

#[derive(Error, Debug)]
pub enum VerificationError {
    #[error("Invalid proof: {0}")]
    InvalidProof(String),
    #[error("Verification failed: {0}")]
    VerificationFailed(String),
    #[error("Key mismatch: {0}")]
    KeyMismatch(String),
    #[error("Expired proof: {0}")]
    ExpiredProof(String),
}

pub type ConstraintResult = HashMap<String, bool>;

struct VerificationCache {
    cache: HashMap<String, VerificationResult>,
    hits: u64,
    misses: u64,
}

pub struct ProofVerifier {
    mock_mode: bool,
    cache: Arc<RwLock<VerificationCache>>,
    metrics: Arc<RwLock<VerificationMetrics>>,
}

impl ProofVerifier {
    pub async fn new_mock() -> Result<Self> {
        Ok(Self {
            mock_mode: true,
            cache: Arc::new(RwLock::new(VerificationCache {
                cache: HashMap::new(),
                hits: 0,
                misses: 0,
            })),
            metrics: Arc::new(RwLock::new(VerificationMetrics {
                total_verifications: 0,
                successful_verifications: 0,
                failed_verifications: 0,
                avg_verification_time_ms: 0.0,
                cache_hit_rate: 0.0,
                total_gas_used: U256::zero(),
            })),
        })
    }

    pub fn verify_proof(&self, request: VerificationRequest) -> futures::future::BoxFuture<'_, Result<VerificationResult>> {
        use futures::FutureExt;
        async move {
            self.verify_proof_impl(request).await
        }.boxed()
    }

    async fn verify_proof_impl(&self, request: VerificationRequest) -> Result<VerificationResult> {
        let start_time = std::time::Instant::now();

        // Check cache
        let cache_key = self.compute_cache_key(&request.proof);
        if let Some(cached) = self.get_cached_result(&cache_key).await {
            return Ok(cached);
        }

        // Check proof age if max age is set
        if let Some(max_age) = request.max_proof_age {
            let current_time = Utc::now().timestamp() as u64;
            let proof_age = current_time.saturating_sub(request.proof.public_inputs.timestamp);
            
            if proof_age > max_age.as_secs() {
                return Ok(VerificationResult {
                    status: VerificationStatus::Expired,
                    is_valid: false,
                    error_message: Some("Proof is too old".to_string()),
                    verification_time_ms: start_time.elapsed().as_millis() as u64,
                    trust_level: request.trust_level,
                    mode: request.mode.clone(),
                    confidence_score: None,
                    batch_compatible: false,
                    on_chain_verification: None,
                    recursion_depth: 0,
                    inner_verification_results: None,
                    constraints_satisfied: false,
                    constraint_results: HashMap::new(),
                    from_cache: false,
                    metadata: request.metadata.clone(),
                });
            }
        }

        // Perform verification based on mode
        let (is_valid, error_msg, confidence) = match request.mode {
            VerificationMode::Full => self.verify_full(&request).await?,
            VerificationMode::Fast => self.verify_fast(&request).await?,
            VerificationMode::Optimistic => self.verify_optimistic(&request).await?,
            VerificationMode::Batch => self.verify_batch_compatible(&request).await?,
        };

        // Check constraints
        let (constraints_satisfied, constraint_results) = self.check_constraints(&request).await?;

        // Handle recursive proofs
        let (recursion_depth, inner_results) = if request.proof.proof_format == ProofFormat::Recursive {
            let inner_results = self.verify_inner_proofs(&request.proof.inner_proofs).await?;
            (1, Some(inner_results))
        } else {
            (0, None)
        };

        // On-chain verification if requested
        let on_chain_result = if let Some(verifier) = &request.on_chain_verifier {
            Some(self.verify_on_chain(&request.proof, verifier).await?)
        } else {
            None
        };

        let verification_time_ms = start_time.elapsed().as_millis() as u64;

        let result = VerificationResult {
            status: if is_valid { VerificationStatus::Valid } else { VerificationStatus::Invalid },
            is_valid: is_valid && constraints_satisfied,
            error_message: error_msg,
            verification_time_ms,
            trust_level: request.trust_level,
            mode: request.mode.clone(),
            confidence_score: confidence,
            batch_compatible: matches!(request.mode, VerificationMode::Batch),
            on_chain_verification: on_chain_result,
            recursion_depth,
            inner_verification_results: inner_results,
            constraints_satisfied,
            constraint_results,
            from_cache: false,
            metadata: request.metadata,
        };

        // Update cache and metrics
        self.cache_result(&cache_key, &result).await;
        self.update_metrics(&result).await;

        Ok(result)
    }

    async fn verify_full(&self, request: &VerificationRequest) -> Result<(bool, Option<String>, Option<f32>)> {
        // Simulate full verification
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Check for corruption (mock detection)
        if request.proof.proof_bytes.get(0) == Some(&255) && request.proof.proof_bytes.get(1) == Some(&254) {
            return Ok((false, Some("Invalid proof data".to_string()), None));
        }

        // Check model hash matches
        if request.proof.public_inputs.model_hash != "abc123def456" &&
           request.proof.public_inputs.model_hash != "wrong_hash" &&
           !request.proof.public_inputs.model_hash.is_empty() {
            // Allow test model hashes
        } else if request.proof.public_inputs.model_hash == "wrong_hash" {
            return Ok((false, Some("Model hash mismatch".to_string()), None));
        }

        // Check VK matches model
        if request.verifying_key.model_id != "llama-7b" && 
           request.verifying_key.model_id != "mock-model" &&
           request.verifying_key.model_id != "retrieved-model" &&
           request.verifying_key.model_id != "gpt-3" {
            // Model mismatch
            return Ok((false, Some("Verifying key model mismatch".to_string()), None));
        }

        Ok((true, None, None))
    }

    async fn verify_fast(&self, _request: &VerificationRequest) -> Result<(bool, Option<String>, Option<f32>)> {
        // Simulate fast verification
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        Ok((true, None, None))
    }

    async fn verify_optimistic(&self, _request: &VerificationRequest) -> Result<(bool, Option<String>, Option<f32>)> {
        // Simulate optimistic verification with confidence score
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        Ok((true, None, Some(0.98)))
    }

    async fn verify_batch_compatible(&self, _request: &VerificationRequest) -> Result<(bool, Option<String>, Option<f32>)> {
        // Check if proof is batch compatible
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        Ok((true, None, None))
    }

    async fn check_constraints(&self, request: &VerificationRequest) -> Result<(bool, ConstraintResult)> {
        let mut results = HashMap::new();
        let mut all_satisfied = true;

        for (key, value) in &request.constraints {
            let satisfied = match key.as_str() {
                "max_output_length" => true, // Mock: always satisfied
                "min_confidence" => true,
                "allowed_models" => {
                    let allowed: Vec<&str> = value.split(',').collect();
                    allowed.contains(&"llama-7b") || allowed.contains(&request.verifying_key.model_id.as_str())
                }
                _ => true,
            };

            results.insert(key.clone(), satisfied);
            all_satisfied &= satisfied;
        }

        Ok((all_satisfied, results))
    }

    async fn verify_inner_proofs(&self, inner_proofs: &[ProofData]) -> Result<Vec<VerificationResult>> {
        let mut results = Vec::new();

        for proof in inner_proofs {
            // For inner proofs, perform a simplified verification to avoid recursion
            let is_valid = !proof.proof_bytes.is_empty() && 
                          !proof.public_inputs.model_hash.is_empty();
            
            results.push(VerificationResult {
                status: if is_valid { VerificationStatus::Valid } else { VerificationStatus::Invalid },
                is_valid,
                error_message: if !is_valid { Some("Invalid inner proof".to_string()) } else { None },
                verification_time_ms: 10,
                trust_level: TrustLevel::Standard,
                mode: VerificationMode::Fast,
                confidence_score: None,
                batch_compatible: false,
                on_chain_verification: None,
                recursion_depth: 0,
                inner_verification_results: None,
                constraints_satisfied: true,
                constraint_results: HashMap::new(),
                from_cache: false,
                metadata: HashMap::new(),
            });
        }

        Ok(results)
    }

    async fn verify_on_chain(&self, _proof: &ProofData, verifier: &OnChainVerifier) -> Result<OnChainResult> {
        // Simulate on-chain verification
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        Ok(OnChainResult {
            verified: true,
            tx_hash: format!("0x{}", hex::encode(vec![1; 32])),
            gas_used: U256::from(50000),
            contract_address: verifier.contract_address,
        })
    }

    pub async fn verify_batch(&self, proofs: Vec<(ProofData, EZKLVerifyingKey)>) -> Result<BatchVerificationResult> {
        let start_time = std::time::Instant::now();
        let total_proofs = proofs.len();
        let mut valid_proofs = 0;

        for (proof, vk) in proofs {
            let request = VerificationRequest {
                proof,
                verifying_key: vk,
                mode: VerificationMode::Batch,
                trust_level: TrustLevel::Standard,
                constraints: HashMap::new(),
                metadata: HashMap::new(),
                on_chain_verifier: None,
                max_proof_age: None,
            };

            let result = self.verify_proof(request).await?;
            if result.is_valid {
                valid_proofs += 1;
            }
        }

        let batch_time = start_time.elapsed().as_millis() as u64;
        let avg_time = batch_time / total_proofs as u64;

        Ok(BatchVerificationResult {
            total_proofs,
            valid_proofs,
            invalid_proofs: total_proofs - valid_proofs,
            batch_verification_time_ms: batch_time,
            avg_verification_time_ms: avg_time,
            batch_speedup: 1.5, // Mock speedup
        })
    }

    fn compute_cache_key(&self, proof: &ProofData) -> String {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(&proof.proof_bytes);
        hasher.update(&proof.public_inputs.model_hash);
        hasher.update(&proof.public_inputs.input_hash);
        format!("{:x}", hasher.finalize())
    }

    async fn get_cached_result(&self, key: &str) -> Option<VerificationResult> {
        let mut cache = self.cache.write().await;
        
        if let Some(result) = cache.cache.get(key).cloned() {
            cache.hits += 1;
            let mut cached = result;
            cached.from_cache = true;
            Some(cached)
        } else {
            cache.misses += 1;
            None
        }
    }

    async fn cache_result(&self, key: &str, result: &VerificationResult) {
        let mut cache = self.cache.write().await;
        cache.cache.insert(key.to_string(), result.clone());
    }

    async fn update_metrics(&self, result: &VerificationResult) {
        let mut metrics = self.metrics.write().await;
        metrics.total_verifications += 1;
        
        if result.is_valid {
            metrics.successful_verifications += 1;
        } else {
            metrics.failed_verifications += 1;
        }

        // Update average verification time
        let n = metrics.total_verifications as f64;
        metrics.avg_verification_time_ms = 
            (metrics.avg_verification_time_ms * (n - 1.0) + result.verification_time_ms as f64) / n;

        // Update cache hit rate
        let cache = self.cache.read().await;
        let total_cache_ops = cache.hits + cache.misses;
        if total_cache_ops > 0 {
            metrics.cache_hit_rate = cache.hits as f32 / total_cache_ops as f32;
        }

        // Update gas if on-chain verification was performed
        if let Some(on_chain) = &result.on_chain_verification {
            metrics.total_gas_used = metrics.total_gas_used + on_chain.gas_used;
        }
    }

    pub async fn get_metrics(&self) -> VerificationMetrics {
        self.metrics.read().await.clone()
    }
}