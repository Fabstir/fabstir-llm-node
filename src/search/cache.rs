// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! TTL-based search result caching

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use super::types::SearchResult;

/// TTL-based cache for search results
pub struct SearchCache {
    cache: RwLock<HashMap<String, CachedEntry>>,
    ttl: Duration,
    max_entries: usize,
}

struct CachedEntry {
    results: Vec<SearchResult>,
    provider: String,
    inserted_at: Instant,
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Total entries in cache
    pub total: usize,
    /// Expired entries (not yet evicted)
    pub expired: usize,
    /// Maximum cache capacity
    pub max: usize,
}

impl SearchCache {
    /// Create a new search cache
    ///
    /// # Arguments
    /// * `ttl_secs` - Time-to-live for cache entries in seconds
    /// * `max_entries` - Maximum number of entries to store
    pub fn new(ttl_secs: u64, max_entries: usize) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            ttl: Duration::from_secs(ttl_secs),
            max_entries,
        }
    }

    /// Get cached results for a query
    ///
    /// Returns None if not found or expired
    pub fn get(&self, query: &str) -> Option<(Vec<SearchResult>, String)> {
        let cache = self.cache.read().ok()?;
        let key = Self::cache_key(query);
        let entry = cache.get(&key)?;

        if entry.inserted_at.elapsed() > self.ttl {
            return None; // Expired
        }

        Some((entry.results.clone(), entry.provider.clone()))
    }

    /// Insert results into cache
    pub fn insert(&self, query: &str, results: &[SearchResult], provider: &str) {
        let mut cache = match self.cache.write() {
            Ok(c) => c,
            Err(_) => return,
        };

        // Evict oldest if at capacity
        if cache.len() >= self.max_entries {
            self.evict_oldest(&mut cache);
        }

        let key = Self::cache_key(query);
        cache.insert(
            key,
            CachedEntry {
                results: results.to_vec(),
                provider: provider.to_string(),
                inserted_at: Instant::now(),
            },
        );
    }

    /// Clear all cache entries
    pub fn clear(&self) {
        if let Ok(mut cache) = self.cache.write() {
            cache.clear();
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let cache = match self.cache.read() {
            Ok(c) => c,
            Err(_) => {
                return CacheStats {
                    total: 0,
                    expired: 0,
                    max: self.max_entries,
                }
            }
        };

        let total = cache.len();
        let expired = cache
            .values()
            .filter(|e| e.inserted_at.elapsed() > self.ttl)
            .count();

        CacheStats {
            total,
            expired,
            max: self.max_entries,
        }
    }

    /// Generate cache key from query
    fn cache_key(query: &str) -> String {
        query.to_lowercase().trim().to_string()
    }

    /// Evict the oldest entry from the cache
    fn evict_oldest(&self, cache: &mut HashMap<String, CachedEntry>) {
        if let Some(oldest_key) = cache
            .iter()
            .min_by_key(|(_, v)| v.inserted_at)
            .map(|(k, _)| k.clone())
        {
            cache.remove(&oldest_key);
        }
    }

    /// Remove expired entries from cache
    pub fn cleanup_expired(&self) {
        if let Ok(mut cache) = self.cache.write() {
            cache.retain(|_, entry| entry.inserted_at.elapsed() <= self.ttl);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_creation() {
        let cache = SearchCache::new(3600, 1000);
        let stats = cache.stats();
        assert_eq!(stats.total, 0);
        assert_eq!(stats.max, 1000);
    }

    #[test]
    fn test_cache_insert_and_get() {
        let cache = SearchCache::new(3600, 100);
        let results = vec![SearchResult {
            title: "Test".to_string(),
            url: "https://example.com".to_string(),
            snippet: "A test".to_string(),
            published_date: None,
            source: "test".to_string(),
        }];

        cache.insert("test query", &results, "brave");

        let (cached, provider) = cache.get("test query").unwrap();
        assert_eq!(cached.len(), 1);
        assert_eq!(cached[0].title, "Test");
        assert_eq!(provider, "brave");
    }

    #[test]
    fn test_cache_key_normalization() {
        let cache = SearchCache::new(3600, 100);
        let results = vec![];

        cache.insert("TEST Query", &results, "brave");

        // Should find with different casing
        assert!(cache.get("test query").is_some());
        assert!(cache.get("TEST QUERY").is_some());
        assert!(cache.get("  test query  ").is_some());
    }

    #[test]
    fn test_cache_miss() {
        let cache = SearchCache::new(3600, 100);
        assert!(cache.get("nonexistent").is_none());
    }

    #[test]
    fn test_cache_clear() {
        let cache = SearchCache::new(3600, 100);
        cache.insert("test", &[], "brave");
        assert!(cache.get("test").is_some());

        cache.clear();
        assert!(cache.get("test").is_none());
    }

    #[test]
    fn test_cache_stats() {
        let cache = SearchCache::new(3600, 100);
        cache.insert("query1", &[], "brave");
        cache.insert("query2", &[], "brave");

        let stats = cache.stats();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.expired, 0);
    }

    #[test]
    fn test_cache_eviction_at_capacity() {
        let cache = SearchCache::new(3600, 2);

        cache.insert("query1", &[], "brave");
        cache.insert("query2", &[], "brave");
        cache.insert("query3", &[], "brave");

        let stats = cache.stats();
        assert_eq!(stats.total, 2); // Should have evicted one
    }

    #[test]
    fn test_cache_ttl_expiration() {
        // Create cache with 0 second TTL (immediate expiration)
        let cache = SearchCache::new(0, 100);
        cache.insert("test", &[], "brave");

        // Should be expired immediately
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(cache.get("test").is_none());
    }
}
