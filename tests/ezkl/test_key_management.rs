// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Key Management Tests
//!
//! Tests for EZKL proving and verification key loading, caching, and validation.

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

// TODO: Import key manager types when implemented
// use fabstir_llm_node::crypto::ezkl::{KeyManager, KeyCache};

#[test]
fn test_key_manager_creation() {
    // Test creating a new key manager
    // TODO: Implement when KeyManager is available
    // let manager = KeyManager::new();
    // assert!(manager is created successfully);
}

#[test]
fn test_load_proving_key_from_file() -> Result<()> {
    // Test loading proving key from file
    let temp_dir = TempDir::new()?;
    let key_path = temp_dir.path().join("proving_key.bin");

    // Create a mock proving key file
    use fabstir_llm_node::crypto::ezkl::setup::{generate_keys, compile_circuit, save_proving_key};
    use fabstir_llm_node::crypto::ezkl::CommitmentCircuit;

    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    let compiled = compile_circuit(&circuit)?;
    let (proving_key, _) = generate_keys(&compiled)?;
    save_proving_key(&proving_key, &key_path)?;

    // TODO: Load key using KeyManager
    // let manager = KeyManager::new();
    // let loaded_key = manager.load_proving_key(&key_path)?;
    // assert_eq!(loaded_key.key_data, proving_key.key_data);

    Ok(())
}

#[test]
fn test_load_verifying_key_from_file() -> Result<()> {
    // Test loading verification key from file
    let temp_dir = TempDir::new()?;
    let key_path = temp_dir.path().join("verifying_key.bin");

    // Create a mock verification key file
    use fabstir_llm_node::crypto::ezkl::setup::{generate_keys, compile_circuit, save_verifying_key};
    use fabstir_llm_node::crypto::ezkl::CommitmentCircuit;

    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    let compiled = compile_circuit(&circuit)?;
    let (_, verifying_key) = generate_keys(&compiled)?;
    save_verifying_key(&verifying_key, &key_path)?;

    // TODO: Load key using KeyManager
    // let manager = KeyManager::new();
    // let loaded_key = manager.load_verifying_key(&key_path)?;
    // assert_eq!(loaded_key.key_data, verifying_key.key_data);

    Ok(())
}

#[test]
fn test_key_caching_in_memory() -> Result<()> {
    // Test that keys are cached after first load
    let temp_dir = TempDir::new()?;
    let key_path = temp_dir.path().join("proving_key.bin");

    // Create key file
    use fabstir_llm_node::crypto::ezkl::setup::{generate_keys, compile_circuit, save_proving_key};
    use fabstir_llm_node::crypto::ezkl::CommitmentCircuit;

    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    let compiled = compile_circuit(&circuit)?;
    let (proving_key, _) = generate_keys(&compiled)?;
    save_proving_key(&proving_key, &key_path)?;

    // TODO: Test caching behavior
    // let manager = KeyManager::new();

    // First load - should read from disk
    // let start = std::time::Instant::now();
    // let key1 = manager.load_proving_key(&key_path)?;
    // let first_load_time = start.elapsed();

    // Second load - should hit cache (much faster)
    // let start = std::time::Instant::now();
    // let key2 = manager.load_proving_key(&key_path)?;
    // let cached_load_time = start.elapsed();

    // assert_eq!(key1.key_data, key2.key_data);
    // assert!(cached_load_time < first_load_time / 10); // Cache should be 10x faster

    Ok(())
}

#[test]
fn test_key_validation_on_load() -> Result<()> {
    // Test that keys are validated when loaded
    let temp_dir = TempDir::new()?;
    let key_path = temp_dir.path().join("invalid_key.bin");

    // Write invalid key
    std::fs::write(&key_path, vec![0x00; 1000])?; // Wrong marker

    // TODO: Verify validation error
    // let manager = KeyManager::new();
    // let result = manager.load_proving_key(&key_path);
    // assert!(result.is_err());
    // assert!(result.unwrap_err().to_string().contains("Invalid"));

    Ok(())
}

#[test]
fn test_concurrent_key_loading() -> Result<()> {
    // Test that multiple threads can load keys concurrently
    let temp_dir = TempDir::new()?;
    let key_path = temp_dir.path().join("proving_key.bin");

    // Create key file
    use fabstir_llm_node::crypto::ezkl::setup::{generate_keys, compile_circuit, save_proving_key};
    use fabstir_llm_node::crypto::ezkl::CommitmentCircuit;

    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    let compiled = compile_circuit(&circuit)?;
    let (proving_key, _) = generate_keys(&compiled)?;
    save_proving_key(&proving_key, &key_path)?;

    // TODO: Test concurrent access
    // let manager = Arc::new(KeyManager::new());
    // let mut handles = vec![];

    // for _ in 0..10 {
    //     let manager_clone = Arc::clone(&manager);
    //     let path_clone = key_path.clone();
    //     let handle = std::thread::spawn(move || {
    //         manager_clone.load_proving_key(&path_clone)
    //     });
    //     handles.push(handle);
    // }

    // for handle in handles {
    //     let key = handle.join().unwrap()?;
    //     assert_eq!(key.key_data, proving_key.key_data);
    // }

    Ok(())
}

#[test]
fn test_key_cache_size_limit() {
    // Test that cache has a size limit
    // TODO: Create KeyManager with small cache size
    // TODO: Load more keys than cache can hold
    // TODO: Verify old keys are evicted
}

#[test]
fn test_key_cache_eviction_lru() {
    // Test that least recently used keys are evicted first
    // TODO: Create KeyManager with cache size = 2
    // TODO: Load keys A, B, C (A should be evicted)
    // TODO: Access A again (should reload from disk)
    // TODO: Verify B and C are still cached
}

#[test]
fn test_key_preloading() -> Result<()> {
    // Test that keys can be preloaded into cache
    let temp_dir = TempDir::new()?;
    let key_path = temp_dir.path().join("proving_key.bin");

    // Create key file
    use fabstir_llm_node::crypto::ezkl::setup::{generate_keys, compile_circuit, save_proving_key};
    use fabstir_llm_node::crypto::ezkl::CommitmentCircuit;

    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    let compiled = compile_circuit(&circuit)?;
    let (proving_key, _) = generate_keys(&compiled)?;
    save_proving_key(&proving_key, &key_path)?;

    // TODO: Preload key
    // let manager = KeyManager::new();
    // manager.preload_proving_key(&key_path)?;

    // First access should be fast (from cache)
    // let start = std::time::Instant::now();
    // let key = manager.load_proving_key(&key_path)?;
    // let load_time = start.elapsed();
    // assert!(load_time < Duration::from_millis(10)); // Should be very fast

    Ok(())
}

#[test]
fn test_key_cache_invalidation() -> Result<()> {
    // Test that cache can be invalidated
    let temp_dir = TempDir::new()?;
    let key_path = temp_dir.path().join("proving_key.bin");

    // Create key file
    use fabstir_llm_node::crypto::ezkl::setup::{generate_keys, compile_circuit, save_proving_key};
    use fabstir_llm_node::crypto::ezkl::CommitmentCircuit;

    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    let compiled = compile_circuit(&circuit)?;
    let (proving_key, _) = generate_keys(&compiled)?;
    save_proving_key(&proving_key, &key_path)?;

    // TODO: Load and invalidate
    // let manager = KeyManager::new();
    // manager.load_proving_key(&key_path)?;
    // manager.invalidate_cache();

    // Next load should read from disk again
    // manager.load_proving_key(&key_path)?;

    Ok(())
}

#[test]
fn test_key_manager_with_environment_paths() {
    // Test that KeyManager reads paths from environment
    // TODO: Set environment variables
    // std::env::set_var("EZKL_PROVING_KEY_PATH", "/path/to/key");

    // TODO: Create KeyManager
    // let manager = KeyManager::from_env();
    // assert_eq!(manager.proving_key_path(), Some("/path/to/key"));
}

#[test]
fn test_key_cache_statistics() {
    // Test that cache provides statistics
    // TODO: Create KeyManager
    // TODO: Load keys multiple times
    // TODO: Get cache stats
    // let stats = manager.cache_stats();
    // assert_eq!(stats.hits, expected_hits);
    // assert_eq!(stats.misses, expected_misses);
    // assert!(stats.hit_rate() > 0.5);
}

#[test]
fn test_lazy_key_loading() -> Result<()> {
    // Test that keys are loaded lazily on first use
    let temp_dir = TempDir::new()?;
    let key_path = temp_dir.path().join("proving_key.bin");

    // Create key file
    use fabstir_llm_node::crypto::ezkl::setup::{generate_keys, compile_circuit, save_proving_key};
    use fabstir_llm_node::crypto::ezkl::CommitmentCircuit;

    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    let compiled = compile_circuit(&circuit)?;
    let (proving_key, _) = generate_keys(&compiled)?;
    save_proving_key(&proving_key, &key_path)?;

    // TODO: Create manager with lazy loading
    // let manager = KeyManager::with_lazy_loading(&key_path);

    // Key should not be loaded yet
    // assert!(!manager.is_loaded());

    // First use triggers load
    // let key = manager.get_proving_key()?;
    // assert!(manager.is_loaded());

    Ok(())
}

#[test]
fn test_key_rotation() -> Result<()> {
    // Test that keys can be rotated without restart
    let temp_dir = TempDir::new()?;
    let key_path = temp_dir.path().join("proving_key.bin");

    // Create initial key
    use fabstir_llm_node::crypto::ezkl::setup::{generate_keys, compile_circuit, save_proving_key};
    use fabstir_llm_node::crypto::ezkl::CommitmentCircuit;

    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    let compiled = compile_circuit(&circuit)?;
    let (proving_key1, _) = generate_keys(&compiled)?;
    save_proving_key(&proving_key1, &key_path)?;

    // TODO: Load initial key
    // let manager = KeyManager::new();
    // let key1 = manager.load_proving_key(&key_path)?;

    // Create new key
    let (proving_key2, _) = generate_keys(&compiled)?;
    save_proving_key(&proving_key2, &key_path)?;

    // TODO: Reload with new key
    // manager.reload_proving_key(&key_path)?;
    // let key2 = manager.load_proving_key(&key_path)?;

    // assert_ne!(key1.key_data, key2.key_data);

    Ok(())
}

#[test]
fn test_key_memory_usage() -> Result<()> {
    // Test that key caching uses reasonable memory
    let temp_dir = TempDir::new()?;

    // Create multiple keys
    use fabstir_llm_node::crypto::ezkl::setup::{generate_keys, compile_circuit, save_proving_key};
    use fabstir_llm_node::crypto::ezkl::CommitmentCircuit;

    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    let compiled = compile_circuit(&circuit)?;

    for i in 0..10 {
        let (proving_key, _) = generate_keys(&compiled)?;
        let key_path = temp_dir.path().join(format!("key_{}.bin", i));
        save_proving_key(&proving_key, &key_path)?;
    }

    // TODO: Load all keys and measure memory
    // let manager = KeyManager::new();
    // for i in 0..10 {
    //     let key_path = temp_dir.path().join(format!("key_{}.bin", i));
    //     manager.load_proving_key(&key_path)?;
    // }

    // Memory should be reasonable (< 500MB for 10 keys)
    // let memory_usage = manager.memory_usage_bytes();
    // assert!(memory_usage < 500_000_000);

    Ok(())
}

#[test]
fn test_key_path_canonicalization() -> Result<()> {
    // Test that relative paths are canonicalized
    let temp_dir = TempDir::new()?;
    let key_path = temp_dir.path().join("proving_key.bin");

    // Create key file
    use fabstir_llm_node::crypto::ezkl::setup::{generate_keys, compile_circuit, save_proving_key};
    use fabstir_llm_node::crypto::ezkl::CommitmentCircuit;

    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    let compiled = compile_circuit(&circuit)?;
    let (proving_key, _) = generate_keys(&compiled)?;
    save_proving_key(&proving_key, &key_path)?;

    // TODO: Test with relative path
    // let manager = KeyManager::new();
    // let rel_path = PathBuf::from("./proving_key.bin");
    // Test that relative and absolute paths refer to same cached key

    Ok(())
}

#[test]
fn test_shared_key_cache() -> Result<()> {
    // Test that multiple KeyManager instances can share a cache
    // TODO: Create shared cache
    // let cache = Arc::new(KeyCache::new(10));

    // TODO: Create multiple managers with shared cache
    // let manager1 = KeyManager::with_cache(Arc::clone(&cache));
    // let manager2 = KeyManager::with_cache(Arc::clone(&cache));

    // TODO: Load key with manager1
    // TODO: Access same key with manager2 (should hit cache)

    Ok(())
}

#[test]
fn test_key_loading_performance() -> Result<()> {
    // Test that key loading meets performance targets
    let temp_dir = TempDir::new()?;
    let key_path = temp_dir.path().join("proving_key.bin");

    // Create key file
    use fabstir_llm_node::crypto::ezkl::setup::{generate_keys, compile_circuit, save_proving_key};
    use fabstir_llm_node::crypto::ezkl::CommitmentCircuit;

    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    let compiled = compile_circuit(&circuit)?;
    let (proving_key, _) = generate_keys(&compiled)?;
    save_proving_key(&proving_key, &key_path)?;

    // TODO: Test loading performance
    // let manager = KeyManager::new();

    // First load from disk: < 50ms target
    // let start = std::time::Instant::now();
    // manager.load_proving_key(&key_path)?;
    // let load_time = start.elapsed();
    // assert!(load_time < Duration::from_millis(50));

    // Cached load: < 1ms target
    // let start = std::time::Instant::now();
    // manager.load_proving_key(&key_path)?;
    // let cached_time = start.elapsed();
    // assert!(cached_time < Duration::from_millis(1));

    Ok(())
}
