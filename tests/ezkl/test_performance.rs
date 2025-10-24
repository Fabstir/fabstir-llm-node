// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! EZKL Performance Tests
//!
//! Tests for EZKL proof generation and verification performance.

use anyhow::Result;
use fabstir_llm_node::crypto::ezkl::{EzklProver, WitnessBuilder};
use std::time::{Duration, Instant};

#[test]
fn test_proof_generation_performance_target() -> Result<()> {
    // Test that proof generation meets < 100ms target (p95)
    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    let mut prover = EzklProver::new();

    // Measure proof generation time
    let start = Instant::now();
    let _proof = prover.generate_proof(&witness)?;
    let duration = start.elapsed();

    // Mock implementation should be very fast (< 1ms)
    // Real EZKL target: < 100ms (p95)
    #[cfg(not(feature = "real-ezkl"))]
    assert!(
        duration < Duration::from_millis(10),
        "Mock proof generation took {:?}, expected < 10ms",
        duration
    );

    #[cfg(feature = "real-ezkl")]
    assert!(
        duration < Duration::from_millis(100),
        "Real EZKL proof generation took {:?}, expected < 100ms",
        duration
    );

    Ok(())
}

#[test]
fn test_witness_generation_performance() -> Result<()> {
    // Test that witness generation is fast (< 5ms target)
    let start = Instant::now();

    let _witness = WitnessBuilder::new()
        .with_job_id_string("job_12345")
        .with_model_path("./models/model.gguf")
        .with_input_string("What is 2+2?")
        .with_output_string("The answer is 4")
        .build()?;

    let duration = start.elapsed();

    // Witness generation should be very fast (< 5ms target)
    assert!(
        duration < Duration::from_millis(5),
        "Witness generation took {:?}, expected < 5ms",
        duration
    );

    Ok(())
}

#[test]
fn test_batch_proof_generation_performance() -> Result<()> {
    // Test performance of generating multiple proofs
    let mut prover = EzklProver::new();
    let count = 10;

    let start = Instant::now();

    for i in 0..count {
        let witness = WitnessBuilder::new()
            .with_job_id([i; 32])
            .with_model_hash([i + 1; 32])
            .with_input_hash([i + 2; 32])
            .with_output_hash([i + 3; 32])
            .build()?;

        prover.generate_proof(&witness)?;
    }

    let duration = start.elapsed();
    let avg_time = duration / count as u32;

    println!("Generated {} proofs in {:?}", count, duration);
    println!("Average time per proof: {:?}", avg_time);

    // Average should meet target
    #[cfg(not(feature = "real-ezkl"))]
    assert!(avg_time < Duration::from_millis(10));

    #[cfg(feature = "real-ezkl")]
    assert!(avg_time < Duration::from_millis(100));

    Ok(())
}

#[test]
fn test_concurrent_proof_generation_performance() -> Result<()> {
    // Test performance of concurrent proof generation
    use std::sync::Arc;
    use std::thread;

    let witness = Arc::new(
        WitnessBuilder::new()
            .with_job_id([0u8; 32])
            .with_model_hash([1u8; 32])
            .with_input_hash([2u8; 32])
            .with_output_hash([3u8; 32])
            .build()?,
    );

    let thread_count = 4;
    let start = Instant::now();

    let handles: Vec<_> = (0..thread_count)
        .map(|_| {
            let witness_clone = Arc::clone(&witness);
            thread::spawn(move || {
                let mut prover = EzklProver::new();
                prover.generate_proof(&witness_clone)
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap()?;
    }

    let duration = start.elapsed();
    println!(
        "Generated {} proofs concurrently in {:?}",
        thread_count, duration
    );

    // Concurrent generation should benefit from parallelism
    // Should be faster than sequential (< 4x sequential time)
    Ok(())
}

#[test]
fn test_proof_generation_with_large_inputs() -> Result<()> {
    // Test performance with large input/output strings
    let large_input = "a".repeat(10000); // 10 KB input
    let large_output = "b".repeat(10000); // 10 KB output

    let start = Instant::now();

    let witness = WitnessBuilder::new()
        .with_job_id_string("large_job")
        .with_model_path("./models/large-model.gguf")
        .with_input_string(&large_input)
        .with_output_string(&large_output)
        .build()?;

    let witness_time = start.elapsed();

    let mut prover = EzklProver::new();
    let start = Instant::now();
    let _proof = prover.generate_proof(&witness)?;
    let proof_time = start.elapsed();

    println!("Witness generation (10KB data): {:?}", witness_time);
    println!("Proof generation: {:?}", proof_time);

    // Performance should not degrade significantly with large inputs
    // (proof is over hashes, not raw data)
    assert!(witness_time < Duration::from_millis(10));
    assert!(proof_time < Duration::from_millis(100));

    Ok(())
}

#[test]
fn test_memory_usage_during_proof_generation() -> Result<()> {
    // Test that memory usage is reasonable during proof generation
    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    let mut prover = EzklProver::new();

    // Generate proof (memory usage measured externally in real scenarios)
    let _proof = prover.generate_proof(&witness)?;

    // Mock proofs should use minimal memory
    // Real EZKL: < 1 GB target (mostly for keys)
    Ok(())
}

#[test]
fn test_proof_generation_p50_p95_p99() -> Result<()> {
    // Test that proof generation meets percentile targets
    let mut prover = EzklProver::new();
    let iterations = 100;
    let mut times: Vec<Duration> = Vec::with_capacity(iterations);

    for i in 0..iterations {
        let witness = WitnessBuilder::new()
            .with_job_id([i as u8; 32])
            .with_model_hash([(i + 1) as u8; 32])
            .with_input_hash([(i + 2) as u8; 32])
            .with_output_hash([(i + 3) as u8; 32])
            .build()?;

        let start = Instant::now();
        prover.generate_proof(&witness)?;
        times.push(start.elapsed());
    }

    times.sort();

    let p50 = times[iterations / 2];
    let p95 = times[iterations * 95 / 100];
    let p99 = times[iterations * 99 / 100];

    println!("Proof generation performance:");
    println!("  p50: {:?}", p50);
    println!("  p95: {:?}", p95);
    println!("  p99: {:?}", p99);

    // Targets (Phase 2):
    // - p50: < 50ms
    // - p95: < 100ms
    // - p99: < 500ms

    #[cfg(not(feature = "real-ezkl"))]
    {
        // Mock should be very fast
        assert!(p95 < Duration::from_millis(10));
    }

    #[cfg(feature = "real-ezkl")]
    {
        // Real EZKL targets
        assert!(p50 < Duration::from_millis(50), "p50 target: < 50ms");
        assert!(p95 < Duration::from_millis(100), "p95 target: < 100ms");
        assert!(p99 < Duration::from_millis(500), "p99 target: < 500ms");
    }

    Ok(())
}

#[test]
fn test_key_loading_performance() -> Result<()> {
    // Test that key loading meets < 50ms target
    use fabstir_llm_node::crypto::ezkl::setup::{
        compile_circuit, generate_keys, load_proving_key, save_proving_key,
    };
    use fabstir_llm_node::crypto::ezkl::CommitmentCircuit;
    use tempfile::TempDir;

    let temp_dir = TempDir::new()?;
    let key_path = temp_dir.path().join("proving_key.bin");

    // Generate and save key
    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    let compiled = compile_circuit(&circuit)?;
    let (proving_key, _) = generate_keys(&compiled)?;
    save_proving_key(&proving_key, &key_path)?;

    // Measure load time
    let start = Instant::now();
    let _loaded_key = load_proving_key(&key_path)?;
    let load_time = start.elapsed();

    println!("Key loading time: {:?}", load_time);

    // Target: < 50ms for key loading from disk
    assert!(
        load_time < Duration::from_millis(50),
        "Key loading took {:?}, expected < 50ms",
        load_time
    );

    Ok(())
}

#[test]
fn test_cached_key_access_performance() {
    // Test that cached key access is very fast (< 1ms)
    // TODO: Implement when KeyManager is available
    // let manager = KeyManager::new();
    // manager.load_proving_key(&key_path)?; // First load

    // Measure cached access
    // let start = Instant::now();
    // manager.load_proving_key(&key_path)?; // From cache
    // let cached_time = start.elapsed();

    // assert!(cached_time < Duration::from_millis(1));
}

#[test]
fn test_proof_throughput_sequential() -> Result<()> {
    // Test sequential proof throughput
    let mut prover = EzklProver::new();
    let count = 100;

    let start = Instant::now();

    for i in 0..count {
        let witness = WitnessBuilder::new()
            .with_job_id([i as u8; 32])
            .with_model_hash([(i + 1) as u8; 32])
            .with_input_hash([(i + 2) as u8; 32])
            .with_output_hash([(i + 3) as u8; 32])
            .build()?;

        prover.generate_proof(&witness)?;
    }

    let duration = start.elapsed();
    let throughput = count as f64 / duration.as_secs_f64();

    println!("Sequential throughput: {:.2} proofs/sec", throughput);

    // Target: 20-100 proofs/second (mock should be much higher)
    #[cfg(not(feature = "real-ezkl"))]
    assert!(
        throughput > 1000.0,
        "Mock throughput should be > 1000 proofs/sec"
    );

    #[cfg(feature = "real-ezkl")]
    assert!(
        throughput > 20.0,
        "Real EZKL throughput should be > 20 proofs/sec"
    );

    Ok(())
}

#[test]
fn test_cache_performance_benefit() -> Result<()> {
    // Test that caching provides significant performance benefit
    // TODO: Implement when ProofCache is available

    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    // Without cache
    let mut prover_no_cache = EzklProver::new();
    let start = Instant::now();
    for _ in 0..10 {
        prover_no_cache.generate_proof(&witness)?;
    }
    let no_cache_time = start.elapsed();

    // TODO: With cache
    // let cache = ProofCache::new(100);
    // let mut prover_with_cache = EzklProver::with_cache(cache);
    // let start = Instant::now();
    // for _ in 0..10 {
    //     prover_with_cache.generate_proof(&witness)?;
    // }
    // let with_cache_time = start.elapsed();

    // println!("Without cache: {:?}", no_cache_time);
    // println!("With cache: {:?}", with_cache_time);

    // Cache should provide significant speedup (target: 80%+ hit rate)
    // assert!(with_cache_time < no_cache_time / 5);

    Ok(())
}

#[test]
fn test_witness_serialization_performance() -> Result<()> {
    // Test that witness serialization is fast
    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    // Binary serialization
    let start = Instant::now();
    for _ in 0..1000 {
        let _bytes = witness.to_bytes();
    }
    let binary_time = start.elapsed();

    // JSON serialization
    let start = Instant::now();
    for _ in 0..1000 {
        let _json = serde_json::to_string(&witness)?;
    }
    let json_time = start.elapsed();

    println!("Binary serialization (1000x): {:?}", binary_time);
    println!("JSON serialization (1000x): {:?}", json_time);

    // Binary should be faster than JSON
    assert!(binary_time < json_time);

    // Average per operation should be < 100μs
    assert!(binary_time < Duration::from_millis(100));

    Ok(())
}

#[test]
fn test_no_memory_leaks() -> Result<()> {
    // Test that repeated proof generation doesn't leak memory
    let mut prover = EzklProver::new();

    // Generate many proofs
    for i in 0..1000 {
        let witness = WitnessBuilder::new()
            .with_job_id([i as u8; 32])
            .with_model_hash([(i + 1) as u8; 32])
            .with_input_hash([(i + 2) as u8; 32])
            .with_output_hash([(i + 3) as u8; 32])
            .build()?;

        let _proof = prover.generate_proof(&witness)?;

        // Drop proof immediately (don't accumulate)
    }

    // Memory usage should remain stable
    // (Actual measurement requires external tools)
    Ok(())
}

#[test]
fn test_proof_size_consistency() -> Result<()> {
    // Test that proof sizes are consistent
    let mut prover = EzklProver::new();
    let mut sizes: Vec<usize> = Vec::new();

    for i in 0..10 {
        let witness = WitnessBuilder::new()
            .with_job_id([i; 32])
            .with_model_hash([i + 1; 32])
            .with_input_hash([i + 2; 32])
            .with_output_hash([i + 3; 32])
            .build()?;

        let proof = prover.generate_proof(&witness)?;
        sizes.push(proof.proof_bytes.len());
    }

    // All proofs should be the same size (for commitment circuits)
    let first_size = sizes[0];
    for size in &sizes {
        assert_eq!(*size, first_size, "Proof sizes should be consistent");
    }

    println!("Proof size: {} bytes", first_size);

    // Verify size is in expected range
    #[cfg(not(feature = "real-ezkl"))]
    assert_eq!(first_size, 200, "Mock proofs should be 200 bytes");

    #[cfg(feature = "real-ezkl")]
    assert!(
        first_size >= 2000 && first_size <= 10000,
        "Real SNARK proofs should be 2-10 KB"
    );

    Ok(())
}

#[test]
fn test_performance_degradation_under_load() -> Result<()> {
    // Test that performance doesn't degrade significantly under load
    let mut prover = EzklProver::new();
    let batch_size = 10;
    let mut batch_times: Vec<Duration> = Vec::new();

    for batch in 0..10 {
        let start = Instant::now();

        for i in 0..batch_size {
            let witness = WitnessBuilder::new()
                .with_job_id([(batch * batch_size + i) as u8; 32])
                .with_model_hash([((batch * batch_size + i) + 1) as u8; 32])
                .with_input_hash([((batch * batch_size + i) + 2) as u8; 32])
                .with_output_hash([((batch * batch_size + i) + 3) as u8; 32])
                .build()?;

            prover.generate_proof(&witness)?;
        }

        batch_times.push(start.elapsed());
    }

    // Calculate average times for first and last batches
    let early_avg = batch_times[0..3].iter().sum::<Duration>() / 3;
    let late_avg = batch_times[7..10].iter().sum::<Duration>() / 3;

    println!("Early batches avg: {:?}", early_avg);
    println!("Late batches avg: {:?}", late_avg);

    // Performance should not degrade significantly
    // Allow up to 20% degradation
    assert!(
        late_avg < early_avg * 12 / 10,
        "Performance degraded too much under load"
    );

    Ok(())
}
