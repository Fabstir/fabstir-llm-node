// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! OCR request types and validation

use serde::{Deserialize, Serialize};

use crate::api::errors::ApiError;

/// Supported image formats
const SUPPORTED_FORMATS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];

/// Supported OCR languages
const SUPPORTED_LANGUAGES: &[&str] = &["en", "zh", "ja", "ko"];

/// Maximum image size (10MB base64 encoded)
const MAX_IMAGE_SIZE: usize = 10 * 1024 * 1024;

fn default_format() -> String {
    "png".to_string()
}

fn default_language() -> String {
    "en".to_string()
}

fn default_chain_id() -> u64 {
    84532 // Base Sepolia
}

/// Request for OCR processing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrRequest {
    /// Base64-encoded image data
    #[serde(default)]
    pub image: Option<String>,

    /// Image format hint (png, jpg, webp, gif)
    #[serde(default = "default_format")]
    pub format: String,

    /// Language hint for OCR (en, zh, ja, ko)
    #[serde(default = "default_language")]
    pub language: String,

    /// Chain ID for pricing/metering
    #[serde(default = "default_chain_id")]
    pub chain_id: u64,
}

impl OcrRequest {
    /// Validate the OCR request
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

        // Validate language
        if !SUPPORTED_LANGUAGES.contains(&self.language.to_lowercase().as_str()) {
            return Err(ApiError::ValidationError {
                field: "language".to_string(),
                message: format!(
                    "unsupported language '{}', supported: {:?}",
                    self.language, SUPPORTED_LANGUAGES
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
        let request: OcrRequest = serde_json::from_str(r#"{"image": "dGVzdA=="}"#).unwrap();
        assert_eq!(request.format, "png");
        assert_eq!(request.language, "en");
        assert_eq!(request.chain_id, 84532);
    }

    #[test]
    fn test_validation_missing_image() {
        let request = OcrRequest {
            image: None,
            format: "png".to_string(),
            language: "en".to_string(),
            chain_id: 84532,
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn test_validation_empty_image() {
        let request = OcrRequest {
            image: Some("".to_string()),
            format: "png".to_string(),
            language: "en".to_string(),
            chain_id: 84532,
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn test_validation_invalid_format() {
        let request = OcrRequest {
            image: Some("dGVzdA==".to_string()),
            format: "bmp".to_string(),
            language: "en".to_string(),
            chain_id: 84532,
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn test_validation_invalid_language() {
        let request = OcrRequest {
            image: Some("dGVzdA==".to_string()),
            format: "png".to_string(),
            language: "fr".to_string(),
            chain_id: 84532,
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn test_validation_invalid_chain_id() {
        let request = OcrRequest {
            image: Some("dGVzdA==".to_string()),
            format: "png".to_string(),
            language: "en".to_string(),
            chain_id: 1,
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn test_validation_valid_request() {
        let request = OcrRequest {
            image: Some("dGVzdA==".to_string()),
            format: "png".to_string(),
            language: "en".to_string(),
            chain_id: 84532,
        };
        assert!(request.validate().is_ok());
    }

    #[test]
    fn test_camel_case_deserialization() {
        let json = r#"{
            "image": "dGVzdA==",
            "format": "jpg",
            "language": "zh",
            "chainId": 5611
        }"#;
        let request: OcrRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.format, "jpg");
        assert_eq!(request.language, "zh");
        assert_eq!(request.chain_id, 5611);
    }
}
