//! Content caching for fetched web pages
//!
//! Provides TTL-based caching to reduce latency and bandwidth for repeated fetches.

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Cached page content
#[derive(Debug, Clone)]
pub struct CachedContent {
    pub url: String,
    pub title: String,
    pub text: String,
    pub fetched_at: Instant,
}

/// Content cache statistics
#[derive(Debug)]
pub struct ContentCacheStats {
    pub total: usize,
    pub expired: usize,
    pub max: usize,
}

/// Content cache with TTL-based expiration
pub struct ContentCache {
    cache: RwLock<HashMap<String, CachedContent>>,
    ttl: Duration,
    max_entries: usize,
}

impl ContentCache {
    /// Create a new content cache
    ///
    /// # Arguments
    /// * `ttl_secs` - Time-to-live for cached entries in seconds
    /// * `max_entries` - Maximum number of entries before eviction
    pub fn new(ttl_secs: u64, max_entries: usize) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            ttl: Duration::from_secs(ttl_secs),
            max_entries,
        }
    }

    /// Get cached content if not expired
    pub fn get(&self, url: &str) -> Option<CachedContent> {
        let cache = self.cache.read().ok()?;
        let key = Self::normalize_url(url);
        let entry = cache.get(&key)?;

        if entry.fetched_at.elapsed() > self.ttl {
            return None; // Expired
        }

        Some(entry.clone())
    }

    /// Insert content into cache
    pub fn insert(&self, url: &str, title: String, text: String) {
        let mut cache = match self.cache.write() {
            Ok(c) => c,
            Err(_) => return,
        };

        // Evict oldest if at capacity
        if cache.len() >= self.max_entries {
            Self::evict_oldest(&mut cache);
        }

        let key = Self::normalize_url(url);
        cache.insert(
            key,
            CachedContent {
                url: url.to_string(),
                title,
                text,
                fetched_at: Instant::now(),
            },
        );
    }

    /// Clear all cached entries
    pub fn clear(&self) {
        if let Ok(mut cache) = self.cache.write() {
            cache.clear();
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> ContentCacheStats {
        let cache = match self.cache.read() {
            Ok(c) => c,
            Err(_) => {
                return ContentCacheStats {
                    total: 0,
                    expired: 0,
                    max: self.max_entries,
                }
            }
        };
        let total = cache.len();
        let expired = cache
            .values()
            .filter(|e| e.fetched_at.elapsed() > self.ttl)
            .count();
        ContentCacheStats {
            total,
            expired,
            max: self.max_entries,
        }
    }

    /// Normalize URL for cache key (lowercase, remove trailing slash)
    fn normalize_url(url: &str) -> String {
        url.to_lowercase().trim_end_matches('/').to_string()
    }

    fn evict_oldest(cache: &mut HashMap<String, CachedContent>) {
        if let Some(oldest_key) = cache
            .iter()
            .min_by_key(|(_, v)| v.fetched_at)
            .map(|(k, _)| k.clone())
        {
            cache.remove(&oldest_key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_cache_insert_and_get() {
        let cache = ContentCache::new(3600, 100);

        cache.insert(
            "https://example.com/page",
            "Example Title".to_string(),
            "Example content".to_string(),
        );

        let result = cache.get("https://example.com/page");
        assert!(result.is_some());

        let content = result.unwrap();
        assert_eq!(content.title, "Example Title");
        assert_eq!(content.text, "Example content");
    }

    #[test]
    fn test_cache_ttl_expiration() {
        let cache = ContentCache::new(1, 100); // 1 second TTL

        cache.insert(
            "https://example.com/expire",
            "Title".to_string(),
            "Content".to_string(),
        );

        // Should exist immediately
        assert!(cache.get("https://example.com/expire").is_some());

        // Wait for expiration
        sleep(Duration::from_secs(2));

        // Should be expired
        assert!(cache.get("https://example.com/expire").is_none());
    }

    #[test]
    fn test_cache_key_normalization() {
        let cache = ContentCache::new(3600, 100);

        cache.insert(
            "https://Example.COM/Page/",
            "Title".to_string(),
            "Content".to_string(),
        );

        // Should match with different case/trailing slash
        assert!(cache.get("https://example.com/page").is_some());
        assert!(cache.get("HTTPS://EXAMPLE.COM/PAGE/").is_some());
    }

    #[test]
    fn test_cache_max_entries() {
        let cache = ContentCache::new(3600, 3); // Max 3 entries

        for i in 0..5 {
            cache.insert(
                &format!("https://example.com/{}", i),
                format!("Title {}", i),
                format!("Content {}", i),
            );
        }

        let stats = cache.stats();
        assert!(stats.total <= 3);
    }

    #[test]
    fn test_cache_stats() {
        let cache = ContentCache::new(3600, 100);

        cache.insert("https://example.com/1", "T1".to_string(), "C1".to_string());
        cache.insert("https://example.com/2", "T2".to_string(), "C2".to_string());

        let stats = cache.stats();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.expired, 0);
        assert_eq!(stats.max, 100);
    }
}
