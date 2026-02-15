// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for WebSocket image generation message types (Phase 3.4)

use fabstir_llm_node::api::websocket::message_types::{MessageType, WebSocketMessage};

#[test]
fn test_message_type_image_generation_serde_roundtrip() {
    let msg_type = MessageType::ImageGeneration;
    let json = serde_json::to_string(&msg_type).unwrap();
    assert_eq!(json, "\"image_generation\"");
    let deserialized: MessageType = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, MessageType::ImageGeneration);
}

#[test]
fn test_message_type_image_generation_result_serde_roundtrip() {
    let msg_type = MessageType::ImageGenerationResult;
    let json = serde_json::to_string(&msg_type).unwrap();
    assert_eq!(json, "\"image_generation_result\"");
    let deserialized: MessageType = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, MessageType::ImageGenerationResult);
}

#[test]
fn test_websocket_message_with_image_generation_type() {
    let msg = WebSocketMessage::new(
        MessageType::ImageGeneration,
        serde_json::json!({
            "prompt": "a sunset over mountains",
            "size": "1024x1024",
            "steps": 4
        }),
    );
    assert_eq!(msg.msg_type, MessageType::ImageGeneration);
    assert_eq!(msg.payload["prompt"], "a sunset over mountains");
    assert_eq!(msg.payload["steps"], 4);
}

#[test]
fn test_websocket_message_with_image_generation_result() {
    let msg = WebSocketMessage::new(
        MessageType::ImageGenerationResult,
        serde_json::json!({
            "image": "base64data",
            "model": "flux2-klein-4b",
            "processingTimeMs": 1500,
            "safety": {
                "promptSafe": true,
                "outputSafe": true,
                "safetyLevel": "strict"
            }
        }),
    );
    assert_eq!(msg.msg_type, MessageType::ImageGenerationResult);
    assert_eq!(msg.payload["image"], "base64data");
    assert!(msg.payload["safety"]["promptSafe"].as_bool().unwrap());
}

#[test]
fn test_image_generation_payload_deserializes_to_request() {
    use fabstir_llm_node::api::generate_image::GenerateImageRequest;

    let payload = serde_json::json!({
        "prompt": "a beautiful landscape",
        "size": "1024x768",
        "steps": 20,
        "seed": 42
    });
    let req: GenerateImageRequest = serde_json::from_value(payload).unwrap();
    assert_eq!(req.prompt, "a beautiful landscape");
    assert_eq!(req.size.as_deref(), Some("1024x768"));
    assert_eq!(req.steps, Some(20));
    assert_eq!(req.seed, Some(42));
}

#[test]
fn test_image_generation_result_contains_safety_info() {
    use fabstir_llm_node::api::generate_image::{BillingInfo, GenerateImageResponse, SafetyInfo};

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

    // Wrap in a WebSocketMessage payload
    let json_value = serde_json::to_value(&resp).unwrap();
    let msg = WebSocketMessage::new(MessageType::ImageGenerationResult, json_value.clone());

    assert_eq!(msg.msg_type, MessageType::ImageGenerationResult);
    assert!(msg.payload["safety"]["promptSafe"].as_bool().unwrap());
    assert!(msg.payload["safety"]["outputSafe"].as_bool().unwrap());
    assert_eq!(msg.payload["safety"]["safetyLevel"], "strict");
}
