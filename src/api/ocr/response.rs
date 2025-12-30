// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! OCR response types

use serde::{Deserialize, Serialize};

/// Bounding box for a text region
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A detected text region
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextRegion {
    /// Extracted text
    pub text: String,
    /// Confidence score (0.0-1.0)
    pub confidence: f32,
    /// Bounding box location
    pub bounding_box: BoundingBox,
}

/// Response from OCR processing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrResponse {
    /// Full extracted text (all regions combined)
    pub text: String,
    /// Average confidence score (0.0-1.0)
    pub confidence: f32,
    /// Individual text regions with bounding boxes
    pub regions: Vec<TextRegion>,
    /// Processing time in milliseconds
    pub processing_time_ms: u64,
    /// Model used for OCR
    pub model: String,
    /// Provider (always "host")
    pub provider: String,
    /// Chain ID
    pub chain_id: u64,
    /// Chain name (e.g., "Base Sepolia")
    pub chain_name: String,
    /// Native token symbol (e.g., "ETH")
    pub native_token: String,
}

impl OcrResponse {
    /// Create a new OCR response with chain context
    pub fn new(
        text: String,
        confidence: f32,
        regions: Vec<TextRegion>,
        processing_time_ms: u64,
        chain_id: u64,
    ) -> Self {
        let (chain_name, native_token) = match chain_id {
            84532 => ("Base Sepolia", "ETH"),
            5611 => ("opBNB Testnet", "BNB"),
            _ => ("Base Sepolia", "ETH"),
        };

        Self {
            text,
            confidence,
            regions,
            processing_time_ms,
            model: "paddleocr".to_string(),
            provider: "host".to_string(),
            chain_id,
            chain_name: chain_name.to_string(),
            native_token: native_token.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ocr_response_serialization() {
        let response = OcrResponse::new(
            "Hello World".to_string(),
            0.95,
            vec![],
            150,
            84532,
        );
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"text\":\"Hello World\""));
        assert!(json.contains("\"processingTimeMs\":150"));
        assert!(json.contains("\"chainName\":\"Base Sepolia\""));
    }

    #[test]
    fn test_chain_context_base_sepolia() {
        let response = OcrResponse::new("test".to_string(), 0.9, vec![], 100, 84532);
        assert_eq!(response.chain_name, "Base Sepolia");
        assert_eq!(response.native_token, "ETH");
    }

    #[test]
    fn test_chain_context_opbnb() {
        let response = OcrResponse::new("test".to_string(), 0.9, vec![], 100, 5611);
        assert_eq!(response.chain_name, "opBNB Testnet");
        assert_eq!(response.native_token, "BNB");
    }

    #[test]
    fn test_text_region_serialization() {
        let region = TextRegion {
            text: "Hello".to_string(),
            confidence: 0.98,
            bounding_box: BoundingBox {
                x: 10,
                y: 20,
                width: 100,
                height: 30,
            },
        };
        let json = serde_json::to_string(&region).unwrap();
        assert!(json.contains("\"boundingBox\""));
    }
}
