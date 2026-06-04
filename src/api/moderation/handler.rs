// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! HTTP handler for `POST /v1/moderate/asset` (B8).

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use base64::Engine;

use crate::api::moderation::request::ModerateAssetRequest;
use crate::api::moderation::response::ModerateAssetResponse;
use crate::api::server::ApiServer;
use crate::moderation::asset::AssetModerator;
use crate::moderation::types::AssetKind;

/// Max decoded asset size (defense-in-depth alongside the router body limit).
pub const MAX_ASSET_BYTES: usize = 20 * 1024 * 1024;

fn parse_kind(s: &str) -> Option<AssetKind> {
    match s {
        "image" => Some(AssetKind::Image),
        "subtitle" => Some(AssetKind::Subtitle),
        "video_keyframe" | "videoKeyframe" => Some(AssetKind::VideoKeyframe),
        _ => None,
    }
}

/// Pure, testable core: validate + base64-decode + moderate. `max_bytes` is
/// injected so the size limit is testable without a multi-MB body. Fail-closed:
/// bad input is rejected; the moderator itself holds on any moderation error.
pub fn moderate_asset_inner(
    am: &AssetModerator,
    req: &ModerateAssetRequest,
    max_bytes: usize,
) -> Result<ModerateAssetResponse, (StatusCode, String)> {
    let kind = parse_kind(&req.kind).ok_or((
        StatusCode::BAD_REQUEST,
        format!("unknown asset kind: {}", req.kind),
    ))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(req.data.as_bytes())
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid base64 data".to_string()))?;
    if bytes.len() > max_bytes {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            "asset exceeds size limit".to_string(),
        ));
    }
    Ok(ModerateAssetResponse::from(am.moderate(kind, &bytes)))
}

/// `POST /v1/moderate/asset`.
pub async fn moderate_asset_handler(
    State(server): State<Arc<ApiServer>>,
    Json(req): Json<ModerateAssetRequest>,
) -> impl IntoResponse {
    let am = server.build_asset_moderator();
    match moderate_asset_inner(&am, &req, MAX_ASSET_BYTES) {
        Ok(resp) => {
            // Observability (§8 #7): count the verdict.
            let verdict = match resp.verdict.as_str() {
                "cleared" => crate::moderation::types::Verdict::Cleared,
                "flagged" => crate::moderation::types::Verdict::Flagged,
                _ => crate::moderation::types::Verdict::Blocked,
            };
            server.moderation_metrics().record_verdict(verdict);
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err((status, msg)) => (status, Json(serde_json::json!({ "error": msg }))).into_response(),
    }
}
