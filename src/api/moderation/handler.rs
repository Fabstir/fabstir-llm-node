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
use crate::moderation::csam::quarantine::{evidence_category, preserve_if_blocked, Quarantine};
use crate::moderation::types::{AssetKind, ModerationResult};

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

/// Shared private core (R3-D3): validate + base64-decode + moderate, returning the
/// `AssetKind`, the ORIGINAL decoded file bytes, and the verdict. `max_bytes` is
/// injected so the size limit is testable without a multi-MB body. Fail-closed: bad
/// input is rejected; the moderator itself holds on any moderation error. Both
/// `moderate_asset_inner` (verdict-only) and `moderate_asset_inner_preserving`
/// (block ⇒ preserve) build on this, so they cannot drift.
fn decode_and_moderate(
    am: &AssetModerator,
    req: &ModerateAssetRequest,
    max_bytes: usize,
) -> Result<(AssetKind, Vec<u8>, ModerationResult), (StatusCode, String)> {
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
    let result = am.moderate(kind, &bytes);
    Ok((kind, bytes, result))
}

/// Pure, testable core: validate + base64-decode + moderate.
///
/// ⚠️ **Verdict-only — this does NOT preserve evidence.** Block paths MUST use
/// [`moderate_asset_inner_preserving`] instead, or a `blocked` verdict is returned
/// with no quarantined evidence (the B6 gap). Retained at this 3-arg signature for
/// its committed callers (6 in `test_moderate_asset.rs` + this handler's history);
/// see R3-D3/R4-C2.
pub fn moderate_asset_inner(
    am: &AssetModerator,
    req: &ModerateAssetRequest,
    max_bytes: usize,
) -> Result<ModerateAssetResponse, (StatusCode, String)> {
    let (_kind, _bytes, result) = decode_and_moderate(am, req, max_bytes)?;
    Ok(ModerateAssetResponse::from(result))
}

/// Block-aware variant (B6): runs the shared core, then on a non-`Cleared` verdict
/// **preserves the ORIGINAL file bytes** under the kind-derived [`evidence_category`]
/// before returning. 🚨 Fail-closed: a preserve failure ⇒ `503` HOLD — never a
/// `cleared`/`blocked`-without-evidence response (R2-F1/F2). `job` is `None` here
/// (the `/asset` path has no transcode job; `/frames` passes `Some`).
pub fn moderate_asset_inner_preserving(
    am: &AssetModerator,
    req: &ModerateAssetRequest,
    max_bytes: usize,
    quarantine: &std::sync::Mutex<Quarantine>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<ModerateAssetResponse, (StatusCode, String)> {
    let (kind, bytes, result) = decode_and_moderate(am, req, max_bytes)?;
    // Dispatch on the ACTUAL moderation OUTCOME (not a list-availability pre-gate, which
    // would mask an own-hash match while the NCMEC list is unavailable — R9-B):
    //  • Cleared              ⇒ release (200, no preserve).
    //  • genuine match/flag   ⇒ preserve the ORIGINAL bytes (B6) + 200 verdict. This
    //    includes an own-hash hit while NCMEC is unavailable AND an undecodable exact-SHA
    //    hit (the match runs over the raw bytes before decode — R9-A), so a `blocked`
    //    verdict is never returned with no evidence.
    //  • any other non-Cleared (infra/can't-scan HOLD: list unavailable & no own-hash hit,
    //    undecodable-with-no-hit, PDQ-compute-fail, invalid-UTF-8) ⇒ retryable 503,
    //    preserve NOTHING (no matched content; also closes the unauthenticated /asset
    //    quarantine-fill, since unmatched/garbage bytes are never quarantined).
    if result.verdict.releases() {
        return Ok(ModerateAssetResponse::from(result));
    }
    if !result.is_genuine_hit() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "moderation unavailable".to_string(),
        ));
    }
    let category = evidence_category(kind);
    let mut q = quarantine.lock().unwrap_or_else(|e| e.into_inner());
    preserve_if_blocked(
        &mut q,
        result.verdict,
        &[bytes.as_slice()],
        category,
        None,
        now,
    )
    .map_err(|_| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "moderation unavailable".to_string(),
        )
    })?;
    Ok(ModerateAssetResponse::from(result))
}

/// `POST /v1/moderate/asset`.
pub async fn moderate_asset_handler(
    State(server): State<Arc<ApiServer>>,
    Json(req): Json<ModerateAssetRequest>,
) -> impl IntoResponse {
    let am = server.build_asset_moderator();
    match moderate_asset_inner_preserving(
        &am,
        &req,
        MAX_ASSET_BYTES,
        server.moderation_quarantine(),
        chrono::Utc::now(),
    ) {
        Ok(resp) => {
            // Observability (§8 #7): count the verdict.
            let verdict = match resp.verdict.as_str() {
                "cleared" => crate::moderation::types::Verdict::Cleared,
                "flagged" => crate::moderation::types::Verdict::Flagged,
                _ => crate::moderation::types::Verdict::Blocked,
            };
            server.moderation_metrics().record_verdict(verdict);
            // A 200 `Blocked` from the preserving path is a genuine Track-1 match (R9
            // dispatch returns 503 for infra/can't-scan holds) — count it as a match.
            if verdict == crate::moderation::types::Verdict::Blocked {
                server.moderation_metrics().record_match();
            }
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err((status, msg)) => (status, Json(serde_json::json!({ "error": msg }))).into_response(),
    }
}
