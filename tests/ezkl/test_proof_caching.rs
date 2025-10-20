// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Proof Caching Tests
//!
//! Tests for EZKL proof caching with LRU eviction.

use anyhow::Result;
use fabstir_llm_node::crypto::ezkl::WitnessBuilder;
use std::time::Duration;

// TODO: Import cache types when implemented
// use fabstir_llm_node::crypto::ezkl::{ProofCache, CacheStats};

#[test]
fn test_proof_cache_creation() {
    // Test creating a new proof cache
    // TODO: Implement when ProofCache is available
    // let cache = ProofCache::new(100); // 100 entry capacity
    // assert_eq!(cache.capacity(), 100);
}

#[test]
fn test_cache_hit_on_repeated_witness() -> Result<()> {
    // Test that identical witness hits cache
    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    // TODO: Test caching behavior
    // let cache = ProofCache::new(100);
    // let mut prover = EzklProver::with_cache(cache);

    // First generation - cache miss
    // let proof1 = prover.generate_proof(&witness)?;

    // Second generation - cache hit
    // let proof2 = prover.generate_proof(&witness)?;

    // assert_eq!(proof1.proof_bytes, proof2.proof_bytes);
    // assert_eq!(cache.stats().hits, 1);
    // assert_eq!(cache.stats().misses, 1);

    Ok(())
}

#[test]
fn test_cache_miss_on_different_witness() -> Result<()> {
    // Test that different witnesses cause cache miss
    let witness1 = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    let witness2 = WitnessBuilder::new()
        .with_job_id([9u8; 32])
        .with_model_hash([8u8; 32])
        .with_input_hash([7u8; 32])
        .with_output_hash([6u8; 32])
        .build()?;

    // TODO: Test cache behavior
    // let cache = ProofCache::new(100);
    // let mut prover = EzklProver::with_cache(cache);

    // let proof1 = prover.generate_proof(&witness1)?;
    // let proof2 = prover.generate_proof(&witness2)?;

    // assert_ne!(proof1.proof_bytes, proof2.proof_bytes);
    // assert_eq!(cache.stats().hits, 0);
    // assert_eq!(cache.stats().misses, 2);

    Ok(())
}

#[test]
fn test_lru_eviction() -> Result<()> {
    // Test that LRU eviction works correctly
    // TODO: Create cache with capacity 2
    // let cache = ProofCache::new(2);
    // let mut prover = EzklProver::with_cache(cache);

    // Create 3 different witnesses
    let witness_a = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    let witness_b = WitnessBuilder::new()
        .with_job_id([4u8; 32])
        .with_model_hash([5u8; 32])
        .with_input_hash([6u8; 32])
        .with_output_hash([7u8; 32])
        .build()?;

    let witness_c = WitnessBuilder::new()
        .with_job_id([8u8; 32])
        .with_model_hash([9u8; 32])
        .with_input_hash([10u8; 32])
        .with_output_hash([11u8; 32])
        .build()?;

    // TODO: Test LRU eviction
    // prover.generate_proof(&witness_a)?; // Cache: [A]
    // prover.generate_proof(&witness_b)?; // Cache: [A, B]
    // prover.generate_proof(&witness_c)?; // Cache: [B, C] (A evicted)

    // Access A again - should be cache miss (was evicted)
    // prover.generate_proof(&witness_a)?; // Cache: [C, A] (B evicted)

    // Access C - should be cache hit
    // prover.generate_proof(&witness_c)?; // Cache: [A, C]

    // Verify statistics
    // assert_eq!(cache.stats().evictions, 2); // A and B were evicted

    Ok(())
}

#[test]
fn test_cache_size_limit() -> Result<()> {
    // Test that cache respects size limit
    // TODO: Create cache with small capacity
    // let cache = ProofCache::new(3);

    // Generate more proofs than capacity
    for i in 0..10 {
        let witness = WitnessBuilder::new()
            .with_job_id([i; 32])
            .with_model_hash([i + 1; 32])
            .with_input_hash([i + 2; 32])
            .with_output_hash([i + 3; 32])
            .build()?;

        // TODO: Generate proof
        // prover.generate_proof(&witness)?;
    }

    // TODO: Verify cache size doesn't exceed limit
    // assert_eq!(cache.len(), 3);
    // assert_eq!(cache.stats().evictions, 7); // 10 - 3

    Ok(())
}

#[test]
fn test_cache_hit_rate() -> Result<()> {
    // Test that cache hit rate is calculated correctly
    // TODO: Create cache
    // let cache = ProofCache::new(10);
    // let mut prover = EzklProver::with_cache(cache);

    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    // TODO: Generate proofs
    // prover.generate_proof(&witness)?; // Miss
    // prover.generate_proof(&witness)?; // Hit
    // prover.generate_proof(&witness)?; // Hit

    // let stats = cache.stats();
    // assert_eq!(stats.hits, 2);
    // assert_eq!(stats.misses, 1);
    // assert_eq!(stats.hit_rate(), 2.0 / 3.0);

    Ok(())
}

#[test]
fn test_cache_invalidation() -> Result<()> {
    // Test that cache can be invalidated
    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    // TODO: Test cache invalidation
    // let cache = ProofCache::new(10);
    // let mut prover = EzklProver::with_cache(cache);

    // prover.generate_proof(&witness)?; // Cache proof

    // cache.invalidate();

    // prover.generate_proof(&witness)?; // Should be miss (cache cleared)
    // assert_eq!(cache.stats().misses, 2); // Both were misses

    Ok(())
}

#[test]
fn test_cache_statistics() {
    // Test that cache provides detailed statistics
    // TODO: Create cache and generate various proofs
    // let cache = ProofCache::new(10);

    // TODO: Access cache multiple times
    // let stats = cache.stats();

    // assert_eq!(stats.hits, expected);
    // assert_eq!(stats.misses, expected);
    // assert_eq!(stats.evictions, expected);
    // assert_eq!(stats.total_requests(), hits + misses);
    // assert!(stats.hit_rate() >= 0.0 && stats.hit_rate() <= 1.0);
}

#[test]
fn test_concurrent_cache_access() -> Result<()> {
    // Test that cache is thread-safe
    use std::sync::Arc;
    use std::thread;

    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    // TODO: Test concurrent access
    // let cache = Arc::new(ProofCache::new(100));
    // let mut handles = vec![];

    // for _ in 0..10 {
    //     let cache_clone = Arc::clone(&cache);
    //     let witness_clone = witness.clone();
    //     let handle = thread::spawn(move || {
    //         let mut prover = EzklProver::with_cache(cache_clone);
    //         prover.generate_proof(&witness_clone)
    //     });
    //     handles.push(handle);
    // }

    // for handle in handles {
    //     handle.join().unwrap()?;
    // }

    // First request was miss, rest should be hits
    // assert!(cache.stats().hits >= 9);

    Ok(())
}

#[test]
fn test_cache_with_ttl() {
    // Test that cache entries can have TTL (time-to-live)
    // TODO: Create cache with TTL
    // let cache = ProofCache::with_ttl(10, Duration::from_secs(1));

    // TODO: Generate proof and wait for expiry
    // prover.generate_proof(&witness)?;
    // thread::sleep(Duration::from_secs(2));
    // prover.generate_proof(&witness)?; // Should be miss (expired)
}

#[test]
fn test_cache_memory_usage() -> Result<()> {
    // Test that cache tracks memory usage
    // TODO: Create cache
    // let cache = ProofCache::new(100);
    // let mut prover = EzklProver::with_cache(cache);

    // Generate multiple proofs
    for i in 0..10 {
        let witness = WitnessBuilder::new()
            .with_job_id([i; 32])
            .with_model_hash([i + 1; 32])
            .with_input_hash([i + 2; 32])
            .with_output_hash([i + 3; 32])
            .build()?;

        // TODO: Generate proof
        // prover.generate_proof(&witness)?;
    }

    // TODO: Check memory usage
    // let memory_bytes = cache.memory_usage_bytes();
    // assert!(memory_bytes > 0);
    // assert!(memory_bytes < 10_000_000); // < 10 MB for 10 proofs

    Ok(())
}

#[test]
fn test_cache_key_computation() -> Result<()> {
    // Test that cache keys are computed correctly from witness
    let witness1 = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    let witness2 = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    // TODO: Test cache key computation
    // let key1 = ProofCache::compute_key(&witness1);
    // let key2 = ProofCache::compute_key(&witness2);

    // Same witness should produce same key
    // assert_eq!(key1, key2);

    Ok(())
}

#[test]
fn test_cache_with_different_hash_collisions() -> Result<()> {
    // Test that cache handles hash collisions correctly
    // This is a corner case but important for correctness

    // TODO: Create witnesses that might produce cache key collisions
    // TODO: Verify that correct proofs are returned even with collisions

    Ok(())
}

#[test]
fn test_cache_warmup() -> Result<()> {
    // Test that cache can be warmed up with common witnesses
    // TODO: Create cache
    // let cache = ProofCache::new(10);

    // TODO: Warm up cache with common witnesses
    // cache.warmup(&[witness1, witness2, witness3])?;

    // TODO: Verify proofs are cached
    // assert_eq!(cache.len(), 3);

    Ok(())
}

#[test]
fn test_cache_persistence() {
    // Test that cache can be persisted to disk (optional feature)
    // TODO: Create cache
    // TODO: Generate proofs
    // TODO: Save cache to disk
    // TODO: Load cache from disk
    // TODO: Verify loaded cache has same entries
}

#[test]
fn test_cache_performance_benefit() -> Result<()> {
    // Test that cache provides performance benefit
    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    // TODO: Test performance difference
    // let cache = ProofCache::new(10);
    // let mut prover = EzklProver::with_cache(cache);

    // First generation - no cache
    // let start = std::time::Instant::now();
    // prover.generate_proof(&witness)?;
    // let uncached_time = start.elapsed();

    // Second generation - from cache
    // let start = std::time::Instant::now();
    // prover.generate_proof(&witness)?;
    // let cached_time = start.elapsed();

    // Cache should be significantly faster
    // assert!(cached_time < uncached_time / 10);

    Ok(())
}

#[test]
fn test_cache_with_metrics_integration() {
    // Test that cache integrates with metrics system
    // TODO: Create cache with metrics
    // TODO: Generate proofs
    // TODO: Verify metrics are recorded
    // assert!(metrics.ezkl_cache_hits > 0);
    // assert!(metrics.ezkl_cache_misses > 0);
}

#[test]
fn test_cache_entry_size_estimation() -> Result<()> {
    // Test that cache accurately estimates entry sizes
    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    // TODO: Test size estimation
    // let entry_size = ProofCache::estimate_entry_size(&witness);
    // Mock proofs are ~200 bytes, real proofs 2-10 KB
    // assert!(entry_size >= 200);

    Ok(())
}

#[test]
fn test_cache_with_zero_capacity() {
    // Test that cache with capacity 0 doesn't cache anything
    // TODO: Create cache with capacity 0
    // let cache = ProofCache::new(0);

    // TODO: Generate proofs
    // Every request should be a miss
    // assert_eq!(cache.stats().hits, 0);
}

#[test]
fn test_cache_clear_by_pattern() -> Result<()> {
    // Test that cache can clear entries matching a pattern
    // For example, clear all proofs for a specific model

    // TODO: Generate proofs with different models
    let witness1 = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    let witness2 = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([9u8; 32]) // Different model
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    // TODO: Clear entries for model [1u8; 32]
    // cache.clear_by_model_hash([1u8; 32]);

    // witness1 should be miss, witness2 should be hit
    Ok(())
}
