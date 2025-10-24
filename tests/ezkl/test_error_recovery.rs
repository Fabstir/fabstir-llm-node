// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Error Recovery Tests for EZKL Proof System
//!
//! Tests error handling and recovery scenarios:
//! - Missing keys
//! - Corrupted proof data
//! - Store failures
//! - Network errors
//! - Graceful degradation

use anyhow::Result;
use chrono::Utc;
use fabstir_llm_node::results::packager::{InferenceResult, ResultMetadata};
use fabstir_llm_node::results::proofs::{ProofGenerationConfig, ProofGenerator, ProofType};
use fabstir_llm_node::settlement::validator::SettlementValidator;
use fabstir_llm_node::storage::{ProofStore, ResultStore};
use std::sync::Arc;
use tokio::sync::RwLock;

fn create_test_result(job_id: u64) -> InferenceResult {
    InferenceResult {
        job_id: job_id.to_string(),
        model_id: "test-model".to_string(),
        prompt: "Test prompt".to_string(),
        response: "Test response".to_string(),
        tokens_generated: 100,
        inference_time_ms: 50,
        timestamp: Utc::now(),
        node_id: "test-node".to_string(),
        metadata: ResultMetadata::default(),
    }
}

fn create_proof_generator() -> Arc<ProofGenerator> {
    let config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "/test/model".to_string(),
        settings_path: None,
        max_proof_size: 10000,
    };
    Arc::new(ProofGenerator::new(config, "test-node".to_string()))
}

#[tokio::test]
async fn test_missing_proof_error_recovery() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let validator =
        SettlementValidator::new(proof_gen.clone(), proof_store.clone(), result_store.clone());

    // Store result but NO proof
    let result = create_test_result(1000);
    result_store
        .write()
        .await
        .store_result(1000, result)
        .await?;

    // Attempt 1: Should error with missing proof
    let validation_result = validator.validate_before_settlement(1000).await;
    assert!(
        validation_result.is_err(),
        "Should error when proof is missing"
    );

    // Recovery: Add the proof
    let proof = proof_gen.generate_proof(&create_test_result(1000)).await?;
    proof_store.write().await.store_proof(1000, proof).await?;

    // Attempt 2: Should succeed after recovery
    let is_valid = validator.validate_before_settlement(1000).await?;
    assert!(is_valid, "Should succeed after proof is added");

    Ok(())
}

#[tokio::test]
async fn test_missing_result_error_recovery() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let validator =
        SettlementValidator::new(proof_gen.clone(), proof_store.clone(), result_store.clone());

    // Store proof but NO result
    let result = create_test_result(2000);
    let proof = proof_gen.generate_proof(&result).await?;
    proof_store.write().await.store_proof(2000, proof).await?;

    // Attempt 1: Should error with missing result
    let validation_result = validator.validate_before_settlement(2000).await;
    assert!(
        validation_result.is_err(),
        "Should error when result is missing"
    );

    // Recovery: Add the result
    result_store
        .write()
        .await
        .store_result(2000, result)
        .await?;

    // Attempt 2: Should succeed after recovery
    let is_valid = validator.validate_before_settlement(2000).await?;
    assert!(is_valid, "Should succeed after result is added");

    Ok(())
}

#[tokio::test]
async fn test_corrupted_proof_detection() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let validator =
        SettlementValidator::new(proof_gen.clone(), proof_store.clone(), result_store.clone());

    // Generate valid proof
    let result = create_test_result(3000);
    let mut proof = proof_gen.generate_proof(&result).await?;

    // Corrupt the proof data
    if !proof.proof_data.is_empty() {
        proof.proof_data[0] = !proof.proof_data[0]; // Flip bits
    }

    // Store corrupted proof
    proof_store.write().await.store_proof(3000, proof).await?;
    result_store
        .write()
        .await
        .store_result(3000, result)
        .await?;

    // Validation should detect corruption
    let is_valid = validator.validate_before_settlement(3000).await?;
    assert!(!is_valid, "Corrupted proof should fail validation");

    Ok(())
}

#[tokio::test]
async fn test_proof_store_concurrent_access_recovery() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    // Simulate concurrent writes to store
    let handles: Vec<_> = (4000..4010u64)
        .map(|job_id| {
            let proof_gen = proof_gen.clone();
            let proof_store = proof_store.clone();
            let result_store = result_store.clone();
            tokio::spawn(async move {
                let result = create_test_result(job_id);
                let proof = proof_gen.generate_proof(&result).await.unwrap();
                result_store
                    .write()
                    .await
                    .store_result(job_id, result)
                    .await
                    .unwrap();
                proof_store
                    .write()
                    .await
                    .store_proof(job_id, proof)
                    .await
                    .unwrap();
            })
        })
        .collect();

    for handle in handles {
        handle.await.unwrap();
    }

    // Verify all data is accessible
    let validator =
        SettlementValidator::new(proof_gen.clone(), proof_store.clone(), result_store.clone());

    for job_id in 4000..4010 {
        let is_valid = validator.validate_before_settlement(job_id).await?;
        assert!(
            is_valid,
            "Job {} should be valid after concurrent writes",
            job_id
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_store_clear_recovery() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let validator =
        SettlementValidator::new(proof_gen.clone(), proof_store.clone(), result_store.clone());

    // Store valid data
    let result = create_test_result(5000);
    let proof = proof_gen.generate_proof(&result).await?;
    result_store
        .write()
        .await
        .store_result(5000, result.clone())
        .await?;
    proof_store
        .write()
        .await
        .store_proof(5000, proof.clone())
        .await?;

    // Verify it works
    let is_valid = validator.validate_before_settlement(5000).await?;
    assert!(is_valid);

    // Simulate store clear (memory pressure scenario)
    proof_store.write().await.clear().await;
    result_store.write().await.clear().await;

    // Should error after clear
    let validation_result = validator.validate_before_settlement(5000).await;
    assert!(validation_result.is_err(), "Should error after store clear");

    // Recovery: Re-add the data
    result_store
        .write()
        .await
        .store_result(5000, result)
        .await?;
    proof_store.write().await.store_proof(5000, proof).await?;

    // Should work again
    let is_valid = validator.validate_before_settlement(5000).await?;
    assert!(is_valid, "Should work after recovery");

    Ok(())
}

#[tokio::test]
async fn test_validation_with_empty_stores() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let validator =
        SettlementValidator::new(proof_gen.clone(), proof_store.clone(), result_store.clone());

    // Validate with completely empty stores
    let validation_result = validator.validate_before_settlement(6000).await;
    assert!(validation_result.is_err(), "Should error with empty stores");

    // Check stores are indeed empty
    assert!(proof_store.read().await.is_empty().await);
    assert!(result_store.read().await.is_empty().await);

    Ok(())
}

#[tokio::test]
async fn test_multiple_validation_failures_recovery() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let validator =
        SettlementValidator::new(proof_gen.clone(), proof_store.clone(), result_store.clone());

    // Multiple failed attempts
    for _ in 0..5 {
        let result = validator.validate_before_settlement(7000).await;
        assert!(result.is_err(), "Should keep failing");
    }

    // Check metrics track all failures
    let metrics = validator.metrics();
    assert_eq!(metrics.validations_total(), 5);
    assert_eq!(metrics.validations_failed(), 5);

    // Recovery
    let result = create_test_result(7000);
    let proof = proof_gen.generate_proof(&result).await?;
    result_store
        .write()
        .await
        .store_result(7000, result)
        .await?;
    proof_store.write().await.store_proof(7000, proof).await?;

    // Should succeed
    let is_valid = validator.validate_before_settlement(7000).await?;
    assert!(is_valid);

    // Check updated metrics
    let metrics = validator.metrics();
    assert_eq!(metrics.validations_total(), 6);
    assert_eq!(metrics.validations_passed(), 1);
    assert_eq!(metrics.validations_failed(), 5);

    Ok(())
}

#[tokio::test]
async fn test_cleanup_and_revalidation() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let validator =
        SettlementValidator::new(proof_gen.clone(), proof_store.clone(), result_store.clone());

    // First validation cycle
    let result = create_test_result(8000);
    let proof = proof_gen.generate_proof(&result).await?;
    result_store
        .write()
        .await
        .store_result(8000, result.clone())
        .await?;
    proof_store
        .write()
        .await
        .store_proof(8000, proof.clone())
        .await?;

    let is_valid = validator.validate_before_settlement(8000).await?;
    assert!(is_valid);

    // Cleanup after settlement
    validator.cleanup_job(8000).await?;

    // Revalidation should fail (data cleaned up)
    let validation_result = validator.validate_before_settlement(8000).await;
    assert!(validation_result.is_err(), "Should fail after cleanup");

    // Recovery: Re-add for new settlement attempt
    result_store
        .write()
        .await
        .store_result(8000, result)
        .await?;
    proof_store.write().await.store_proof(8000, proof).await?;

    let is_valid = validator.validate_before_settlement(8000).await?;
    assert!(is_valid, "Should work after re-adding data");

    Ok(())
}
