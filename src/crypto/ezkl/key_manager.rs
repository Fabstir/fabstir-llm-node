// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Key Management and Caching
//!
//! Handles loading, caching, and validation of EZKL proving and verification keys.
//! Provides thread-safe key caching with Arc<RwLock> for concurrent access.

use super::error::{EzklError, EzklResult};
use super::setup::{
    load_proving_key, load_verifying_key, validate_proving_key, validate_verifying_key, ProvingKey,
    VerificationKey,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Statistics about key cache performance
#[derive(Debug, Clone, Default)]
pub struct KeyCacheStats {
    /// Number of cache hits
    pub hits: u64,
    /// Number of cache misses
    pub misses: u64,
    /// Number of keys currently cached
    pub cached_keys: usize,
    /// Total memory usage in bytes (approximate)
    pub memory_bytes: usize,
}

impl KeyCacheStats {
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

/// Cached key entry
#[derive(Debug, Clone)]
struct CachedKey<T> {
    /// The key data
    key: T,
    /// When the key was loaded
    loaded_at: Instant,
    /// Size in bytes (for memory tracking)
    size_bytes: usize,
}

/// Thread-safe key cache
#[derive(Debug)]
struct KeyCache<T> {
    /// Cached keys indexed by canonical path
    keys: HashMap<PathBuf, CachedKey<T>>,
    /// Cache statistics
    stats: KeyCacheStats,
}

impl<T: Clone> KeyCache<T> {
    fn new() -> Self {
        Self {
            keys: HashMap::new(),
            stats: KeyCacheStats::default(),
        }
    }

    fn get(&mut self, path: &Path) -> Option<T> {
        if let Some(cached) = self.keys.get(path) {
            self.stats.hits += 1;
            tracing::debug!("🎯 Key cache hit for {:?}", path);
            Some(cached.key.clone())
        } else {
            self.stats.misses += 1;
            tracing::debug!("❌ Key cache miss for {:?}", path);
            None
        }
    }

    fn insert(&mut self, path: PathBuf, key: T, size_bytes: usize) {
        let cached = CachedKey {
            key,
            loaded_at: Instant::now(),
            size_bytes,
        };

        self.keys.insert(path, cached);
        self.update_stats();
    }

    fn invalidate(&mut self) {
        self.keys.clear();
        self.update_stats();
        tracing::info!("🗑️  Key cache invalidated");
    }

    fn remove(&mut self, path: &Path) -> bool {
        let removed = self.keys.remove(path).is_some();
        if removed {
            self.update_stats();
            tracing::debug!("🗑️  Removed key from cache: {:?}", path);
        }
        removed
    }

    fn update_stats(&mut self) {
        self.stats.cached_keys = self.keys.len();
        self.stats.memory_bytes = self.keys.values().map(|cached| cached.size_bytes).sum();
    }

    fn stats(&self) -> KeyCacheStats {
        self.stats.clone()
    }
}

/// Key manager with caching
///
/// Provides thread-safe loading and caching of EZKL proving and verification keys.
pub struct KeyManager {
    /// Cache for proving keys
    proving_key_cache: Arc<RwLock<KeyCache<ProvingKey>>>,
    /// Cache for verification keys
    verifying_key_cache: Arc<RwLock<KeyCache<VerificationKey>>>,
    /// Default proving key path (from environment or config)
    default_proving_key_path: Option<PathBuf>,
    /// Default verification key path (from environment or config)
    default_verifying_key_path: Option<PathBuf>,
}

impl KeyManager {
    /// Create a new key manager
    pub fn new() -> Self {
        Self {
            proving_key_cache: Arc::new(RwLock::new(KeyCache::new())),
            verifying_key_cache: Arc::new(RwLock::new(KeyCache::new())),
            default_proving_key_path: None,
            default_verifying_key_path: None,
        }
    }

    /// Create key manager from environment variables
    pub fn from_env() -> Self {
        let proving_key_path = std::env::var("EZKL_PROVING_KEY_PATH")
            .ok()
            .map(PathBuf::from);
        let verifying_key_path = std::env::var("EZKL_VERIFYING_KEY_PATH")
            .ok()
            .map(PathBuf::from);

        Self {
            proving_key_cache: Arc::new(RwLock::new(KeyCache::new())),
            verifying_key_cache: Arc::new(RwLock::new(KeyCache::new())),
            default_proving_key_path: proving_key_path,
            default_verifying_key_path: verifying_key_path,
        }
    }

    /// Create key manager with shared caches
    pub fn with_shared_caches(
        proving_cache: Arc<RwLock<KeyCache<ProvingKey>>>,
        verifying_cache: Arc<RwLock<KeyCache<VerificationKey>>>,
    ) -> Self {
        Self {
            proving_key_cache: proving_cache,
            verifying_key_cache: verifying_cache,
            default_proving_key_path: None,
            default_verifying_key_path: None,
        }
    }

    /// Get default proving key path
    pub fn proving_key_path(&self) -> Option<&Path> {
        self.default_proving_key_path.as_deref()
    }

    /// Get default verifying key path
    pub fn verifying_key_path(&self) -> Option<&Path> {
        self.default_verifying_key_path.as_deref()
    }

    /// Load proving key (with caching)
    ///
    /// Checks cache first. If not found, loads from disk and caches.
    pub fn load_proving_key(&self, path: &Path) -> EzklResult<ProvingKey> {
        // Canonicalize path for consistent cache keys
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        // Check cache first
        {
            let mut cache = self.proving_key_cache.write().unwrap();
            if let Some(key) = cache.get(&canonical_path) {
                return Ok(key);
            }
        }

        // Load from disk
        tracing::info!("📖 Loading proving key from {:?}", path);
        let start = Instant::now();

        let key = load_proving_key(path).map_err(|e| EzklError::KeyLoadFailed {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;

        // Validate
        validate_proving_key(&key)?;

        let load_time = start.elapsed();
        tracing::info!("✅ Loaded proving key in {:?}", load_time);

        // Cache the key
        let size_bytes = key.key_data.len();
        {
            let mut cache = self.proving_key_cache.write().unwrap();
            cache.insert(canonical_path, key.clone(), size_bytes);
        }

        Ok(key)
    }

    /// Load verification key (with caching)
    ///
    /// Checks cache first. If not found, loads from disk and caches.
    pub fn load_verifying_key(&self, path: &Path) -> EzklResult<VerificationKey> {
        // Canonicalize path for consistent cache keys
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        // Check cache first
        {
            let mut cache = self.verifying_key_cache.write().unwrap();
            if let Some(key) = cache.get(&canonical_path) {
                return Ok(key);
            }
        }

        // Load from disk
        tracing::info!("📖 Loading verification key from {:?}", path);
        let start = Instant::now();

        let key = load_verifying_key(path).map_err(|e| EzklError::KeyLoadFailed {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;

        // Validate
        validate_verifying_key(&key)?;

        let load_time = start.elapsed();
        tracing::info!("✅ Loaded verification key in {:?}", load_time);

        // Cache the key
        let size_bytes = key.key_data.len();
        {
            let mut cache = self.verifying_key_cache.write().unwrap();
            cache.insert(canonical_path, key.clone(), size_bytes);
        }

        Ok(key)
    }

    /// Preload proving key into cache
    ///
    /// Useful for warming up the cache before actual use.
    pub fn preload_proving_key(&self, path: &Path) -> EzklResult<()> {
        self.load_proving_key(path)?;
        Ok(())
    }

    /// Preload verification key into cache
    pub fn preload_verifying_key(&self, path: &Path) -> EzklResult<()> {
        self.load_verifying_key(path)?;
        Ok(())
    }

    /// Invalidate all cached keys
    pub fn invalidate_cache(&self) {
        {
            let mut cache = self.proving_key_cache.write().unwrap();
            cache.invalidate();
        }
        {
            let mut cache = self.verifying_key_cache.write().unwrap();
            cache.invalidate();
        }
    }

    /// Reload proving key (invalidate and load fresh)
    pub fn reload_proving_key(&self, path: &Path) -> EzklResult<ProvingKey> {
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        {
            let mut cache = self.proving_key_cache.write().unwrap();
            cache.remove(&canonical_path);
        }

        self.load_proving_key(path)
    }

    /// Reload verification key (invalidate and load fresh)
    pub fn reload_verifying_key(&self, path: &Path) -> EzklResult<VerificationKey> {
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        {
            let mut cache = self.verifying_key_cache.write().unwrap();
            cache.remove(&canonical_path);
        }

        self.load_verifying_key(path)
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> (KeyCacheStats, KeyCacheStats) {
        let proving_stats = {
            let cache = self.proving_key_cache.read().unwrap();
            cache.stats()
        };

        let verifying_stats = {
            let cache = self.verifying_key_cache.read().unwrap();
            cache.stats()
        };

        (proving_stats, verifying_stats)
    }

    /// Get total memory usage in bytes
    pub fn memory_usage_bytes(&self) -> usize {
        let (proving_stats, verifying_stats) = self.cache_stats();
        proving_stats.memory_bytes + verifying_stats.memory_bytes
    }

    /// Check if a proving key is loaded in cache
    pub fn is_proving_key_cached(&self, path: &Path) -> bool {
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        let cache = self.proving_key_cache.read().unwrap();
        cache.keys.contains_key(&canonical_path)
    }

    /// Check if a verification key is loaded in cache
    pub fn is_verifying_key_cached(&self, path: &Path) -> bool {
        let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

        let cache = self.verifying_key_cache.read().unwrap();
        cache.keys.contains_key(&canonical_path)
    }
}

impl Default for KeyManager {
    fn default() -> Self {
        Self::new()
    }
}

// Make KeyManager thread-safe
unsafe impl Send for KeyManager {}
unsafe impl Sync for KeyManager {}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Key manager tests are only for EZKL (SNARK proofs with keys)
    // Risc0 uses transparent setup (no keys), so these tests don't apply
    #[cfg(not(feature = "real-ezkl"))]
    use crate::crypto::ezkl::setup::{
        compile_circuit, generate_keys, save_proving_key, save_verifying_key,
    };
    #[cfg(not(feature = "real-ezkl"))]
    use crate::crypto::ezkl::CommitmentCircuit;
    #[cfg(not(feature = "real-ezkl"))]
    use tempfile::TempDir;

    #[cfg(not(feature = "real-ezkl"))]
    fn setup_test_keys() -> (TempDir, PathBuf, PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let proving_path = temp_dir.path().join("proving_key.bin");
        let verifying_path = temp_dir.path().join("verifying_key.bin");

        let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
        let compiled = compile_circuit(&circuit).unwrap();
        let (proving_key, verifying_key) = generate_keys(&compiled).unwrap();

        save_proving_key(&proving_key, &proving_path).unwrap();
        save_verifying_key(&verifying_key, &verifying_path).unwrap();

        (temp_dir, proving_path, verifying_path)
    }

    #[test]
    fn test_key_manager_new() {
        let manager = KeyManager::new();
        assert!(manager.proving_key_path().is_none());
        assert!(manager.verifying_key_path().is_none());
    }

    #[test]
    #[cfg(not(feature = "real-ezkl"))]
    fn test_load_proving_key() {
        let (_temp_dir, proving_path, _) = setup_test_keys();
        let manager = KeyManager::new();

        let key = manager.load_proving_key(&proving_path).unwrap();
        assert!(!key.key_data.is_empty());
    }

    #[test]
    #[cfg(not(feature = "real-ezkl"))]
    fn test_load_verifying_key() {
        let (_temp_dir, _, verifying_path) = setup_test_keys();
        let manager = KeyManager::new();

        let key = manager.load_verifying_key(&verifying_path).unwrap();
        assert!(!key.key_data.is_empty());
    }

    #[test]
    #[cfg(not(feature = "real-ezkl"))]
    fn test_key_caching() {
        let (_temp_dir, proving_path, _) = setup_test_keys();
        let manager = KeyManager::new();

        // First load - cache miss
        manager.load_proving_key(&proving_path).unwrap();

        // Second load - cache hit
        manager.load_proving_key(&proving_path).unwrap();

        let (proving_stats, _) = manager.cache_stats();
        assert_eq!(proving_stats.hits, 1);
        assert_eq!(proving_stats.misses, 1);
        assert_eq!(proving_stats.cached_keys, 1);
    }

    #[test]
    #[cfg(not(feature = "real-ezkl"))]
    fn test_cache_invalidation() {
        let (_temp_dir, proving_path, _) = setup_test_keys();
        let manager = KeyManager::new();

        manager.load_proving_key(&proving_path).unwrap();
        assert_eq!(manager.cache_stats().0.cached_keys, 1);

        manager.invalidate_cache();
        assert_eq!(manager.cache_stats().0.cached_keys, 0);
    }

    #[test]
    #[cfg(not(feature = "real-ezkl"))]
    fn test_is_key_cached() {
        let (_temp_dir, proving_path, _) = setup_test_keys();
        let manager = KeyManager::new();

        assert!(!manager.is_proving_key_cached(&proving_path));

        manager.load_proving_key(&proving_path).unwrap();

        assert!(manager.is_proving_key_cached(&proving_path));
    }

    #[test]
    #[cfg(not(feature = "real-ezkl"))]
    fn test_memory_usage_tracking() {
        let (_temp_dir, proving_path, _) = setup_test_keys();
        let manager = KeyManager::new();

        let initial_memory = manager.memory_usage_bytes();
        assert_eq!(initial_memory, 0);

        manager.load_proving_key(&proving_path).unwrap();

        let after_load = manager.memory_usage_bytes();
        assert!(after_load > 0);
    }
}
