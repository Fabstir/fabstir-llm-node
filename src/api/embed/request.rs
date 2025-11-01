// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! EmbedRequest type for POST /v1/embed endpoint (Sub-phase 2.1)
//!
//! This module defines the request structure for the embedding API with
//! comprehensive validation logic.

use crate::api::ApiError;
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

impl EmbedRequest {
    /// Validates the embed request
    ///
    /// # Validation Rules
    /// 1. **texts**: Must contain 1-96 items
    /// 2. **text length**: Each text must be 1-8192 characters
    /// 3. **whitespace**: Texts cannot be empty or whitespace-only
    /// 4. **chain_id**: Must be 84532 (Base Sepolia) or 5611 (opBNB Testnet)
    /// 5. **model**: Must not be empty
    ///
    /// # Returns
    /// - `Ok(())` if validation passes
    /// - `Err(ApiError)` with clear error message if validation fails
    ///
    /// # Example
    /// ```ignore
    /// let request = EmbedRequest { /* ... */ };
    /// request.validate()?;
    /// ```
    pub fn validate(&self) -> Result<(), ApiError> {
        // Validate texts count (1-96)
        if self.texts.is_empty() {
            return Err(ApiError::ValidationError {
                field: "texts".to_string(),
                message: "texts array must contain at least 1 item".to_string(),
            });
        }

        if self.texts.len() > 96 {
            return Err(ApiError::ValidationError {
                field: "texts".to_string(),
                message: format!(
                    "texts array cannot contain more than 96 items (got {})",
                    self.texts.len()
                ),
            });
        }

        // Validate each text
        for (index, text) in self.texts.iter().enumerate() {
            // Check if text is empty or whitespace-only
            if text.trim().is_empty() {
                return Err(ApiError::ValidationError {
                    field: format!("texts[{}]", index),
                    message: "text cannot be empty or contain only whitespace".to_string(),
                });
            }

            // Check text length (1-8192 characters)
            if text.len() > 8192 {
                return Err(ApiError::ValidationError {
                    field: format!("texts[{}]", index),
                    message: format!(
                        "text cannot exceed 8192 characters (got {} characters)",
                        text.len()
                    ),
                });
            }
        }

        // Validate chain_id (must be 84532 or 5611)
        if self.chain_id != 84532 && self.chain_id != 5611 {
            return Err(ApiError::ValidationError {
                field: "chain_id".to_string(),
                message: format!(
                    "chain_id must be 84532 (Base Sepolia) or 5611 (opBNB Testnet), got {}",
                    self.chain_id
                ),
            });
        }

        // Validate model name (must not be empty)
        if self.model.trim().is_empty() {
            return Err(ApiError::ValidationError {
                field: "model".to_string(),
                message: "model name cannot be empty".to_string(),
            });
        }

        Ok(())
    }

    /// Returns the supported chain IDs
    pub fn supported_chain_ids() -> Vec<u64> {
        vec![84532, 5611]
    }

    /// Checks if a chain_id is supported
    pub fn is_chain_supported(chain_id: u64) -> bool {
        chain_id == 84532 || chain_id == 5611
    }
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
