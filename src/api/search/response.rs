// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Search API response types

use serde::{Deserialize, Serialize};

use crate::search::types::SearchResult;

/// Response body for POST /v1/search
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchApiResponse {
    /// The original search query
    pub query: String,

    /// List of search results
    pub results: Vec<SearchResult>,

    /// Number of results returned
    pub result_count: usize,

    /// Time taken for the search in milliseconds
    pub search_time_ms: u64,

    /// Search provider used
    pub provider: String,

    /// Whether the result was served from cache
    pub cached: bool,

    /// Chain ID for billing context
    pub chain_id: u64,

    /// Chain name for display
    pub chain_name: String,
}

impl SearchApiResponse {
    /// Create a new search API response
    pub fn new(
        query: String,
        results: Vec<SearchResult>,
        search_time_ms: u64,
        provider: String,
        cached: bool,
        chain_id: u64,
    ) -> Self {
        let chain_name = match chain_id {
            84532 => "Base Sepolia".to_string(),
            5611 => "opBNB Testnet".to_string(),
            _ => "Unknown".to_string(),
        };

        Self {
            result_count: results.len(),
            query,
            results,
            search_time_ms,
            provider,
            cached,
            chain_id,
            chain_name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_serialization() {
        let response = SearchApiResponse::new(
            "test query".to_string(),
            vec![],
            100,
            "brave".to_string(),
            false,
            84532,
        );

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("query"));
        assert!(json.contains("results"));
        assert!(json.contains("searchTimeMs"));
    }

    #[test]
    fn test_response_chain_name_base_sepolia() {
        let response = SearchApiResponse::new(
            "test".to_string(),
            vec![],
            0,
            "brave".to_string(),
            false,
            84532,
        );
        assert_eq!(response.chain_name, "Base Sepolia");
    }

    #[test]
    fn test_response_chain_name_opbnb() {
        let response = SearchApiResponse::new(
            "test".to_string(),
            vec![],
            0,
            "brave".to_string(),
            false,
            5611,
        );
        assert_eq!(response.chain_name, "opBNB Testnet");
    }

    #[test]
    fn test_response_chain_name_unknown() {
        let response = SearchApiResponse::new(
            "test".to_string(),
            vec![],
            0,
            "brave".to_string(),
            false,
            12345,
        );
        assert_eq!(response.chain_name, "Unknown");
    }

    #[test]
    fn test_response_result_count() {
        let results = vec![
            SearchResult {
                title: "Result 1".to_string(),
                url: "https://example.com/1".to_string(),
                snippet: "Snippet 1".to_string(),
                published_date: None,
                source: "brave".to_string(),
            },
            SearchResult {
                title: "Result 2".to_string(),
                url: "https://example.com/2".to_string(),
                snippet: "Snippet 2".to_string(),
                published_date: None,
                source: "brave".to_string(),
            },
        ];

        let response = SearchApiResponse::new(
            "test".to_string(),
            results,
            100,
            "brave".to_string(),
            false,
            84532,
        );

        assert_eq!(response.result_count, 2);
    }
}
