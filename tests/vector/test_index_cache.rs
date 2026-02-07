// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// TDD Tests for Index Caching (Sub-phase 5.2)
// Tests LRU caching, TTL expiration, memory limits, and cache metrics

use fabstir_llm_node::storage::manifest::Vector;
use fabstir_llm_node::vector::hnsw::HnswIndex;
use fabstir_llm_node::vector::index_cache::IndexCache;
use serde_json::json;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Helper: Create test vectors
fn create_test_vectors(count: usize, dimensions: usize) -> Vec<Vector> {
    (0..count)
        .map(|i| {
            let vector: Vec<f32> = (0..dimensions).map(|d| (i + d) as f32 * 0.01).collect();
            Vector {
                id: format!("vec-{}", i),
                vector,
                metadata: json!({"index": i}),
            }
        })
        .collect()
}

/// Helper: Build HNSW index
fn build_test_index(vector_count: usize, dimensions: usize) -> Arc<HnswIndex> {
    let vectors = create_test_vectors(vector_count, dimensions);
    let index = HnswIndex::build(vectors, dimensions).expect("Failed to build index");
    Arc::new(index)
}

// ============================================================================
// Test Category 1: Basic Cache Operations
// ============================================================================

#[test]
fn test_cache_creation() {
    let cache = IndexCache::new(10, Duration::from_secs(3600), 100);

    assert_eq!(cache.len(), 0);
    assert_eq!(cache.capacity(), 10);
    assert!(cache.is_empty());
}

#[test]
fn test_cache_insert_and_get() {
    let mut cache = IndexCache::new(10, Duration::from_secs(3600), 100);

    let index = build_test_index(100, 384);
    let manifest_path = "home/vector-databases/0xUser/db1/manifest.json".to_string();

    cache.insert(manifest_path.clone(), index.clone());

    let retrieved = cache.get(&manifest_path);
    assert!(retrieved.is_some());
    assert_eq!(cache.len(), 1);
}

#[test]
fn test_cache_miss() {
    let mut cache = IndexCache::new(10, Duration::from_secs(3600), 100);

    let result = cache.get("nonexistent/path");
    assert!(result.is_none());
}

#[test]
fn test_cache_overwrite() {
    let mut cache = IndexCache::new(10, Duration::from_secs(3600), 100);

    let index1 = build_test_index(100, 384);
    let index2 = build_test_index(200, 384);
    let path = "home/vector-databases/0xUser/db1/manifest.json".to_string();

    cache.insert(path.clone(), index1);
    cache.insert(path.clone(), index2.clone());

    let retrieved = cache.get(&path).unwrap();
    assert_eq!(retrieved.vector_count(), index2.vector_count());
    assert_eq!(cache.len(), 1); // Should still be 1 entry
}

// ============================================================================
// Test Category 2: LRU Eviction
// ============================================================================

#[test]
fn test_lru_eviction_basic() {
    let mut cache = IndexCache::new(3, Duration::from_secs(3600), 100);

    let index1 = build_test_index(50, 384);
    let index2 = build_test_index(50, 384);
    let index3 = build_test_index(50, 384);
    let index4 = build_test_index(50, 384);

    cache.insert("db1".to_string(), index1);
    cache.insert("db2".to_string(), index2);
    cache.insert("db3".to_string(), index3);

    assert_eq!(cache.len(), 3);

    // Insert 4th item, should evict db1 (least recently used)
    cache.insert("db4".to_string(), index4);

    assert_eq!(cache.len(), 3);
    assert!(cache.get("db1").is_none(), "db1 should have been evicted");
    assert!(cache.get("db2").is_some());
    assert!(cache.get("db3").is_some());
    assert!(cache.get("db4").is_some());
}

#[test]
fn test_lru_get_updates_recency() {
    let mut cache = IndexCache::new(3, Duration::from_secs(3600), 100);

    let index1 = build_test_index(50, 384);
    let index2 = build_test_index(50, 384);
    let index3 = build_test_index(50, 384);
    let index4 = build_test_index(50, 384);

    cache.insert("db1".to_string(), index1);
    cache.insert("db2".to_string(), index2);
    cache.insert("db3".to_string(), index3);

    // Access db1 to make it most recently used
    let _ = cache.get("db1");

    // Insert db4, should evict db2 (now least recently used)
    cache.insert("db4".to_string(), index4);

    assert!(
        cache.get("db1").is_some(),
        "db1 was accessed, should not be evicted"
    );
    assert!(cache.get("db2").is_none(), "db2 should have been evicted");
    assert!(cache.get("db3").is_some());
    assert!(cache.get("db4").is_some());
}

#[test]
fn test_lru_multiple_evictions() {
    let mut cache = IndexCache::new(2, Duration::from_secs(3600), 100);

    cache.insert("db1".to_string(), build_test_index(50, 384));
    cache.insert("db2".to_string(), build_test_index(50, 384));
    cache.insert("db3".to_string(), build_test_index(50, 384));
    cache.insert("db4".to_string(), build_test_index(50, 384));

    assert_eq!(cache.len(), 2);
    assert!(cache.get("db1").is_none());
    assert!(cache.get("db2").is_none());
    assert!(cache.get("db3").is_some());
    assert!(cache.get("db4").is_some());
}

// ============================================================================
// Test Category 3: TTL Expiration
// ============================================================================

#[test]
fn test_ttl_expiration() {
    let mut cache = IndexCache::new(10, Duration::from_millis(100), 100);

    let index = build_test_index(50, 384);
    cache.insert("db1".to_string(), index);

    // Should be accessible immediately
    assert!(cache.get("db1").is_some());

    // Wait for TTL to expire
    thread::sleep(Duration::from_millis(150));

    // Should be evicted by TTL
    cache.evict_expired();
    assert!(cache.get("db1").is_none(), "Entry should have expired");
}

#[test]
fn test_ttl_multiple_entries() {
    let mut cache = IndexCache::new(10, Duration::from_millis(100), 100);

    cache.insert("db1".to_string(), build_test_index(50, 384));
    thread::sleep(Duration::from_millis(50));
    cache.insert("db2".to_string(), build_test_index(50, 384));
    thread::sleep(Duration::from_millis(60)); // db1 expired, db2 not yet

    cache.evict_expired();

    assert!(cache.get("db1").is_none(), "db1 should have expired");
    assert!(cache.get("db2").is_some(), "db2 should still be valid");
}

#[test]
fn test_ttl_access_doesnt_refresh() {
    let mut cache = IndexCache::new(10, Duration::from_millis(100), 100);

    cache.insert("db1".to_string(), build_test_index(50, 384));

    thread::sleep(Duration::from_millis(50));
    let _ = cache.get("db1"); // Access but TTL should not refresh

    thread::sleep(Duration::from_millis(60));
    cache.evict_expired();

    assert!(
        cache.get("db1").is_none(),
        "TTL should not refresh on access"
    );
}

// ============================================================================
// Test Category 4: Memory Limits
// ============================================================================

#[test]
fn test_memory_limit_enforcement() {
    // Each 1K vector index with 384 dimensions is approximately:
    // 1000 vectors * 384 floats * 4 bytes + overhead ≈ 1.5 MB
    let mut cache = IndexCache::new(10, Duration::from_secs(3600), 5); // 5 MB limit

    // Insert 3 large indexes (each ~1.5 MB)
    cache.insert("db1".to_string(), build_test_index(1000, 384));
    cache.insert("db2".to_string(), build_test_index(1000, 384));
    cache.insert("db3".to_string(), build_test_index(1000, 384));

    let memory_mb = cache.memory_usage_mb();
    assert!(
        memory_mb <= 5,
        "Memory usage {} MB should not exceed 5 MB limit",
        memory_mb
    );
}

#[test]
fn test_memory_usage_tracking() {
    let mut cache = IndexCache::new(10, Duration::from_secs(3600), 100);

    assert_eq!(cache.memory_usage_mb(), 0);

    // Use 1000 vectors to ensure memory usage shows up in MB
    cache.insert("db1".to_string(), build_test_index(1000, 384));
    let usage1 = cache.memory_usage_mb();
    assert!(
        usage1 > 0,
        "Memory usage should increase after insert. Got: {} MB",
        usage1
    );

    cache.insert("db2".to_string(), build_test_index(1000, 384));
    let usage2 = cache.memory_usage_mb();
    assert!(
        usage2 > usage1,
        "Memory usage should increase with more indexes. Got {} MB vs {} MB",
        usage2,
        usage1
    );
}

// ============================================================================
// Test Category 5: Cache Metrics
// ============================================================================

#[test]
fn test_cache_hit_metric() {
    let mut cache = IndexCache::new(10, Duration::from_secs(3600), 100);

    cache.insert("db1".to_string(), build_test_index(50, 384));

    let _ = cache.get("db1"); // Hit
    let _ = cache.get("db1"); // Hit

    let metrics = cache.metrics();
    assert_eq!(metrics.hits, 2);
    assert_eq!(metrics.misses, 0);
}

#[test]
fn test_cache_miss_metric() {
    let mut cache = IndexCache::new(10, Duration::from_secs(3600), 100);

    let _ = cache.get("nonexistent1");
    let _ = cache.get("nonexistent2");

    let metrics = cache.metrics();
    assert_eq!(metrics.hits, 0);
    assert_eq!(metrics.misses, 2);
}

#[test]
fn test_cache_eviction_metric() {
    let mut cache = IndexCache::new(2, Duration::from_secs(3600), 100);

    cache.insert("db1".to_string(), build_test_index(50, 384));
    cache.insert("db2".to_string(), build_test_index(50, 384));
    cache.insert("db3".to_string(), build_test_index(50, 384)); // Should evict db1

    let metrics = cache.metrics();
    assert_eq!(metrics.evictions, 1);
}

#[test]
fn test_cache_hit_rate() {
    let mut cache = IndexCache::new(10, Duration::from_secs(3600), 100);

    cache.insert("db1".to_string(), build_test_index(50, 384));

    let _ = cache.get("db1"); // Hit
    let _ = cache.get("db1"); // Hit
    let _ = cache.get("db2"); // Miss
    let _ = cache.get("db3"); // Miss

    let metrics = cache.metrics();
    assert_eq!(metrics.hit_rate(), 0.5); // 2 hits out of 4 total
}

// ============================================================================
// Test Category 6: Clear and Reset
// ============================================================================

#[test]
fn test_cache_clear() {
    let mut cache = IndexCache::new(10, Duration::from_secs(3600), 100);

    cache.insert("db1".to_string(), build_test_index(50, 384));
    cache.insert("db2".to_string(), build_test_index(50, 384));

    assert_eq!(cache.len(), 2);

    cache.clear();

    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());
    assert_eq!(cache.memory_usage_mb(), 0);
}

#[test]
fn test_metrics_reset() {
    let mut cache = IndexCache::new(10, Duration::from_secs(3600), 100);

    cache.insert("db1".to_string(), build_test_index(50, 384));
    let _ = cache.get("db1");
    let _ = cache.get("db2");

    cache.reset_metrics();

    let metrics = cache.metrics();
    assert_eq!(metrics.hits, 0);
    assert_eq!(metrics.misses, 0);
    assert_eq!(metrics.evictions, 0);
}
