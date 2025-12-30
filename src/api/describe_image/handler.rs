// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Describe image endpoint handler

use axum::{extract::State, http::StatusCode, Json};

use super::request::DescribeImageRequest;
use super::response::DescribeImageResponse;
use crate::api::http_server::AppState;

/// POST /v1/describe-image - Generate a description of an image
///
/// Accepts a base64-encoded image and returns a text description.
/// Uses Florence-2 running on CPU.
pub async fn describe_image_handler(
    State(state): State<AppState>,
    Json(request): Json<DescribeImageRequest>,
) -> Result<Json<DescribeImageResponse>, (StatusCode, String)> {
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

    // Get Florence model
    let _florence_model = manager.get_florence_model().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Florence model not loaded".to_string(),
        )
    })?;

    // TODO: Implement in Sub-phase 5.3
    // 1. Decode base64 image
    // 2. Run Florence description
    // 3. Build response

    // For now, return a stub response
    Err((
        StatusCode::NOT_IMPLEMENTED,
        "Image description not yet implemented".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handler_exists() {
        // Just verify the handler compiles
        let _ = describe_image_handler;
    }
}
