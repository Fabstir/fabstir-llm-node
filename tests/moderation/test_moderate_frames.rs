// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 2.2 — `POST /v1/moderate/frames` (seam #1 node side). 🚨
//!
//! Block/clear behaviour is driven through `moderate_frames_inner` with an injected
//! *Loaded* match-state, because the PRODUCTION handler builds an `Unavailable`
//! snapshot (fail-closed HOLD until the real NCMEC list lands) — so a "benign⇒cleared"
//! result is only observable via the inner. Auth (401) and unknown-task (404) are the
//! handler's own logic and are exercised through the router.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use chrono::{DateTime, Utc};
use tower::util::ServiceExt;

use fabstir_llm_node::api::moderation::{moderate_frames_inner, ModerateFramesRequest};
use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::moderation::csam::hashlist::{HashListSnapshot, HashListSource};
use fabstir_llm_node::moderation::csam::mock_source::MockHashListSource;
use fabstir_llm_node::moderation::csam::ownhash::OwnHashList;
use fabstir_llm_node::moderation::csam::pdq::compute_pdq_rgb;
use fabstir_llm_node::moderation::csam::quarantine::{Quarantine, Role};
use fabstir_llm_node::moderation::types::{Category, ModerationResult, Verdict};
use fabstir_llm_node::moderation::verdict_store::VerdictStore;

fn at() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).unwrap()
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// A non-degenerate (varied-pixel) PNG so PDQ is well-defined and stable.
fn varied_png(w: u32, h: u32) -> Vec<u8> {
    let mut img = image::RgbImage::new(w, h);
    for (x, y, px) in img.enumerate_pixels_mut() {
        *px = image::Rgb([
            (x.wrapping_mul(7)) as u8,
            (y.wrapping_mul(13)) as u8,
            (x.wrapping_add(y).wrapping_mul(5)) as u8,
        ]);
    }
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .unwrap();
    buf.into_inner()
}

/// Compute the PDQ of a PNG via the SAME decode path the node uses, so a seeded
/// list entry matches the submitted keyframe at distance 0 (R3-D6 two-step seed).
fn pdq_of(png: &[u8]) -> fabstir_llm_node::moderation::types::Pdq256 {
    let img = image::load_from_memory(png).unwrap();
    let rgb = img.to_rgb8();
    let (w, h) = (rgb.width(), rgb.height());
    compute_pdq_rgb(rgb.as_raw(), w, h).unwrap().hash
}

fn req(token: &str, task: &str, pngs: Vec<String>) -> ModerateFramesRequest {
    ModerateFramesRequest {
        ingest_token: token.into(),
        task_id: task.into(),
        keyframes_png_base64: pngs,
        source_sha256: None,
    }
}

fn loaded(pdq: Vec<fabstir_llm_node::moderation::types::Pdq256>) -> HashListSnapshot {
    MockHashListSource::loaded(vec![], pdq).refresh().unwrap()
}

// --- production fail-closed path: unavailable list / empty input must HOLD without
//     preserving or poisoning the job (the over-preserve + state-poisoning fixes) ---

#[test]
fn unavailable_list_holds_503_no_preserve_no_write() {
    // Production posture (build_frames_match_state returns Unavailable): an unavailable
    // list is a RETRYABLE infra HOLD, not a content determination — 503, NOTHING
    // preserved, and NO VerdictStore write (so a retry once the list lands isn't poisoned).
    let png = varied_png(64, 64);
    let store = VerdictStore::new();
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let r = req("x", "x", vec![b64(&png)]);
    let err = moderate_frames_inner(
        7,
        &store,
        &q,
        (HashListSnapshot::unavailable(), OwnHashList::new(), 31),
        &r,
        at(),
    )
    .unwrap_err();
    assert_eq!(err.0, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        q.lock().unwrap().is_empty(),
        "preserve must NOT fire on an unavailable-list infra-hold (benign frames)"
    );
    assert!(
        store.get(7).is_none(),
        "no verdict written on an infra-hold ⇒ retryable, not poisoned"
    );
}

#[test]
fn empty_keyframes_is_400_no_write() {
    // An empty submission is malformed/retryable — 400, NO store write (a stored Blocked
    // here would permanently poison the job via set_if_not_downgrade), nothing preserved.
    let store = VerdictStore::new();
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let r = req("x", "x", vec![]);
    let err = moderate_frames_inner(
        8,
        &store,
        &q,
        (loaded(vec![]), OwnHashList::new(), 31),
        &r,
        at(),
    )
    .unwrap_err();
    assert_eq!(err.0, StatusCode::BAD_REQUEST);
    assert!(
        store.get(8).is_none(),
        "empty submission must not poison the job"
    );
    assert!(q.lock().unwrap().is_empty());
}

#[test]
fn unavailable_hold_then_loaded_retry_is_not_poisoned() {
    // The state-poisoning regression: a transient unavailable HOLD must not permanently
    // block a later benign retry of the SAME job once the list is Loaded.
    let png = varied_png(64, 64);
    let store = VerdictStore::new();
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let r = req("x", "x", vec![b64(&png)]);
    // 1) list unavailable ⇒ 503, no write.
    let _ = moderate_frames_inner(
        3,
        &store,
        &q,
        (HashListSnapshot::unavailable(), OwnHashList::new(), 31),
        &r,
        at(),
    )
    .unwrap_err();
    assert!(store.get(3).is_none(), "the hold wrote no verdict");
    // 2) retry once the list is Loaded + content benign ⇒ Cleared stored (not stuck Blocked).
    let resp = moderate_frames_inner(
        3,
        &store,
        &q,
        (loaded(vec![]), OwnHashList::new(), 31),
        &r,
        at(),
    )
    .unwrap();
    assert_eq!(resp.verdict, "cleared");
    assert_eq!(
        store.get(3).unwrap().verdict,
        Verdict::Cleared,
        "the job is NOT permanently poisoned by the earlier transient hold"
    );
}

// --- inner: block / clear / preserve-fail / no-downgrade / bad-input ---

#[test]
fn known_bad_pdq_blocks_preserves_and_categorises() {
    let png = varied_png(64, 64);
    let snapshot = loaded(vec![pdq_of(&png)]);
    let store = VerdictStore::new();
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let r = req("ignored", "ignored", vec![b64(&png)]);
    let resp = moderate_frames_inner(77, &store, &q, (snapshot, OwnHashList::new(), 31), &r, at())
        .unwrap();
    assert_eq!(resp.verdict, "blocked");
    assert_eq!(store.get(77).unwrap().verdict, Verdict::Blocked);
    let mut guard = q.lock().unwrap();
    assert_eq!(
        guard.len(),
        1,
        "the matched keyframe is preserved in the live path"
    );
    assert_eq!(guard.category("case-0"), Some(Category::Csam));
    // R2-F4: the ORIGINAL PNG bytes are preserved (not the decoded RGB).
    let got = guard
        .retrieve("case-0", Role::Reviewer, "tester", at())
        .unwrap();
    assert_eq!(
        got, png,
        "preserved evidence is the original PNG, re-hashable"
    );
}

#[test]
fn benign_clears() {
    let png = varied_png(64, 64);
    let snapshot = loaded(vec![]); // Loaded but empty ⇒ clean miss ⇒ cleared
    let store = VerdictStore::new();
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let r = req("x", "x", vec![b64(&png)]);
    let resp =
        moderate_frames_inner(5, &store, &q, (snapshot, OwnHashList::new(), 31), &r, at()).unwrap();
    assert_eq!(resp.verdict, "cleared");
    assert_eq!(store.get(5).unwrap().verdict, Verdict::Cleared);
    assert!(
        q.lock().unwrap().is_empty(),
        "benign keyframes preserve nothing"
    );
}

#[test]
fn preserve_failure_is_503_not_cleared() {
    let png = varied_png(64, 64);
    let snapshot = loaded(vec![pdq_of(&png)]); // would block
    let store = VerdictStore::new();
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    q.lock().unwrap().fail_next_preserve();
    let r = req("x", "x", vec![b64(&png)]);
    let err = moderate_frames_inner(9, &store, &q, (snapshot, OwnHashList::new(), 31), &r, at())
        .unwrap_err();
    assert_eq!(err.0, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        store.get(9).is_none(),
        "a preserve failure writes NO releasing verdict — HOLD, never cleared (R2-F2)"
    );
}

#[test]
fn cleared_does_not_overwrite_blocked() {
    let png = varied_png(64, 64);
    let snapshot = loaded(vec![]); // benign ⇒ would compute cleared
    let store = VerdictStore::new();
    store.set(3, ModerationResult::blocked("earlier csam")); // a prior block exists
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let r = req("x", "x", vec![b64(&png)]);
    let resp =
        moderate_frames_inner(3, &store, &q, (snapshot, OwnHashList::new(), 31), &r, at()).unwrap();
    assert_eq!(
        resp.verdict, "cleared",
        "the endpoint computed cleared for these frames"
    );
    assert_eq!(
        store.get(3).unwrap().verdict,
        Verdict::Blocked,
        "but a stored Blocked is NOT downgraded by a later benign POST (C4)"
    );
}

#[test]
fn bad_base64_is_400() {
    let store = VerdictStore::new();
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let snapshot = loaded(vec![]);
    let r = req("x", "x", vec!["!!! not base64 !!!".into()]);
    let err = moderate_frames_inner(1, &store, &q, (snapshot, OwnHashList::new(), 31), &r, at())
        .unwrap_err();
    assert_eq!(err.0, StatusCode::BAD_REQUEST);
}

// --- handler/router: auth (401) + unknown task (404) ---

fn body(token: &str, task: &str, pngs: Vec<String>) -> String {
    serde_json::to_string(&req(token, task, pngs)).unwrap()
}

fn post_frames(body: String) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/moderate/frames")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn missing_or_wrong_token_is_401_no_write() {
    let mut s = ApiServer::new_for_test();
    s.set_ingest_token(Some("secret".into()));
    s.record_task_job("task-x".into(), 5);
    let server = Arc::new(s);
    let app = ApiServer::create_router(server.clone());
    let resp = app
        .oneshot(post_frames(body("wrong-token", "task-x", vec![])))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert!(
        server.moderation_store().get(5).is_none(),
        "a 401 must write NO verdict (R2-F3)"
    );
}

#[tokio::test]
async fn server_token_unset_rejects_all() {
    // R3-C1: server MODERATION_INGEST_TOKEN unset ⇒ reject every request (never accept-all).
    let server = Arc::new(ApiServer::new_for_test()); // token None
    let app = ApiServer::create_router(server);
    let resp = app
        .oneshot(post_frames(body("", "whatever", vec![])))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn unknown_task_is_404() {
    let mut s = ApiServer::new_for_test();
    s.set_ingest_token(Some("secret".into()));
    let server = Arc::new(s);
    let app = ApiServer::create_router(server);
    let resp = app
        .oneshot(post_frames(body("secret", "nonexistent-task", vec![])))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn route_frames_on_unavailable_list_returns_503() {
    // DEFAULT-STATE path (R11): `new_for_test` hardcodes the fail-closed default
    // (env-blind — WP-N2), so build_frames_match_state() serves an Unavailable
    // snapshot and a fully-authed, valid keyframe POST HOLDs as 503 — proving the
    // whole chain (handler → build_frames_match_state → inner) is wired
    // fail-closed (parity with the asset route_image_on_unavailable_list test).
    // Production may now be Loaded via an explicit operator list (WP-N2); a
    // fail-open regression in the DEFAULT state would flip this to 200 and fail.
    let mut s = ApiServer::new_for_test();
    s.set_ingest_token(Some("secret".into()));
    s.record_task_job("task-z".into(), 12);
    let server = Arc::new(s);
    let app = ApiServer::create_router(server);
    let png = varied_png(64, 64);
    let resp = app
        .oneshot(post_frames(body("secret", "task-z", vec![b64(&png)])))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

// --- R9-5: sourceSha256 (source-file exact / own-hash re-upload halt) is wired ---

#[test]
fn source_sha256_ownhash_match_blocks_and_preserves_even_when_unavailable() {
    // R9-5 + R9-B: a `sourceSha256` that is a confirmed own-hash blocks AND preserves the
    // keyframes even when the NCMEC list is unavailable (the source-file re-upload halt is
    // definitive, list-independent).
    let png = varied_png(64, 64);
    let bad_source = b"the known-bad source file bytes";
    let sha = fabstir_llm_node::moderation::csam::matcher::Matcher::sha256(bad_source);
    let mut own = OwnHashList::new();
    own.add(sha);
    let store = VerdictStore::new();
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let r = ModerateFramesRequest {
        ingest_token: "x".into(),
        task_id: "x".into(),
        keyframes_png_base64: vec![b64(&png)],
        source_sha256: Some(hex::encode(sha)),
    };
    let resp = moderate_frames_inner(
        5,
        &store,
        &q,
        (HashListSnapshot::unavailable(), own, 31),
        &r,
        at(),
    )
    .unwrap();
    assert_eq!(
        resp.verdict, "blocked",
        "an own-hash sourceSha256 blocks even with NCMEC unavailable"
    );
    assert_eq!(store.get(5).unwrap().verdict, Verdict::Blocked);
    assert_eq!(
        q.lock().unwrap().len(),
        1,
        "the keyframes are preserved as evidence of the matched source"
    );
}

#[test]
fn bad_source_sha256_is_400() {
    // A malformed sourceSha256 is rejected (400) — fail-closed input validation; no write.
    let png = varied_png(64, 64);
    let store = VerdictStore::new();
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let r = ModerateFramesRequest {
        ingest_token: "x".into(),
        task_id: "x".into(),
        keyframes_png_base64: vec![b64(&png)],
        source_sha256: Some("not-valid-hex".into()),
    };
    let err = moderate_frames_inner(
        6,
        &store,
        &q,
        (loaded(vec![]), OwnHashList::new(), 31),
        &r,
        at(),
    )
    .unwrap_err();
    assert_eq!(err.0, StatusCode::BAD_REQUEST);
    assert!(store.get(6).is_none());
}
