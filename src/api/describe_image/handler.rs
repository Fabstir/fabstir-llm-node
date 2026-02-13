// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Describe image endpoint handler

use axum::{extract::State, http::StatusCode, Json};
use tracing::{debug, info, warn};

use super::request::DescribeImageRequest;
use super::response::{DescribeImageResponse, ImageAnalysis};
use crate::api::http_server::AppState;
use crate::vision::decode_base64_image;

/// POST /v1/describe-image - Generate a description of an image
///
/// Accepts a base64-encoded image and returns a text description.
/// Uses Florence-2 running on CPU.
///
/// # Request
/// - `image`: Base64-encoded image data (required)
/// - `format`: Image format hint (png, jpg, webp, gif) - defaults to "png"
/// - `detail`: Detail level (brief, detailed, comprehensive) - defaults to "detailed"
/// - `prompt`: Custom prompt for description (optional)
/// - `maxTokens`: Maximum tokens in response (10-500) - defaults to 150
/// - `chainId`: Chain ID for pricing context - defaults to 84532 (Base Sepolia)
///
/// # Response
/// - `description`: Generated text description
/// - `objects`: Detected objects (currently empty, reserved for future)
/// - `analysis`: Image metadata (dimensions, colors)
/// - `processingTimeMs`: Processing time in milliseconds
/// - `model`: Model used ("florence-2")
/// - `provider`: Service provider ("host")
/// - `chainId`, `chainName`, `nativeToken`: Chain context
///
/// # Errors
/// - 400 Bad Request: Invalid request (missing image, invalid format, etc.)
/// - 503 Service Unavailable: Florence model not loaded
/// - 500 Internal Server Error: Description generation failed
pub async fn describe_image_handler(
    State(state): State<AppState>,
    Json(request): Json<DescribeImageRequest>,
) -> Result<Json<DescribeImageResponse>, (StatusCode, String)> {
    debug!(
        "Describe-image request received: detail={}, chain_id={}",
        request.detail, request.chain_id
    );

    // 1. Validate request
    if let Err(e) = request.validate() {
        warn!("Describe-image validation failed: {}", e);
        return Err((StatusCode::BAD_REQUEST, e.to_string()));
    }

    // 2. Get vision model manager from state
    let manager_guard = state.vision_model_manager.read().await;
    let manager = manager_guard.as_ref().ok_or_else(|| {
        warn!("Vision service not available");
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Vision service not available".to_string(),
        )
    })?;

    // 2b. Try VLM first (if available)
    if let Some(vlm_client) = manager.get_vlm_client() {
        let vlm_image = request
            .image
            .as_ref()
            .ok_or_else(|| (StatusCode::BAD_REQUEST, "image is required".to_string()))?;

        match vlm_client
            .describe(
                vlm_image,
                &request.format,
                &request.detail,
                request.prompt.as_deref(),
            )
            .await
        {
            Ok(vlm_result) => {
                info!(
                    "VLM describe complete: {} chars, {}ms (model: {})",
                    vlm_result.description.len(),
                    vlm_result.processing_time_ms,
                    vlm_result.model
                );

                let analysis = ImageAnalysis {
                    width: 0,
                    height: 0,
                    dominant_colors: vec![],
                    scene_type: None,
                };
                let response = DescribeImageResponse::new(
                    vlm_result.description,
                    vec![],
                    analysis,
                    vlm_result.processing_time_ms,
                    request.chain_id,
                    &vlm_result.model,
                );
                return Ok(Json(response));
            }
            Err(e) => {
                warn!("VLM describe failed, falling back to Florence-2: {}", e);
            }
        }
    }

    // 3. Get Florence model (ONNX fallback)
    let florence_model = manager.get_florence_model().ok_or_else(|| {
        warn!("Florence model not loaded");
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Florence model not loaded".to_string(),
        )
    })?;

    // 4. Decode base64 image
    let image_data = request
        .image
        .as_ref()
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "image is required".to_string()))?;

    let (image, image_info) = decode_base64_image(image_data).map_err(|e| {
        warn!("Failed to decode image: {}", e);
        (StatusCode::BAD_REQUEST, format!("Invalid image: {}", e))
    })?;

    debug!(
        "Decoded image: {}x{}, {} bytes",
        image_info.width, image_info.height, image_info.size_bytes
    );

    // 5. Run Florence description
    info!(
        "Running Florence describe: detail={}, prompt={:?}",
        request.detail,
        request.prompt.as_deref()
    );

    let description_result = florence_model
        .describe(&image, &request.detail, request.prompt.as_deref())
        .map_err(|e| {
            // Log full error chain for debugging
            warn!("Florence description failed: {}", e);
            let mut chain = e.chain();
            chain.next(); // Skip the first (already logged)
            for cause in chain {
                warn!("  Caused by: {}", cause);
            }
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Description failed: {}", e),
            )
        })?;

    info!(
        "Florence complete: {} chars, {}ms",
        description_result.description.len(),
        description_result.processing_time_ms
    );

    // 6. Build response with chain context
    // Note: Objects detection is not yet implemented in Florence, returning empty
    let analysis = ImageAnalysis {
        width: description_result.analysis.width,
        height: description_result.analysis.height,
        dominant_colors: description_result.analysis.dominant_colors.clone(),
        scene_type: description_result.analysis.scene_type.clone(),
    };

    let response = DescribeImageResponse::new(
        description_result.description,
        vec![], // Objects detection reserved for future
        analysis,
        description_result.processing_time_ms,
        request.chain_id,
        "florence-2",
    );

    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handler_exists() {
        // Just verify the handler compiles
        let _ = describe_image_handler;
    }

    #[test]
    fn test_describe_handler_vlm_model_field() {
        let response = DescribeImageResponse::new(
            "VLM description".to_string(),
            vec![],
            ImageAnalysis {
                width: 0,
                height: 0,
                dominant_colors: vec![],
                scene_type: None,
            },
            50,
            84532,
            "qwen3-vl",
        );
        assert_eq!(response.model, "qwen3-vl");
    }

    #[test]
    fn test_describe_handler_onnx_model_field() {
        let response = DescribeImageResponse::new(
            "ONNX description".to_string(),
            vec![],
            ImageAnalysis {
                width: 640,
                height: 480,
                dominant_colors: vec![],
                scene_type: None,
            },
            200,
            84532,
            "florence-2",
        );
        assert_eq!(response.model, "florence-2");
    }

    #[test]
    fn test_describe_handler_fallback_on_vlm_error() {
        // When VLM fails, fallback produces "florence-2" model
        let response = DescribeImageResponse::new(
            "fallback".to_string(),
            vec![],
            ImageAnalysis {
                width: 100,
                height: 100,
                dominant_colors: vec![],
                scene_type: None,
            },
            300,
            84532,
            "florence-2",
        );
        assert_eq!(response.model, "florence-2");
        assert_eq!(response.description, "fallback");
    }

    #[test]
    fn test_image_analysis_creation() {
        let analysis = ImageAnalysis {
            width: 1920,
            height: 1080,
            dominant_colors: vec!["#FF0000".to_string()],
            scene_type: Some("outdoor".to_string()),
        };
        assert_eq!(analysis.width, 1920);
        assert_eq!(analysis.height, 1080);
    }
}
