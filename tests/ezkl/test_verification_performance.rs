// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Verification Performance Tests (Sub-phase 3.1)
//!
//! Performance benchmarks for EZKL proof verification.
//! Target: < 10ms per verification

use anyhow::Result;
use fabstir_llm_node::crypto::ezkl::{EzklProver, EzklVerifier, WitnessBuilder};
use std::time::{Duration, Instant};

/// Helper to create test witness
fn create_test_witness(seed: u8) -> fabstir_llm_node::crypto::ezkl::Witness {
    WitnessBuilder::new()
        .with_job_id([seed; 32])
        .with_model_hash([seed + 1; 32])
        .with_input_hash([seed + 2; 32])
        .with_output_hash([seed + 3; 32])
        .build()
        .unwrap()
}

#[test]
fn test_single_verification_latency() -> Result<()> {
    let witness = create_test_witness(0);

    let mut prover = EzklProver::new();
    let proof = prover
        .generate_proof(&witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut verifier = EzklVerifier::new();

    // Measure single verification
    let start = Instant::now();
    let is_valid = verifier
        .verify_proof(&proof, &witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let duration = start.elapsed();

    assert!(is_valid, "Proof should be valid");

    println!("Single verification latency: {:?}", duration);

    // Target: < 10ms for mock, will be higher for real EZKL
    #[cfg(not(feature = "real-ezkl"))]
    assert!(
        duration < Duration::from_millis(10),
        "Mock verification should be < 10ms, got {:?}",
        duration
    );

    #[cfg(feature = "real-ezkl")]
    assert!(
        duration < Duration::from_millis(50),
        "Real verification should be < 50ms, got {:?}",
        duration
    );

    Ok(())
}

#[test]
fn test_batch_verification_throughput() -> Result<()> {
    let batch_size = 100;
    let mut verifier = EzklVerifier::new();

    // Generate batch of proofs
    let mut proofs = Vec::new();
    let mut witnesses = Vec::new();

    for i in 0..batch_size {
        let witness = create_test_witness(i as u8);
        let mut prover = EzklProver::new();
        let proof = prover
            .generate_proof(&witness)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        witnesses.push(witness);
        proofs.push(proof);
    }

    // Measure batch verification
    let start = Instant::now();
    let mut valid_count = 0;

    for (proof, witness) in proofs.iter().zip(witnesses.iter()) {
        if verifier
            .verify_proof(proof, witness)
            .map_err(|e| anyhow::anyhow!("{}", e))?
        {
            valid_count += 1;
        }
    }

    let duration = start.elapsed();
    let avg_per_proof = duration / batch_size;

    assert_eq!(valid_count, batch_size, "All proofs should be valid");

    println!("Batch verification:");
    println!("  Total time: {:?}", duration);
    println!("  Per proof: {:?}", avg_per_proof);
    println!(
        "  Throughput: {:.2} verifications/sec",
        batch_size as f64 / duration.as_secs_f64()
    );

    Ok(())
}

#[test]
fn test_verification_percentiles() -> Result<()> {
    let iterations = 100;
    let mut times: Vec<Duration> = Vec::with_capacity(iterations);

    let witness = create_test_witness(42);
    let mut prover = EzklProver::new();
    let proof = prover
        .generate_proof(&witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut verifier = EzklVerifier::new();

    // Collect timing samples
    for _ in 0..iterations {
        let start = Instant::now();
        verifier
            .verify_proof(&proof, &witness)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        times.push(start.elapsed());
    }

    // Sort for percentile calculation
    times.sort();

    let p50 = times[iterations / 2];
    let p95 = times[(iterations * 95) / 100];
    let p99 = times[(iterations * 99) / 100];

    println!("Verification latency percentiles:");
    println!("  p50: {:?}", p50);
    println!("  p95: {:?}", p95);
    println!("  p99: {:?}", p99);

    // Target: p95 < 10ms for mock
    #[cfg(not(feature = "real-ezkl"))]
    assert!(
        p95 < Duration::from_millis(10),
        "p95 latency should be < 10ms, got {:?}",
        p95
    );

    Ok(())
}

#[test]
fn test_concurrent_verification_performance() -> Result<()> {
    use std::sync::{Arc, Mutex};
    use std::thread;

    let witness = create_test_witness(0);
    let mut prover = EzklProver::new();
    let proof = prover
        .generate_proof(&witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let witness_arc = Arc::new(witness);
    let proof_arc = Arc::new(proof);
    let times = Arc::new(Mutex::new(Vec::new()));

    let thread_count = 10;
    let verifications_per_thread = 10;

    let start = Instant::now();
    let mut handles = vec![];

    for _ in 0..thread_count {
        let witness_clone = Arc::clone(&witness_arc);
        let proof_clone = Arc::clone(&proof_arc);
        let times_clone = Arc::clone(&times);

        let handle = thread::spawn(move || {
            let mut verifier = EzklVerifier::new();
            let mut thread_times = Vec::new();

            for _ in 0..verifications_per_thread {
                let start = Instant::now();
                let _ = verifier.verify_proof(&proof_clone, &witness_clone);
                thread_times.push(start.elapsed());
            }

            let mut times_lock = times_clone.lock().unwrap();
            times_lock.extend(thread_times);
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let total_duration = start.elapsed();
    let all_times = times.lock().unwrap();
    let total_verifications = thread_count * verifications_per_thread;

    let avg_time: Duration = all_times.iter().sum::<Duration>() / all_times.len() as u32;
    let throughput = total_verifications as f64 / total_duration.as_secs_f64();

    println!("Concurrent verification ({} threads):", thread_count);
    println!("  Total time: {:?}", total_duration);
    println!("  Avg per verification: {:?}", avg_time);
    println!("  Throughput: {:.2} verifications/sec", throughput);

    Ok(())
}

#[test]
fn test_verification_key_loading_performance() -> Result<()> {
    // Measure cold start (first verification with key loading)
    let witness = create_test_witness(0);
    let mut prover = EzklProver::new();
    let proof = prover
        .generate_proof(&witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let start = Instant::now();
    let mut verifier = EzklVerifier::new();
    let _ = verifier.verify_proof(&proof, &witness);
    let cold_start = start.elapsed();

    // Measure warm (subsequent verifications with cached key)
    let start = Instant::now();
    let _ = verifier.verify_proof(&proof, &witness);
    let warm_time = start.elapsed();

    println!("Verification key loading:");
    println!("  Cold start: {:?}", cold_start);
    println!("  Warm (cached): {:?}", warm_time);

    // Warm should be faster or equal (key already loaded)
    assert!(
        warm_time <= cold_start,
        "Warm verification should not be slower than cold start"
    );

    Ok(())
}

#[test]
fn test_memory_usage_during_verification() -> Result<()> {
    use std::sync::Arc;

    let witness = create_test_witness(0);
    let mut prover = EzklProver::new();
    let proof = prover
        .generate_proof(&witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Verify multiple times and check no memory leaks
    let mut verifier = EzklVerifier::new();
    let iterations = 1000;

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = verifier.verify_proof(&proof, &witness);
    }
    let duration = start.elapsed();

    println!("Memory test ({} verifications): {:?}", iterations, duration);

    // Test should complete without excessive memory growth
    // Note: Actual memory measurement would require platform-specific APIs
    // This test ensures no obvious leaks cause slowdown

    let avg_time = duration / iterations;
    println!("  Avg per verification: {:?}", avg_time);

    Ok(())
}

#[test]
fn test_cache_hit_performance() -> Result<()> {
    // Test verification performance with key reuse
    let mut verifier = EzklVerifier::new();

    // First verification (potential key loading)
    let witness1 = create_test_witness(1);
    let mut prover1 = EzklProver::new();
    let proof1 = prover1
        .generate_proof(&witness1)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let start1 = Instant::now();
    verifier
        .verify_proof(&proof1, &witness1)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let time1 = start1.elapsed();

    // Second verification with same verifier (key cached)
    let witness2 = create_test_witness(2);
    let mut prover2 = EzklProver::new();
    let proof2 = prover2
        .generate_proof(&witness2)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let start2 = Instant::now();
    verifier
        .verify_proof(&proof2, &witness2)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let time2 = start2.elapsed();

    println!("Cache hit performance:");
    println!("  First verification: {:?}", time1);
    println!("  Second verification (cached): {:?}", time2);

    // Second verification should benefit from cached key
    assert!(
        time2 <= time1 * 2,
        "Cached verification should not be significantly slower"
    );

    Ok(())
}

#[test]
fn test_cold_start_vs_warm_cache() -> Result<()> {
    let witness = create_test_witness(0);
    let mut prover = EzklProver::new();
    let proof = prover
        .generate_proof(&witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Measure cold start (new verifier)
    let mut cold_times = Vec::new();
    for _ in 0..5 {
        let mut verifier = EzklVerifier::new();
        let start = Instant::now();
        verifier
            .verify_proof(&proof, &witness)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        cold_times.push(start.elapsed());
    }

    // Measure warm cache (reused verifier)
    let mut verifier = EzklVerifier::new();
    let mut warm_times = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        verifier
            .verify_proof(&proof, &witness)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        warm_times.push(start.elapsed());
    }

    let avg_cold: Duration = cold_times.iter().sum::<Duration>() / cold_times.len() as u32;
    let avg_warm: Duration = warm_times.iter().sum::<Duration>() / warm_times.len() as u32;

    println!("Cold start vs warm cache:");
    println!("  Avg cold start: {:?}", avg_cold);
    println!("  Avg warm cache: {:?}", avg_warm);
    println!(
        "  Speedup: {:.2}x",
        avg_cold.as_secs_f64() / avg_warm.as_secs_f64()
    );

    Ok(())
}
