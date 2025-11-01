// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! EmbedRequest type for POST /v1/embed endpoint (Sub-phase 1.2)
//!
//! This module defines the request structure for the embedding API.
//! Full validation logic will be implemented in Sub-phase 2.1.

use serde::{Deserialize, Serialize};

/// Request body for POST /v1/embed endpoint
///
/// # Fields
/// - `texts`: Array of 1-96 text strings to embed
/// - `model`: Embedding model name (default: "all-MiniLM-L6-v2")
/// - `chain_id`: Chain ID for pricing/metering (default: 84532 - Base Sepolia)
///
/// # Example
/// ```json
/// {
///   "texts": ["Hello world", "Another text"],
///   "model": "all-MiniLM-L6-v2",
///   "chain_id": 84532
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedRequest {
    /// Text strings to embed (1-96 items)
    pub texts: Vec<String>,

    /// Embedding model name
    /// Default: "all-MiniLM-L6-v2"
    #[serde(default = "default_model")]
    pub model: String,

    /// Chain ID for pricing/metering
    /// Default: 84532 (Base Sepolia)
    #[serde(default = "default_chain_id")]
    pub chain_id: u64,
}

/// Default model: all-MiniLM-L6-v2 (384 dimensions)
fn default_model() -> String {
    "all-MiniLM-L6-v2".to_string()
}

/// Default chain ID: Base Sepolia (84532)
fn default_chain_id() -> u64 {
    84532
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialization_with_defaults() {
        let json = r#"{"texts": ["test"]}"#;
        let req: EmbedRequest = serde_json::from_str(json).unwrap();

        assert_eq!(req.texts.len(), 1);
        assert_eq!(req.texts[0], "test");
        assert_eq!(req.model, "all-MiniLM-L6-v2");
        assert_eq!(req.chain_id, 84532);
    }

    #[test]
    fn test_deserialization_with_explicit_values() {
        let json = r#"{
            "texts": ["test1", "test2"],
            "model": "all-MiniLM-L6-v2",
            "chainId": 84532
        }"#;
        let req: EmbedRequest = serde_json::from_str(json).unwrap();

        assert_eq!(req.texts.len(), 2);
        assert_eq!(req.model, "all-MiniLM-L6-v2");
        assert_eq!(req.chain_id, 84532);
    }
}
