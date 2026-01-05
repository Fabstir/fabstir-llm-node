// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Search provider trait definition

use async_trait::async_trait;

use super::types::{SearchError, SearchResult};

/// Trait for implementing search providers
///
/// Search providers implement this trait to provide web search functionality.
/// Multiple providers can be configured with automatic failover.
#[async_trait]
pub trait SearchProvider: Send + Sync {
    /// Perform a web search
    ///
    /// # Arguments
    /// * `query` - The search query string
    /// * `num_results` - Maximum number of results to return
    ///
    /// # Returns
    /// A vector of search results or an error
    async fn search(
        &self,
        query: &str,
        num_results: usize,
    ) -> Result<Vec<SearchResult>, SearchError>;

    /// Get the provider name for logging and billing
    fn name(&self) -> &'static str;

    /// Check if the provider is available (has API key, etc.)
    fn is_available(&self) -> bool;

    /// Get provider priority (lower = preferred)
    ///
    /// Default priority is 100. Providers with lower priority
    /// are tried first during failover.
    fn priority(&self) -> u8 {
        100
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockProvider {
        available: bool,
    }

    #[async_trait]
    impl SearchProvider for MockProvider {
        async fn search(
            &self,
            query: &str,
            _num_results: usize,
        ) -> Result<Vec<SearchResult>, SearchError> {
            Ok(vec![SearchResult {
                title: format!("Result for {}", query),
                url: "https://example.com".to_string(),
                snippet: "A mock result".to_string(),
                published_date: None,
                source: "mock".to_string(),
            }])
        }

        fn name(&self) -> &'static str {
            "mock"
        }

        fn is_available(&self) -> bool {
            self.available
        }

        fn priority(&self) -> u8 {
            50
        }
    }

    #[test]
    fn test_provider_trait_default_priority() {
        // Test that the default priority is 100
        struct DefaultPriorityProvider;

        #[async_trait]
        impl SearchProvider for DefaultPriorityProvider {
            async fn search(
                &self,
                _query: &str,
                _num_results: usize,
            ) -> Result<Vec<SearchResult>, SearchError> {
                Ok(vec![])
            }

            fn name(&self) -> &'static str {
                "default"
            }

            fn is_available(&self) -> bool {
                true
            }
        }

        let provider = DefaultPriorityProvider;
        assert_eq!(provider.priority(), 100);
    }

    #[tokio::test]
    async fn test_mock_provider_search() {
        let provider = MockProvider { available: true };
        let results = provider.search("test", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].title.contains("test"));
    }

    #[test]
    fn test_mock_provider_availability() {
        let available = MockProvider { available: true };
        let unavailable = MockProvider { available: false };

        assert!(available.is_available());
        assert!(!unavailable.is_available());
    }
}
