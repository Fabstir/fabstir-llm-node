// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! OCR Endpoint tests for POST /v1/ocr endpoint (Sub-phase 5.2)
//!
//! These TDD tests verify that the ocr_handler correctly:
//! - Validates requests and returns appropriate errors
//! - Processes images using PaddleOCR
//! - Adds chain context to responses
//! - Handles all error cases gracefully
//!
//! Test-Driven Development (TDD) Approach:
//! 1. Write these tests FIRST (they will fail initially)
//! 2. Implement ocr_handler in src/api/ocr/handler.rs
//! 3. Run tests to verify handler works correctly

use axum::{extract::State, http::StatusCode, Json};
use fabstir_llm_node::{
    api::{
        http_server::AppState,
        ocr::{OcrRequest, OcrResponse},
    },
    vision::{VisionModelConfig, VisionModelManager},
};
use std::sync::Arc;

// Model paths
const OCR_MODEL_DIR: &str = "/workspace/models/paddleocr-onnx";

// Test images (base64 encoded)
// 1x1 red PNG - minimal valid image
const TINY_PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

// 1x1 GIF - minimal valid image
const TINY_GIF_BASE64: &str = "R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==";

/// Helper: Create test AppState with real OCR model
async fn setup_test_state_with_ocr() -> AppState {
    let config = VisionModelConfig {
        ocr_model_dir: Some(OCR_MODEL_DIR.to_string()),
        florence_model_dir: None, // Skip Florence for OCR tests
    };

    let manager = VisionModelManager::new(config)
        .await
        .expect("Failed to create vision model manager");

    let mut state = AppState::new_for_test();
    *state.vision_model_manager.write().await = Some(Arc::new(manager));
    state
}

/// Helper: Create test AppState without OCR model (for error testing)
async fn setup_test_state_without_ocr() -> AppState {
    let config = VisionModelConfig {
        ocr_model_dir: None,
        florence_model_dir: None,
    };

    let manager = VisionModelManager::new(config)
        .await
        .expect("Failed to create vision model manager");

    let mut state = AppState::new_for_test();
    *state.vision_model_manager.write().await = Some(Arc::new(manager));
    state
}

/// Helper: Create test AppState with no vision manager at all
fn setup_test_state_no_vision() -> AppState {
    AppState::new_for_test()
}

#[cfg(test)]
mod ocr_handler_tests {
    use super::*;
    use fabstir_llm_node::api::ocr::ocr_handler;

    // =============================================================================
    // Request Validation Tests (Unit Tests - No Model Required)
    // =============================================================================

    /// Test 1: Validation error when image is missing
    #[tokio::test]
    async fn test_validation_error_missing_image() {
        let state = setup_test_state_no_vision();

        let request = OcrRequest {
            image: None,
            format: "png".to_string(),
            language: "en".to_string(),
            chain_id: 84532,
        };

        let result = ocr_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should fail when image is missing");
        let (status, message) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("image"), "Error should mention 'image'");
    }

    /// Test 2: Validation error when image is empty
    #[tokio::test]
    async fn test_validation_error_empty_image() {
        let state = setup_test_state_no_vision();

        let request = OcrRequest {
            image: Some("".to_string()),
            format: "png".to_string(),
            language: "en".to_string(),
            chain_id: 84532,
        };

        let result = ocr_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should fail when image is empty");
        let (status, _message) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    /// Test 3: Validation error when format is invalid
    #[tokio::test]
    async fn test_validation_error_invalid_format() {
        let state = setup_test_state_no_vision();

        let request = OcrRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "bmp".to_string(), // Not supported
            language: "en".to_string(),
            chain_id: 84532,
        };

        let result = ocr_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should fail when format is invalid");
        let (status, message) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("format"), "Error should mention 'format'");
    }

    /// Test 4: Validation error when language is invalid
    #[tokio::test]
    async fn test_validation_error_invalid_language() {
        let state = setup_test_state_no_vision();

        let request = OcrRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "png".to_string(),
            language: "fr".to_string(), // Not supported
            chain_id: 84532,
        };

        let result = ocr_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should fail when language is invalid");
        let (status, message) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("language"), "Error should mention 'language'");
    }

    /// Test 5: Validation error when chain_id is invalid
    #[tokio::test]
    async fn test_validation_error_invalid_chain_id() {
        let state = setup_test_state_no_vision();

        let request = OcrRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "png".to_string(),
            language: "en".to_string(),
            chain_id: 1, // Invalid chain
        };

        let result = ocr_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should fail when chain_id is invalid");
        let (status, message) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("chain_id"), "Error should mention 'chain_id'");
    }

    // =============================================================================
    // Service Availability Tests
    // =============================================================================

    /// Test 6: Service unavailable when vision manager is not initialized
    #[tokio::test]
    async fn test_service_unavailable_no_vision_manager() {
        let state = setup_test_state_no_vision();

        let request = OcrRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "png".to_string(),
            language: "en".to_string(),
            chain_id: 84532,
        };

        let result = ocr_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should fail when vision manager is not available");
        let (status, message) = result.unwrap_err();
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            message.contains("service") || message.contains("available"),
            "Error should indicate service unavailability"
        );
    }

    /// Test 7: Service unavailable when OCR model is not loaded
    #[tokio::test]
    async fn test_service_unavailable_no_ocr_model() {
        let state = setup_test_state_without_ocr().await;

        let request = OcrRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "png".to_string(),
            language: "en".to_string(),
            chain_id: 84532,
        };

        let result = ocr_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should fail when OCR model is not loaded");
        let (status, message) = result.unwrap_err();
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            message.contains("OCR") || message.contains("model"),
            "Error should mention OCR model"
        );
    }

    // =============================================================================
    // OCR Processing Tests (Integration Tests - Require Model)
    // =============================================================================

    /// Test 8: Successful OCR with minimal image (returns empty text, no error)
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_ocr_success_minimal_image() {
        let state = setup_test_state_with_ocr().await;

        let request = OcrRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "png".to_string(),
            language: "en".to_string(),
            chain_id: 84532,
        };

        let result = ocr_handler(State(state), Json(request)).await;

        assert!(result.is_ok(), "Should succeed with minimal image: {:?}", result.err());

        let response = result.unwrap().0; // Extract from Json wrapper
        // Minimal image has no text - that's OK
        assert_eq!(response.model, "paddleocr");
        assert_eq!(response.provider, "host");
        assert_eq!(response.chain_id, 84532);
        assert_eq!(response.chain_name, "Base Sepolia");
        assert_eq!(response.native_token, "ETH");
    }

    /// Test 9: Response includes correct chain context for Base Sepolia
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_ocr_chain_context_base_sepolia() {
        let state = setup_test_state_with_ocr().await;

        let request = OcrRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "png".to_string(),
            language: "en".to_string(),
            chain_id: 84532,
        };

        let result = ocr_handler(State(state), Json(request)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.chain_id, 84532);
        assert_eq!(response.chain_name, "Base Sepolia");
        assert_eq!(response.native_token, "ETH");
    }

    /// Test 10: Response includes correct chain context for opBNB
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_ocr_chain_context_opbnb() {
        let state = setup_test_state_with_ocr().await;

        let request = OcrRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "png".to_string(),
            language: "en".to_string(),
            chain_id: 5611,
        };

        let result = ocr_handler(State(state), Json(request)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.chain_id, 5611);
        assert_eq!(response.chain_name, "opBNB Testnet");
        assert_eq!(response.native_token, "BNB");
    }

    /// Test 11: Processing time is recorded
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_ocr_processing_time_recorded() {
        let state = setup_test_state_with_ocr().await;

        let request = OcrRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "png".to_string(),
            language: "en".to_string(),
            chain_id: 84532,
        };

        let result = ocr_handler(State(state), Json(request)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        // Processing time should be > 0 (actual OCR was performed)
        assert!(
            response.processing_time_ms > 0,
            "Processing time should be recorded"
        );
    }

    /// Test 12: OCR works with GIF format
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_ocr_gif_format() {
        let state = setup_test_state_with_ocr().await;

        let request = OcrRequest {
            image: Some(TINY_GIF_BASE64.to_string()),
            format: "gif".to_string(),
            language: "en".to_string(),
            chain_id: 84532,
        };

        let result = ocr_handler(State(state), Json(request)).await;

        assert!(result.is_ok(), "Should handle GIF format: {:?}", result.err());
    }

    /// Test 13: Default values work correctly
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_ocr_with_defaults() {
        let state = setup_test_state_with_ocr().await;

        // Only provide image, let other fields use defaults
        let json = serde_json::json!({
            "image": TINY_PNG_BASE64
        });
        let request: OcrRequest = serde_json::from_value(json).unwrap();

        let result = ocr_handler(State(state), Json(request)).await;

        assert!(result.is_ok(), "Should work with default values: {:?}", result.err());

        let response = result.unwrap().0;
        // Default chain_id is 84532
        assert_eq!(response.chain_id, 84532);
    }

    /// Test 14: Bad request for invalid base64
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_ocr_invalid_base64() {
        let state = setup_test_state_with_ocr().await;

        let request = OcrRequest {
            image: Some("not-valid-base64!!!".to_string()),
            format: "png".to_string(),
            language: "en".to_string(),
            chain_id: 84532,
        };

        let result = ocr_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should fail with invalid base64");
        let (status, _message) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    /// Test 15: Bad request for valid base64 but not an image
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_ocr_not_an_image() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};

        let state = setup_test_state_with_ocr().await;

        // Valid base64, but random bytes (not an image)
        let not_image = STANDARD.encode([0x00, 0x01, 0x02, 0x03, 0x04, 0x05]);

        let request = OcrRequest {
            image: Some(not_image),
            format: "png".to_string(),
            language: "en".to_string(),
            chain_id: 84532,
        };

        let result = ocr_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should fail when data is not an image");
        let (status, _message) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    /// Test 16: Response model info is correct
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_ocr_model_info() {
        let state = setup_test_state_with_ocr().await;

        let request = OcrRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "png".to_string(),
            language: "en".to_string(),
            chain_id: 84532,
        };

        let result = ocr_handler(State(state), Json(request)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.model, "paddleocr", "Model should be paddleocr");
        assert_eq!(response.provider, "host", "Provider should be host");
    }

    /// Test 17: Confidence score is valid
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_ocr_confidence_valid() {
        let state = setup_test_state_with_ocr().await;

        let request = OcrRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "png".to_string(),
            language: "en".to_string(),
            chain_id: 84532,
        };

        let result = ocr_handler(State(state), Json(request)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        // Confidence should be between 0.0 and 1.0
        assert!(
            (0.0..=1.0).contains(&response.confidence),
            "Confidence should be between 0.0 and 1.0, got {}",
            response.confidence
        );
    }
}
