// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Configuration for web search functionality

use std::env;

/// Configuration for web search functionality
#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// Whether web search is enabled
    pub enabled: bool,
    /// Provider-specific configuration
    pub providers: SearchProviderConfig,
    /// Cache TTL in seconds
    pub cache_ttl_secs: u64,
    /// Maximum searches per single request
    pub max_searches_per_request: u32,
    /// Maximum searches per session
    pub max_searches_per_session: u32,
    /// Rate limit (requests per minute)
    pub rate_limit_per_minute: u32,
    /// Default number of results per search
    pub default_num_results: usize,
    /// Request timeout in milliseconds
    pub request_timeout_ms: u64,
}

/// Provider-specific configuration
#[derive(Debug, Clone)]
pub struct SearchProviderConfig {
    /// Brave Search API key
    pub brave_api_key: Option<String>,
    /// Bing Search API key
    pub bing_api_key: Option<String>,
    /// Preferred search provider
    pub preferred_provider: String,
}

impl SearchConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        Self {
            // Web search enabled by default (DuckDuckGo requires no API key)
            // Set WEB_SEARCH_ENABLED=false to disable
            enabled: env::var("WEB_SEARCH_ENABLED")
                .map(|v| v.to_lowercase() != "false")
                .unwrap_or(true),
            providers: SearchProviderConfig {
                brave_api_key: env::var("BRAVE_API_KEY").ok(),
                bing_api_key: env::var("BING_API_KEY").ok(),
                preferred_provider: env::var("SEARCH_PROVIDER")
                    .unwrap_or_else(|_| "brave".to_string()),
            },
            cache_ttl_secs: env::var("SEARCH_CACHE_TTL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3600),
            max_searches_per_request: env::var("MAX_SEARCHES_PER_REQUEST")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(20),
            max_searches_per_session: env::var("MAX_SEARCHES_PER_SESSION")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(200),
            rate_limit_per_minute: env::var("SEARCH_RATE_LIMIT_PER_MINUTE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
            default_num_results: 10,
            request_timeout_ms: 10000,
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), String> {
        // Note: DuckDuckGo is always available (no API key needed)
        // so no error if enabled without API keys
        if self.cache_ttl_secs == 0 {
            return Err("Cache TTL must be greater than 0".to_string());
        }
        if self.rate_limit_per_minute == 0 {
            return Err("Rate limit must be greater than 0".to_string());
        }
        Ok(())
    }

    /// Check if any search provider is configured
    pub fn has_any_provider(&self) -> bool {
        self.providers.brave_api_key.is_some() || self.providers.bing_api_key.is_some()
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            enabled: true, // Enabled by default (DuckDuckGo needs no API key)
            providers: SearchProviderConfig {
                brave_api_key: None,
                bing_api_key: None,
                preferred_provider: "brave".to_string(),
            },
            cache_ttl_secs: 3600,
            max_searches_per_request: 20,
            max_searches_per_session: 200,
            rate_limit_per_minute: 60,
            default_num_results: 10,
            request_timeout_ms: 10000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SearchConfig::default();
        // Web search is enabled by default (DuckDuckGo needs no API key)
        assert!(config.enabled);
        assert_eq!(config.cache_ttl_secs, 3600);
        assert_eq!(config.rate_limit_per_minute, 60);
        assert_eq!(config.default_num_results, 10);
        // Validates successfully since DuckDuckGo is always available
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_with_brave() {
        let mut config = SearchConfig::default();
        config.providers.brave_api_key = Some("test-key".to_string());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_has_any_provider() {
        let mut config = SearchConfig::default();
        assert!(!config.has_any_provider());

        config.providers.brave_api_key = Some("key".to_string());
        assert!(config.has_any_provider());
    }

    #[test]
    fn test_config_validation_zero_cache_ttl() {
        let mut config = SearchConfig::default();
        config.cache_ttl_secs = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_zero_rate_limit() {
        let mut config = SearchConfig::default();
        config.rate_limit_per_minute = 0;
        assert!(config.validate().is_err());
    }
}
