// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Proof Caching with LRU Eviction
//!
//! Caches generated proofs to avoid regenerating identical proofs.
//! Uses LRU (Least Recently Used) eviction when capacity is reached.

use super::prover::ProofData;
use super::witness::Witness;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Cache key type (hash of witness)
type CacheKey = [u8; 32];

/// Cached proof entry
#[derive(Debug, Clone)]
struct CachedProof {
    /// The proof data
    proof: ProofData,
    /// When the proof was generated
    cached_at: Instant,
    /// Last accessed time (for LRU)
    last_accessed: Instant,
    /// Access count
    access_count: u64,
    /// Size in bytes (for memory tracking)
    size_bytes: usize,
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    /// Number of cache hits
    pub hits: u64,
    /// Number of cache misses
    pub misses: u64,
    /// Number of evictions
    pub evictions: u64,
    /// Number of entries currently cached
    pub entries: usize,
    /// Total memory usage in bytes
    pub memory_bytes: usize,
}

impl CacheStats {
    /// Calculate hit rate (0.0 to 1.0)
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// Total requests
    pub fn total_requests(&self) -> u64 {
        self.hits + self.misses
    }
}

/// Inner cache implementation
#[derive(Debug)]
struct ProofCacheInner {
    /// Maximum number of entries
    capacity: usize,
    /// Cached proofs indexed by cache key
    proofs: HashMap<CacheKey, CachedProof>,
    /// LRU queue (cache keys in order of access)
    lru_queue: VecDeque<CacheKey>,
    /// Cache statistics
    stats: CacheStats,
    /// Optional TTL for entries
    ttl: Option<Duration>,
}

impl ProofCacheInner {
    fn new(capacity: usize, ttl: Option<Duration>) -> Self {
        Self {
            capacity,
            proofs: HashMap::new(),
            lru_queue: VecDeque::new(),
            stats: CacheStats::default(),
            ttl,
        }
    }

    fn get(&mut self, key: &CacheKey) -> Option<ProofData> {
        // Check if entry exists
        if let Some(cached) = self.proofs.get_mut(key) {
            // Check TTL if enabled
            if let Some(ttl) = self.ttl {
                if cached.cached_at.elapsed() > ttl {
                    // Entry expired
                    tracing::debug!("⏱️  Cache entry expired");
                    self.remove(key);
                    self.stats.misses += 1;
                    return None;
                }
            }

            // Update access time and count
            cached.last_accessed = Instant::now();
            cached.access_count += 1;

            // Move to front of LRU queue
            if let Some(pos) = self.lru_queue.iter().position(|k| k == key) {
                self.lru_queue.remove(pos);
            }
            self.lru_queue.push_front(*key);

            self.stats.hits += 1;
            tracing::debug!("🎯 Proof cache hit");
            Some(cached.proof.clone())
        } else {
            self.stats.misses += 1;
            tracing::debug!("❌ Proof cache miss");
            None
        }
    }

    fn insert(&mut self, key: CacheKey, proof: ProofData) {
        // Don't cache if capacity is 0
        if self.capacity == 0 {
            return;
        }

        let size_bytes = proof.proof_bytes.len();

        // Check if we need to evict
        while self.proofs.len() >= self.capacity {
            self.evict_lru();
        }

        let cached = CachedProof {
            proof,
            cached_at: Instant::now(),
            last_accessed: Instant::now(),
            access_count: 0,
            size_bytes,
        };

        self.proofs.insert(key, cached);
        self.lru_queue.push_front(key);

        self.update_stats();
        tracing::debug!("💾 Cached proof ({} entries)", self.proofs.len());
    }

    fn evict_lru(&mut self) {
        if let Some(oldest_key) = self.lru_queue.pop_back() {
            self.proofs.remove(&oldest_key);
            self.stats.evictions += 1;
            tracing::debug!("🗑️  Evicted LRU cache entry");
        }
    }

    fn remove(&mut self, key: &CacheKey) {
        if self.proofs.remove(key).is_some() {
            if let Some(pos) = self.lru_queue.iter().position(|k| k == key) {
                self.lru_queue.remove(pos);
            }
            self.update_stats();
        }
    }

    fn invalidate(&mut self) {
        self.proofs.clear();
        self.lru_queue.clear();
        self.update_stats();
        tracing::info!("🗑️  Proof cache invalidated");
    }

    fn update_stats(&mut self) {
        self.stats.entries = self.proofs.len();
        self.stats.memory_bytes = self.proofs.values().map(|cached| cached.size_bytes).sum();
    }

    fn stats(&self) -> CacheStats {
        self.stats.clone()
    }

    fn len(&self) -> usize {
        self.proofs.len()
    }
}

/// Thread-safe proof cache with LRU eviction
#[derive(Debug, Clone)]
pub struct ProofCache {
    inner: Arc<RwLock<ProofCacheInner>>,
}

impl ProofCache {
    /// Create a new proof cache with given capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ProofCacheInner::new(capacity, None))),
        }
    }

    /// Create a new proof cache with capacity and TTL
    pub fn with_ttl(capacity: usize, ttl: Duration) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ProofCacheInner::new(capacity, Some(ttl)))),
        }
    }

    /// Compute cache key from witness
    pub fn compute_key(witness: &Witness) -> CacheKey {
        let witness_bytes = witness.to_bytes();
        let hash = Sha256::digest(&witness_bytes);
        hash.into()
    }

    /// Get proof from cache
    pub fn get(&self, witness: &Witness) -> Option<ProofData> {
        let key = Self::compute_key(witness);
        let mut inner = self.inner.write().unwrap();
        inner.get(&key)
    }

    /// Insert proof into cache
    pub fn insert(&self, witness: &Witness, proof: ProofData) {
        let key = Self::compute_key(witness);
        let mut inner = self.inner.write().unwrap();
        inner.insert(key, proof);
    }

    /// Invalidate all cached proofs
    pub fn invalidate(&self) {
        let mut inner = self.inner.write().unwrap();
        inner.invalidate();
    }

    /// Remove specific proof from cache
    pub fn remove(&self, witness: &Witness) {
        let key = Self::compute_key(witness);
        let mut inner = self.inner.write().unwrap();
        inner.remove(&key);
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let inner = self.inner.read().unwrap();
        inner.stats()
    }

    /// Get number of cached entries
    pub fn len(&self) -> usize {
        let inner = self.inner.read().unwrap();
        inner.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get cache capacity
    pub fn capacity(&self) -> usize {
        let inner = self.inner.read().unwrap();
        inner.capacity
    }

    /// Get memory usage in bytes
    pub fn memory_usage_bytes(&self) -> usize {
        let stats = self.stats();
        stats.memory_bytes
    }

    /// Warm up cache with proofs for common witnesses
    pub fn warmup(&self, witnesses: &[Witness]) -> crate::crypto::ezkl::error::EzklResult<()> {
        use super::prover::EzklProver;

        let mut prover = EzklProver::new();

        for witness in witnesses {
            let proof = prover.generate_proof(witness)?;
            self.insert(witness, proof);
        }

        tracing::info!("🔥 Warmed up proof cache with {} entries", witnesses.len());
        Ok(())
    }

    /// Clear entries matching a specific model hash
    pub fn clear_by_model_hash(&self, model_hash: [u8; 32]) {
        let mut inner = self.inner.write().unwrap();
        let keys_to_remove: Vec<CacheKey> = inner
            .proofs
            .iter()
            .filter(|(_, cached)| cached.proof.model_hash == model_hash)
            .map(|(key, _)| *key)
            .collect();

        for key in keys_to_remove {
            inner.remove(&key);
        }

        tracing::info!(
            "🗑️  Cleared cache entries for model hash {:?}",
            &model_hash[..8]
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::ezkl::WitnessBuilder;

    fn create_test_witness(seed: u8) -> Witness {
        WitnessBuilder::new()
            .with_job_id([seed; 32])
            .with_model_hash([seed + 1; 32])
            .with_input_hash([seed + 2; 32])
            .with_output_hash([seed + 3; 32])
            .build()
            .unwrap()
    }

    #[test]
    fn test_cache_creation() {
        let cache = ProofCache::new(100);
        assert_eq!(cache.capacity(), 100);
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_key_computation() {
        let witness1 = create_test_witness(0);
        let witness2 = create_test_witness(0);
        let witness3 = create_test_witness(1);

        let key1 = ProofCache::compute_key(&witness1);
        let key2 = ProofCache::compute_key(&witness2);
        let key3 = ProofCache::compute_key(&witness3);

        // Same witness should produce same key
        assert_eq!(key1, key2);

        // Different witness should produce different key
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_cache_hit_miss() {
        let cache = ProofCache::new(10);
        let witness = create_test_witness(0);

        // First access - miss
        assert!(cache.get(&witness).is_none());

        // Insert proof
        use super::super::prover::EzklProver;
        let mut prover = EzklProver::new();
        let proof = prover.generate_proof(&witness).unwrap();
        cache.insert(&witness, proof);

        // Second access - hit
        assert!(cache.get(&witness).is_some());

        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn test_cache_size_limit() {
        let cache = ProofCache::new(2);
        let mut prover = crate::crypto::ezkl::EzklProver::new();

        // Insert 3 proofs (capacity is 2)
        for i in 0..3u8 {
            let witness = create_test_witness(i);
            let proof = prover.generate_proof(&witness).unwrap();
            cache.insert(&witness, proof);
        }

        // Cache should only have 2 entries
        assert_eq!(cache.len(), 2);

        let stats = cache.stats();
        assert_eq!(stats.evictions, 1); // First entry should be evicted
    }

    #[test]
    fn test_lru_eviction() {
        let cache = ProofCache::new(2);
        let mut prover = crate::crypto::ezkl::EzklProver::new();

        let witness_a = create_test_witness(0);
        let witness_b = create_test_witness(1);
        let witness_c = create_test_witness(2);

        // Insert A and B
        let proof_a = prover.generate_proof(&witness_a).unwrap();
        cache.insert(&witness_a, proof_a);

        let proof_b = prover.generate_proof(&witness_b).unwrap();
        cache.insert(&witness_b, proof_b);

        // Access A (makes it more recent than B)
        cache.get(&witness_a);

        // Insert C (should evict B, not A)
        let proof_c = prover.generate_proof(&witness_c).unwrap();
        cache.insert(&witness_c, proof_c);

        // A and C should be cached, B should be evicted
        assert!(cache.get(&witness_a).is_some());
        assert!(cache.get(&witness_b).is_none());
        assert!(cache.get(&witness_c).is_some());
    }

    #[test]
    fn test_cache_invalidation() {
        let cache = ProofCache::new(10);
        let mut prover = crate::crypto::ezkl::EzklProver::new();

        let witness = create_test_witness(0);
        let proof = prover.generate_proof(&witness).unwrap();
        cache.insert(&witness, proof);

        assert_eq!(cache.len(), 1);

        cache.invalidate();

        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_statistics() {
        let cache = ProofCache::new(10);
        let mut prover = crate::crypto::ezkl::EzklProver::new();

        let witness = create_test_witness(0);
        let proof = prover.generate_proof(&witness).unwrap();

        // Miss
        cache.get(&witness);

        // Insert
        cache.insert(&witness, proof);

        // Hit
        cache.get(&witness);
        cache.get(&witness);

        let stats = cache.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hit_rate(), 2.0 / 3.0);
        assert_eq!(stats.entries, 1);
    }

    #[test]
    fn test_memory_usage_tracking() {
        let cache = ProofCache::new(10);
        let mut prover = crate::crypto::ezkl::EzklProver::new();

        let initial_memory = cache.memory_usage_bytes();
        assert_eq!(initial_memory, 0);

        let witness = create_test_witness(0);
        let proof = prover.generate_proof(&witness).unwrap();
        cache.insert(&witness, proof);

        let after_insert = cache.memory_usage_bytes();
        assert!(after_insert > 0);
    }

    #[test]
    fn test_zero_capacity_cache() {
        let cache = ProofCache::new(0);
        let mut prover = crate::crypto::ezkl::EzklProver::new();

        let witness = create_test_witness(0);
        let proof = prover.generate_proof(&witness).unwrap();

        cache.insert(&witness, proof);

        // Cache with 0 capacity should not store anything
        assert_eq!(cache.len(), 0);
    }
}
