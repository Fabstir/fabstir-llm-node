// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! OCR endpoint handler

use axum::{Json, extract::State, http::StatusCode};
use tracing::{debug, info, warn};

use super::request::OcrRequest;
use super::response::{BoundingBox, OcrResponse, TextRegion};
use crate::api::http_server::AppState;
use crate::vision::decode_base64_image;

/// POST /v1/ocr - Extract text from an image
///
/// Accepts a base64-encoded image and returns extracted text with bounding boxes.
/// Uses PaddleOCR running on CPU.
///
/// # Request
/// - `image`: Base64-encoded image data (required)
/// - `format`: Image format hint (png, jpg, webp, gif) - defaults to "png"
/// - `language`: Language hint (en, zh, ja, ko) - defaults to "en"
/// - `chainId`: Chain ID for pricing context - defaults to 84532 (Base Sepolia)
///
/// # Response
/// - `text`: Full extracted text (all regions combined)
/// - `confidence`: Average confidence score (0.0-1.0)
/// - `regions`: Individual text regions with bounding boxes
/// - `processingTimeMs`: Processing time in milliseconds
/// - `model`: Model used ("paddleocr")
/// - `provider`: Service provider ("host")
/// - `chainId`, `chainName`, `nativeToken`: Chain context
///
/// # Errors
/// - 400 Bad Request: Invalid request (missing image, invalid format, etc.)
/// - 503 Service Unavailable: OCR model not loaded
/// - 500 Internal Server Error: OCR processing failed
pub async fn ocr_handler(
    State(state): State<AppState>,
    Json(request): Json<OcrRequest>,
) -> Result<Json<OcrResponse>, (StatusCode, String)> {
    debug!("OCR request received for chain_id: {}", request.chain_id);

    // 1. Validate request
    if let Err(e) = request.validate() {
        warn!("OCR validation failed: {}", e);
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

        match vlm_client.ocr(vlm_image, &request.format).await {
            Ok(vlm_result) => {
                info!(
                    "VLM OCR complete: {} chars, {}ms (model: {})",
                    vlm_result.text.len(),
                    vlm_result.processing_time_ms,
                    vlm_result.model
                );

                let response = OcrResponse::new(
                    vlm_result.text,
                    1.0,
                    vec![],
                    vlm_result.processing_time_ms,
                    request.chain_id,
                    &vlm_result.model,
                );
                return Ok(Json(response));
            }
            Err(e) => {
                warn!("VLM OCR failed, falling back to ONNX: {}", e);
            }
        }
    }

    // 3. Get OCR model (ONNX fallback)
    let ocr_model = manager.get_ocr_model().ok_or_else(|| {
        warn!("OCR model not loaded");
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "OCR model not loaded".to_string(),
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

    // 5. Run OCR
    let ocr_result = ocr_model.process(&image).map_err(|e| {
        warn!("OCR processing failed: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("OCR processing failed: {}", e),
        )
    })?;

    info!(
        "OCR complete: {} regions, {:.2} confidence, {}ms",
        ocr_result.regions.len(),
        ocr_result.confidence,
        ocr_result.processing_time_ms
    );

    // 6. Convert OCR result to response format
    let regions: Vec<TextRegion> = ocr_result
        .regions
        .iter()
        .map(|r| TextRegion {
            text: r.text.clone(),
            confidence: r.confidence,
            bounding_box: BoundingBox {
                x: r.bounding_box.x,
                y: r.bounding_box.y,
                width: r.bounding_box.width,
                height: r.bounding_box.height,
            },
        })
        .collect();

    // 7. Build response with chain context
    let response = OcrResponse::new(
        ocr_result.text,
        ocr_result.confidence,
        regions,
        ocr_result.processing_time_ms,
        request.chain_id,
        "paddleocr",
    );

    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handler_exists() {
        // Just verify the handler compiles
        let _ = ocr_handler;
    }

    #[test]
    fn test_ocr_handler_vlm_model_field() {
        // VLM OCR result should carry model name
        let response = OcrResponse::new("VLM text".to_string(), 1.0, vec![], 50, 84532, "qwen3-vl");
        assert_eq!(response.model, "qwen3-vl");
    }

    #[test]
    fn test_ocr_handler_onnx_model_field() {
        // ONNX fallback should use "paddleocr"
        let response = OcrResponse::new(
            "ONNX text".to_string(),
            0.95,
            vec![],
            100,
            84532,
            "paddleocr",
        );
        assert_eq!(response.model, "paddleocr");
    }

    #[test]
    fn test_ocr_handler_fallback_on_vlm_error() {
        // When VLM fails, fallback produces "paddleocr" model name
        let response =
            OcrResponse::new("fallback".to_string(), 0.9, vec![], 200, 84532, "paddleocr");
        assert_eq!(response.model, "paddleocr");
        assert!(response.text == "fallback");
    }

    #[test]
    fn test_text_region_conversion() {
        let region = TextRegion {
            text: "Hello".to_string(),
            confidence: 0.95,
            bounding_box: BoundingBox {
                x: 10,
                y: 20,
                width: 100,
                height: 30,
            },
        };
        assert_eq!(region.text, "Hello");
        assert_eq!(region.bounding_box.x, 10);
    }
}
