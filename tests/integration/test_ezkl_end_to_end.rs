// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! EZKL End-to-End Integration Tests
//!
//! Comprehensive tests for full system integration:
//! - Complete inference → proof → checkpoint → settlement flow
//! - Multi-job concurrent processing
//! - Store persistence and retrieval
//! - Metrics and monitoring integration
//! - Error propagation across components

use anyhow::Result;
use chrono::Utc;
use fabstir_llm_node::results::packager::{InferenceResult, ResultMetadata};
use fabstir_llm_node::results::proofs::{ProofGenerationConfig, ProofGenerator, ProofType};
use fabstir_llm_node::settlement::validator::SettlementValidator;
use fabstir_llm_node::storage::{ProofStore, ResultStore};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

fn create_inference_result(job_id: u64, model: &str, prompt: &str, response: &str, tokens: u32) -> InferenceResult {
    InferenceResult {
        job_id: job_id.to_string(),
        model_id: model.to_string(),
        prompt: prompt.to_string(),
        response: response.to_string(),
        tokens_generated: tokens,
        inference_time_ms: 150,
        timestamp: Utc::now(),
        node_id: "test-host-1".to_string(),
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
        model_path: "/models/test-model".to_string(),
        settings_path: None,
        max_proof_size: 10000,
    };
    Arc::new(ProofGenerator::new(config, "test-host-1".to_string()))
}

#[tokio::test]
async fn test_e2e_single_job_complete_flow() -> Result<()> {
    println!("\n=== E2E Test: Single Job Complete Flow ===\n");

    // Step 1: Inference completes
    println!("1️⃣  Simulating inference completion...");
    let job_id = 1000u64;
    let result = create_inference_result(
        job_id,
        "gpt-test",
        "Explain quantum computing",
        "Quantum computing uses quantum mechanics to process information...",
        150,
    );
    println!("✅ Inference complete: {} tokens generated", result.tokens_generated);

    // Step 2: Generate EZKL proof
    println!("\n2️⃣  Generating EZKL proof...");
    let proof_gen = create_proof_generator();
    let start = Instant::now();
    let proof = proof_gen.generate_proof(&result).await?;
    let proof_time = start.elapsed();
    println!("✅ Proof generated in {:?} ({} bytes)", proof_time, proof.proof_data.len());
    assert!(!proof.proof_data.is_empty(), "Proof data should not be empty");

    // Step 3: Store proof and result (checkpoint would do this)
    println!("\n3️⃣  Storing proof and result...");
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    result_store.write().await.store_result(job_id, result.clone()).await?;
    proof_store.write().await.store_proof(job_id, proof.clone()).await?;
    println!("✅ Data stored successfully");

    // Step 4: Validate before settlement
    println!("\n4️⃣  Validating proof before settlement...");
    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    let start = Instant::now();
    let is_valid = validator.validate_before_settlement(job_id).await?;
    let validate_time = start.elapsed();
    println!("✅ Validation complete in {:?}: {}", validate_time, if is_valid { "VALID" } else { "INVALID" });
    assert!(is_valid, "Proof validation must pass");

    // Step 5: Check metrics
    println!("\n5️⃣  Checking validation metrics...");
    let metrics = validator.metrics();
    println!("📊 Metrics:");
    println!("   - Total validations: {}", metrics.validations_total());
    println!("   - Passed: {}", metrics.validations_passed());
    println!("   - Failed: {}", metrics.validations_failed());
    println!("   - Success rate: {:.1}%", metrics.validation_success_rate());
    println!("   - Avg duration: {:.2}ms", metrics.avg_validation_ms());

    assert_eq!(metrics.validations_total(), 1);
    assert_eq!(metrics.validations_passed(), 1);
    assert_eq!(metrics.validations_failed(), 0);

    // Step 6: Simulate settlement (payment release)
    println!("\n6️⃣  Simulating settlement...");
    println!("💰 Payment released: {} tokens × rate = settlement amount", result.tokens_generated);
    println!("✅ Settlement complete");

    // Step 7: Cleanup
    println!("\n7️⃣  Cleaning up post-settlement...");
    validator.cleanup_job(job_id).await?;
    assert!(!proof_store.read().await.has_proof(job_id).await);
    assert!(!result_store.read().await.has_result(job_id).await);
    println!("✅ Cleanup complete");

    println!("\n🎉 E2E Flow Complete: All steps passed!\n");
    Ok(())
}

#[tokio::test]
async fn test_e2e_multi_job_concurrent_flow() -> Result<()> {
    println!("\n=== E2E Test: Multi-Job Concurrent Flow ===\n");

    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    // Process 10 jobs concurrently
    println!("🚀 Processing 10 jobs concurrently...");
    let start = Instant::now();

    let handles: Vec<_> = (2000..2010u64)
        .map(|job_id| {
            let proof_gen = proof_gen.clone();
            let proof_store = proof_store.clone();
            let result_store = result_store.clone();

            tokio::spawn(async move {
                // Inference
                let result = create_inference_result(
                    job_id,
                    "gpt-test",
                    &format!("Prompt for job {}", job_id),
                    &format!("Response for job {}", job_id),
                    100 + (job_id % 10) as u32,
                );

                // Proof generation
                let proof = proof_gen.generate_proof(&result).await.unwrap();

                // Storage
                result_store.write().await.store_result(job_id, result).await.unwrap();
                proof_store.write().await.store_proof(job_id, proof).await.unwrap();

                job_id
            })
        })
        .collect();

    let mut completed_jobs = Vec::new();
    for handle in handles {
        completed_jobs.push(handle.await.unwrap());
    }

    let processing_time = start.elapsed();
    println!("✅ All {} jobs processed in {:?}", completed_jobs.len(), processing_time);
    println!("⚡ Avg time per job: {:?}", processing_time / completed_jobs.len() as u32);

    // Validate all jobs
    println!("\n🔍 Validating all jobs...");
    let validator = Arc::new(SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    ));

    let start = Instant::now();
    let validation_handles: Vec<_> = completed_jobs
        .iter()
        .map(|&job_id| {
            let validator = validator.clone();
            tokio::spawn(async move {
                validator.validate_before_settlement(job_id).await.unwrap()
            })
        })
        .collect();

    let mut validation_results = Vec::new();
    for handle in validation_handles {
        validation_results.push(handle.await.unwrap());
    }

    let validation_time = start.elapsed();
    println!("✅ All {} validations complete in {:?}", validation_results.len(), validation_time);
    println!("⚡ Avg validation time: {:?}", validation_time / validation_results.len() as u32);

    // Check all passed
    let all_valid = validation_results.iter().all(|&v| v);
    assert!(all_valid, "All validations must pass");
    println!("✅ All jobs passed validation");

    // Check metrics
    let metrics = validator.metrics();
    println!("\n📊 Final Metrics:");
    println!("   - Total validations: {}", metrics.validations_total());
    println!("   - Success rate: {:.1}%", metrics.validation_success_rate());
    println!("   - Avg duration: {:.2}ms", metrics.avg_validation_ms());

    assert_eq!(metrics.validations_total(), 10);
    assert_eq!(metrics.validations_passed(), 10);

    println!("\n🎉 Concurrent Flow Complete!\n");
    Ok(())
}

#[tokio::test]
async fn test_e2e_mixed_valid_invalid_jobs() -> Result<()> {
    println!("\n=== E2E Test: Mixed Valid/Invalid Jobs ===\n");

    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    // Job 3000: Valid
    println!("1️⃣  Processing valid job 3000...");
    let result_valid = create_inference_result(3000, "model-a", "prompt", "response", 100);
    let proof_valid = proof_gen.generate_proof(&result_valid).await?;
    result_store.write().await.store_result(3000, result_valid).await?;
    proof_store.write().await.store_proof(3000, proof_valid).await?;

    // Job 3001: Invalid (tampered output)
    println!("2️⃣  Processing invalid job 3001 (tampered)...");
    let result_original = create_inference_result(3001, "model-a", "prompt", "original", 100);
    let proof_tampered = proof_gen.generate_proof(&result_original).await?;
    let result_tampered = create_inference_result(3001, "model-a", "prompt", "tampered", 100);
    result_store.write().await.store_result(3001, result_tampered).await?;
    proof_store.write().await.store_proof(3001, proof_tampered).await?;

    // Job 3002: Invalid (missing proof)
    println!("3️⃣  Processing invalid job 3002 (missing proof)...");
    let result_missing = create_inference_result(3002, "model-a", "prompt", "response", 100);
    result_store.write().await.store_result(3002, result_missing).await?;
    // No proof stored!

    // Validate all
    println!("\n🔍 Validating all jobs...");
    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    // Job 3000 should pass
    let valid_result = validator.validate_before_settlement(3000).await?;
    assert!(valid_result, "Job 3000 should be valid");
    println!("✅ Job 3000: VALID");

    // Job 3001 should fail (tampered)
    let invalid_result_1 = validator.validate_before_settlement(3001).await?;
    assert!(!invalid_result_1, "Job 3001 should be invalid");
    println!("❌ Job 3001: INVALID (tampered)");

    // Job 3002 should error (missing proof)
    let invalid_result_2 = validator.validate_before_settlement(3002).await;
    assert!(invalid_result_2.is_err(), "Job 3002 should error");
    println!("❌ Job 3002: ERROR (missing proof)");

    // Check metrics
    let metrics = validator.metrics();
    println!("\n📊 Final Metrics:");
    println!("   - Total: {} (1 valid, 1 invalid, 1 error)", metrics.validations_total());
    println!("   - Passed: {}", metrics.validations_passed());
    println!("   - Failed: {}", metrics.validations_failed());
    println!("   - Success rate: {:.1}%", metrics.validation_success_rate());

    assert_eq!(metrics.validations_total(), 3);
    assert_eq!(metrics.validations_passed(), 1);
    assert_eq!(metrics.validations_failed(), 2);

    println!("\n🎉 Mixed Jobs Test Complete!\n");
    Ok(())
}

#[tokio::test]
async fn test_e2e_store_statistics_tracking() -> Result<()> {
    println!("\n=== E2E Test: Store Statistics Tracking ===\n");

    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    // Process multiple jobs
    println!("📦 Processing 5 jobs to populate stores...");
    for job_id in 4000..4005 {
        let result = create_inference_result(
            job_id,
            "model",
            "prompt",
            &format!("response {}", job_id),
            100,
        );
        let proof = proof_gen.generate_proof(&result).await?;

        result_store.write().await.store_result(job_id, result).await?;
        proof_store.write().await.store_proof(job_id, proof).await?;
    }

    // Check proof store stats
    let proof_stats = proof_store.read().await.stats().await;
    println!("\n📊 Proof Store Stats:");
    println!("   - Total proofs: {}", proof_stats.total_proofs);
    println!("   - Total size: {} bytes", proof_stats.total_size_bytes);
    println!("   - Hits: {}", proof_stats.hits);
    println!("   - Misses: {}", proof_stats.misses);
    assert_eq!(proof_stats.total_proofs, 5);

    // Check result store stats
    let result_stats = result_store.read().await.stats().await;
    println!("\n📊 Result Store Stats:");
    println!("   - Total results: {}", result_stats.total_results);
    println!("   - Total tokens: {}", result_stats.total_tokens);
    println!("   - Hits: {}", result_stats.hits);
    println!("   - Misses: {}", result_stats.misses);
    assert_eq!(result_stats.total_results, 5);
    assert_eq!(result_stats.total_tokens, 500); // 5 × 100 tokens

    // Validate (causes hits)
    println!("\n🔍 Performing validations (generates hits)...");
    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    for job_id in 4000..4005 {
        validator.validate_before_settlement(job_id).await?;
    }

    // Check updated stats
    let proof_stats_after = proof_store.read().await.stats().await;
    let result_stats_after = result_store.read().await.stats().await;

    println!("\n📊 Updated Stats After Validation:");
    println!("   - Proof hits: {} (was {})", proof_stats_after.hits, proof_stats.hits);
    println!("   - Result hits: {} (was {})", result_stats_after.hits, result_stats.hits);

    assert_eq!(proof_stats_after.hits, 5, "Should have 5 proof hits");
    assert_eq!(result_stats_after.hits, 5, "Should have 5 result hits");

    // Try non-existent job (generates miss)
    println!("\n🔍 Attempting validation of non-existent job...");
    let _ = validator.validate_before_settlement(9999).await;

    let proof_stats_final = proof_store.read().await.stats().await;
    let result_stats_final = result_store.read().await.stats().await;

    println!("📊 Final Stats After Miss:");
    println!("   - Proof misses: {}", proof_stats_final.misses);
    println!("   - Result misses: {}", result_stats_final.misses);

    assert!(proof_stats_final.misses > 0, "Should have proof miss");

    println!("\n🎉 Statistics Tracking Complete!\n");
    Ok(())
}

#[tokio::test]
async fn test_e2e_cleanup_workflow() -> Result<()> {
    println!("\n=== E2E Test: Cleanup Workflow ===\n");

    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    // Setup 3 jobs
    println!("📦 Setting up 3 jobs...");
    for job_id in 5000..5003 {
        let result = create_inference_result(job_id, "model", "prompt", "response", 100);
        let proof = proof_gen.generate_proof(&result).await?;
        result_store.write().await.store_result(job_id, result).await?;
        proof_store.write().await.store_proof(job_id, proof).await?;
    }

    let result_stats_before = result_store.read().await.stats().await;
    let proof_stats_before = proof_store.read().await.stats().await;
    println!("✅ Initial state: {} results, {} proofs",
        result_stats_before.total_results,
        proof_stats_before.total_proofs
    );

    // Validate and settle job 5000
    println!("\n💰 Settling job 5000...");
    let validator = SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    );

    validator.validate_before_settlement(5000).await?;
    validator.cleanup_job(5000).await?;

    let result_stats_after = result_store.read().await.stats().await;
    let proof_stats_after = proof_store.read().await.stats().await;
    println!("✅ After cleanup: {} results (was {}), {} proofs (was {})",
        result_stats_after.total_results, result_stats_before.total_results,
        proof_stats_after.total_proofs, proof_stats_before.total_proofs
    );

    assert_eq!(result_stats_after.total_results, 2);
    assert_eq!(proof_stats_after.total_proofs, 2);

    // Cleanup remaining jobs
    println!("\n🧹 Cleaning up remaining jobs...");
    for job_id in 5001..5003 {
        validator.cleanup_job(job_id).await?;
    }

    let result_stats_final = result_store.read().await.stats().await;
    let proof_stats_final = proof_store.read().await.stats().await;
    println!("✅ Final state: {} results, {} proofs (all cleaned)",
        result_stats_final.total_results,
        proof_stats_final.total_proofs
    );

    assert_eq!(result_stats_final.total_results, 0);
    assert_eq!(proof_stats_final.total_proofs, 0);

    println!("\n🎉 Cleanup Workflow Complete!\n");
    Ok(())
}
