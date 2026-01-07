//! Configuration for content fetching
//!
//! Defines settings for HTTP fetching, content limits, and caching.

use std::env;

/// Configuration for content fetching
#[derive(Debug, Clone)]
pub struct ContentFetchConfig {
    /// Enable content fetching (default: true when search enabled)
    pub enabled: bool,
    /// Maximum pages to fetch per search (default: 3)
    pub max_pages: usize,
    /// Maximum characters per page (default: 3000)
    pub max_chars_per_page: usize,
    /// Maximum total characters for all pages (default: 8000)
    pub max_total_chars: usize,
    /// Timeout per page fetch in seconds (default: 5)
    pub timeout_per_page_secs: u64,
    /// Total timeout for all fetches in seconds (default: 10)
    pub total_timeout_secs: u64,
    /// Cache TTL in seconds (default: 1800 = 30 minutes)
    pub cache_ttl_secs: u64,
    /// Maximum cache entries (default: 500)
    pub max_cache_entries: usize,
}

impl ContentFetchConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        Self {
            enabled: env::var("CONTENT_FETCH_ENABLED")
                .map(|v| v.to_lowercase() == "true")
                .unwrap_or(true), // Enabled by default when search is enabled
            max_pages: env::var("CONTENT_FETCH_MAX_PAGES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3)
                .min(5), // Cap at 5
            max_chars_per_page: env::var("CONTENT_FETCH_MAX_CHARS_PER_PAGE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3000),
            max_total_chars: env::var("CONTENT_FETCH_MAX_TOTAL_CHARS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8000),
            timeout_per_page_secs: env::var("CONTENT_FETCH_TIMEOUT_PER_PAGE_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
            total_timeout_secs: env::var("CONTENT_FETCH_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
            cache_ttl_secs: env::var("CONTENT_FETCH_CACHE_TTL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1800),
            max_cache_entries: 500,
        }
    }

    /// Validate configuration values
    pub fn validate(&self) -> Result<(), String> {
        if self.max_pages == 0 {
            return Err("max_pages must be at least 1".to_string());
        }
        if self.max_chars_per_page < 100 {
            return Err("max_chars_per_page must be at least 100".to_string());
        }
        if self.timeout_per_page_secs == 0 {
            return Err("timeout_per_page_secs must be at least 1".to_string());
        }
        Ok(())
    }
}

impl Default for ContentFetchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_pages: 3,
            max_chars_per_page: 3000,
            max_total_chars: 8000,
            timeout_per_page_secs: 5,
            total_timeout_secs: 10,
            cache_ttl_secs: 1800,
            max_cache_entries: 500,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_fetch_config_defaults() {
        let config = ContentFetchConfig::default();
        assert!(config.enabled);
        assert_eq!(config.max_pages, 3);
        assert_eq!(config.max_chars_per_page, 3000);
        assert_eq!(config.max_total_chars, 8000);
        assert_eq!(config.timeout_per_page_secs, 5);
        assert_eq!(config.total_timeout_secs, 10);
        assert_eq!(config.cache_ttl_secs, 1800);
    }

    #[test]
    fn test_content_fetch_config_validation() {
        let mut config = ContentFetchConfig::default();
        assert!(config.validate().is_ok());

        config.max_pages = 0;
        assert!(config.validate().is_err());

        config.max_pages = 3;
        config.max_chars_per_page = 50;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_content_fetch_config_from_env() {
        // Test that from_env doesn't panic with no env vars
        let config = ContentFetchConfig::from_env();
        assert!(config.max_pages <= 5); // Should be capped
    }
}
