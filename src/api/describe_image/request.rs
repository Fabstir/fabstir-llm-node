// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Describe image request types and validation

use serde::{Deserialize, Serialize};

use crate::api::errors::ApiError;

/// Supported image formats
const SUPPORTED_FORMATS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];

/// Supported detail levels
const SUPPORTED_DETAILS: &[&str] = &["brief", "detailed", "comprehensive"];

/// Maximum image size (10MB base64 encoded)
const MAX_IMAGE_SIZE: usize = 10 * 1024 * 1024;

/// Maximum tokens for description
const MAX_TOKENS_LIMIT: usize = 500;

/// Minimum tokens for description
const MIN_TOKENS_LIMIT: usize = 10;

fn default_format() -> String {
    "png".to_string()
}

fn default_detail() -> String {
    "detailed".to_string()
}

fn default_max_tokens() -> usize {
    150
}

fn default_chain_id() -> u64 {
    84532 // Base Sepolia
}

/// Request for image description
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeImageRequest {
    /// Base64-encoded image data
    #[serde(default)]
    pub image: Option<String>,

    /// Image format hint (png, jpg, webp, gif)
    #[serde(default = "default_format")]
    pub format: String,

    /// Detail level: brief, detailed, comprehensive
    #[serde(default = "default_detail")]
    pub detail: String,

    /// Custom prompt for description (optional)
    #[serde(default)]
    pub prompt: Option<String>,

    /// Maximum tokens in response (10-500)
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,

    /// Chain ID for pricing/metering
    #[serde(default = "default_chain_id")]
    pub chain_id: u64,
}

impl DescribeImageRequest {
    /// Validate the describe image request
    pub fn validate(&self) -> Result<(), ApiError> {
        // Validate image is provided
        if self.image.is_none() || self.image.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
            return Err(ApiError::ValidationError {
                field: "image".to_string(),
                message: "image is required".to_string(),
            });
        }

        // Validate image size
        if let Some(ref image) = self.image {
            if image.len() > MAX_IMAGE_SIZE {
                return Err(ApiError::ValidationError {
                    field: "image".to_string(),
                    message: format!("image exceeds maximum size of {} bytes", MAX_IMAGE_SIZE),
                });
            }
        }

        // Validate format
        if !SUPPORTED_FORMATS.contains(&self.format.to_lowercase().as_str()) {
            return Err(ApiError::ValidationError {
                field: "format".to_string(),
                message: format!(
                    "unsupported format '{}', supported: {:?}",
                    self.format, SUPPORTED_FORMATS
                ),
            });
        }

        // Validate detail level
        if !SUPPORTED_DETAILS.contains(&self.detail.to_lowercase().as_str()) {
            return Err(ApiError::ValidationError {
                field: "detail".to_string(),
                message: format!(
                    "unsupported detail level '{}', supported: {:?}",
                    self.detail, SUPPORTED_DETAILS
                ),
            });
        }

        // Validate max_tokens range
        if self.max_tokens < MIN_TOKENS_LIMIT || self.max_tokens > MAX_TOKENS_LIMIT {
            return Err(ApiError::ValidationError {
                field: "max_tokens".to_string(),
                message: format!(
                    "max_tokens must be between {} and {}, got {}",
                    MIN_TOKENS_LIMIT, MAX_TOKENS_LIMIT, self.max_tokens
                ),
            });
        }

        // Validate chain_id
        if self.chain_id != 84532 && self.chain_id != 5611 {
            return Err(ApiError::ValidationError {
                field: "chain_id".to_string(),
                message: format!(
                    "chain_id must be 84532 (Base Sepolia) or 5611 (opBNB Testnet), got {}",
                    self.chain_id
                ),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_values() {
        let request: DescribeImageRequest =
            serde_json::from_str(r#"{"image": "dGVzdA=="}"#).unwrap();
        assert_eq!(request.format, "png");
        assert_eq!(request.detail, "detailed");
        assert_eq!(request.max_tokens, 150);
        assert_eq!(request.chain_id, 84532);
    }

    #[test]
    fn test_validation_missing_image() {
        let request = DescribeImageRequest {
            image: None,
            format: "png".to_string(),
            detail: "detailed".to_string(),
            prompt: None,
            max_tokens: 150,
            chain_id: 84532,
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn test_validation_invalid_format() {
        let request = DescribeImageRequest {
            image: Some("dGVzdA==".to_string()),
            format: "bmp".to_string(),
            detail: "detailed".to_string(),
            prompt: None,
            max_tokens: 150,
            chain_id: 84532,
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn test_validation_invalid_detail() {
        let request = DescribeImageRequest {
            image: Some("dGVzdA==".to_string()),
            format: "png".to_string(),
            detail: "verbose".to_string(),
            prompt: None,
            max_tokens: 150,
            chain_id: 84532,
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn test_validation_max_tokens_too_low() {
        let request = DescribeImageRequest {
            image: Some("dGVzdA==".to_string()),
            format: "png".to_string(),
            detail: "detailed".to_string(),
            prompt: None,
            max_tokens: 5,
            chain_id: 84532,
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn test_validation_max_tokens_too_high() {
        let request = DescribeImageRequest {
            image: Some("dGVzdA==".to_string()),
            format: "png".to_string(),
            detail: "detailed".to_string(),
            prompt: None,
            max_tokens: 1000,
            chain_id: 84532,
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn test_validation_valid_request() {
        let request = DescribeImageRequest {
            image: Some("dGVzdA==".to_string()),
            format: "png".to_string(),
            detail: "brief".to_string(),
            prompt: Some("Describe the main subject".to_string()),
            max_tokens: 100,
            chain_id: 84532,
        };
        assert!(request.validate().is_ok());
    }

    #[test]
    fn test_camel_case_deserialization() {
        let json = r#"{
            "image": "dGVzdA==",
            "format": "jpg",
            "detail": "comprehensive",
            "maxTokens": 200,
            "chainId": 5611
        }"#;
        let request: DescribeImageRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.detail, "comprehensive");
        assert_eq!(request.max_tokens, 200);
        assert_eq!(request.chain_id, 5611);
    }
}
