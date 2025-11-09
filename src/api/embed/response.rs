// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! EmbedResponse and EmbeddingResult types (Sub-phase 2.2)
//!
//! This module defines the response structure for the embedding API with
//! helper methods for chain context, validation, and convenience builders.

use crate::api::ApiError;
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

impl EmbedResponse {
    /// Adds chain context to the response
    ///
    /// Populates chain_name and native_token based on the chain_id.
    /// Supports Base Sepolia (84532) and opBNB Testnet (5611).
    ///
    /// # Arguments
    /// - `chain_id`: The chain ID to get context for
    ///
    /// # Returns
    /// Self with chain context populated (builder pattern)
    ///
    /// # Example
    /// ```ignore
    /// let response = EmbedResponse { /* ... */ }
    ///     .add_chain_context(84532);
    /// assert_eq!(response.chain_name, "Base Sepolia");
    /// ```
    pub fn add_chain_context(mut self, chain_id: u64) -> Self {
        // Map chain_id to chain context
        // Uses same pattern as handler stub from Sub-phase 1.2
        let (chain_name, native_token) = match chain_id {
            84532 => ("Base Sepolia", "ETH"),
            5611 => ("opBNB Testnet", "BNB"),
            _ => {
                // Unknown chain - fall back to Base Sepolia
                ("Base Sepolia", "ETH")
            }
        };

        self.chain_id = chain_id;
        self.chain_name = chain_name.to_string();
        self.native_token = native_token.to_string();

        self
    }

    /// Validates that all embeddings are exactly 384 dimensions
    ///
    /// The vector database requires exactly 384-dimensional embeddings.
    /// This method performs defensive validation to ensure all embeddings
    /// meet this requirement.
    ///
    /// # Returns
    /// - `Ok(())` if all embeddings are 384 dimensions
    /// - `Err(ApiError::ValidationError)` if any embedding has wrong dimensions
    ///
    /// # Example
    /// ```ignore
    /// let response = EmbedResponse { /* ... */ };
    /// response.validate_embedding_dimensions()?;
    /// ```
    pub fn validate_embedding_dimensions(&self) -> Result<(), ApiError> {
        for (index, result) in self.embeddings.iter().enumerate() {
            if result.embedding.len() != 384 {
                return Err(ApiError::ValidationError {
                    field: format!("embeddings[{}].embedding", index),
                    message: format!(
                        "embedding must be exactly 384 dimensions (got {})",
                        result.embedding.len()
                    ),
                });
            }
        }
        Ok(())
    }

    /// Returns the total number of float values across all embeddings
    ///
    /// # Example
    /// ```ignore
    /// let response = EmbedResponse { /* 3 embeddings */ };
    /// assert_eq!(response.total_dimensions(), 384 * 3); // 1152
    /// ```
    pub fn total_dimensions(&self) -> usize {
        self.embeddings.iter().map(|e| e.embedding.len()).sum()
    }

    /// Returns the number of embeddings in the response
    ///
    /// # Example
    /// ```ignore
    /// let response = EmbedResponse { /* ... */ };
    /// assert_eq!(response.embedding_count(), 5);
    /// ```
    pub fn embedding_count(&self) -> usize {
        self.embeddings.len()
    }

    /// Sets the model name (builder pattern)
    ///
    /// # Example
    /// ```ignore
    /// let response = EmbedResponse::from(embeddings)
    ///     .with_model("all-MiniLM-L6-v2".to_string());
    /// ```
    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }
}

impl From<Vec<EmbeddingResult>> for EmbedResponse {
    /// Creates an EmbedResponse from a vector of EmbeddingResults
    ///
    /// This builder pattern convenience method creates a response with:
    /// - provider: "host"
    /// - cost: 0.0
    /// - total_tokens: sum of all token_counts
    /// - chain_id: 84532 (Base Sepolia default)
    /// - model: "all-MiniLM-L6-v2" (default)
    ///
    /// Use `with_model()` and `add_chain_context()` to customize.
    ///
    /// # Example
    /// ```ignore
    /// let embeddings = vec![/* ... */];
    /// let response: EmbedResponse = embeddings.into();
    /// ```
    fn from(embeddings: Vec<EmbeddingResult>) -> Self {
        let total_tokens: usize = embeddings.iter().map(|e| e.token_count).sum();

        EmbedResponse {
            embeddings,
            model: "all-MiniLM-L6-v2".to_string(),
            provider: "host".to_string(),
            total_tokens,
            cost: 0.0,
            chain_id: 84532, // Default to Base Sepolia
            chain_name: String::new(),
            native_token: String::new(),
        }
    }
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
