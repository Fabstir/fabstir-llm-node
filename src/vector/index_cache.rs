// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! HNSW Index Cache for Performance Optimization (Sub-phase 5.2)
//!
//! Caches built HNSW indexes to avoid rebuilding them for repeated searches.
//! Implements LRU eviction, TTL expiration, and memory limits.
//!
//! ## Features
//!
//! - **LRU Eviction**: Automatically evicts least recently used indexes when capacity is reached
//! - **TTL Expiration**: Automatically expires indexes after a configurable time-to-live
//! - **Memory Limits**: Tracks memory usage and prevents cache from growing too large
//! - **Cache Metrics**: Tracks hits, misses, evictions, and hit rate
//! - **Thread-Safe**: Safe for concurrent access from multiple threads
//!
//! ## Performance
//!
//! - Cache hit: ~1μs (no rebuild needed)
//! - Cache miss: Full index rebuild time (varies by dataset size)
//! - Target: >90% time savings on cache hits
//!
//! ## Usage
//!
//! ```rust,ignore
//! use fabstir_llm_node::vector::index_cache::IndexCache;
//! use std::time::Duration;
//!
//! // Create cache with 10 max entries, 24-hour TTL, 100MB memory limit
//! let mut cache = IndexCache::new(10, Duration::from_secs(86400), 100);
//!
//! // Try to get cached index
//! if let Some(index) = cache.get(manifest_path) {
//!     // Cache hit - use existing index
//! } else {
//!     // Cache miss - build new index and cache it
//!     let index = build_index(vectors)?;
//!     cache.insert(manifest_path.to_string(), index.clone());
//! }
//! ```

use crate::monitoring::S5Metrics;
use crate::vector::hnsw::HnswIndex;
use lru::LruCache;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Cache metrics for monitoring performance
#[derive(Debug, Clone, Default)]
pub struct CacheMetrics {
    /// Number of cache hits
    pub hits: usize,
    /// Number of cache misses
    pub misses: usize,
    /// Number of evictions (LRU or TTL)
    pub evictions: usize,
}

impl CacheMetrics {
    /// Calculate hit rate (hits / total requests)
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// Entry in the cache with TTL tracking
struct CacheEntry {
    /// The cached HNSW index
    index: Arc<HnswIndex>,
    /// When this entry was inserted
    inserted_at: Instant,
}

impl CacheEntry {
    /// Check if this entry has expired
    fn is_expired(&self, ttl: Duration) -> bool {
        self.inserted_at.elapsed() > ttl
    }
}

/// LRU cache for HNSW indexes with TTL and memory limits
///
/// Caches built indexes keyed by manifest_path to avoid rebuilding.
pub struct IndexCache {
    /// LRU cache storage
    cache: LruCache<String, CacheEntry>,
    /// Time-to-live for cache entries
    ttl: Duration,
    /// Maximum memory usage in MB
    max_memory_mb: usize,
    /// Cache metrics
    metrics: CacheMetrics,
    /// Optional S5 metrics for global monitoring
    s5_metrics: Option<Arc<S5Metrics>>,
}

impl IndexCache {
    /// Create a new index cache
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of indexes to cache
    /// * `ttl` - Time-to-live for cached indexes (recommended: 24 hours)
    /// * `max_memory_mb` - Maximum memory usage in MB (recommended: 100-1000 MB)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::time::Duration;
    ///
    /// // 10 indexes max, 24-hour TTL, 100MB limit
    /// let cache = IndexCache::new(10, Duration::from_secs(86400), 100);
    /// ```
    pub fn new(capacity: usize, ttl: Duration, max_memory_mb: usize) -> Self {
        Self {
            cache: LruCache::new(NonZeroUsize::new(capacity).unwrap()),
            ttl,
            max_memory_mb,
            metrics: CacheMetrics::default(),
            s5_metrics: None,
        }
    }

    /// Set S5 metrics for global monitoring
    ///
    /// # Arguments
    /// * `metrics` - S5Metrics instance for recording cache performance
    pub fn with_s5_metrics(mut self, metrics: Arc<S5Metrics>) -> Self {
        self.s5_metrics = Some(metrics);
        self
    }

    /// Get a cached index by manifest path
    ///
    /// # Arguments
    ///
    /// * `manifest_path` - Path to the manifest (cache key)
    ///
    /// # Returns
    ///
    /// * `Some(Arc<HnswIndex>)` if found and not expired
    /// * `None` if not found or expired
    ///
    /// Updates metrics (hits/misses) and LRU ordering.
    pub fn get(&mut self, manifest_path: &str) -> Option<Arc<HnswIndex>> {
        // Try to get from cache
        if let Some(entry) = self.cache.get(manifest_path) {
            // Check if expired
            if entry.is_expired(self.ttl) {
                // Expired - remove and count as miss
                self.cache.pop(manifest_path);
                self.metrics.misses += 1;
                self.metrics.evictions += 1;

                // Record cache miss in S5 metrics (async)
                if let Some(ref metrics) = self.s5_metrics {
                    let metrics = Arc::clone(metrics);
                    tokio::spawn(async move {
                        metrics.record_cache_miss().await;
                    });
                }

                None
            } else {
                // Valid - count as hit
                self.metrics.hits += 1;

                // Record cache hit in S5 metrics (async)
                if let Some(ref metrics) = self.s5_metrics {
                    let metrics = Arc::clone(metrics);
                    tokio::spawn(async move {
                        metrics.record_cache_hit().await;
                    });
                }

                Some(entry.index.clone())
            }
        } else {
            // Not found - count as miss
            self.metrics.misses += 1;

            // Record cache miss in S5 metrics (async)
            if let Some(ref metrics) = self.s5_metrics {
                let metrics = Arc::clone(metrics);
                tokio::spawn(async move {
                    metrics.record_cache_miss().await;
                });
            }

            None
        }
    }

    /// Insert an index into the cache
    ///
    /// # Arguments
    ///
    /// * `manifest_path` - Path to the manifest (cache key)
    /// * `index` - The built HNSW index to cache
    ///
    /// If cache is full, evicts the least recently used entry.
    /// If memory limit is exceeded, may evict multiple entries.
    pub fn insert(&mut self, manifest_path: String, index: Arc<HnswIndex>) {
        let entry = CacheEntry {
            index,
            inserted_at: Instant::now(),
        };

        // Insert into LRU cache (may evict oldest if full)
        if let Some(_evicted) = self.cache.push(manifest_path, entry) {
            self.metrics.evictions += 1;
        }

        // Check memory limit and evict if needed
        self.enforce_memory_limit();
    }

    /// Evict expired entries based on TTL
    ///
    /// This should be called periodically to clean up expired entries.
    /// Entries are also checked for expiration on `get()`.
    pub fn evict_expired(&mut self) {
        let ttl = self.ttl;
        let mut to_remove = Vec::new();

        // Find expired entries
        for (key, entry) in self.cache.iter() {
            if entry.is_expired(ttl) {
                to_remove.push(key.clone());
            }
        }

        // Remove them
        for key in to_remove {
            self.cache.pop(&key);
            self.metrics.evictions += 1;
        }
    }

    /// Enforce memory limit by evicting entries if needed
    ///
    /// Evicts least recently used entries until memory usage is under limit.
    fn enforce_memory_limit(&mut self) {
        while self.memory_usage_mb() > self.max_memory_mb && !self.cache.is_empty() {
            // Pop least recently used entry
            if let Some((_key, _entry)) = self.cache.pop_lru() {
                self.metrics.evictions += 1;
            }
        }
    }

    /// Get current memory usage in MB
    ///
    /// Estimates memory usage based on index sizes.
    /// Each index uses approximately: vector_count * dimensions * 4 bytes + overhead
    pub fn memory_usage_mb(&self) -> usize {
        let mut total_bytes: usize = 0;

        for (_key, entry) in self.cache.iter() {
            let index = &entry.index;
            let vector_count = index.vector_count();
            let dimensions = index.dimensions();

            // Estimate: vectors + metadata + HNSW graph overhead
            // vectors: count * dimensions * 4 bytes (f32)
            // metadata: ~200 bytes per vector (conservative estimate)
            // HNSW overhead: ~50% of vector data
            let vector_bytes = vector_count * dimensions * 4;
            let metadata_bytes = vector_count * 200;
            let hnsw_overhead = vector_bytes / 2;

            total_bytes += vector_bytes + metadata_bytes + hnsw_overhead;
        }

        total_bytes / (1024 * 1024) // Convert to MB
    }

    /// Get cache metrics
    pub fn metrics(&self) -> CacheMetrics {
        self.metrics.clone()
    }

    /// Reset metrics (for testing or monitoring)
    pub fn reset_metrics(&mut self) {
        self.metrics = CacheMetrics::default();
    }

    /// Get number of cached indexes
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Get cache capacity
    pub fn capacity(&self) -> usize {
        self.cache.cap().get()
    }

    /// Clear all cached indexes
    pub fn clear(&mut self) {
        self.cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::manifest::Vector;
    use serde_json::json;

    fn create_test_index(vector_count: usize) -> Arc<HnswIndex> {
        let vectors: Vec<Vector> = (0..vector_count)
            .map(|i| Vector {
                id: format!("vec-{}", i),
                vector: vec![i as f32; 384],
                metadata: json!({"index": i}),
            })
            .collect();

        Arc::new(HnswIndex::build(vectors, 384).unwrap())
    }

    #[test]
    fn test_basic_cache_operations() {
        let mut cache = IndexCache::new(5, Duration::from_secs(3600), 100);

        let index = create_test_index(100);
        cache.insert("test".to_string(), index.clone());

        assert_eq!(cache.len(), 1);
        let retrieved = cache.get("test");
        assert!(retrieved.is_some());
    }

    #[test]
    fn test_lru_eviction() {
        let mut cache = IndexCache::new(2, Duration::from_secs(3600), 100);

        cache.insert("db1".to_string(), create_test_index(50));
        cache.insert("db2".to_string(), create_test_index(50));
        cache.insert("db3".to_string(), create_test_index(50));

        assert_eq!(cache.len(), 2);
        assert!(cache.get("db1").is_none());
    }
}
