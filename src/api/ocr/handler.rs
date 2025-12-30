// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! OCR endpoint handler

use axum::{extract::State, http::StatusCode, Json};

use super::request::OcrRequest;
use super::response::OcrResponse;
use crate::api::http_server::AppState;

/// POST /v1/ocr - Extract text from an image
///
/// Accepts a base64-encoded image and returns extracted text with bounding boxes.
/// Uses PaddleOCR running on CPU.
pub async fn ocr_handler(
    State(state): State<AppState>,
    Json(request): Json<OcrRequest>,
) -> Result<Json<OcrResponse>, (StatusCode, String)> {
    // Validate request
    if let Err(e) = request.validate() {
        return Err((StatusCode::BAD_REQUEST, e.to_string()));
    }

    // Get vision model manager from state
    let manager_guard = state.vision_model_manager.read().await;
    let manager = manager_guard.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Vision service not available".to_string(),
        )
    })?;

    // Get OCR model
    let _ocr_model = manager.get_ocr_model().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "OCR model not loaded".to_string(),
        )
    })?;

    // TODO: Implement in Sub-phase 5.2
    // 1. Decode base64 image
    // 2. Run OCR
    // 3. Build response

    // For now, return a stub response
    Err((
        StatusCode::NOT_IMPLEMENTED,
        "OCR processing not yet implemented".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handler_exists() {
        // Just verify the handler compiles
        let _ = ocr_handler;
    }
}
