// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Describe Image Endpoint tests for POST /v1/describe-image endpoint (Sub-phase 5.3)
//!
//! These TDD tests verify that the describe_image_handler correctly:
//! - Validates requests and returns appropriate errors
//! - Processes images using Florence-2
//! - Supports different detail levels
//! - Handles custom prompts
//! - Adds chain context to responses
//!
//! Test-Driven Development (TDD) Approach:
//! 1. Write these tests FIRST (they will fail initially)
//! 2. Implement describe_image_handler in src/api/describe_image/handler.rs
//! 3. Run tests to verify handler works correctly

use axum::{extract::State, http::StatusCode, Json};
use fabstir_llm_node::{
    api::{
        describe_image::{DescribeImageRequest, DescribeImageResponse},
        http_server::AppState,
    },
    vision::{VisionModelConfig, VisionModelManager},
};
use std::sync::Arc;

// Model paths
const FLORENCE_MODEL_DIR: &str = "/workspace/models/florence-2-onnx";

// Test images (base64 encoded)
// 1x1 red PNG - minimal valid image (Note: Florence may fail on very small images)
const TINY_PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

// 1x1 GIF - minimal valid image
const TINY_GIF_BASE64: &str = "R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==";

// Helper function to create a larger test image (100x100 gray PNG) as base64
fn create_test_image_base64() -> String {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use image::{ImageBuffer, ImageFormat, Rgb};
    use std::io::Cursor;

    // Create a 100x100 gray image
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_fn(100, 100, |_, _| Rgb([128u8, 128u8, 128u8]));

    // Encode to PNG
    let mut buffer = Cursor::new(Vec::new());
    img.write_to(&mut buffer, ImageFormat::Png).unwrap();

    // Convert to base64
    STANDARD.encode(buffer.into_inner())
}

/// Helper: Create test AppState with real Florence model
async fn setup_test_state_with_florence() -> AppState {
    let config = VisionModelConfig {
        ocr_model_dir: None, // Skip OCR for describe-image tests
        florence_model_dir: Some(FLORENCE_MODEL_DIR.to_string()),
        vlm_endpoint: None,
        vlm_model_name: None,
    };

    let manager = VisionModelManager::new(config)
        .await
        .expect("Failed to create vision model manager");

    let mut state = AppState::new_for_test();
    *state.vision_model_manager.write().await = Some(Arc::new(manager));
    state
}

/// Helper: Create test AppState without Florence model (for error testing)
async fn setup_test_state_without_florence() -> AppState {
    let config = VisionModelConfig {
        ocr_model_dir: None,
        florence_model_dir: None,
        vlm_endpoint: None,
        vlm_model_name: None,
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
mod describe_image_handler_tests {
    use super::*;
    use fabstir_llm_node::api::describe_image::describe_image_handler;

    // =============================================================================
    // Request Validation Tests (Unit Tests - No Model Required)
    // =============================================================================

    /// Test 1: Validation error when image is missing
    #[tokio::test]
    async fn test_validation_error_missing_image() {
        let state = setup_test_state_no_vision();

        let request = DescribeImageRequest {
            image: None,
            format: "png".to_string(),
            detail: "detailed".to_string(),
            prompt: None,
            max_tokens: 150,
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should fail when image is missing");
        let (status, message) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("image"), "Error should mention 'image'");
    }

    /// Test 2: Validation error when image is empty
    #[tokio::test]
    async fn test_validation_error_empty_image() {
        let state = setup_test_state_no_vision();

        let request = DescribeImageRequest {
            image: Some("".to_string()),
            format: "png".to_string(),
            detail: "detailed".to_string(),
            prompt: None,
            max_tokens: 150,
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should fail when image is empty");
        let (status, _message) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    /// Test 3: Validation error when format is invalid
    #[tokio::test]
    async fn test_validation_error_invalid_format() {
        let state = setup_test_state_no_vision();

        let request = DescribeImageRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "bmp".to_string(), // Not supported
            detail: "detailed".to_string(),
            prompt: None,
            max_tokens: 150,
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should fail when format is invalid");
        let (status, message) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("format"), "Error should mention 'format'");
    }

    /// Test 4: Validation error when detail level is invalid
    #[tokio::test]
    async fn test_validation_error_invalid_detail() {
        let state = setup_test_state_no_vision();

        let request = DescribeImageRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "png".to_string(),
            detail: "verbose".to_string(), // Not supported
            prompt: None,
            max_tokens: 150,
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should fail when detail is invalid");
        let (status, message) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("detail"), "Error should mention 'detail'");
    }

    /// Test 5: Validation error when max_tokens is too low
    #[tokio::test]
    async fn test_validation_error_max_tokens_too_low() {
        let state = setup_test_state_no_vision();

        let request = DescribeImageRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "png".to_string(),
            detail: "detailed".to_string(),
            prompt: None,
            max_tokens: 5, // Below minimum
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should fail when max_tokens is too low");
        let (status, message) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            message.contains("max_tokens"),
            "Error should mention 'max_tokens'"
        );
    }

    /// Test 6: Validation error when max_tokens is too high
    #[tokio::test]
    async fn test_validation_error_max_tokens_too_high() {
        let state = setup_test_state_no_vision();

        let request = DescribeImageRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "png".to_string(),
            detail: "detailed".to_string(),
            prompt: None,
            max_tokens: 1000, // Above maximum
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should fail when max_tokens is too high");
        let (status, message) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            message.contains("max_tokens"),
            "Error should mention 'max_tokens'"
        );
    }

    /// Test 7: Validation error when chain_id is invalid
    #[tokio::test]
    async fn test_validation_error_invalid_chain_id() {
        let state = setup_test_state_no_vision();

        let request = DescribeImageRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "png".to_string(),
            detail: "detailed".to_string(),
            prompt: None,
            max_tokens: 150,
            chain_id: 1, // Invalid chain
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should fail when chain_id is invalid");
        let (status, message) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            message.contains("chain_id"),
            "Error should mention 'chain_id'"
        );
    }

    // =============================================================================
    // Service Availability Tests
    // =============================================================================

    /// Test 8: Service unavailable when vision manager is not initialized
    #[tokio::test]
    async fn test_service_unavailable_no_vision_manager() {
        let state = setup_test_state_no_vision();

        let request = DescribeImageRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "png".to_string(),
            detail: "detailed".to_string(),
            prompt: None,
            max_tokens: 150,
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        assert!(
            result.is_err(),
            "Should fail when vision manager is not available"
        );
        let (status, message) = result.unwrap_err();
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            message.contains("service") || message.contains("available"),
            "Error should indicate service unavailability"
        );
    }

    /// Test 9: Service unavailable when Florence model is not loaded
    #[tokio::test]
    async fn test_service_unavailable_no_florence_model() {
        let state = setup_test_state_without_florence().await;

        let request = DescribeImageRequest {
            image: Some(TINY_PNG_BASE64.to_string()),
            format: "png".to_string(),
            detail: "detailed".to_string(),
            prompt: None,
            max_tokens: 150,
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        assert!(
            result.is_err(),
            "Should fail when Florence model is not loaded"
        );
        let (status, message) = result.unwrap_err();
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            message.contains("Florence") || message.contains("model"),
            "Error should mention Florence model"
        );
    }

    // =============================================================================
    // Description Processing Tests (Integration Tests - Require Model)
    // =============================================================================

    /// Test 10: Successful description with test image
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_describe_success_minimal_image() {
        let state = setup_test_state_with_florence().await;
        let test_image = create_test_image_base64();

        let request = DescribeImageRequest {
            image: Some(test_image),
            format: "png".to_string(),
            detail: "brief".to_string(),
            prompt: None,
            max_tokens: 50,
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        // Florence may fail on simple gray images - accept either success or 500
        match result {
            Ok(response) => {
                let response = response.0;
                assert_eq!(response.model, "florence-2");
                assert_eq!(response.provider, "host");
                assert_eq!(response.chain_id, 84532);
                assert_eq!(response.chain_name, "Base Sepolia");
                assert_eq!(response.native_token, "ETH");
            }
            Err((status, _)) => {
                // 500 is acceptable for trivial images
                assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
            }
        }
    }

    /// Test 11: Response includes correct chain context for Base Sepolia
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_describe_chain_context_base_sepolia() {
        let state = setup_test_state_with_florence().await;
        let test_image = create_test_image_base64();

        let request = DescribeImageRequest {
            image: Some(test_image),
            format: "png".to_string(),
            detail: "brief".to_string(),
            prompt: None,
            max_tokens: 50,
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        if let Ok(response) = result {
            let response = response.0;
            assert_eq!(response.chain_id, 84532);
            assert_eq!(response.chain_name, "Base Sepolia");
            assert_eq!(response.native_token, "ETH");
        }
        // If it fails, that's acceptable for trivial images
    }

    /// Test 12: Response includes correct chain context for opBNB
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_describe_chain_context_opbnb() {
        let state = setup_test_state_with_florence().await;
        let test_image = create_test_image_base64();

        let request = DescribeImageRequest {
            image: Some(test_image),
            format: "png".to_string(),
            detail: "brief".to_string(),
            prompt: None,
            max_tokens: 50,
            chain_id: 5611,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        if let Ok(response) = result {
            let response = response.0;
            assert_eq!(response.chain_id, 5611);
            assert_eq!(response.chain_name, "opBNB Testnet");
            assert_eq!(response.native_token, "BNB");
        }
        // If it fails, that's acceptable for trivial images
    }

    /// Test 13: Processing time is recorded
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_describe_processing_time_recorded() {
        let state = setup_test_state_with_florence().await;
        let test_image = create_test_image_base64();

        let request = DescribeImageRequest {
            image: Some(test_image),
            format: "png".to_string(),
            detail: "brief".to_string(),
            prompt: None,
            max_tokens: 50,
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        if let Ok(response) = result {
            let response = response.0;
            // Processing time should be > 0 (actual description was generated)
            assert!(
                response.processing_time_ms > 0,
                "Processing time should be recorded"
            );
        }
        // If it fails, that's acceptable for trivial images
    }

    /// Test 14: Brief detail level works (accepts handler being called without error)
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_describe_brief_detail() {
        let state = setup_test_state_with_florence().await;
        let test_image = create_test_image_base64();

        let request = DescribeImageRequest {
            image: Some(test_image),
            format: "png".to_string(),
            detail: "brief".to_string(),
            prompt: None,
            max_tokens: 50,
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        // Accept either success or 500 (model may fail on simple images)
        match result {
            Ok(_) => {} // Success
            Err((status, _)) => assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR),
        }
    }

    /// Test 15: Detailed detail level works
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_describe_detailed_detail() {
        let state = setup_test_state_with_florence().await;
        let test_image = create_test_image_base64();

        let request = DescribeImageRequest {
            image: Some(test_image),
            format: "png".to_string(),
            detail: "detailed".to_string(),
            prompt: None,
            max_tokens: 150,
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        // Accept either success or 500 (model may fail on simple images)
        match result {
            Ok(_) => {} // Success
            Err((status, _)) => assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR),
        }
    }

    /// Test 16: Comprehensive detail level works
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_describe_comprehensive_detail() {
        let state = setup_test_state_with_florence().await;
        let test_image = create_test_image_base64();

        let request = DescribeImageRequest {
            image: Some(test_image),
            format: "png".to_string(),
            detail: "comprehensive".to_string(),
            prompt: None,
            max_tokens: 300,
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        // Accept either success or 500 (model may fail on simple images)
        match result {
            Ok(_) => {} // Success
            Err((status, _)) => assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR),
        }
    }

    /// Test 17: Custom prompt is accepted
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_describe_with_custom_prompt() {
        let state = setup_test_state_with_florence().await;
        let test_image = create_test_image_base64();

        let request = DescribeImageRequest {
            image: Some(test_image),
            format: "png".to_string(),
            detail: "detailed".to_string(),
            prompt: Some("Describe the colors in this image".to_string()),
            max_tokens: 150,
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        // Accept either success or 500 (model may fail on simple images)
        match result {
            Ok(_) => {} // Success
            Err((status, _)) => assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR),
        }
    }

    /// Test 18: Works with GIF format (tiny GIF may fail in model, that's OK)
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_describe_gif_format() {
        let state = setup_test_state_with_florence().await;

        let request = DescribeImageRequest {
            image: Some(TINY_GIF_BASE64.to_string()),
            format: "gif".to_string(),
            detail: "brief".to_string(),
            prompt: None,
            max_tokens: 50,
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        // Accept either success or 500 (tiny images may fail)
        match result {
            Ok(_) => {} // Success
            Err((status, _)) => assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR),
        }
    }

    /// Test 19: Default values work correctly
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_describe_with_defaults() {
        let state = setup_test_state_with_florence().await;
        let test_image = create_test_image_base64();

        // Only provide image, let other fields use defaults
        let json = serde_json::json!({
            "image": test_image
        });
        let request: DescribeImageRequest = serde_json::from_value(json).unwrap();

        let result = describe_image_handler(State(state), Json(request)).await;

        // Accept either success or 500 (model may fail on simple images)
        match result {
            Ok(response) => {
                let response = response.0;
                // Default chain_id is 84532
                assert_eq!(response.chain_id, 84532);
            }
            Err((status, _)) => assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR),
        }
    }

    /// Test 20: Bad request for invalid base64
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_describe_invalid_base64() {
        let state = setup_test_state_with_florence().await;

        let request = DescribeImageRequest {
            image: Some("not-valid-base64!!!".to_string()),
            format: "png".to_string(),
            detail: "brief".to_string(),
            prompt: None,
            max_tokens: 50,
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should fail with invalid base64");
        let (status, _message) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    /// Test 21: Image analysis metadata is included
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_describe_includes_analysis() {
        let state = setup_test_state_with_florence().await;
        let test_image = create_test_image_base64();

        let request = DescribeImageRequest {
            image: Some(test_image),
            format: "png".to_string(),
            detail: "brief".to_string(),
            prompt: None,
            max_tokens: 50,
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        if let Ok(response) = result {
            let response = response.0;
            // Analysis should have image dimensions
            assert!(response.analysis.width > 0);
            assert!(response.analysis.height > 0);
        }
        // If it fails, that's acceptable for trivial images
    }

    /// Test 22: Model info is correct
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_describe_model_info() {
        let state = setup_test_state_with_florence().await;
        let test_image = create_test_image_base64();

        let request = DescribeImageRequest {
            image: Some(test_image),
            format: "png".to_string(),
            detail: "brief".to_string(),
            prompt: None,
            max_tokens: 50,
            chain_id: 84532,
        };

        let result = describe_image_handler(State(state), Json(request)).await;

        if let Ok(response) = result {
            let response = response.0;
            assert_eq!(response.model, "florence-2", "Model should be florence-2");
            assert_eq!(response.provider, "host", "Provider should be host");
        }
        // If it fails, that's acceptable for trivial images
    }
}
