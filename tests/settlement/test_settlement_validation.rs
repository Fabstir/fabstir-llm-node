// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Settlement Validation Tests
//!
//! Tests that demonstrate proof validation before payment settlement.
//! Validates that invalid proofs prevent payment release.

use anyhow::Result;
use chrono::Utc;
use fabstir_llm_node::results::packager::{InferenceResult, ResultMetadata};
use fabstir_llm_node::results::proofs::{ProofGenerationConfig, ProofGenerator, ProofType};
use fabstir_llm_node::settlement::validator::SettlementValidator;
use fabstir_llm_node::storage::{ProofStore, ResultStore};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Helper to create test inference result
fn create_test_result(job_id: u64, response: &str) -> InferenceResult {
    InferenceResult {
        job_id: job_id.to_string(),
        model_id: "test-model".to_string(),
        prompt: "Test prompt".to_string(),
        response: response.to_string(),
        tokens_generated: 100,
        inference_time_ms: 50,
        timestamp: Utc::now(),
        node_id: "test-node".to_string(),
        metadata: ResultMetadata::default(),
    }
}

/// Helper to create proof generator
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
async fn test_validate_with_valid_proof() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    // Store result and proof
    let result = create_test_result(100, "Valid response");
    let proof = proof_gen.generate_proof(&result).await?;

    result_store.write().await.store_result(100, result).await?;
    proof_store.write().await.store_proof(100, proof).await?;

    // Validate should succeed
    let is_valid = validator.validate_before_settlement(100).await?;
    assert!(is_valid, "Valid proof should pass validation");

    // Check metrics
    assert_eq!(validator.metrics().validations_total(), 1);
    assert_eq!(validator.metrics().validations_passed(), 1);
    assert_eq!(validator.metrics().validations_failed(), 0);

    Ok(())
}

#[tokio::test]
async fn test_validate_with_invalid_proof() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    // Store result and proof
    let result = create_test_result(200, "Original response");
    let proof = proof_gen.generate_proof(&result).await?;

    // Store proof with original result
    proof_store.write().await.store_proof(200, proof).await?;

    // But store DIFFERENT result (tampering scenario)
    let tampered_result = create_test_result(200, "Tampered response");
    result_store
        .write()
        .await
        .store_result(200, tampered_result)
        .await?;

    // Validation should fail (hash mismatch)
    let is_valid = validator.validate_before_settlement(200).await?;
    assert!(!is_valid, "Tampered result should fail validation");

    // Check metrics
    assert_eq!(validator.metrics().validations_total(), 1);
    assert_eq!(validator.metrics().validations_passed(), 0);
    assert_eq!(validator.metrics().validations_failed(), 1);

    Ok(())
}

#[tokio::test]
async fn test_validate_with_missing_proof() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    // Store only result, no proof
    let result = create_test_result(300, "Response");
    result_store.write().await.store_result(300, result).await?;

    // Validation should error (missing proof)
    let validation_result = validator.validate_before_settlement(300).await;
    assert!(validation_result.is_err(), "Missing proof should error");

    // Metrics should track failure
    assert_eq!(validator.metrics().validations_total(), 1);
    assert_eq!(validator.metrics().validations_failed(), 1);

    Ok(())
}

#[tokio::test]
async fn test_validate_with_tampered_result() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    // Generate proof for original result
    let original = create_test_result(400, "Original output");
    let proof = proof_gen.generate_proof(&original).await?;

    // Store proof
    proof_store.write().await.store_proof(400, proof).await?;

    // Store tampered result with different output
    let tampered = create_test_result(400, "Tampered output - malicious");
    result_store.write().await.store_result(400, tampered).await?;

    // Validation should fail
    let is_valid = validator.validate_before_settlement(400).await?;
    assert!(
        !is_valid,
        "Tampered result should not pass validation - this prevents payment fraud"
    );

    Ok(())
}

#[tokio::test]
async fn test_validate_blocks_settlement_on_failure() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    // Setup invalid scenario (missing proof)
    let result = create_test_result(500, "Response");
    result_store.write().await.store_result(500, result).await?;

    // Validation should error, blocking settlement
    let validation_result = validator.validate_before_settlement(500).await;
    assert!(
        validation_result.is_err(),
        "Validation should error without proof, blocking settlement"
    );

    // In real settlement flow, this would prevent payment release
    // Settlement manager would check: if !validator.validate() { return Err(...) }

    Ok(())
}

#[tokio::test]
async fn test_validator_metrics_tracking() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    // Validation 1: Success
    let result1 = create_test_result(600, "Response 1");
    let proof1 = proof_gen.generate_proof(&result1).await?;
    result_store.write().await.store_result(600, result1).await?;
    proof_store.write().await.store_proof(600, proof1).await?;
    let _ = validator.validate_before_settlement(600).await?;

    // Validation 2: Failure (missing proof)
    let result2 = create_test_result(601, "Response 2");
    result_store.write().await.store_result(601, result2).await?;
    let _ = validator.validate_before_settlement(601).await;

    // Validation 3: Success
    let result3 = create_test_result(602, "Response 3");
    let proof3 = proof_gen.generate_proof(&result3).await?;
    result_store.write().await.store_result(602, result3).await?;
    proof_store.write().await.store_proof(602, proof3).await?;
    let _ = validator.validate_before_settlement(602).await?;

    // Check metrics
    let metrics = validator.metrics();
    assert_eq!(metrics.validations_total(), 3);
    assert_eq!(metrics.validations_passed(), 2);
    assert_eq!(metrics.validations_failed(), 1);
    assert!(metrics.avg_validation_ms() >= 0.0); // Can be 0 for very fast mock operations
    assert!(metrics.validation_success_rate() > 60.0);
    assert!(metrics.validation_success_rate() < 70.0);

    Ok(())
}

#[tokio::test]
async fn test_concurrent_validations() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let validator = Arc::new(SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    ));

    // Setup 10 valid jobs
    for job_id in 700..710 {
        let result = create_test_result(job_id, &format!("Response {}", job_id));
        let proof = proof_gen.generate_proof(&result).await?;
        result_store.write().await.store_result(job_id, result).await?;
        proof_store.write().await.store_proof(job_id, proof).await?;
    }

    // Validate concurrently
    let handles: Vec<_> = (700..710)
        .map(|job_id| {
            let validator = validator.clone();
            tokio::spawn(async move {
                validator
                    .validate_before_settlement(job_id)
                    .await
                    .unwrap()
            })
        })
        .collect();

    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await.unwrap());
    }

    // All should pass
    assert_eq!(results.len(), 10);
    assert!(results.iter().all(|&v| v), "All validations should pass");

    Ok(())
}

#[tokio::test]
async fn test_validator_caching() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    // Setup job
    let result = create_test_result(800, "Response");
    let proof = proof_gen.generate_proof(&result).await?;
    result_store.write().await.store_result(800, result).await?;
    proof_store.write().await.store_proof(800, proof).await?;

    // Validate twice - second should use cached data
    let is_valid1 = validator.validate_before_settlement(800).await?;
    let is_valid2 = validator.validate_before_settlement(800).await?;

    assert!(is_valid1);
    assert!(is_valid2);

    // Check store stats (2 retrievals = 2 hits)
    let proof_stats = proof_store.read().await.stats().await;
    assert_eq!(proof_stats.hits, 2);

    let result_stats = result_store.read().await.stats().await;
    assert_eq!(result_stats.hits, 2);

    Ok(())
}

#[tokio::test]
async fn test_validator_cleanup() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    // Setup and validate job
    let result = create_test_result(900, "Response");
    let proof = proof_gen.generate_proof(&result).await?;
    result_store.write().await.store_result(900, result).await?;
    proof_store.write().await.store_proof(900, proof).await?;

    let is_valid = validator.validate_before_settlement(900).await?;
    assert!(is_valid);

    // Cleanup after successful settlement
    validator.cleanup_job(900).await?;

    // Data should be removed
    assert!(!proof_store.read().await.has_proof(900).await);
    assert!(!result_store.read().await.has_result(900).await);

    Ok(())
}
