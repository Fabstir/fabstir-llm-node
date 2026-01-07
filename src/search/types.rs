// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Core types for web search functionality

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A single search result from a web search provider
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    /// Title of the search result
    pub title: String,
    /// URL of the search result
    pub url: String,
    /// Snippet/description of the search result
    pub snippet: String,
    /// Published date if available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_date: Option<String>,
    /// Source provider (e.g., "brave", "bing", "duckduckgo")
    pub source: String,
}

/// Response from a search operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    /// The original search query
    pub query: String,
    /// List of search results
    pub results: Vec<SearchResult>,
    /// Time taken for the search in milliseconds
    pub search_time_ms: u64,
    /// Provider that returned the results
    pub provider: String,
    /// Whether the result was from cache
    pub cached: bool,
    /// Number of results returned
    pub result_count: usize,
}

/// Errors that can occur during search operations
#[derive(Debug, Error)]
pub enum SearchError {
    /// Rate limited by the search provider
    #[error("Rate limited, retry after {retry_after_secs}s")]
    RateLimited {
        /// Seconds to wait before retrying
        retry_after_secs: u64,
    },

    /// API error from the search provider
    #[error("Search API error: {status} - {message}")]
    ApiError {
        /// HTTP status code
        status: u16,
        /// Error message
        message: String,
    },

    /// Search request timed out
    #[error("Search timeout after {timeout_ms}ms")]
    Timeout {
        /// Timeout duration in milliseconds
        timeout_ms: u64,
    },

    /// Search provider is unavailable
    #[error("Provider unavailable: {provider}")]
    ProviderUnavailable {
        /// Name of the unavailable provider
        provider: String,
    },

    /// No API key configured for the provider
    #[error("No API key configured for {provider}")]
    NoApiKey {
        /// Name of the provider missing API key
        provider: String,
    },

    /// Invalid search query
    #[error("Invalid query: {reason}")]
    InvalidQuery {
        /// Reason the query is invalid
        reason: String,
    },

    /// Search is disabled on this host
    #[error("Search disabled on this host")]
    SearchDisabled,
}

/// A search query for batch operations
#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// The search query string
    pub query: String,
    /// Number of results to return
    pub num_results: usize,
    /// Optional request ID for tracking
    pub request_id: Option<String>,
}

/// A search result with optional fetched page content (Phase 9)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultWithContent {
    /// Title of the search result
    pub title: String,
    /// URL of the search result
    pub url: String,
    /// Snippet/description from search (meta description)
    pub snippet: String,
    /// Actual page content if fetched (None if fetch failed/disabled)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Published date if available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_date: Option<String>,
    /// Source provider
    pub source: String,
}

/// Response from a search operation with content fetching (Phase 9)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponseWithContent {
    /// The original search query
    pub query: String,
    /// List of search results with content
    pub results: Vec<SearchResultWithContent>,
    /// Time taken for the search in milliseconds
    pub search_time_ms: u64,
    /// Time taken for content fetching in milliseconds
    pub content_fetch_time_ms: u64,
    /// Provider that returned the results
    pub provider: String,
    /// Whether the search result was from cache
    pub cached: bool,
    /// Number of results returned
    pub result_count: usize,
    /// Number of results with content fetched
    pub content_fetched_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_result_serialization() {
        let result = SearchResult {
            title: "Test Title".to_string(),
            url: "https://example.com".to_string(),
            snippet: "Test snippet".to_string(),
            published_date: Some("2025-01-05".to_string()),
            source: "brave".to_string(),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("testTitle") || json.contains("title"));
    }

    #[test]
    fn test_search_result_deserialization() {
        let json = r#"{
            "title": "Test",
            "url": "https://example.com",
            "snippet": "A test",
            "source": "brave"
        }"#;

        let result: SearchResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.title, "Test");
        assert_eq!(result.source, "brave");
    }

    #[test]
    fn test_search_response_serialization() {
        let response = SearchResponse {
            query: "test query".to_string(),
            results: vec![],
            search_time_ms: 100,
            provider: "brave".to_string(),
            cached: false,
            result_count: 0,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("query"));
    }

    #[test]
    fn test_search_error_display() {
        let error = SearchError::RateLimited { retry_after_secs: 60 };
        assert!(error.to_string().contains("60"));

        let error = SearchError::ApiError {
            status: 500,
            message: "Internal error".to_string(),
        };
        assert!(error.to_string().contains("500"));
    }

    #[test]
    fn test_search_query_creation() {
        let query = SearchQuery {
            query: "test".to_string(),
            num_results: 10,
            request_id: Some("req-123".to_string()),
        };

        assert_eq!(query.query, "test");
        assert_eq!(query.num_results, 10);
    }
}
