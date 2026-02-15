// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Image generation endpoint handler

use axum::{extract::State, http::StatusCode, Json};
use tracing::{debug, info, warn};

use super::request::GenerateImageRequest;
use super::response::{BillingInfo, GenerateImageResponse, SafetyInfo};
use crate::api::http_server::AppState;
use crate::diffusion::client::ImageSize;
use crate::diffusion::prompt_safety::PromptSafetyClassifier;
use crate::diffusion::safety::SafetyConfig;

/// POST /v1/images/generate - Generate an image from a text prompt
///
/// Pipeline:
/// 1. Validate request
/// 2. Get DiffusionClient from AppState (503 if absent)
/// 3. Run prompt safety keyword check (Layer 1 fast path)
/// 4. If prompt unsafe -> return 400 with reason
/// 5. Call DiffusionClient::generate()
/// 6. Calculate billing units
/// 7. Build and return GenerateImageResponse
pub async fn generate_image_handler(
    State(state): State<AppState>,
    Json(request): Json<GenerateImageRequest>,
) -> Result<Json<GenerateImageResponse>, (StatusCode, String)> {
    debug!(
        "Image generation request received: prompt_len={}, chain_id={:?}",
        request.prompt.len(),
        request.chain_id
    );

    // 1. Validate request
    if let Err(e) = request.validate() {
        warn!("Image generation validation failed: {}", e);
        return Err((StatusCode::BAD_REQUEST, e));
    }

    // 2. Get diffusion client (503 if None)
    let client_guard = state.diffusion_client.read().await;
    let diffusion_client = client_guard.as_ref().ok_or_else(|| {
        warn!("Diffusion service not available");
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Diffusion service not available".to_string(),
        )
    })?;

    // 3. Prompt safety check (Layer 1 — keyword fast path)
    let safety_config = SafetyConfig::default();
    let classifier = PromptSafetyClassifier::new(safety_config);
    let safety_result = classifier.check_keywords(&request.prompt);

    if !safety_result.is_safe {
        let reason = safety_result
            .reason
            .unwrap_or_else(|| "Prompt blocked by safety filter".to_string());
        warn!("Image generation prompt blocked: {}", reason);
        return Err((StatusCode::BAD_REQUEST, reason));
    }

    // 4. Build the internal ImageGenerationRequest for the diffusion client
    let size_str = request.size.as_deref().unwrap_or("1024x1024");
    let steps = request.steps.unwrap_or(4);
    let guidance_scale = request.guidance_scale.unwrap_or(3.5);

    let diffusion_request = crate::diffusion::ImageGenerationRequest {
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

    // 5. Generate image
    let result = diffusion_client
        .generate(&diffusion_request)
        .await
        .map_err(|e| {
            warn!("Diffusion generation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Image generation failed: {}", e),
            )
        })?;

    // 6. Calculate billing
    let size = ImageSize::parse(size_str).unwrap_or(ImageSize {
        width: 1024,
        height: 1024,
    });
    let megapixels = size.megapixels();
    let step_factor = steps as f64 / 20.0;
    let model_multiplier = 1.0;
    let generation_units = megapixels * step_factor * model_multiplier;

    let billing = BillingInfo {
        generation_units,
        model_multiplier,
        megapixels,
        steps,
    };

    let safety_info = SafetyInfo {
        prompt_safe: true,
        output_safe: true, // Output safety via VLM deferred to future phase
        safety_level: request
            .safety_level
            .as_deref()
            .unwrap_or("strict")
            .to_string(),
    };

    let chain_id = request.chain_id.unwrap_or(84532);

    info!(
        "Image generated: model={}, size={}, steps={}, {}ms, {:.2} units",
        result.model, size_str, steps, result.processing_time_ms, generation_units
    );

    // 7. Build response
    let response = GenerateImageResponse::with_chain_context(
        result.base64_image,
        result.model,
        size_str.to_string(),
        result.steps,
        result.seed,
        result.processing_time_ms,
        safety_info,
        billing,
        chain_id,
    );

    Ok(Json(response))
}
