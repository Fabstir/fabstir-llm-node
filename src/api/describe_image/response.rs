// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Describe image response types

use serde::{Deserialize, Serialize};

use crate::api::ocr::response::BoundingBox;

/// A detected object in the image
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedObject {
    /// Object label/class
    pub label: String,
    /// Confidence score (0.0-1.0)
    pub confidence: f32,
    /// Optional bounding box
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounding_box: Option<BoundingBox>,
}

/// Image analysis metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageAnalysis {
    /// Image width
    pub width: u32,
    /// Image height
    pub height: u32,
    /// Dominant colors (hex strings)
    pub dominant_colors: Vec<String>,
    /// Scene type (indoor, outdoor, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene_type: Option<String>,
}

/// Response from image description
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeImageResponse {
    /// Generated description text
    pub description: String,
    /// Detected objects
    pub objects: Vec<DetectedObject>,
    /// Image analysis metadata
    pub analysis: ImageAnalysis,
    /// Processing time in milliseconds
    pub processing_time_ms: u64,
    /// Model used for description
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

impl DescribeImageResponse {
    /// Create a new describe image response with chain context
    pub fn new(
        description: String,
        objects: Vec<DetectedObject>,
        analysis: ImageAnalysis,
        processing_time_ms: u64,
        chain_id: u64,
        model: &str,
    ) -> Self {
        let (chain_name, native_token) = match chain_id {
            84532 => ("Base Sepolia", "ETH"),
            5611 => ("opBNB Testnet", "BNB"),
            _ => ("Base Sepolia", "ETH"),
        };

        Self {
            description,
            objects,
            analysis,
            processing_time_ms,
            model: model.to_string(),
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
    fn test_describe_image_response_serialization() {
        let response = DescribeImageResponse::new(
            "A cat sitting on a windowsill".to_string(),
            vec![DetectedObject {
                label: "cat".to_string(),
                confidence: 0.95,
                bounding_box: None,
            }],
            ImageAnalysis {
                width: 1920,
                height: 1080,
                dominant_colors: vec!["#FF0000".to_string()],
                scene_type: Some("indoor".to_string()),
            },
            4500,
            84532,
            "florence-2",
        );
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"description\":\"A cat sitting on a windowsill\""));
        assert!(json.contains("\"processingTimeMs\":4500"));
        assert!(json.contains("\"model\":\"florence-2\""));
    }

    #[test]
    fn test_chain_context_base_sepolia() {
        let response = DescribeImageResponse::new(
            "test".to_string(),
            vec![],
            ImageAnalysis {
                width: 100,
                height: 100,
                dominant_colors: vec![],
                scene_type: None,
            },
            100,
            84532,
            "florence-2",
        );
        assert_eq!(response.chain_name, "Base Sepolia");
        assert_eq!(response.native_token, "ETH");
    }

    #[test]
    fn test_chain_context_opbnb() {
        let response = DescribeImageResponse::new(
            "test".to_string(),
            vec![],
            ImageAnalysis {
                width: 100,
                height: 100,
                dominant_colors: vec![],
                scene_type: None,
            },
            100,
            5611,
            "florence-2",
        );
        assert_eq!(response.chain_name, "opBNB Testnet");
        assert_eq!(response.native_token, "BNB");
    }

    #[test]
    fn test_describe_response_custom_model() {
        let response = DescribeImageResponse::new(
            "test".to_string(),
            vec![],
            ImageAnalysis {
                width: 100,
                height: 100,
                dominant_colors: vec![],
                scene_type: None,
            },
            100,
            84532,
            "qwen3-vl",
        );
        assert_eq!(response.model, "qwen3-vl");
    }

    #[test]
    fn test_detected_object_serialization() {
        let obj = DetectedObject {
            label: "dog".to_string(),
            confidence: 0.88,
            bounding_box: Some(BoundingBox {
                x: 10,
                y: 20,
                width: 100,
                height: 150,
            }),
        };
        let json = serde_json::to_string(&obj).unwrap();
        assert!(json.contains("\"label\":\"dog\""));
        assert!(json.contains("\"boundingBox\""));
    }

    #[test]
    fn test_detected_object_without_bbox() {
        let obj = DetectedObject {
            label: "sky".to_string(),
            confidence: 0.99,
            bounding_box: None,
        };
        let json = serde_json::to_string(&obj).unwrap();
        assert!(!json.contains("boundingBox"));
    }
}
