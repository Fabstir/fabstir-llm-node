// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! EmbedResponse and EmbeddingResult types (Sub-phase 1.2)
//!
//! This module defines the response structure for the embedding API.
//! Full implementation will be completed in later sub-phases.

use serde::{Deserialize, Serialize};

/// Individual embedding result for one text input
///
/// # Fields
/// - `embedding`: 384-dimensional vector (f32 array)
/// - `text`: Original input text
/// - `token_count`: Number of tokens processed
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingResult {
    /// 384-dimensional embedding vector
    pub embedding: Vec<f32>,

    /// Original input text
    pub text: String,

    /// Number of tokens in the input text
    pub token_count: usize,
}

/// Response body for POST /v1/embed endpoint
///
/// # Fields
/// - `embeddings`: Array of embedding results (one per input text)
/// - `model`: Model used for embedding
/// - `provider`: Always "host" for host-side embeddings
/// - `total_tokens`: Total tokens processed across all texts
/// - `cost`: Always 0.0 for host embeddings
/// - `chain_id`: Chain ID from request
/// - `chain_name`: Human-readable chain name
/// - `native_token`: Native token symbol (ETH/BNB)
///
/// # Example
/// ```json
/// {
///   "embeddings": [
///     {
///       "embedding": [0.1, 0.2, ...],
///       "text": "Hello world",
///       "tokenCount": 2
///     }
///   ],
///   "model": "all-MiniLM-L6-v2",
///   "provider": "host",
///   "totalTokens": 2,
///   "cost": 0.0,
///   "chainId": 84532,
///   "chainName": "Base Sepolia",
///   "nativeToken": "ETH"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedResponse {
    /// Array of embedding results
    pub embeddings: Vec<EmbeddingResult>,

    /// Model used for embedding
    pub model: String,

    /// Provider (always "host" for host-side embeddings)
    pub provider: String,

    /// Total tokens processed
    pub total_tokens: usize,

    /// Cost in USD (always 0.0 for host embeddings)
    pub cost: f64,

    /// Chain ID
    pub chain_id: u64,

    /// Chain name (e.g., "Base Sepolia")
    pub chain_name: String,

    /// Native token symbol (e.g., "ETH", "BNB")
    pub native_token: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedding_result_serialization() {
        let result = EmbeddingResult {
            embedding: vec![0.1, 0.2, 0.3],
            text: "test".to_string(),
            token_count: 1,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("tokenCount")); // camelCase
        assert!(json.contains(r#""text":"test""#));
    }

    #[test]
    fn test_embed_response_serialization() {
        let response = EmbedResponse {
            embeddings: vec![EmbeddingResult {
                embedding: vec![0.1, 0.2, 0.3],
                text: "test".to_string(),
                token_count: 1,
            }],
            model: "all-MiniLM-L6-v2".to_string(),
            provider: "host".to_string(),
            total_tokens: 1,
            cost: 0.0,
            chain_id: 84532,
            chain_name: "Base Sepolia".to_string(),
            native_token: "ETH".to_string(),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("tokenCount"));
        assert!(json.contains("totalTokens"));
        assert!(json.contains("chainId"));
        assert!(json.contains(r#""model":"all-MiniLM-L6-v2""#));
        assert!(json.contains(r#""provider":"host""#));
        assert!(json.contains(r#""cost":0.0"#));
    }
}
