// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for encrypted WebSocket image generation handler (Phase 7)

use fabstir_llm_node::api::generate_image::GenerateImageRequest;
use fabstir_llm_node::api::websocket::handlers::image_generation::{
    build_encrypted_response, handle_encrypted_image_generation,
};
use fabstir_llm_node::crypto::decrypt_with_aead;
use serde_json::json;

// ─── Sub-phase 7.1: Routing detection ─────────────────────────────

/// Test that JSON with "action": "image_generation" is correctly detected
/// as an image generation request for routing purposes.
#[test]
fn test_action_routing_image_generation_detected() {
    let decrypted_json = json!({
        "action": "image_generation",
        "prompt": "A sunset over mountains",
        "size": "1024x1024",
        "steps": 4
    });

    let is_image_gen =
        decrypted_json.get("action").and_then(|v| v.as_str()) == Some("image_generation");

    assert!(is_image_gen, "Should detect image_generation action");
}

/// Test that JSON without an "action" field does NOT match image generation
/// routing — falls through to inference pipeline.
#[test]
fn test_action_routing_no_action_falls_through() {
    let decrypted_json = json!({
        "prompt": "Hello, world!",
        "temperature": 0.7
    });

    let is_image_gen =
        decrypted_json.get("action").and_then(|v| v.as_str()) == Some("image_generation");

    assert!(
        !is_image_gen,
        "Should NOT detect image_generation without action field"
    );
}

// ─── Sub-phase 7.2: Handler unit tests ────────────────────────────

/// SDK camelCase JSON correctly deserializes into GenerateImageRequest
#[test]
fn test_encrypted_image_gen_request_deserialization() {
    let sdk_json = json!({
        "action": "image_generation",
        "prompt": "A cyberpunk city",
        "negativePrompt": "blurry, low quality",
        "guidanceScale": 4.0,
        "safetyLevel": "moderate",
        "size": "1024x768",
        "steps": 8,
        "seed": 42
    });

    let request: GenerateImageRequest = serde_json::from_value(sdk_json).unwrap();
    assert_eq!(request.prompt, "A cyberpunk city");
    assert_eq!(
        request.negative_prompt.as_deref(),
        Some("blurry, low quality")
    );
    assert_eq!(request.guidance_scale, Some(4.0));
    assert_eq!(request.safety_level.as_deref(), Some("moderate"));
    assert_eq!(request.size.as_deref(), Some("1024x768"));
    assert_eq!(request.steps, Some(8));
    assert_eq!(request.seed, Some(42));
}

/// Minimal JSON (action + prompt only) gets correct defaults
#[test]
fn test_encrypted_image_gen_request_defaults() {
    let minimal_json = json!({
        "action": "image_generation",
        "prompt": "A sunset"
    });

    let request: GenerateImageRequest = serde_json::from_value(minimal_json).unwrap();
    assert_eq!(request.prompt, "A sunset");
    assert!(request.size.is_none(), "size should default to None");
    assert!(request.steps.is_none(), "steps should default to None");
    assert!(
        request.guidance_scale.is_none(),
        "guidanceScale should default to None"
    );
    assert!(
        request.negative_prompt.is_none(),
        "negativePrompt should default to None"
    );
    assert!(
        request.safety_level.is_none(),
        "safetyLevel should default to None"
    );
}

/// Unsafe prompt (contains "gore") → encrypted error with code PROMPT_BLOCKED
#[tokio::test]
async fn test_encrypted_image_gen_prompt_safety_rejection() {
    let session_key = [0xABu8; 32];
    let decrypted_json = json!({
        "action": "image_generation",
        "prompt": "A scene of gore and destruction"
    });

    let server = fabstir_llm_node::api::ApiServer::new_for_test();
    let result = handle_encrypted_image_generation(
        &server,
        &decrypted_json,
        &session_key,
        "test-session-1",
        Some(1),
        None,
    )
    .await;

    // Result should be an encrypted_response
    assert_eq!(result["type"], "encrypted_response");

    // Decrypt the payload to verify error code
    let payload = &result["payload"];
    let ct = hex::decode(payload["ciphertextHex"].as_str().unwrap()).unwrap();
    let nonce = hex::decode(payload["nonceHex"].as_str().unwrap()).unwrap();
    let aad = hex::decode(payload["aadHex"].as_str().unwrap()).unwrap();
    let plaintext = decrypt_with_aead(&ct, &nonce, &aad, &session_key).unwrap();
    let inner: serde_json::Value = serde_json::from_slice(&plaintext).unwrap();

    assert_eq!(inner["type"], "image_generation_error");
    assert_eq!(inner["error"]["code"], "PROMPT_BLOCKED");
}

/// No diffusion client set → encrypted error with code DIFFUSION_SERVICE_UNAVAILABLE
#[tokio::test]
async fn test_encrypted_image_gen_missing_diffusion_client_error() {
    let session_key = [0xCDu8; 32];
    let decrypted_json = json!({
        "action": "image_generation",
        "prompt": "A beautiful landscape"
    });

    let server = fabstir_llm_node::api::ApiServer::new_for_test();
    // new_for_test() initializes diffusion_client to None
    let result = handle_encrypted_image_generation(
        &server,
        &decrypted_json,
        &session_key,
        "test-session-2",
        Some(2),
        None,
    )
    .await;

    assert_eq!(result["type"], "encrypted_response");

    let payload = &result["payload"];
    let ct = hex::decode(payload["ciphertextHex"].as_str().unwrap()).unwrap();
    let nonce = hex::decode(payload["nonceHex"].as_str().unwrap()).unwrap();
    let aad = hex::decode(payload["aadHex"].as_str().unwrap()).unwrap();
    let plaintext = decrypt_with_aead(&ct, &nonce, &aad, &session_key).unwrap();
    let inner: serde_json::Value = serde_json::from_slice(&plaintext).unwrap();

    assert_eq!(inner["type"], "image_generation_error");
    assert_eq!(inner["error"]["code"], "DIFFUSION_SERVICE_UNAVAILABLE");
}

/// Exhaust rate limiter → encrypted error with code RATE_LIMIT_EXCEEDED
#[tokio::test]
async fn test_encrypted_image_gen_rate_limit_rejection() {
    let session_key = [0xEFu8; 32];
    let decrypted_json = json!({
        "action": "image_generation",
        "prompt": "A peaceful garden"
    });

    let server = fabstir_llm_node::api::ApiServer::new_for_test();

    // Exhaust the rate limiter (test constructor allows 10/min)
    let session_id = "rate-limit-test-session";
    for _ in 0..10 {
        server.image_gen_rate_limiter().record_request(session_id);
    }

    let result = handle_encrypted_image_generation(
        &server,
        &decrypted_json,
        &session_key,
        session_id,
        Some(3),
        None,
    )
    .await;

    assert_eq!(result["type"], "encrypted_response");

    let payload = &result["payload"];
    let ct = hex::decode(payload["ciphertextHex"].as_str().unwrap()).unwrap();
    let nonce = hex::decode(payload["nonceHex"].as_str().unwrap()).unwrap();
    let aad = hex::decode(payload["aadHex"].as_str().unwrap()).unwrap();
    let plaintext = decrypt_with_aead(&ct, &nonce, &aad, &session_key).unwrap();
    let inner: serde_json::Value = serde_json::from_slice(&plaintext).unwrap();

    assert_eq!(inner["type"], "image_generation_error");
    assert_eq!(inner["error"]["code"], "RATE_LIMIT_EXCEEDED");
}

/// Empty prompt → encrypted error with code VALIDATION_FAILED
#[tokio::test]
async fn test_encrypted_image_gen_empty_prompt_rejected() {
    let session_key = [0x11u8; 32];
    let decrypted_json = json!({
        "action": "image_generation",
        "prompt": ""
    });

    let server = fabstir_llm_node::api::ApiServer::new_for_test();
    let result = handle_encrypted_image_generation(
        &server,
        &decrypted_json,
        &session_key,
        "test-session-empty",
        Some(4),
        None,
    )
    .await;

    assert_eq!(result["type"], "encrypted_response");

    let payload = &result["payload"];
    let ct = hex::decode(payload["ciphertextHex"].as_str().unwrap()).unwrap();
    let nonce = hex::decode(payload["nonceHex"].as_str().unwrap()).unwrap();
    let aad = hex::decode(payload["aadHex"].as_str().unwrap()).unwrap();
    let plaintext = decrypt_with_aead(&ct, &nonce, &aad, &session_key).unwrap();
    let inner: serde_json::Value = serde_json::from_slice(&plaintext).unwrap();

    assert_eq!(inner["type"], "image_generation_error");
    assert_eq!(inner["error"]["code"], "VALIDATION_FAILED");
}

/// Invalid size "999x999" → encrypted error with code VALIDATION_FAILED
#[tokio::test]
async fn test_encrypted_image_gen_invalid_size_returns_validation_error() {
    let session_key = [0x22u8; 32];
    let decrypted_json = json!({
        "action": "image_generation",
        "prompt": "A nice painting",
        "size": "999x999"
    });

    let server = fabstir_llm_node::api::ApiServer::new_for_test();
    let result = handle_encrypted_image_generation(
        &server,
        &decrypted_json,
        &session_key,
        "test-session-bad-size",
        Some(5),
        None,
    )
    .await;

    assert_eq!(result["type"], "encrypted_response");

    let payload = &result["payload"];
    let ct = hex::decode(payload["ciphertextHex"].as_str().unwrap()).unwrap();
    let nonce = hex::decode(payload["nonceHex"].as_str().unwrap()).unwrap();
    let aad = hex::decode(payload["aadHex"].as_str().unwrap()).unwrap();
    let plaintext = decrypt_with_aead(&ct, &nonce, &aad, &session_key).unwrap();
    let inner: serde_json::Value = serde_json::from_slice(&plaintext).unwrap();

    assert_eq!(inner["type"], "image_generation_error");
    assert_eq!(inner["error"]["code"], "VALIDATION_FAILED");
}

/// Outer message has type "encrypted_response" with payload containing
/// ciphertextHex, nonceHex, aadHex
#[tokio::test]
async fn test_encrypted_image_gen_response_encrypted_format() {
    let session_key = [0x33u8; 32];
    let decrypted_json = json!({
        "action": "image_generation",
        "prompt": "A landscape" // will fail at diffusion client (None) but response format is still encrypted
    });

    let server = fabstir_llm_node::api::ApiServer::new_for_test();
    let result = handle_encrypted_image_generation(
        &server,
        &decrypted_json,
        &session_key,
        "test-session-format",
        Some(6),
        None,
    )
    .await;

    // Verify outer envelope structure
    assert_eq!(result["type"], "encrypted_response");
    assert!(result["payload"].is_object(), "payload should be an object");
    assert!(
        result["payload"]["ciphertextHex"].is_string(),
        "should have ciphertextHex"
    );
    assert!(
        result["payload"]["nonceHex"].is_string(),
        "should have nonceHex"
    );
    assert!(
        result["payload"]["aadHex"].is_string(),
        "should have aadHex"
    );
}

/// Error responses (safety rejection) are encrypted, NOT plaintext
#[tokio::test]
async fn test_encrypted_image_gen_error_response_is_encrypted() {
    let session_key = [0x44u8; 32];
    let decrypted_json = json!({
        "action": "image_generation",
        "prompt": "Generate nude content" // blocked keyword
    });

    let server = fabstir_llm_node::api::ApiServer::new_for_test();
    let result = handle_encrypted_image_generation(
        &server,
        &decrypted_json,
        &session_key,
        "test-session-encrypted-error",
        Some(7),
        None,
    )
    .await;

    // Must be encrypted, not a plaintext error
    assert_eq!(result["type"], "encrypted_response");
    assert!(
        result.get("error").is_none(),
        "error should NOT be at top level (plaintext leak)"
    );
    assert!(
        result["payload"]["ciphertextHex"].is_string(),
        "error must be encrypted in payload"
    );

    // Decrypt and verify it's actually an error
    let payload = &result["payload"];
    let ct = hex::decode(payload["ciphertextHex"].as_str().unwrap()).unwrap();
    let nonce = hex::decode(payload["nonceHex"].as_str().unwrap()).unwrap();
    let aad = hex::decode(payload["aadHex"].as_str().unwrap()).unwrap();
    let plaintext = decrypt_with_aead(&ct, &nonce, &aad, &session_key).unwrap();
    let inner: serde_json::Value = serde_json::from_slice(&plaintext).unwrap();
    assert_eq!(inner["type"], "image_generation_error");
}

/// Message ID from outer json_msg appears in the encrypted response
#[tokio::test]
async fn test_encrypted_image_gen_message_id_preserved() {
    let session_key = [0x55u8; 32];
    let decrypted_json = json!({
        "action": "image_generation",
        "prompt": "A flower field"
    });
    let message_id = json!("msg-123-abc");

    let server = fabstir_llm_node::api::ApiServer::new_for_test();
    let result = handle_encrypted_image_generation(
        &server,
        &decrypted_json,
        &session_key,
        "test-session-msgid",
        Some(8),
        Some(&message_id),
    )
    .await;

    assert_eq!(result["type"], "encrypted_response");
    assert_eq!(
        result["id"], "msg-123-abc",
        "message_id should be preserved in outer response"
    );
}

/// session_id appears in the outer encrypted_response message
#[tokio::test]
async fn test_encrypted_image_gen_session_id_in_response() {
    let session_key = [0x66u8; 32];
    let decrypted_json = json!({
        "action": "image_generation",
        "prompt": "Mountains at dawn"
    });

    let server = fabstir_llm_node::api::ApiServer::new_for_test();
    let result = handle_encrypted_image_generation(
        &server,
        &decrypted_json,
        &session_key,
        "session-xyz-789",
        Some(9),
        None,
    )
    .await;

    assert_eq!(result["type"], "encrypted_response");
    assert_eq!(
        result["session_id"], "session-xyz-789",
        "session_id should appear in outer response"
    );
}

/// Helper function produces valid ciphertextHex/nonceHex/aadHex structure
/// that can be decrypted back to the original plaintext.
#[test]
fn test_build_encrypted_response_produces_valid_json() {
    let session_key = [0x77u8; 32];
    let inner_json = json!({
        "type": "image_generation_result",
        "image": "base64data",
        "model": "flux2-klein-4b"
    });

    let result = build_encrypted_response(
        &inner_json,
        &session_key,
        "test-session",
        Some(&json!("id-1")),
    );

    // Verify outer structure
    assert_eq!(result["type"], "encrypted_response");
    assert_eq!(result["session_id"], "test-session");
    assert_eq!(result["id"], "id-1");

    // Decrypt and verify roundtrip
    let payload = &result["payload"];
    let ct = hex::decode(payload["ciphertextHex"].as_str().unwrap()).unwrap();
    let nonce = hex::decode(payload["nonceHex"].as_str().unwrap()).unwrap();
    let aad = hex::decode(payload["aadHex"].as_str().unwrap()).unwrap();
    let plaintext = decrypt_with_aead(&ct, &nonce, &aad, &session_key).unwrap();
    let roundtripped: serde_json::Value = serde_json::from_slice(&plaintext).unwrap();

    assert_eq!(roundtripped["type"], "image_generation_result");
    assert_eq!(roundtripped["image"], "base64data");
    assert_eq!(roundtripped["model"], "flux2-klein-4b");
}
