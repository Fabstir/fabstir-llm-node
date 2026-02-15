// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Encrypted WebSocket handler for image generation (v8.16.0)
//!
//! Handles `"action": "image_generation"` messages received inside
//! `encrypted_message` payloads. All responses (success AND error) are
//! encrypted back with the session key — no plaintext leaks.

use crate::api::generate_image::{
    BillingInfo, GenerateImageRequest, GenerateImageResponse, SafetyInfo,
};
use crate::api::server::ApiServer;
use crate::diffusion::billing::calculate_generation_units;
use crate::diffusion::prompt_safety::PromptSafetyClassifier;
use crate::diffusion::safety::SafetyConfig;
use rand::RngCore;
use serde_json::{json, Value};
use tracing::{error, info, warn};

/// Build an encrypted response envelope wrapping `inner_json`.
///
/// Generates a random 24-byte nonce, encrypts with XChaCha20-Poly1305,
/// and wraps in the standard `encrypted_response` format.
pub fn build_encrypted_response(
    inner_json: &Value,
    session_key: &[u8; 32],
    session_id: &str,
    message_id: Option<&Value>,
) -> Value {
    let plaintext = serde_json::to_vec(inner_json).unwrap_or_default();

    let mut nonce = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce);

    let aad = b"encrypted_image_response";

    match crate::crypto::encrypt_with_aead(&plaintext, &nonce, aad, session_key) {
        Ok(ciphertext) => {
            let mut msg = json!({
                "type": "encrypted_response",
                "payload": {
                    "ciphertextHex": hex::encode(&ciphertext),
                    "nonceHex": hex::encode(&nonce),
                    "aadHex": hex::encode(aad),
                },
                "session_id": session_id,
            });
            if let Some(mid) = message_id {
                msg["id"] = mid.clone();
            }
            msg
        }
        Err(e) => {
            error!("Failed to encrypt image generation response: {}", e);
            let mut msg = json!({
                "type": "error",
                "code": "ENCRYPTION_FAILED",
                "message": format!("Failed to encrypt response: {}", e),
                "session_id": session_id,
            });
            if let Some(mid) = message_id {
                msg["id"] = mid.clone();
            }
            msg
        }
    }
}

/// Build an encrypted error response.
fn build_encrypted_error(
    code: &str,
    message: &str,
    session_key: &[u8; 32],
    session_id: &str,
    message_id: Option<&Value>,
) -> Value {
    let inner = json!({
        "type": "image_generation_error",
        "error": {
            "code": code,
            "message": message,
        }
    });
    build_encrypted_response(&inner, session_key, session_id, message_id)
}

/// Handle an encrypted image generation request.
///
/// Called from `server.rs` after the `encrypted_message` has been decrypted
/// and the `"action": "image_generation"` routing key detected.
///
/// Pipeline:
/// 1. Rate limit check
/// 2. Deserialize request (camelCase → GenerateImageRequest)
/// 3. Validate (empty prompt, invalid size, steps range)
/// 4. Prompt safety (keyword blocklist)
/// 5. Get diffusion client
/// 6. Generate image via sidecar
/// 7. Calculate billing, record rate limit, track billing
/// 8. Build encrypted response
pub async fn handle_encrypted_image_generation(
    server: &ApiServer,
    decrypted_json: &Value,
    session_key: &[u8; 32],
    session_id: &str,
    job_id: Option<u64>,
    message_id: Option<&Value>,
) -> Value {
    // Step 1: Rate limit check
    if !server.image_gen_rate_limiter().check_rate_limit(session_id) {
        warn!(
            "Image generation rate limit exceeded for session {}",
            session_id
        );
        return build_encrypted_error(
            "RATE_LIMIT_EXCEEDED",
            "Image generation rate limit exceeded",
            session_key,
            session_id,
            message_id,
        );
    }

    // Step 2: Deserialize request from camelCase SDK JSON
    let request: GenerateImageRequest = match serde_json::from_value(decrypted_json.clone()) {
        Ok(req) => req,
        Err(e) => {
            warn!("Failed to deserialize image generation request: {}", e);
            return build_encrypted_error(
                "VALIDATION_FAILED",
                &format!("Invalid request: {}", e),
                session_key,
                session_id,
                message_id,
            );
        }
    };

    // Step 3: Validate request
    if let Err(e) = request.validate() {
        warn!("Image generation request validation failed: {}", e);
        return build_encrypted_error("VALIDATION_FAILED", &e, session_key, session_id, message_id);
    }

    // Step 4: Prompt safety (keyword blocklist)
    let safety_classifier = PromptSafetyClassifier::new(SafetyConfig::default());
    let safety_result = safety_classifier.check_keywords(&request.prompt);
    if !safety_result.is_safe {
        let reason = safety_result
            .reason
            .unwrap_or_else(|| "Prompt blocked by safety filter".to_string());
        warn!("Image generation prompt blocked: {}", reason);
        return build_encrypted_error(
            "PROMPT_BLOCKED",
            &reason,
            session_key,
            session_id,
            message_id,
        );
    }

    // Step 5: Get diffusion client
    let diffusion_client = server.get_diffusion_client().await;
    let client = match diffusion_client {
        Some(c) => c,
        None => {
            warn!("Diffusion sidecar not configured");
            return build_encrypted_error(
                "DIFFUSION_SERVICE_UNAVAILABLE",
                "Image generation service is not available on this host",
                session_key,
                session_id,
                message_id,
            );
        }
    };

    // Step 6: Build sidecar request and generate
    let size_str = request.size.as_deref().unwrap_or("1024x1024");
    let steps = request.steps.unwrap_or(4);
    let guidance_scale = request.guidance_scale.unwrap_or(3.5);

    let sidecar_request = crate::diffusion::ImageGenerationRequest {
        prompt: request.prompt.clone(),
        model: request.model.clone(),
        size: size_str.to_string(),
        steps,
        seed: request.seed,
        negative_prompt: request.negative_prompt.clone(),
        guidance_scale,
        response_format: "b64_json".to_string(),
        n: 1,
    };

    let gen_result = match client.generate(&sidecar_request).await {
        Ok(r) => r,
        Err(e) => {
            error!("Diffusion sidecar generation failed: {}", e);
            return build_encrypted_error(
                "IMAGE_GENERATION_FAILED",
                &format!("Generation failed: {}", e),
                session_key,
                session_id,
                message_id,
            );
        }
    };

    // Step 7: Calculate billing, record rate limit, track billing
    let (width, height) = (gen_result.width, gen_result.height);
    let processing_time_ms = gen_result.processing_time_ms;
    let units = calculate_generation_units(width, height, steps, 1.0);

    server.image_gen_rate_limiter().record_request(session_id);

    if let Some(jid) = job_id {
        server
            .image_gen_tracker()
            .track(jid, Some(session_id), units)
            .await;
    }

    info!(
        "Image generated: {}x{}, {} steps, {:.2} units, {}ms",
        width, height, steps, units, processing_time_ms
    );

    // Step 8: Build and encrypt response
    let chain_id = request.chain_id.unwrap_or(84532);
    let safety_level = request
        .safety_level
        .as_deref()
        .unwrap_or("strict")
        .to_string();

    let response = GenerateImageResponse::with_chain_context(
        gen_result.base64_image,
        gen_result.model,
        size_str.to_string(),
        steps,
        gen_result.seed,
        processing_time_ms,
        SafetyInfo {
            prompt_safe: true,
            output_safe: true,
            safety_level,
        },
        BillingInfo {
            generation_units: units,
            model_multiplier: 1.0,
            megapixels: (width as f64 * height as f64) / 1_048_576.0,
            steps,
        },
        chain_id,
    );

    let response_json = json!({
        "type": "image_generation_result",
        "image": response.image,
        "model": response.model,
        "size": response.size,
        "steps": response.steps,
        "seed": response.seed,
        "processingTimeMs": response.processing_time_ms,
        "safety": {
            "promptSafe": response.safety.prompt_safe,
            "outputSafe": response.safety.output_safe,
            "safetyLevel": response.safety.safety_level,
        },
        "billing": {
            "generationUnits": response.billing.generation_units,
            "modelMultiplier": response.billing.model_multiplier,
            "megapixels": response.billing.megapixels,
            "steps": response.billing.steps,
        },
        "provider": response.provider,
        "chainId": response.chain_id,
        "chainName": response.chain_name,
        "nativeToken": response.native_token,
    });

    build_encrypted_response(&response_json, session_key, session_id, message_id)
}
