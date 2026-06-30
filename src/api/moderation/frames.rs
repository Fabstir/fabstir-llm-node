// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! `POST /v1/moderate/frames` — the seam-#1 node endpoint (DESIGN §3.2). 🚨
//!
//! Strict, fail-closed order: (1) verify the ingest token (401, no work, no write);
//! (2) resolve `task_id → job_id` (404 ⇒ HOLD); (3) decode each keyframe to the
//! ORIGINAL PNG bytes (kept for evidence) and a transient `DecodedFrame` (RGB, for
//! PDQ only); (4) `moderate_frames`; (5) on a non-Cleared verdict, preserve every
//! original PNG — a preserve failure ⇒ 503 HOLD, never cleared (R2-F2/F4); (6) write
//! `VerdictStore` via `set_if_not_downgrade` (a Cleared can't overwrite a block, C4);
//! (7) return the verdict synchronously. `reportId` is always `null` (B7: never
//! auto-file — filing stays human-initiated via `/v1/moderate/review`).

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::api::server::ApiServer;
use crate::moderation::csam::hashlist::HashListSnapshot;
use crate::moderation::csam::ownhash::OwnHashList;
use crate::moderation::csam::quarantine::{preserve_if_blocked, Quarantine};
use crate::moderation::csam::{self};
use crate::moderation::ingest::{DecodedFrame, IngestItem};
use crate::moderation::types::{Category, ModerationResult, Verdict};
use crate::moderation::verdict_store::VerdictStore;

/// `POST /v1/moderate/frames` request (DESIGN §3.2). `ingestToken` is the
/// transcoder↔node shared secret (body field, mirroring `/review`'s `reviewer_token`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerateFramesRequest {
    #[serde(rename = "ingestToken")]
    pub ingest_token: String,
    #[serde(rename = "taskId")]
    pub task_id: String,
    #[serde(rename = "keyframes_png_base64")]
    pub keyframes_png_base64: Vec<String>,
    #[serde(
        rename = "sourceSha256",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub source_sha256: Option<String>,
}

/// `POST /v1/moderate/frames` response. `reportId` is ALWAYS `null` here (B7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerateFramesResponse {
    pub verdict: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(rename = "reportId")]
    pub report_id: Option<String>,
}

impl From<ModerationResult> for ModerateFramesResponse {
    fn from(r: ModerationResult) -> Self {
        Self {
            verdict: r.verdict.as_str().to_string(),
            reason: r.reason,
            // B7: the node never auto-files an NCMEC report on this path.
            report_id: None,
        }
    }
}

/// Pure, testable core. `match_state` (snapshot, ownhash, max_distance) is injected
/// so tests can drive a *Loaded* list (production passes the `Unavailable` snapshot
/// from `ApiServer::build_frames_match_state`, which fail-closed HOLDs until the real
/// NCMEC list lands). Decodes each keyframe to the original PNG (kept) + a transient
/// RGB frame (PDQ only); blocks ⇒ preserve-every-PNG-or-503; writes the verdict
/// monotonically. Returns `(StatusCode, msg)` on any fail-closed condition.
pub fn moderate_frames_inner(
    job_id: u64,
    store: &VerdictStore,
    quarantine: &std::sync::Mutex<Quarantine>,
    match_state: (HashListSnapshot, OwnHashList, u32),
    req: &ModerateFramesRequest,
    now: DateTime<Utc>,
) -> Result<ModerateFramesResponse, (StatusCode, String)> {
    let (snapshot, ownhash, max_distance) = match_state;

    // An empty submission is malformed/retryable — 400, write NO verdict (a stored
    // Blocked here would permanently poison the job via set_if_not_downgrade) and
    // preserve nothing.
    if req.keyframes_png_base64.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "no keyframes provided".to_string()));
    }

    // Decode each keyframe to (a) the original PNG bytes (evidence, re-hashable —
    // R2-F4) and (b) a transient DecodedFrame (RGB, for PDQ only).
    let mut png_blobs: Vec<Vec<u8>> = Vec::with_capacity(req.keyframes_png_base64.len());
    let mut frames: Vec<DecodedFrame> = Vec::with_capacity(req.keyframes_png_base64.len());
    for b64png in &req.keyframes_png_base64 {
        let png = base64::engine::general_purpose::STANDARD
            .decode(b64png.as_bytes())
            .map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    "invalid base64 keyframe".to_string(),
                )
            })?;
        let img = image::load_from_memory(&png)
            .map_err(|_| (StatusCode::BAD_REQUEST, "undecodable keyframe".to_string()))?;
        let rgb = img.to_rgb8();
        let (width, height) = (rgb.width(), rgb.height());
        frames.push(DecodedFrame {
            width,
            height,
            rgb: rgb.into_raw(),
        });
        png_blobs.push(png);
    }

    // Optional job-level `sourceSha256` (DESIGN §3.2): the source-file exact / own-hash
    // re-upload halt — the only useful exact-match input on this path (re-encoded
    // keyframes never bit-match NCMEC image SHAs). When present, parse the 64-hex to
    // [u8;32] (malformed ⇒ 400) and run it through the exact-match prefilter alongside the
    // keyframe PDQ; an own-hash hit on the source SHA blocks even when the NCMEC list is
    // unavailable (the matcher checks the own-hash list before availability). Absent ⇒
    // keyframe-PDQ only. (R9-5)
    let item = match &req.source_sha256 {
        Some(hex) => {
            let mut sha = [0u8; 32];
            hex::decode_to_slice(hex.as_bytes(), &mut sha)
                .map_err(|_| (StatusCode::BAD_REQUEST, "invalid sourceSha256".to_string()))?;
            IngestItem::Both {
                job_id,
                frames,
                audio: None,
                sha256: vec![sha],
                pdq: vec![],
            }
        }
        None => IngestItem::Frames {
            job_id,
            frames,
            audio: None,
        },
    };
    let result = csam::moderate_frames(&item, &snapshot, &ownhash, max_distance);

    // Dispatch on the OUTCOME, NOT a list-availability pre-gate (that would mask an
    // own-hash source-SHA match while NCMEC is unavailable — R9-B):
    //  • Cleared          ⇒ record Cleared (C4) + 200.
    //  • genuine match    ⇒ preserve every ORIGINAL PNG (B6) + record Blocked + 200.
    //  • infra/can't-scan HOLD (NCMEC unavailable & no own-hash hit, etc.) ⇒ retryable
    //    503, write NO verdict (so a retry once the list lands isn't poisoned) and
    //    preserve NOTHING.
    if result.verdict.releases() {
        store.set_if_not_downgrade(job_id, result.clone());
        return Ok(ModerateFramesResponse::from(result));
    }
    if !result.is_genuine_hit() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "moderation unavailable".to_string(),
        ));
    }
    // Genuine match ⇒ preserve every ORIGINAL PNG. A preserve failure ⇒ 503 HOLD with no
    // releasing VerdictStore write (R2-F2).
    let blob_slices: Vec<&[u8]> = png_blobs.iter().map(|b| b.as_slice()).collect();
    {
        let mut q = quarantine.lock().unwrap_or_else(|e| e.into_inner());
        preserve_if_blocked(
            &mut q,
            result.verdict,
            &blob_slices,
            Category::Csam,
            Some(job_id),
            now,
        )
        .map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "moderation unavailable".to_string(),
            )
        })?;
    }
    store.set_if_not_downgrade(job_id, result.clone());
    Ok(ModerateFramesResponse::from(result))
}

/// `POST /v1/moderate/frames`. Auth FIRST (no global layer on the nest — the handler
/// checks the body-field `ingestToken` itself, mirroring `/review`).
pub async fn moderate_frames_handler(
    State(server): State<Arc<ApiServer>>,
    Json(req): Json<ModerateFramesRequest>,
) -> impl IntoResponse {
    // (1) Auth — 401 before ANY work or VerdictStore write (R2-F3 / R3-C1).
    if !server.verify_ingest_token(&req.ingest_token) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorised" })),
        )
            .into_response();
    }
    // (2) Resolve task_id → job_id; unknown ⇒ 404 ⇒ the transcoder HOLDs.
    let job_id = match server.job_for_task(&req.task_id) {
        Some(j) => j,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "unknown task" })),
            )
                .into_response();
        }
    };
    match moderate_frames_inner(
        job_id,
        server.moderation_store(),
        server.moderation_quarantine(),
        server.build_frames_match_state(),
        &req,
        Utc::now(),
    ) {
        Ok(resp) => {
            let verdict = match resp.verdict.as_str() {
                "cleared" => Verdict::Cleared,
                "flagged" => Verdict::Flagged,
                _ => Verdict::Blocked,
            };
            server.moderation_metrics().record_verdict(verdict);
            // A 200 `Blocked` from the inner is a genuine Track-1 match (the dispatch
            // returns 503 for infra/can't-scan holds) — count it as a match.
            if verdict == Verdict::Blocked {
                server.moderation_metrics().record_match();
            }
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err((status, msg)) => (status, Json(serde_json::json!({ "error": msg }))).into_response(),
    }
}
