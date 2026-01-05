// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Search service orchestration
//!
//! Coordinates search providers, caching, and rate limiting.

use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

use super::bing::BingSearchProvider;
use super::brave::BraveSearchProvider;
use super::cache::SearchCache;
use super::config::SearchConfig;
use super::duckduckgo::DuckDuckGoProvider;
use super::provider::SearchProvider;
use super::rate_limiter::SearchRateLimiter;
use super::types::{SearchError, SearchResponse, SearchResult};

/// Main search service that orchestrates providers, caching, and rate limiting
pub struct SearchService {
    providers: Vec<Box<dyn SearchProvider>>,
    cache: SearchCache,
    rate_limiter: SearchRateLimiter,
    config: SearchConfig,
}

impl SearchService {
    /// Create a new search service from configuration
    pub fn new(config: SearchConfig) -> Self {
        let mut providers: Vec<Box<dyn SearchProvider>> = Vec::new();

        // Add Brave if configured (priority 10)
        if let Some(ref api_key) = config.providers.brave_api_key {
            if !api_key.is_empty() {
                providers.push(Box::new(BraveSearchProvider::new(api_key.clone())));
                debug!("Brave Search provider enabled");
            }
        }

        // Add Bing if configured (priority 20)
        if let Some(ref api_key) = config.providers.bing_api_key {
            if !api_key.is_empty() {
                providers.push(Box::new(BingSearchProvider::new(api_key.clone())));
                debug!("Bing Search provider enabled");
            }
        }

        // Always add DuckDuckGo as fallback (priority 50)
        providers.push(Box::new(DuckDuckGoProvider::new()));
        debug!("DuckDuckGo provider enabled (fallback)");

        // Sort by priority (lower = preferred)
        providers.sort_by_key(|p| p.priority());

        let cache = SearchCache::new(config.cache_ttl_secs, 1000);
        let rate_limiter = SearchRateLimiter::new(config.rate_limit_per_minute);

        Self {
            providers,
            cache,
            rate_limiter,
            config,
        }
    }

    /// Perform a search
    ///
    /// # Arguments
    /// * `query` - The search query
    /// * `num_results` - Optional number of results (uses default if None)
    ///
    /// # Returns
    /// Search response with results, or error
    pub async fn search(
        &self,
        query: &str,
        num_results: Option<usize>,
    ) -> Result<SearchResponse, SearchError> {
        if !self.config.enabled {
            return Err(SearchError::SearchDisabled);
        }

        let num_results = num_results.unwrap_or(self.config.default_num_results);

        // Check cache first
        if let Some((results, provider)) = self.cache.get(query) {
            debug!("Cache hit for query: {}", query);
            return Ok(SearchResponse {
                query: query.to_string(),
                results: results.clone(),
                search_time_ms: 0,
                provider,
                cached: true,
                result_count: results.len(),
            });
        }

        // Rate limit check
        self.rate_limiter.check()?;

        let start = Instant::now();

        // Try providers in order (by priority)
        for provider in &self.providers {
            if !provider.is_available() {
                continue;
            }

            debug!("Trying search provider: {}", provider.name());

            match provider.search(query, num_results).await {
                Ok(results) => {
                    let elapsed_ms = start.elapsed().as_millis() as u64;

                    // Cache successful results
                    self.cache.insert(query, &results, provider.name());

                    info!(
                        "Search complete: {} results from {} in {}ms",
                        results.len(),
                        provider.name(),
                        elapsed_ms
                    );

                    return Ok(SearchResponse {
                        query: query.to_string(),
                        result_count: results.len(),
                        results,
                        search_time_ms: elapsed_ms,
                        provider: provider.name().to_string(),
                        cached: false,
                    });
                }
                Err(e) => {
                    warn!(
                        "Search provider {} failed: {}, trying next",
                        provider.name(),
                        e
                    );
                    continue;
                }
            }
        }

        Err(SearchError::ProviderUnavailable {
            provider: "all".to_string(),
        })
    }

    /// Perform multiple searches in parallel
    ///
    /// # Arguments
    /// * `queries` - List of search queries
    /// * `num_results_per_query` - Optional number of results per query
    pub async fn batch_search(
        &self,
        queries: Vec<String>,
        num_results_per_query: Option<usize>,
    ) -> Vec<Result<SearchResponse, SearchError>> {
        let futures: Vec<_> = queries
            .iter()
            .map(|q| self.search(q, num_results_per_query))
            .collect();

        futures::future::join_all(futures).await
    }

    /// Check if search is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get list of available provider names
    pub fn available_providers(&self) -> Vec<&str> {
        self.providers
            .iter()
            .filter(|p| p.is_available())
            .map(|p| p.name())
            .collect()
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> super::cache::CacheStats {
        self.cache.stats()
    }

    /// Clear the search cache
    pub fn clear_cache(&self) {
        self.cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_creation_enabled_by_default() {
        // Web search is enabled by default (DuckDuckGo needs no API key)
        let config = SearchConfig::default();
        let service = SearchService::new(config);
        assert!(service.is_enabled());
    }

    #[test]
    fn test_service_creation_explicitly_disabled() {
        let mut config = SearchConfig::default();
        config.enabled = false;
        let service = SearchService::new(config);
        assert!(!service.is_enabled());
    }

    #[test]
    fn test_service_creation_with_providers() {
        let mut config = SearchConfig::default();
        config.enabled = true;
        config.providers.brave_api_key = Some("test-key".to_string());

        let service = SearchService::new(config);
        let providers = service.available_providers();

        // Should have Brave and DuckDuckGo
        assert!(providers.contains(&"brave"));
        assert!(providers.contains(&"duckduckgo"));
    }

    #[test]
    fn test_service_default_providers() {
        let config = SearchConfig::default();
        let service = SearchService::new(config);

        // Should always have DuckDuckGo as fallback
        let providers = service.available_providers();
        assert!(providers.contains(&"duckduckgo"));
    }

    #[tokio::test]
    async fn test_service_search_disabled() {
        // Explicitly disable search to test disabled behavior
        let mut config = SearchConfig::default();
        config.enabled = false;
        let service = SearchService::new(config);

        let result = service.search("test", None).await;
        assert!(matches!(result, Err(SearchError::SearchDisabled)));
    }

    #[test]
    fn test_cache_stats() {
        let config = SearchConfig::default();
        let service = SearchService::new(config);

        let stats = service.cache_stats();
        assert_eq!(stats.total, 0);
    }

    #[test]
    fn test_clear_cache() {
        let mut config = SearchConfig::default();
        config.enabled = true;
        let service = SearchService::new(config);

        // Insert something into cache directly
        service.cache.insert("test", &[], "test");
        assert!(service.cache.get("test").is_some());

        service.clear_cache();
        assert!(service.cache.get("test").is_none());
    }
}
