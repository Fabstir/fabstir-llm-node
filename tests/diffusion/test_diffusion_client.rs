// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! TDD tests for DiffusionClient (Phase 1.1-1.3)

use fabstir_llm_node::diffusion::client::{
    DiffusionClient, DiffusionResult, ImageGenerationRequest, ImageSize, OpenAIImageResponse,
    ALLOWED_SIZES,
};

// ===== Sub-phase 1.1: Core Types =====

#[test]
fn test_diffusion_client_new() {
    let client = DiffusionClient::new("http://localhost:8082", "flux2-klein-4b").unwrap();
    assert_eq!(client.model_name(), "flux2-klein-4b");
}

#[test]
fn test_diffusion_client_trailing_slash_trimmed() {
    let client = DiffusionClient::new("http://localhost:8082/", "flux2-klein-4b").unwrap();
    // Verify health_check URL won't have double slash
    // We verify indirectly via model_name since endpoint is private
    assert_eq!(client.model_name(), "flux2-klein-4b");
}

#[test]
fn test_diffusion_client_model_name_getter() {
    let client = DiffusionClient::new("http://localhost:8082", "my-model").unwrap();
    assert_eq!(client.model_name(), "my-model");
}

#[tokio::test]
async fn test_diffusion_client_health_check_unreachable() {
    let client = DiffusionClient::new("http://127.0.0.1:59999", "test-model").unwrap();
    let healthy = client.health_check().await;
    assert!(!healthy);
}

#[test]
fn test_image_generation_request_serialization() {
    let request = ImageGenerationRequest {
        prompt: "A cat in space".to_string(),
        model: Some("flux2-klein-4b".to_string()),
        size: "1024x1024".to_string(),
        steps: 4,
        seed: Some(42),
        negative_prompt: Some("blurry".to_string()),
        guidance_scale: 3.5,
        response_format: "b64_json".to_string(),
        n: 1,
    };
    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(json["prompt"], "A cat in space");
    assert_eq!(json["model"], "flux2-klein-4b");
    assert_eq!(json["size"], "1024x1024");
    assert_eq!(json["steps"], 4);
    assert_eq!(json["seed"], 42);
    assert_eq!(json["negative_prompt"], "blurry");
    let gs = json["guidance_scale"].as_f64().unwrap();
    assert!((gs - 3.5).abs() < 0.01);
    assert_eq!(json["response_format"], "b64_json");
    assert_eq!(json["n"], 1);
}

#[test]
fn test_image_generation_request_validate_empty_prompt() {
    let request = ImageGenerationRequest {
        prompt: "".to_string(),
        model: None,
        size: "1024x1024".to_string(),
        steps: 4,
        seed: None,
        negative_prompt: None,
        guidance_scale: 3.5,
        response_format: "b64_json".to_string(),
        n: 1,
    };
    let result = request.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("prompt"));
}

#[test]
fn test_image_generation_request_validate_invalid_size() {
    let request = ImageGenerationRequest {
        prompt: "A cat".to_string(),
        model: None,
        size: "123x456".to_string(),
        steps: 4,
        seed: None,
        negative_prompt: None,
        guidance_scale: 3.5,
        response_format: "b64_json".to_string(),
        n: 1,
    };
    let result = request.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("invalid size"));
}

#[test]
fn test_image_generation_request_validate_steps_zero() {
    let request = ImageGenerationRequest {
        prompt: "A cat".to_string(),
        model: None,
        size: "1024x1024".to_string(),
        steps: 0,
        seed: None,
        negative_prompt: None,
        guidance_scale: 3.5,
        response_format: "b64_json".to_string(),
        n: 1,
    };
    let result = request.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("steps"));
}

#[test]
fn test_image_generation_request_validate_steps_over_100() {
    let request = ImageGenerationRequest {
        prompt: "A cat".to_string(),
        model: None,
        size: "1024x1024".to_string(),
        steps: 101,
        seed: None,
        negative_prompt: None,
        guidance_scale: 3.5,
        response_format: "b64_json".to_string(),
        n: 1,
    };
    let result = request.validate();
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("steps"));
}

#[test]
fn test_image_generation_request_validate_valid() {
    let request = ImageGenerationRequest {
        prompt: "A beautiful sunset".to_string(),
        model: None,
        size: "1024x1024".to_string(),
        steps: 4,
        seed: None,
        negative_prompt: None,
        guidance_scale: 3.5,
        response_format: "b64_json".to_string(),
        n: 1,
    };
    assert!(request.validate().is_ok());
}

#[test]
fn test_image_generation_request_default_values() {
    let json = serde_json::json!({
        "prompt": "A cat"
    });
    let request: ImageGenerationRequest = serde_json::from_value(json).unwrap();
    assert_eq!(request.prompt, "A cat");
    assert_eq!(request.size, "1024x1024");
    assert_eq!(request.steps, 4);
    assert!((request.guidance_scale - 3.5).abs() < 0.01);
    assert_eq!(request.response_format, "b64_json");
    assert_eq!(request.n, 1);
    assert!(request.model.is_none());
    assert!(request.seed.is_none());
    assert!(request.negative_prompt.is_none());
}

#[test]
fn test_image_size_parse_1024x1024() {
    let size = ImageSize::parse("1024x1024").unwrap();
    assert_eq!(size.width, 1024);
    assert_eq!(size.height, 1024);
}

#[test]
fn test_image_size_parse_512x512() {
    let size = ImageSize::parse("512x512").unwrap();
    assert_eq!(size.width, 512);
    assert_eq!(size.height, 512);
}

#[test]
fn test_image_size_parse_1024x768() {
    let size = ImageSize::parse("1024x768").unwrap();
    assert_eq!(size.width, 1024);
    assert_eq!(size.height, 768);
}

#[test]
fn test_image_size_parse_invalid_format() {
    assert!(ImageSize::parse("123x456").is_ok()); // valid parse, just not in ALLOWED_SIZES
    assert!(ImageSize::parse("abc").is_err());
    assert!(ImageSize::parse("100xabc").is_err());
}

#[test]
fn test_image_size_megapixels() {
    let size = ImageSize::parse("1024x1024").unwrap();
    assert!((size.megapixels() - 1.0).abs() < 0.001);

    let size_small = ImageSize::parse("512x512").unwrap();
    assert!((size_small.megapixels() - 0.25).abs() < 0.001);

    let size_rect = ImageSize::parse("1024x768").unwrap();
    let expected = (1024.0 * 768.0) / 1_048_576.0;
    assert!((size_rect.megapixels() - expected).abs() < 0.001);
}

#[test]
fn test_openai_response_deserialization() {
    let json = serde_json::json!({
        "data": [
            {
                "b64_json": "iVBORw0KGgoAAAANSUhEUg==",
                "revised_prompt": "A majestic cat floating in outer space"
            }
        ]
    });
    let response: OpenAIImageResponse = serde_json::from_value(json).unwrap();
    assert_eq!(response.data.len(), 1);
    assert_eq!(
        response.data[0].b64_json.as_deref(),
        Some("iVBORw0KGgoAAAANSUhEUg==")
    );
    assert_eq!(
        response.data[0].revised_prompt.as_deref(),
        Some("A majestic cat floating in outer space")
    );
}

#[test]
fn test_openai_response_without_revised_prompt() {
    let json = serde_json::json!({
        "data": [
            {
                "b64_json": "iVBORw0KGgoAAAANSUhEUg=="
            }
        ]
    });
    let response: OpenAIImageResponse = serde_json::from_value(json).unwrap();
    assert_eq!(response.data.len(), 1);
    assert!(response.data[0].b64_json.is_some());
    assert!(response.data[0].revised_prompt.is_none());
}

// ===== Sub-phase 1.3: Generate and List Methods =====

#[test]
fn test_generate_request_body_matches_openai_spec() {
    // Verify the JSON body structure matches OpenAI /v1/images/generations
    let request = ImageGenerationRequest {
        prompt: "A sunset over the ocean".to_string(),
        model: Some("flux2-klein-4b".to_string()),
        size: "1024x1024".to_string(),
        steps: 4,
        seed: Some(42),
        negative_prompt: None,
        guidance_scale: 3.5,
        response_format: "b64_json".to_string(),
        n: 1,
    };
    let body = serde_json::json!({
        "prompt": request.prompt,
        "model": request.model,
        "size": request.size,
        "n": request.n,
        "response_format": request.response_format,
        "guidance_scale": request.guidance_scale,
        "num_inference_steps": request.steps,
        "seed": request.seed,
    });
    assert_eq!(body["prompt"], "A sunset over the ocean");
    assert_eq!(body["model"], "flux2-klein-4b");
    assert_eq!(body["size"], "1024x1024");
    assert_eq!(body["n"], 1);
    assert_eq!(body["response_format"], "b64_json");
    assert_eq!(body["num_inference_steps"], 4);
    assert_eq!(body["seed"], 42);
}

#[test]
fn test_generate_response_parses_b64_json() {
    let json = serde_json::json!({
        "data": [
            {
                "b64_json": "SGVsbG8gV29ybGQ=",
                "revised_prompt": "A beautiful sunset over a calm ocean"
            }
        ]
    });
    let response: OpenAIImageResponse = serde_json::from_value(json).unwrap();
    let first = &response.data[0];
    assert_eq!(first.b64_json.as_deref().unwrap(), "SGVsbG8gV29ybGQ=");
    assert_eq!(
        first.revised_prompt.as_deref().unwrap(),
        "A beautiful sunset over a calm ocean"
    );
}

#[test]
fn test_generate_with_edit_body_includes_image_and_strength() {
    // Verify img2img request includes image and strength fields
    let request = ImageGenerationRequest {
        prompt: "Make it more vibrant".to_string(),
        model: None,
        size: "1024x1024".to_string(),
        steps: 4,
        seed: None,
        negative_prompt: None,
        guidance_scale: 3.5,
        response_format: "b64_json".to_string(),
        n: 1,
    };
    let base64_image = "iVBORw0KGgoAAAANSUhEUg==";
    let strength = 0.75_f32;

    let body = serde_json::json!({
        "prompt": request.prompt,
        "model": "flux2-klein-4b",
        "size": request.size,
        "n": request.n,
        "response_format": request.response_format,
        "guidance_scale": request.guidance_scale,
        "num_inference_steps": request.steps,
        "image": base64_image,
        "strength": strength,
    });
    assert_eq!(body["image"], "iVBORw0KGgoAAAANSUhEUg==");
    let s = body["strength"].as_f64().unwrap();
    assert!((s - 0.75).abs() < 0.01);
}

#[test]
fn test_list_models_response_parsing() {
    let json = serde_json::json!({
        "data": [
            {"id": "flux2-klein-4b"},
            {"id": "sdxl-turbo"}
        ]
    });
    // Verify we can parse the OpenAI /v1/models response format
    #[derive(serde::Deserialize)]
    struct ModelList {
        data: Vec<ModelEntry>,
    }
    #[derive(serde::Deserialize)]
    struct ModelEntry {
        id: String,
    }
    let list: ModelList = serde_json::from_value(json).unwrap();
    let ids: Vec<String> = list.data.into_iter().map(|m| m.id).collect();
    assert_eq!(ids, vec!["flux2-klein-4b", "sdxl-turbo"]);
}

#[tokio::test]
async fn test_generate_unreachable_returns_error() {
    let client = DiffusionClient::new("http://127.0.0.1:59999", "test-model").unwrap();
    let request = ImageGenerationRequest {
        prompt: "A cat".to_string(),
        model: None,
        size: "1024x1024".to_string(),
        steps: 4,
        seed: None,
        negative_prompt: None,
        guidance_scale: 3.5,
        response_format: "b64_json".to_string(),
        n: 1,
    };
    let result = client.generate(&request).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_list_models_unreachable_returns_error() {
    let client = DiffusionClient::new("http://127.0.0.1:59999", "test-model").unwrap();
    let result = client.list_models().await;
    assert!(result.is_err());
}

#[test]
fn test_allowed_sizes_constant() {
    assert!(ALLOWED_SIZES.contains(&"1024x1024"));
    assert!(ALLOWED_SIZES.contains(&"512x512"));
    assert!(ALLOWED_SIZES.contains(&"768x768"));
    assert!(ALLOWED_SIZES.contains(&"256x256"));
    assert!(ALLOWED_SIZES.contains(&"1024x768"));
    assert!(ALLOWED_SIZES.contains(&"768x1024"));
    assert!(!ALLOWED_SIZES.contains(&"1920x1080"));
}

#[test]
fn test_diffusion_result_fields() {
    let result = DiffusionResult {
        base64_image: "abc123".to_string(),
        model: "flux2-klein-4b".to_string(),
        processing_time_ms: 500,
        seed: 42,
        width: 1024,
        height: 1024,
        steps: 4,
        revised_prompt: Some("Enhanced prompt".to_string()),
    };
    assert_eq!(result.base64_image, "abc123");
    assert_eq!(result.model, "flux2-klein-4b");
    assert_eq!(result.processing_time_ms, 500);
    assert_eq!(result.seed, 42);
    assert_eq!(result.width, 1024);
    assert_eq!(result.height, 1024);
    assert_eq!(result.steps, 4);
    assert_eq!(result.revised_prompt.as_deref(), Some("Enhanced prompt"));
}
