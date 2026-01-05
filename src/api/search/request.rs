// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Search API request types

use serde::{Deserialize, Serialize};

/// Request body for POST /v1/search
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchApiRequest {
    /// Search query string (required, max 500 chars)
    pub query: String,

    /// Number of results to return (1-20, default 10)
    #[serde(default = "default_num_results")]
    pub num_results: usize,

    /// Chain ID for billing context (default: 84532 Base Sepolia)
    #[serde(default = "default_chain_id")]
    pub chain_id: u64,

    /// Optional request ID for tracking
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

fn default_num_results() -> usize {
    10
}

fn default_chain_id() -> u64 {
    84532
}

impl SearchApiRequest {
    /// Validate the request
    pub fn validate(&self) -> Result<(), String> {
        if self.query.trim().is_empty() {
            return Err("Query cannot be empty".to_string());
        }
        if self.query.len() > 500 {
            return Err("Query too long (max 500 characters)".to_string());
        }
        if self.num_results < 1 {
            return Err("num_results must be at least 1".to_string());
        }
        if self.num_results > 20 {
            return Err("num_results cannot exceed 20".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_deserialization() {
        let json = r#"{
            "query": "test query",
            "numResults": 5
        }"#;

        let request: SearchApiRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.query, "test query");
        assert_eq!(request.num_results, 5);
        assert_eq!(request.chain_id, 84532); // default
    }

    #[test]
    fn test_request_defaults() {
        let json = r#"{"query": "test"}"#;

        let request: SearchApiRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.num_results, 10);
        assert_eq!(request.chain_id, 84532);
    }

    #[test]
    fn test_request_with_all_fields() {
        let json = r#"{
            "query": "test",
            "numResults": 15,
            "chainId": 5611,
            "requestId": "req-123"
        }"#;

        let request: SearchApiRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.num_results, 15);
        assert_eq!(request.chain_id, 5611);
        assert_eq!(request.request_id, Some("req-123".to_string()));
    }

    #[test]
    fn test_validation_empty_query() {
        let request = SearchApiRequest {
            query: "".to_string(),
            num_results: 10,
            chain_id: 84532,
            request_id: None,
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn test_validation_whitespace_query() {
        let request = SearchApiRequest {
            query: "   ".to_string(),
            num_results: 10,
            chain_id: 84532,
            request_id: None,
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn test_validation_query_too_long() {
        let request = SearchApiRequest {
            query: "a".repeat(501),
            num_results: 10,
            chain_id: 84532,
            request_id: None,
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn test_validation_num_results_zero() {
        let request = SearchApiRequest {
            query: "test".to_string(),
            num_results: 0,
            chain_id: 84532,
            request_id: None,
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn test_validation_num_results_too_high() {
        let request = SearchApiRequest {
            query: "test".to_string(),
            num_results: 21,
            chain_id: 84532,
            request_id: None,
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn test_validation_success() {
        let request = SearchApiRequest {
            query: "valid query".to_string(),
            num_results: 10,
            chain_id: 84532,
            request_id: None,
        };
        assert!(request.validate().is_ok());
    }
}
