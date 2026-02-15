// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for POST /v1/images/generate endpoint (Phase 3)

use fabstir_llm_node::api::generate_image::{
    BillingInfo, GenerateImageRequest, GenerateImageResponse, SafetyInfo,
};

// ============================================================================
// Sub-phase 3.1: Request deserialization and validation tests
// ============================================================================

#[test]
fn test_request_deserialization_all_fields() {
    let json = r#"{
        "prompt": "a sunset over mountains",
        "model": "flux2-klein-4b",
        "size": "1024x768",
        "steps": 20,
        "seed": 42,
        "negativePrompt": "blurry",
        "guidanceScale": 7.5,
        "safetyLevel": "moderate",
        "chainId": 5611,
        "sessionId": "sess-abc",
        "jobId": 123
    }"#;
    let req: GenerateImageRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.prompt, "a sunset over mountains");
    assert_eq!(req.model.as_deref(), Some("flux2-klein-4b"));
    assert_eq!(req.size.as_deref(), Some("1024x768"));
    assert_eq!(req.steps, Some(20));
    assert_eq!(req.seed, Some(42));
    assert_eq!(req.negative_prompt.as_deref(), Some("blurry"));
    assert_eq!(req.guidance_scale, Some(7.5));
    assert_eq!(req.safety_level.as_deref(), Some("moderate"));
    assert_eq!(req.chain_id, Some(5611));
    assert_eq!(req.session_id.as_deref(), Some("sess-abc"));
    assert_eq!(req.job_id, Some(123));
}

#[test]
fn test_request_deserialization_defaults_only() {
    let json = r#"{"prompt": "a cat sitting on a windowsill"}"#;
    let req: GenerateImageRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.prompt, "a cat sitting on a windowsill");
    assert!(req.model.is_none());
    assert!(req.size.is_none());
    assert!(req.steps.is_none());
    assert!(req.seed.is_none());
    assert!(req.negative_prompt.is_none());
    assert!(req.guidance_scale.is_none());
    assert!(req.safety_level.is_none());
    assert!(req.chain_id.is_none());
    assert!(req.session_id.is_none());
    assert!(req.job_id.is_none());
}

#[test]
fn test_validate_empty_prompt_returns_error() {
    let req = GenerateImageRequest {
        prompt: "".to_string(),
        model: None,
        size: None,
        steps: None,
        seed: None,
        negative_prompt: None,
        guidance_scale: None,
        safety_level: None,
        chain_id: None,
        session_id: None,
        job_id: None,
    };
    let result = req.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("prompt"));
}

#[test]
fn test_validate_whitespace_prompt_returns_error() {
    let req = GenerateImageRequest {
        prompt: "   ".to_string(),
        model: None,
        size: None,
        steps: None,
        seed: None,
        negative_prompt: None,
        guidance_scale: None,
        safety_level: None,
        chain_id: None,
        session_id: None,
        job_id: None,
    };
    let result = req.validate();
    assert!(result.is_err());
}

#[test]
fn test_validate_invalid_size_returns_error() {
    let req = GenerateImageRequest {
        prompt: "a landscape".to_string(),
        model: None,
        size: Some("123x456".to_string()),
        steps: None,
        seed: None,
        negative_prompt: None,
        guidance_scale: None,
        safety_level: None,
        chain_id: None,
        session_id: None,
        job_id: None,
    };
    let result = req.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("size"));
}

#[test]
fn test_validate_steps_zero_returns_error() {
    let req = GenerateImageRequest {
        prompt: "a landscape".to_string(),
        model: None,
        size: None,
        steps: Some(0),
        seed: None,
        negative_prompt: None,
        guidance_scale: None,
        safety_level: None,
        chain_id: None,
        session_id: None,
        job_id: None,
    };
    let result = req.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("steps"));
}

#[test]
fn test_validate_steps_101_returns_error() {
    let req = GenerateImageRequest {
        prompt: "a landscape".to_string(),
        model: None,
        size: None,
        steps: Some(101),
        seed: None,
        negative_prompt: None,
        guidance_scale: None,
        safety_level: None,
        chain_id: None,
        session_id: None,
        job_id: None,
    };
    let result = req.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("steps"));
}

#[test]
fn test_validate_valid_request_passes() {
    let req = GenerateImageRequest {
        prompt: "a beautiful sunset over the ocean".to_string(),
        model: Some("flux2-klein-4b".to_string()),
        size: Some("1024x1024".to_string()),
        steps: Some(20),
        seed: Some(42),
        negative_prompt: Some("blurry".to_string()),
        guidance_scale: Some(3.5),
        safety_level: Some("strict".to_string()),
        chain_id: Some(84532),
        session_id: Some("sess-test".to_string()),
        job_id: Some(1),
    };
    assert!(req.validate().is_ok());
}

#[test]
fn test_validate_valid_request_minimal() {
    let req = GenerateImageRequest {
        prompt: "a cat".to_string(),
        model: None,
        size: None,
        steps: None,
        seed: None,
        negative_prompt: None,
        guidance_scale: None,
        safety_level: None,
        chain_id: None,
        session_id: None,
        job_id: None,
    };
    assert!(req.validate().is_ok());
}

// ============================================================================
// Response serialization tests
// ============================================================================

#[test]
fn test_response_serialization_all_fields() {
    let resp = GenerateImageResponse {
        image: "base64data".to_string(),
        model: "flux2-klein-4b".to_string(),
        size: "1024x1024".to_string(),
        steps: 4,
        seed: 42,
        processing_time_ms: 1500,
        safety: SafetyInfo {
            prompt_safe: true,
            output_safe: true,
            safety_level: "strict".to_string(),
        },
        provider: "host".to_string(),
        chain_id: 84532,
        chain_name: "Base Sepolia".to_string(),
        native_token: "ETH".to_string(),
        billing: BillingInfo {
            generation_units: 0.2,
            model_multiplier: 1.0,
            megapixels: 1.0,
            steps: 4,
        },
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["image"], "base64data");
    assert_eq!(json["model"], "flux2-klein-4b");
    assert_eq!(json["processingTimeMs"], 1500);
    assert_eq!(json["safety"]["promptSafe"], true);
    assert_eq!(json["safety"]["safetyLevel"], "strict");
    assert_eq!(json["chainId"], 84532);
    assert_eq!(json["chainName"], "Base Sepolia");
    assert_eq!(json["nativeToken"], "ETH");
    assert_eq!(json["billing"]["generationUnits"], 0.2);
}

#[test]
fn test_response_chain_context_base_sepolia() {
    let resp = GenerateImageResponse::with_chain_context(
        "img".to_string(),
        "model".to_string(),
        "1024x1024".to_string(),
        4,
        42,
        100,
        SafetyInfo {
            prompt_safe: true,
            output_safe: true,
            safety_level: "strict".to_string(),
        },
        BillingInfo {
            generation_units: 0.2,
            model_multiplier: 1.0,
            megapixels: 1.0,
            steps: 4,
        },
        84532,
    );
    assert_eq!(resp.chain_name, "Base Sepolia");
    assert_eq!(resp.native_token, "ETH");
}

#[test]
fn test_response_chain_context_opbnb() {
    let resp = GenerateImageResponse::with_chain_context(
        "img".to_string(),
        "model".to_string(),
        "1024x1024".to_string(),
        4,
        42,
        100,
        SafetyInfo {
            prompt_safe: true,
            output_safe: true,
            safety_level: "strict".to_string(),
        },
        BillingInfo {
            generation_units: 0.2,
            model_multiplier: 1.0,
            megapixels: 1.0,
            steps: 4,
        },
        5611,
    );
    assert_eq!(resp.chain_name, "opBNB Testnet");
    assert_eq!(resp.native_token, "BNB");
}

#[test]
fn test_billing_info_generation_units_calculation() {
    // 1024x1024 = 1.0 megapixels, 4 steps / 20 baseline = 0.2, multiplier 1.0
    let billing = BillingInfo {
        generation_units: 0.2,
        model_multiplier: 1.0,
        megapixels: 1.0,
        steps: 4,
    };
    let expected = billing.megapixels * (billing.steps as f64 / 20.0) * billing.model_multiplier;
    assert!((expected - billing.generation_units).abs() < f64::EPSILON);
}

// ============================================================================
// Sub-phase 3.2: Handler tests (unit-level, no HTTP stack)
// ============================================================================

#[test]
fn test_handler_function_exists() {
    // Verify the handler compiles and is accessible
    let _ = fabstir_llm_node::api::generate_image::generate_image_handler;
}

#[tokio::test]
async fn test_handler_missing_diffusion_client_returns_503() {
    use fabstir_llm_node::api::http_server::AppState;

    let state = AppState::new_for_test();
    let req = GenerateImageRequest {
        prompt: "a sunset".to_string(),
        model: None,
        size: None,
        steps: None,
        seed: None,
        negative_prompt: None,
        guidance_scale: None,
        safety_level: None,
        chain_id: None,
        session_id: None,
        job_id: None,
    };

    let result = fabstir_llm_node::api::generate_image::generate_image_handler(
        axum::extract::State(state),
        axum::Json(req),
    )
    .await;

    assert!(result.is_err());
    let (status, _msg) = result.unwrap_err();
    assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn test_handler_unsafe_prompt_returns_400() {
    use fabstir_llm_node::api::http_server::AppState;
    use fabstir_llm_node::diffusion::DiffusionClient;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    let state = AppState::new_for_test();

    // Set up a diffusion client so we get past the 503 check
    let client = DiffusionClient::new("http://localhost:99999", "test-model").unwrap();
    *state.diffusion_client.write().await = Some(Arc::new(client));

    let req = GenerateImageRequest {
        prompt: "explicit sexual content".to_string(),
        model: None,
        size: None,
        steps: None,
        seed: None,
        negative_prompt: None,
        guidance_scale: None,
        safety_level: None,
        chain_id: None,
        session_id: None,
        job_id: None,
    };

    let result = fabstir_llm_node::api::generate_image::generate_image_handler(
        axum::extract::State(state),
        axum::Json(req),
    )
    .await;

    assert!(result.is_err());
    let (status, msg) = result.unwrap_err();
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
    assert!(msg.to_lowercase().contains("safe") || msg.to_lowercase().contains("block"));
}
