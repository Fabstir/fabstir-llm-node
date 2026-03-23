// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! HTTP handlers for POST /v1/transcode and GET /v1/transcode/:task_id.

use super::request::TranscodeHttpRequest;
use super::response::{TranscodeHttpResponse, TranscodeStatusHttpResponse};
use crate::api::server::ApiServer;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use std::sync::Arc;

/// POST /v1/transcode — submit a transcoding job.
pub async fn transcode_submit_handler(
    State(server): State<Arc<ApiServer>>,
    Json(request): Json<TranscodeHttpRequest>,
) -> impl IntoResponse {
    // Validate
    if request.source_cid.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "source_cid must not be empty"})),
        )
            .into_response();
    }
    if request.media_formats.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "media_formats must not be empty"})),
        )
            .into_response();
    }

    // Check sidecar availability (before capacity — "not configured" is 503, not 429)
    let transcoder_client = match server.get_transcoder_client().await {
        Some(client) => client,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "Transcoder sidecar not configured"})),
            )
                .into_response();
        }
    };

    // Check capacity (read-only — HTTP path does not acquire/release slots)
    if !server.has_sidecar_capacity().await {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": "TRANSCODE_CAPACITY_FULL",
                "message": "All transcode slots are in use — try again later"
            })),
        )
            .into_response();
    }

    // Submit to transcoder
    match transcoder_client
        .submit_transcode(
            &request.source_cid,
            &request.media_formats,
            request.is_encrypted,
            request.is_gpu,
        )
        .await
    {
        Ok(resp) => {
            let response = TranscodeHttpResponse {
                task_id: resp.task_id,
                status: "accepted".to_string(),
                message: resp.message,
            };
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("Transcoder error: {}", e)})),
        )
            .into_response(),
    }
}

/// GET /v1/transcode/:task_id — check transcoding status.
pub async fn transcode_status_handler(
    State(server): State<Arc<ApiServer>>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    // Check sidecar availability
    let transcoder_client = match server.get_transcoder_client().await {
        Some(client) => client,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "Transcoder sidecar not configured"})),
            )
                .into_response();
        }
    };

    match transcoder_client.get_status(&task_id).await {
        Ok(status) => {
            let outputs: Option<serde_json::Value> =
                if !status.metadata.is_empty() && status.metadata != "[]" {
                    serde_json::from_str(&status.metadata).ok()
                } else {
                    None
                };

            let response = TranscodeStatusHttpResponse {
                task_id,
                progress: status.progress,
                status: if status.progress >= 100 {
                    "completed".to_string()
                } else {
                    "in_progress".to_string()
                },
                outputs,
                billing: None,
                duration: status.duration,
            };
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": format!("Transcoder error: {}", e)})),
        )
            .into_response(),
    }
}
