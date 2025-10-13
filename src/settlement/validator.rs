//! Settlement Validation Module
//!
//! Validates EZKL proofs before payment release to ensure proof integrity.
//! Proofs must be verified against the original inference results before
//! settlement can proceed.

use crate::results::packager::InferenceResult;
use crate::results::proofs::{InferenceProof, ProofGenerator};
use crate::storage::{ProofStore, ResultStore};
use anyhow::{anyhow, Result};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Metrics for proof validation
#[derive(Debug, Clone, Default)]
pub struct ValidatorMetrics {
    validations_total: Arc<AtomicU64>,
    validations_passed: Arc<AtomicU64>,
    validations_failed: Arc<AtomicU64>,
    validation_duration_ms: Arc<AtomicU64>,
    validation_count: Arc<AtomicU64>,
}

impl ValidatorMetrics {
    pub fn new() -> Self {
        Self {
            validations_total: Arc::new(AtomicU64::new(0)),
            validations_passed: Arc::new(AtomicU64::new(0)),
            validations_failed: Arc::new(AtomicU64::new(0)),
            validation_duration_ms: Arc::new(AtomicU64::new(0)),
            validation_count: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn record_validation_attempt(&self) {
        self.validations_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_validation_success(&self, duration_ms: u64) {
        self.validations_passed.fetch_add(1, Ordering::Relaxed);
        self.validation_duration_ms
            .fetch_add(duration_ms, Ordering::Relaxed);
        self.validation_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_validation_failure(&self, duration_ms: u64) {
        self.validations_failed.fetch_add(1, Ordering::Relaxed);
        self.validation_duration_ms
            .fetch_add(duration_ms, Ordering::Relaxed);
        self.validation_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn validations_total(&self) -> u64 {
        self.validations_total.load(Ordering::Relaxed)
    }

    pub fn validations_passed(&self) -> u64 {
        self.validations_passed.load(Ordering::Relaxed)
    }

    pub fn validations_failed(&self) -> u64 {
        self.validations_failed.load(Ordering::Relaxed)
    }

    pub fn avg_validation_ms(&self) -> f64 {
        let total_ms = self.validation_duration_ms.load(Ordering::Relaxed);
        let count = self.validation_count.load(Ordering::Relaxed);
        if count == 0 {
            0.0
        } else {
            total_ms as f64 / count as f64
        }
    }

    pub fn validation_success_rate(&self) -> f64 {
        let total = self.validations_total.load(Ordering::Relaxed);
        let passed = self.validations_passed.load(Ordering::Relaxed);
        if total == 0 {
            0.0
        } else {
            (passed as f64 / total as f64) * 100.0
        }
    }
}

/// Settlement validator for verifying proofs before payment release
pub struct SettlementValidator {
    proof_generator: Arc<ProofGenerator>,
    proof_store: Arc<RwLock<ProofStore>>,
    result_store: Arc<RwLock<ResultStore>>,
    metrics: ValidatorMetrics,
}

impl SettlementValidator {
    /// Create a new settlement validator
    pub fn new(
        proof_generator: Arc<ProofGenerator>,
        proof_store: Arc<RwLock<ProofStore>>,
        result_store: Arc<RwLock<ResultStore>>,
    ) -> Self {
        Self {
            proof_generator,
            proof_store,
            result_store,
            metrics: ValidatorMetrics::new(),
        }
    }

    /// Validate proof before settlement
    ///
    /// This is the main entry point for settlement validation. It:
    /// 1. Retrieves the stored proof for the job
    /// 2. Retrieves the original inference result
    /// 3. Verifies the proof against the result
    /// 4. Tracks metrics
    ///
    /// Returns Ok(true) if proof is valid, Ok(false) if invalid, Err if proof/result missing
    pub async fn validate_before_settlement(&self, job_id: u64) -> Result<bool> {
        info!("🔍 [VALIDATOR] Starting validation for job {}", job_id);

        self.metrics.record_validation_attempt();
        let start = Instant::now();

        // Retrieve proof
        let proof = match self.retrieve_proof(job_id).await {
            Ok(p) => p,
            Err(e) => {
                let duration = start.elapsed().as_millis() as u64;
                self.metrics.record_validation_failure(duration);
                error!("❌ [VALIDATOR] Failed to retrieve proof for job {}: {}", job_id, e);
                return Err(e);
            }
        };

        // Retrieve result
        let result = match self.retrieve_result(job_id).await {
            Ok(r) => r,
            Err(e) => {
                let duration = start.elapsed().as_millis() as u64;
                self.metrics.record_validation_failure(duration);
                error!("❌ [VALIDATOR] Failed to retrieve result for job {}: {}", job_id, e);
                return Err(e);
            }
        };

        // Verify proof
        let is_valid = match self.verify_proof(&proof, &result).await {
            Ok(v) => v,
            Err(e) => {
                let duration = start.elapsed().as_millis() as u64;
                self.metrics.record_validation_failure(duration);
                error!("❌ [VALIDATOR] Verification error for job {}: {}", job_id, e);
                return Err(e);
            }
        };

        let duration = start.elapsed().as_millis() as u64;

        if is_valid {
            self.metrics.record_validation_success(duration);
            info!("✅ [VALIDATOR] Proof validated successfully for job {} ({} ms)", job_id, duration);
        } else {
            self.metrics.record_validation_failure(duration);
            warn!("❌ [VALIDATOR] Proof validation failed for job {} ({} ms)", job_id, duration);
        }

        Ok(is_valid)
    }

    /// Retrieve stored proof for a job
    pub async fn retrieve_proof(&self, job_id: u64) -> Result<InferenceProof> {
        debug!("[VALIDATOR] Retrieving proof for job {}", job_id);

        let store = self.proof_store.read().await;
        let proof = store
            .retrieve_proof(job_id)
            .await
            .map_err(|e| anyhow!("Proof not found for job {}: {}", job_id, e))?;

        debug!("[VALIDATOR] ✓ Proof retrieved for job {}", job_id);
        Ok(proof)
    }

    /// Retrieve stored inference result for a job
    pub async fn retrieve_result(&self, job_id: u64) -> Result<InferenceResult> {
        debug!("[VALIDATOR] Retrieving result for job {}", job_id);

        let store = self.result_store.read().await;
        let result = store
            .retrieve_result(job_id)
            .await
            .map_err(|e| anyhow!("Result not found for job {}: {}", job_id, e))?;

        debug!("[VALIDATOR] ✓ Result retrieved for job {}", job_id);
        Ok(result)
    }

    /// Verify proof against inference result
    async fn verify_proof(&self, proof: &InferenceProof, result: &InferenceResult) -> Result<bool> {
        debug!("[VALIDATOR] Verifying proof for job {}", proof.job_id);

        let is_valid = self
            .proof_generator
            .verify_proof(proof, result)
            .await
            .map_err(|e| anyhow!("Proof verification failed: {}", e))?;

        if is_valid {
            debug!("[VALIDATOR] ✓ Proof verification passed");
        } else {
            debug!("[VALIDATOR] ✗ Proof verification failed");
        }

        Ok(is_valid)
    }

    /// Check if proof and result exist for a job
    pub async fn has_required_data(&self, job_id: u64) -> bool {
        let has_proof = self.proof_store.read().await.has_proof(job_id).await;
        let has_result = self.result_store.read().await.has_result(job_id).await;
        has_proof && has_result
    }

    /// Get validation metrics
    pub fn metrics(&self) -> ValidatorMetrics {
        self.metrics.clone()
    }

    /// Clean up data for a job (after successful settlement)
    pub async fn cleanup_job(&self, job_id: u64) -> Result<()> {
        info!("[VALIDATOR] Cleaning up data for job {}", job_id);

        // Remove proof
        if let Err(e) = self.proof_store.write().await.remove_proof(job_id).await {
            warn!("[VALIDATOR] Failed to remove proof for job {}: {}", job_id, e);
        }

        // Remove result
        if let Err(e) = self.result_store.write().await.remove_result(job_id).await {
            warn!("[VALIDATOR] Failed to remove result for job {}: {}", job_id, e);
        }

        info!("[VALIDATOR] ✓ Cleanup completed for job {}", job_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results::proofs::{ProofGenerationConfig, ProofType};
    use chrono::Utc;

    fn create_test_proof_generator() -> Arc<ProofGenerator> {
        let config = ProofGenerationConfig {
            proof_type: ProofType::EZKL,
            model_path: "/test/model".to_string(),
            settings_path: None,
            max_proof_size: 10000,
        };
        Arc::new(ProofGenerator::new(config, "test-node".to_string()))
    }

    fn create_test_result(job_id: &str) -> InferenceResult {
        use crate::results::packager::ResultMetadata;
        InferenceResult {
            job_id: job_id.to_string(),
            model_id: "test-model".to_string(),
            prompt: "test prompt".to_string(),
            response: "test response".to_string(),
            tokens_generated: 100,
            inference_time_ms: 50,
            timestamp: Utc::now(),
            node_id: "test-node".to_string(),
            metadata: ResultMetadata::default(),
        }
    }

    #[tokio::test]
    async fn test_validator_creation() {
        let proof_generator = create_test_proof_generator();
        let proof_store = Arc::new(RwLock::new(ProofStore::new()));
        let result_store = Arc::new(RwLock::new(ResultStore::new()));

        let validator = SettlementValidator::new(
            proof_generator,
            proof_store,
            result_store,
        );

        assert_eq!(validator.metrics.validations_total(), 0);
    }

    #[tokio::test]
    async fn test_validate_missing_proof() {
        let proof_generator = create_test_proof_generator();
        let proof_store = Arc::new(RwLock::new(ProofStore::new()));
        let result_store = Arc::new(RwLock::new(ResultStore::new()));

        let validator = SettlementValidator::new(
            proof_generator,
            proof_store,
            result_store,
        );

        // Try to validate non-existent proof
        let result = validator.validate_before_settlement(123).await;
        assert!(result.is_err());
        assert_eq!(validator.metrics.validations_total(), 1);
        assert_eq!(validator.metrics.validations_failed(), 1);
    }

    #[tokio::test]
    async fn test_has_required_data() {
        let proof_generator = create_test_proof_generator();
        let proof_store = Arc::new(RwLock::new(ProofStore::new()));
        let result_store = Arc::new(RwLock::new(ResultStore::new()));

        let validator = SettlementValidator::new(
            proof_generator.clone(),
            proof_store.clone(),
            result_store.clone(),
        );

        // Initially no data
        assert!(!validator.has_required_data(456).await);

        // Store result only
        let result = create_test_result("456");
        result_store.write().await.store_result(456, result.clone()).await.unwrap();
        assert!(!validator.has_required_data(456).await);

        // Store proof
        let proof = proof_generator.generate_proof(&result).await.unwrap();
        proof_store.write().await.store_proof(456, proof).await.unwrap();

        // Now both exist
        assert!(validator.has_required_data(456).await);
    }

    #[tokio::test]
    async fn test_cleanup_job() {
        let proof_generator = create_test_proof_generator();
        let proof_store = Arc::new(RwLock::new(ProofStore::new()));
        let result_store = Arc::new(RwLock::new(ResultStore::new()));

        let validator = SettlementValidator::new(
            proof_generator.clone(),
            proof_store.clone(),
            result_store.clone(),
        );

        // Store data
        let result = create_test_result("789");
        result_store.write().await.store_result(789, result.clone()).await.unwrap();
        let proof = proof_generator.generate_proof(&result).await.unwrap();
        proof_store.write().await.store_proof(789, proof).await.unwrap();

        assert!(validator.has_required_data(789).await);

        // Cleanup
        validator.cleanup_job(789).await.unwrap();

        assert!(!validator.has_required_data(789).await);
    }

    #[tokio::test]
    async fn test_metrics_tracking() {
        let proof_generator = create_test_proof_generator();
        let proof_store = Arc::new(RwLock::new(ProofStore::new()));
        let result_store = Arc::new(RwLock::new(ResultStore::new()));

        let validator = SettlementValidator::new(
            proof_generator.clone(),
            proof_store.clone(),
            result_store.clone(),
        );

        // Attempt 1: Missing proof (failure)
        let _ = validator.validate_before_settlement(111).await;
        assert_eq!(validator.metrics.validations_total(), 1);
        assert_eq!(validator.metrics.validations_failed(), 1);

        // Attempt 2: Valid proof+result (success)
        let result = create_test_result("222");
        result_store.write().await.store_result(222, result.clone()).await.unwrap();
        let proof = proof_generator.generate_proof(&result).await.unwrap();
        proof_store.write().await.store_proof(222, proof).await.unwrap();

        let is_valid = validator.validate_before_settlement(222).await.unwrap();
        assert!(is_valid);
        assert_eq!(validator.metrics.validations_total(), 2);
        assert_eq!(validator.metrics.validations_passed(), 1);
        assert!(validator.metrics.avg_validation_ms() > 0.0);
    }
}
