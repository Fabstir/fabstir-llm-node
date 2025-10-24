// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! EZKL Load and Performance Tests
//!
//! Tests system performance under load:
//! - High-throughput proof generation
//! - Concurrent validation stress tests
//! - Memory pressure scenarios
//! - Performance degradation detection
//! - Scalability testing

use anyhow::Result;
use chrono::Utc;
use fabstir_llm_node::results::packager::{InferenceResult, ResultMetadata};
use fabstir_llm_node::results::proofs::{ProofGenerationConfig, ProofGenerator, ProofType};
use fabstir_llm_node::settlement::validator::SettlementValidator;
use fabstir_llm_node::storage::{ProofStore, ResultStore};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

fn create_test_result(job_id: u64, size: usize) -> InferenceResult {
    let response = "x".repeat(size); // Variable size response
    InferenceResult {
        job_id: job_id.to_string(),
        model_id: "test-model".to_string(),
        prompt: "Test prompt".to_string(),
        response,
        tokens_generated: (size / 4) as u32, // Approximate tokens
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
async fn test_load_sequential_proof_generation() -> Result<()> {
    println!("\n=== Load Test: Sequential Proof Generation ===\n");

    let proof_gen = create_proof_generator();
    let count = 50;

    println!("🔄 Generating {} proofs sequentially...", count);
    let start = Instant::now();
    let mut proof_times = Vec::new();

    for job_id in 0..count {
        let result = create_test_result(job_id, 1000); // 1KB response
        let proof_start = Instant::now();
        let proof = proof_gen.generate_proof(&result).await?;
        let proof_time = proof_start.elapsed();
        proof_times.push(proof_time);

        assert!(!proof.proof_data.is_empty());
    }

    let total_time = start.elapsed();
    let avg_time = total_time / count as u32;

    // Calculate p50, p95, p99
    proof_times.sort();
    let p50 = proof_times[(count / 2) as usize];
    let p95 = proof_times[((count * 95) / 100) as usize];
    let p99 = proof_times[((count * 99) / 100) as usize];

    println!("\n📊 Performance Metrics:");
    println!("   - Total time: {:?}", total_time);
    println!("   - Avg time: {:?}", avg_time);
    println!("   - p50: {:?}", p50);
    println!("   - p95: {:?}", p95);
    println!("   - p99: {:?}", p99);
    println!(
        "   - Throughput: {:.1} proofs/sec",
        count as f64 / total_time.as_secs_f64()
    );

    // Performance targets (mock EZKL should be fast)
    assert!(
        avg_time < Duration::from_millis(100),
        "Avg proof time should be < 100ms"
    );
    assert!(p95 < Duration::from_millis(200), "p95 should be < 200ms");

    Ok(())
}

#[tokio::test]
async fn test_load_concurrent_proof_generation() -> Result<()> {
    println!("\n=== Load Test: Concurrent Proof Generation ===\n");

    let proof_gen = create_proof_generator();
    let count = 100;
    let concurrency = 20;

    println!(
        "🚀 Generating {} proofs with {} concurrent tasks...",
        count, concurrency
    );
    let start = Instant::now();

    let mut handles = Vec::new();
    for job_id in 0..count {
        let proof_gen = proof_gen.clone();
        let handle = tokio::spawn(async move {
            let result = create_test_result(job_id, 1000);
            let proof_start = Instant::now();
            let proof = proof_gen.generate_proof(&result).await.unwrap();
            (proof_start.elapsed(), proof.proof_data.len())
        });
        handles.push(handle);

        // Limit concurrency
        if handles.len() >= concurrency {
            let (time, size) = handles.remove(0).await.unwrap();
            assert!(size > 0);
            let _ = time; // Use time to avoid warning
        }
    }

    // Wait for remaining
    for handle in handles {
        let (time, size) = handle.await.unwrap();
        assert!(size > 0);
        let _ = time;
    }

    let total_time = start.elapsed();
    let throughput = count as f64 / total_time.as_secs_f64();

    println!("\n📊 Concurrent Performance:");
    println!("   - Total time: {:?}", total_time);
    println!("   - Throughput: {:.1} proofs/sec", throughput);
    println!("   - Concurrency: {} tasks", concurrency);

    // Should be faster than sequential
    println!("✅ Concurrent generation complete");

    Ok(())
}

#[tokio::test]
async fn test_load_high_volume_validation() -> Result<()> {
    println!("\n=== Load Test: High Volume Validation ===\n");

    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));
    let count = 100;

    // Setup jobs
    println!("📦 Setting up {} jobs...", count);
    for job_id in 0..count {
        let result = create_test_result(job_id, 500);
        let proof = proof_gen.generate_proof(&result).await?;
        result_store
            .write()
            .await
            .store_result(job_id, result)
            .await?;
        proof_store.write().await.store_proof(job_id, proof).await?;
    }
    println!("✅ Setup complete");

    // Validate all
    println!("\n🔍 Validating {} jobs...", count);
    let validator = Arc::new(SettlementValidator::new(
        proof_gen.clone(),
        proof_store.clone(),
        result_store.clone(),
    ));

    let start = Instant::now();
    let mut handles = Vec::new();

    for job_id in 0..count {
        let validator = validator.clone();
        let handle = tokio::spawn(async move {
            let validation_start = Instant::now();
            let is_valid = validator.validate_before_settlement(job_id).await.unwrap();
            (validation_start.elapsed(), is_valid)
        });
        handles.push(handle);
    }

    let mut validation_times = Vec::new();
    for handle in handles {
        let (time, is_valid) = handle.await.unwrap();
        assert!(is_valid);
        validation_times.push(time);
    }

    let total_time = start.elapsed();
    validation_times.sort();

    let p50 = validation_times[(count / 2) as usize];
    let p95 = validation_times[((count * 95) / 100) as usize];
    let throughput = count as f64 / total_time.as_secs_f64();

    println!("\n📊 Validation Performance:");
    println!("   - Total time: {:?}", total_time);
    println!("   - p50: {:?}", p50);
    println!("   - p95: {:?}", p95);
    println!("   - Throughput: {:.1} validations/sec", throughput);

    // Check metrics
    let metrics = validator.metrics();
    println!("\n📊 Validator Metrics:");
    println!("   - Total: {}", metrics.validations_total());
    println!("   - Passed: {}", metrics.validations_passed());
    println!(
        "   - Success rate: {:.1}%",
        metrics.validation_success_rate()
    );
    println!("   - Avg time: {:.2}ms", metrics.avg_validation_ms());

    assert_eq!(metrics.validations_total(), count as u64);
    assert_eq!(metrics.validations_passed(), count as u64);

    Ok(())
}

#[tokio::test]
async fn test_load_memory_pressure() -> Result<()> {
    println!("\n=== Load Test: Memory Pressure ===\n");

    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    // Store large number of jobs
    let count = 500;
    println!("📦 Storing {} jobs (simulating memory pressure)...", count);

    for job_id in 0..count {
        let result = create_test_result(job_id, 2000); // 2KB each
        let proof = proof_gen.generate_proof(&result).await?;
        result_store
            .write()
            .await
            .store_result(job_id, result)
            .await?;
        proof_store.write().await.store_proof(job_id, proof).await?;
    }

    // Check store stats
    let proof_stats = proof_store.read().await.stats().await;
    let result_stats = result_store.read().await.stats().await;

    println!("\n📊 Store Statistics:");
    println!("   - Proofs stored: {}", proof_stats.total_proofs);
    println!(
        "   - Proof storage: {:.2} MB",
        proof_stats.total_size_bytes as f64 / 1_000_000.0
    );
    println!("   - Results stored: {}", result_stats.total_results);
    println!("   - Total tokens: {}", result_stats.total_tokens);

    assert_eq!(proof_stats.total_proofs, count as usize);
    assert_eq!(result_stats.total_results, count as usize);

    // Simulate cleanup under pressure
    println!("\n🧹 Cleaning up old jobs...");
    let validator =
        SettlementValidator::new(proof_gen.clone(), proof_store.clone(), result_store.clone());

    // Cleanup first 250 jobs
    for job_id in 0..(count / 2) {
        validator.cleanup_job(job_id).await?;
    }

    let proof_stats_after = proof_store.read().await.stats().await;
    let result_stats_after = result_store.read().await.stats().await;

    println!("📊 After Cleanup:");
    println!(
        "   - Proofs: {} (was {})",
        proof_stats_after.total_proofs, proof_stats.total_proofs
    );
    println!(
        "   - Results: {} (was {})",
        result_stats_after.total_results, result_stats.total_results
    );

    assert_eq!(proof_stats_after.total_proofs, (count / 2) as usize);
    assert_eq!(result_stats_after.total_results, (count / 2) as usize);

    println!("✅ Memory pressure handling successful");

    Ok(())
}

#[tokio::test]
async fn test_load_variable_proof_sizes() -> Result<()> {
    println!("\n=== Load Test: Variable Proof Sizes ===\n");

    let proof_gen = create_proof_generator();
    let sizes = vec![100, 500, 1000, 5000, 10000]; // bytes

    println!("📏 Testing proof generation with variable sizes...\n");

    for size in sizes {
        let result = create_test_result(0, size);

        let start = Instant::now();
        let proof = proof_gen.generate_proof(&result).await?;
        let time = start.elapsed();

        println!(
            "   Input size: {:>5} bytes → Proof: {:>4} bytes in {:>6.2}ms",
            size,
            proof.proof_data.len(),
            time.as_secs_f64() * 1000.0
        );

        assert!(!proof.proof_data.is_empty());
    }

    println!("\n✅ Variable size handling successful");

    Ok(())
}

#[tokio::test]
async fn test_load_burst_traffic() -> Result<()> {
    println!("\n=== Load Test: Burst Traffic ===\n");

    let proof_gen = create_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    // Simulate 3 bursts of traffic
    let burst_size = 50;
    let num_bursts = 3;

    println!(
        "💥 Simulating {} bursts of {} jobs each...\n",
        num_bursts, burst_size
    );

    for burst in 0..num_bursts {
        let start_id = burst * burst_size;
        let end_id = start_id + burst_size;

        println!(
            "   Burst {} (jobs {}-{})...",
            burst + 1,
            start_id,
            end_id - 1
        );
        let start = Instant::now();

        // Process burst concurrently
        let handles: Vec<_> = (start_id..end_id)
            .map(|job_id| {
                let proof_gen = proof_gen.clone();
                let proof_store = proof_store.clone();
                let result_store = result_store.clone();

                tokio::spawn(async move {
                    let result = create_test_result(job_id, 1000);
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

        let burst_time = start.elapsed();
        println!(
            "      ✅ Completed in {:?} ({:.1} jobs/sec)",
            burst_time,
            burst_size as f64 / burst_time.as_secs_f64()
        );

        // Brief pause between bursts
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Verify all data stored
    let proof_stats = proof_store.read().await.stats().await;
    let result_stats = result_store.read().await.stats().await;

    println!("\n📊 Final Statistics:");
    println!("   - Total proofs: {}", proof_stats.total_proofs);
    println!("   - Total results: {}", result_stats.total_results);

    assert_eq!(proof_stats.total_proofs, (burst_size * num_bursts) as usize);
    assert_eq!(
        result_stats.total_results,
        (burst_size * num_bursts) as usize
    );

    println!("✅ Burst traffic handling successful");

    Ok(())
}

#[tokio::test]
async fn test_load_sustained_throughput() -> Result<()> {
    println!("\n=== Load Test: Sustained Throughput ===\n");

    let proof_gen = create_proof_generator();
    let duration = Duration::from_secs(5);
    let concurrency = 10;

    println!(
        "⏱️  Running sustained load for {:?} with {} concurrent tasks...",
        duration, concurrency
    );

    let start = Instant::now();
    let mut job_id = 0u64;
    let mut completed = 0;

    while start.elapsed() < duration {
        let mut handles = Vec::new();

        for _ in 0..concurrency {
            let proof_gen = proof_gen.clone();
            let current_id = job_id;
            job_id += 1;

            let handle = tokio::spawn(async move {
                let result = create_test_result(current_id, 1000);
                proof_gen.generate_proof(&result).await.unwrap();
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await.unwrap();
            completed += 1;
        }
    }

    let total_time = start.elapsed();
    let throughput = completed as f64 / total_time.as_secs_f64();

    println!("\n📊 Sustained Performance:");
    println!("   - Duration: {:?}", total_time);
    println!("   - Jobs completed: {}", completed);
    println!("   - Throughput: {:.1} jobs/sec", throughput);
    println!("   - Avg time per job: {:.2}ms", 1000.0 / throughput);

    println!("✅ Sustained throughput test complete");

    Ok(())
}
