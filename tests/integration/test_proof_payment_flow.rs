//! End-to-End Proof Payment Flow Integration Tests
//!
//! Tests the complete flow from inference → proof generation → checkpoint →
//! validation → payment settlement.

use anyhow::Result;
use chrono::Utc;
use fabstir_llm_node::results::packager::{InferenceResult, ResultMetadata};
use fabstir_llm_node::results::proofs::{ProofGenerationConfig, ProofGenerator, ProofType};
use fabstir_llm_node::settlement::validator::SettlementValidator;
use fabstir_llm_node::storage::{ProofStore, ResultStore};
use std::sync::Arc;
use tokio::sync::RwLock;

fn create_inference_result(job_id: u64, tokens: u32) -> InferenceResult {
    InferenceResult {
        job_id: job_id.to_string(),
        model_id: "gpt-test".to_string(),
        prompt: "Explain quantum computing".to_string(),
        response: "Quantum computing uses quantum mechanics...".to_string(),
        tokens_generated: tokens,
        inference_time_ms: 150,
        timestamp: Utc::now(),
        node_id: "host-node-1".to_string(),
        metadata: ResultMetadata {
            temperature: 0.7,
            max_tokens: 1000,
            top_p: 0.9,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
        },
    }
}

fn create_proof_generator() -> Arc<ProofGenerator> {
    let config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "/models/gpt-test".to_string(),
        settings_path: None,
        max_proof_size: 10000,
    };
    Arc::new(ProofGenerator::new(config, "host-node-1".to_string()))
}

#[tokio::test]
async fn test_full_inference_to_payment_flow() -> Result<()> {
    // 1. Inference completes
    let result = create_inference_result(1000, 250);

    // 2. Generate proof
    let proof_gen = create_proof_generator();
    let proof = proof_gen.generate_proof(&result).await?;
    assert!(!proof.proof_data.is_empty());

    // 3. Store result and proof (checkpoint would do this)
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    result_store.write().await.store_result(1000, result.clone()).await?;
    proof_store.write().await.store_proof(1000, proof.clone()).await?;

    // 4. Settlement validation
    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    let is_valid = validator.validate_before_settlement(1000).await?;
    assert!(is_valid, "Proof validation must pass for payment");

    // 5. Payment would be released here (mock)
    println!("✅ Payment released for job 1000 after successful validation");

    Ok(())
}

#[tokio::test]
async fn test_invalid_proof_prevents_payment() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    // 1. Original inference
    let original_result = create_inference_result(2000, 100);
    let proof = proof_gen.generate_proof(&original_result).await?;

    // 2. Store proof
    proof_store.write().await.store_proof(2000, proof).await?;

    // 3. Attacker tampers with result
    let tampered_result = InferenceResult {
        job_id: "2000".to_string(),
        model_id: "gpt-test".to_string(),
        prompt: "Explain quantum computing".to_string(),
        response: "MALICIOUS TAMPERED RESPONSE - trying to claim more payment".to_string(),
        tokens_generated: 1000, // Inflated!
        inference_time_ms: 150,
        timestamp: Utc::now(),
        node_id: "host-node-1".to_string(),
        metadata: ResultMetadata::default(),
    };

    result_store
        .write()
        .await
        .store_result(2000, tampered_result)
        .await?;

    // 4. Settlement validation should FAIL
    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    let is_valid = validator.validate_before_settlement(2000).await?;
    assert!(!is_valid, "Tampered result MUST fail validation");

    // 5. Payment blocked (settlement would check is_valid and abort)
    println!("❌ Payment BLOCKED for job 2000 - proof validation failed");

    Ok(())
}

#[tokio::test]
async fn test_missing_proof_prevents_payment() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    // 1. Result exists but proof is missing
    let result = create_inference_result(3000, 150);
    result_store.write().await.store_result(3000, result).await?;

    // 2. Try to settle without proof
    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    let validation_result = validator.validate_before_settlement(3000).await;

    // 3. Should error (missing proof)
    assert!(
        validation_result.is_err(),
        "Missing proof MUST prevent settlement"
    );

    println!("❌ Payment BLOCKED for job 3000 - proof not found");

    Ok(())
}

#[tokio::test]
async fn test_checkpoint_proof_payment_settlement() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    // Simulate checkpoint flow for job with multiple checkpoints
    let result = create_inference_result(4000, 300);

    // 1. Checkpoint 1: 100 tokens
    let proof1 = proof_gen.generate_proof(&result).await?;
    proof_store.write().await.store_proof(4000, proof1).await?;
    result_store.write().await.store_result(4000, result.clone()).await?;

    // 2. Checkpoint 2: 200 more tokens (cumulative)
    let proof2 = proof_gen.generate_proof(&result).await?;
    proof_store.write().await.store_proof(4000, proof2).await?;

    // 3. Final settlement
    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    let is_valid = validator.validate_before_settlement(4000).await?;
    assert!(is_valid);

    println!("✅ Multi-checkpoint payment released after validation");

    Ok(())
}

#[tokio::test]
async fn test_multi_checkpoint_proof_flow() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    // Simulate streaming inference with multiple checkpoints
    for checkpoint_id in 0..3 {
        let job_id = 5000 + checkpoint_id;
        let result = create_inference_result(job_id, 100 * (checkpoint_id as u32 + 1));

        // Generate proof for checkpoint
        let proof = proof_gen.generate_proof(&result).await?;

        // Store
        result_store.write().await.store_result(job_id, result).await?;
        proof_store.write().await.store_proof(job_id, proof).await?;
    }

    // Validate all checkpoints
    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    for checkpoint_id in 0..3 {
        let job_id = 5000 + checkpoint_id;
        let is_valid = validator.validate_before_settlement(job_id).await?;
        assert!(is_valid, "Checkpoint {} should be valid", checkpoint_id);
    }

    Ok(())
}

#[tokio::test]
async fn test_proof_validation_timeout() -> Result<()> {
    // Test that validation completes quickly (< 50ms)
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let result = create_inference_result(6000, 200);
    let proof = proof_gen.generate_proof(&result).await?;

    result_store.write().await.store_result(6000, result).await?;
    proof_store.write().await.store_proof(6000, proof).await?;

    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    let start = std::time::Instant::now();
    let is_valid = validator.validate_before_settlement(6000).await?;
    let duration = start.elapsed();

    assert!(is_valid);
    assert!(
        duration.as_millis() < 50,
        "Validation should be fast (< 50ms), was {}ms",
        duration.as_millis()
    );

    Ok(())
}

#[tokio::test]
async fn test_concurrent_job_settlements() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    // Setup 10 jobs
    for job_id in 7000..7010 {
        let result = create_inference_result(job_id, 150);
        let proof = proof_gen.generate_proof(&result).await?;

        result_store.write().await.store_result(job_id, result).await?;
        proof_store.write().await.store_proof(job_id, proof).await?;
    }

    let validator = Arc::new(SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    ));

    // Settle concurrently
    let handles: Vec<_> = (7000..7010)
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

    for handle in handles {
        let is_valid = handle.await.unwrap();
        assert!(is_valid);
    }

    Ok(())
}

#[tokio::test]
async fn test_settlement_with_chain_specific_proof() -> Result<()> {
    // Test proof validation works for multi-chain settlement
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    // Job on Base Sepolia (chain_id: 84532)
    let result_base = create_inference_result(8000, 200);
    let proof_base = proof_gen.generate_proof(&result_base).await?;

    result_store
        .write()
        .await
        .store_result(8000, result_base)
        .await?;
    proof_store
        .write()
        .await
        .store_proof(8000, proof_base)
        .await?;

    // Job on opBNB (chain_id: 5611)
    let result_opbnb = create_inference_result(8001, 250);
    let proof_opbnb = proof_gen.generate_proof(&result_opbnb).await?;

    result_store
        .write()
        .await
        .store_result(8001, result_opbnb)
        .await?;
    proof_store
        .write()
        .await
        .store_proof(8001, proof_opbnb)
        .await?;

    // Validate both
    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    let base_valid = validator.validate_before_settlement(8000).await?;
    let opbnb_valid = validator.validate_before_settlement(8001).await?;

    assert!(base_valid, "Base Sepolia settlement should validate");
    assert!(opbnb_valid, "opBNB settlement should validate");

    Ok(())
}

#[tokio::test]
async fn test_proof_storage_retrieval_e2e() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    // 1. Generate and store
    let result = create_inference_result(9000, 300);
    let proof = proof_gen.generate_proof(&result).await?;

    let proof_size = proof.proof_data.len();

    result_store.write().await.store_result(9000, result).await?;
    proof_store.write().await.store_proof(9000, proof).await?;

    // 2. Retrieve for validation
    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    let retrieved_proof = validator.retrieve_proof(9000).await?;
    assert_eq!(retrieved_proof.proof_data.len(), proof_size);

    let retrieved_result = validator.retrieve_result(9000).await?;
    assert_eq!(retrieved_result.job_id, "9000");

    // 3. Validate
    let is_valid = validator.validate_before_settlement(9000).await?;
    assert!(is_valid);

    // 4. Cleanup after settlement
    validator.cleanup_job(9000).await?;

    assert!(!proof_store.read().await.has_proof(9000).await);
    assert!(!result_store.read().await.has_result(9000).await);

    Ok(())
}

#[tokio::test]
async fn test_validator_error_recovery() -> Result<()> {
    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    // Attempt 1: Missing data (should error)
    let result1 = validator.validate_before_settlement(10000).await;
    assert!(result1.is_err());

    // Store data
    let result = create_inference_result(10000, 100);
    let proof = proof_gen.generate_proof(&result).await?;
    result_store
        .write()
        .await
        .store_result(10000, result)
        .await?;
    proof_store.write().await.store_proof(10000, proof).await?;

    // Attempt 2: Should succeed after data is available
    let result2 = validator.validate_before_settlement(10000).await?;
    assert!(result2, "Should recover and validate successfully");

    Ok(())
}
