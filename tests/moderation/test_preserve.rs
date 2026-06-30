// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 1.1 — `preserve_if_blocked` helper (the B6 detect→preserve wiring). 🚨
//!
//! The committed node detects + blocks but never calls `quarantine.preserve()` in
//! the live path — so a real match returns `blocked` with NO evidence stored. This
//! helper is the bridge. It is fail-closed (a preserve failure ⇒ `Err`, the caller
//! HOLDs, never clears), idempotent by content (retry-safe in the no-delete store),
//! and records per-job provenance even when dedup collapses identical content.

use std::sync::Mutex;

use base64::Engine;
use chrono::{DateTime, Utc};

use fabstir_llm_node::api::moderation::{moderate_asset_inner_preserving, ModerateAssetRequest};
use fabstir_llm_node::moderation::asset::{AssetModerator, TextScanList};
use fabstir_llm_node::moderation::csam::hashlist::{HashListSnapshot, HashListSource};
use fabstir_llm_node::moderation::csam::matcher::Matcher;
use fabstir_llm_node::moderation::csam::mock_source::MockHashListSource;
use fabstir_llm_node::moderation::csam::ownhash::OwnHashList;
use fabstir_llm_node::moderation::csam::quarantine::{
    evidence_category, preserve_if_blocked, Quarantine, Role,
};
use fabstir_llm_node::moderation::types::{AssetKind, Category, Verdict};

fn at() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).unwrap()
}

fn fresh() -> Quarantine {
    Quarantine::new(b"preserve-test-key".to_vec(), 90)
}

#[test]
fn cleared_preserves_nothing() {
    let mut q = fresh();
    let benign: &[u8] = b"benign";
    let cases = preserve_if_blocked(
        &mut q,
        Verdict::Cleared,
        &[benign],
        Category::Csam,
        Some(1),
        at(),
    )
    .unwrap();
    assert!(cases.is_empty(), "Cleared preserves nothing");
    assert!(q.is_empty(), "quarantine stays empty on Cleared");
}

#[test]
fn blocked_preserves_encrypted() {
    let mut q = fresh();
    let content: &[u8] = b"matched evidence bytes";
    let cases = preserve_if_blocked(
        &mut q,
        Verdict::Blocked,
        &[content],
        Category::Csam,
        Some(7),
        at(),
    )
    .unwrap();
    assert_eq!(cases.len(), 1);
    assert!(q.contains(&cases[0]));
    // Stored as ciphertext, never plaintext (nonce+tag overhead ⇒ strictly larger).
    assert!(q.sealed_len(&cases[0]).unwrap() > content.len());
    assert_eq!(q.category(&cases[0]), Some(Category::Csam));
}

#[test]
fn blocked_preserves_all_blobs() {
    let mut q = fresh();
    let a: &[u8] = b"frame-a";
    let b: &[u8] = b"frame-b";
    let cases = preserve_if_blocked(
        &mut q,
        Verdict::Blocked,
        &[a, b],
        Category::Csam,
        Some(1),
        at(),
    )
    .unwrap();
    assert_eq!(
        cases.len(),
        2,
        "every keyframe is preserved (no under-preserve)"
    );
    assert_ne!(cases[0], cases[1]);
    assert!(q.contains(&cases[0]) && q.contains(&cases[1]));
    assert_eq!(q.len(), 2);
}

#[test]
fn flagged_preserves_as_illegal_speech() {
    let mut q = fresh();
    let sub: &[u8] = b"illegal subtitle text";
    let cases = preserve_if_blocked(
        &mut q,
        Verdict::Flagged,
        &[sub],
        Category::IllegalSpeech,
        None,
        at(),
    )
    .unwrap();
    assert_eq!(cases.len(), 1);
    assert_eq!(
        q.category(&cases[0]),
        Some(Category::IllegalSpeech),
        "subtitle evidence is labelled IllegalSpeech, not Csam"
    );
}

#[test]
fn preserve_failure_returns_err_not_ok() {
    // Fail-closed (R2-F2): a preserve failure on a block must surface as Err so the
    // caller HOLDs — a `blocked` verdict can NEVER be silently un-preserved.
    let mut q = fresh();
    q.fail_next_preserve();
    let x: &[u8] = b"x";
    let res = preserve_if_blocked(
        &mut q,
        Verdict::Blocked,
        &[x],
        Category::Csam,
        Some(1),
        at(),
    );
    assert!(res.is_err(), "a preserve failure must NOT return Ok");
    assert!(q.is_empty(), "nothing is recorded when the seal fails");
}

#[test]
fn evidence_category_maps_kind() {
    assert_eq!(evidence_category(AssetKind::Image), Category::Csam);
    assert_eq!(evidence_category(AssetKind::VideoKeyframe), Category::Csam);
    assert_eq!(
        evidence_category(AssetKind::Subtitle),
        Category::IllegalSpeech
    );
}

#[test]
fn retry_does_not_duplicate() {
    // R3-D1 idempotency: re-preserving identical content (a transcoder retry after a
    // partial-preserve 503) reuses the existing case id — no unbounded growth in the
    // no-delete store.
    let mut q = fresh();
    let same: &[u8] = b"same evidence";
    let c1 = preserve_if_blocked(
        &mut q,
        Verdict::Blocked,
        &[same],
        Category::Csam,
        Some(1),
        at(),
    )
    .unwrap();
    let c2 = preserve_if_blocked(
        &mut q,
        Verdict::Blocked,
        &[same],
        Category::Csam,
        Some(1),
        at(),
    )
    .unwrap();
    assert_eq!(c1, c2, "identical content ⇒ identical case id");
    assert_eq!(q.len(), 1, "no duplicate evidence on retry");
}

#[test]
fn dedup_records_each_job() {
    // R4-C1 provenance: the same blocked frame in two jobs collapses to ONE case id,
    // but the audit trail must still name BOTH jobs (NCMEC/review attribution).
    let mut q = fresh();
    let shared: &[u8] = b"shared frame";
    let c1 = preserve_if_blocked(
        &mut q,
        Verdict::Blocked,
        &[shared],
        Category::Csam,
        Some(11),
        at(),
    )
    .unwrap();
    let c2 = preserve_if_blocked(
        &mut q,
        Verdict::Blocked,
        &[shared],
        Category::Csam,
        Some(22),
        at(),
    )
    .unwrap();
    assert_eq!(c1, c2);
    assert_eq!(q.len(), 1, "dedup ⇒ one stored case");
    let audit = q.audit_log();
    assert!(
        audit.iter().any(|e| e.action.contains("job=11")),
        "job 11 must be attributed in the audit log"
    );
    assert!(
        audit.iter().any(|e| e.action.contains("job=22")),
        "job 22 must be attributed even on the dedup no-op"
    );
}

#[test]
fn preserve_then_reviewable() {
    let mut q = fresh();
    let content: &[u8] = b"retrievable evidence";
    let cases = preserve_if_blocked(
        &mut q,
        Verdict::Blocked,
        &[content],
        Category::Csam,
        Some(1),
        at(),
    )
    .unwrap();
    let got = q
        .retrieve(&cases[0], Role::Reviewer, "alice", at())
        .unwrap();
    assert_eq!(
        got, content,
        "an authorised role retrieves the original bytes"
    );
    assert!(q.audit_log().iter().any(|e| e.action.contains("retrieve")));
}

// --- Sub-phase 1.2: live asset-path wiring (B6) — not just the isolated helper ---

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn png(color: [u8; 3]) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(8, 8, image::Rgb(color));
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut buf, image::ImageFormat::Png)
        .unwrap();
    buf.into_inner()
}

fn am_with(snapshot: HashListSnapshot) -> AssetModerator {
    AssetModerator::new(
        snapshot,
        OwnHashList::new(),
        31,
        TextScanList::launch_mock(),
    )
}

#[test]
fn asset_csam_match_is_preserved_as_csam() {
    // The committed handler blocked but never preserved (B6). The preserving variant
    // must store the ORIGINAL file bytes with the kind-derived Csam category.
    let bytes = png([1, 2, 3]);
    let sha = Matcher::sha256(&bytes);
    let am = am_with(
        MockHashListSource::loaded(vec![sha], vec![])
            .refresh()
            .unwrap(),
    );
    let req = ModerateAssetRequest {
        kind: "image".into(),
        data: b64(&bytes),
    };
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let resp = moderate_asset_inner_preserving(&am, &req, 20 * 1024 * 1024, &q, at()).unwrap();
    assert_eq!(resp.verdict, "blocked");
    let guard = q.lock().unwrap();
    assert_eq!(
        guard.len(),
        1,
        "the blocked image is preserved in the live path"
    );
    // Deterministic first case id; category is kind-derived (Image ⇒ Csam), not parsed.
    assert_eq!(guard.category("case-0"), Some(Category::Csam));
}

#[test]
fn asset_subtitle_flag_preserved_as_illegal_speech() {
    // A flagged subtitle preserves as IllegalSpeech — proving the category is derived
    // from AssetKind, not hardcoded to Csam.
    let am = am_with(HashListSnapshot::unavailable());
    let sub = format!(
        "WEBVTT\n\n00:00.000 --> 00:01.000\n{}\n",
        TextScanList::MOCK_BAD_KEYWORDS[0]
    );
    let req = ModerateAssetRequest {
        kind: "subtitle".into(),
        data: b64(sub.as_bytes()),
    };
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let resp = moderate_asset_inner_preserving(&am, &req, 20 * 1024 * 1024, &q, at()).unwrap();
    assert_eq!(resp.verdict, "flagged");
    let guard = q.lock().unwrap();
    assert_eq!(guard.len(), 1);
    assert_eq!(
        guard.category("case-0"),
        Some(Category::IllegalSpeech),
        "subtitle evidence ⇒ IllegalSpeech, not Csam"
    );
}

#[test]
fn asset_image_unavailable_list_holds_503_no_preserve() {
    // Production posture for /asset images: an UNAVAILABLE CSAM list is a retryable infra
    // HOLD ⇒ 503, nothing preserved (closes the over-preserve + the unauthenticated
    // no-delete quarantine-fill against the production Unavailable list).
    let bytes = png([4, 5, 6]);
    let am = am_with(HashListSnapshot::unavailable());
    let req = ModerateAssetRequest {
        kind: "image".into(),
        data: b64(&bytes),
    };
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let err = moderate_asset_inner_preserving(&am, &req, 20 * 1024 * 1024, &q, at()).unwrap_err();
    assert_eq!(err.0, axum::http::StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        q.lock().unwrap().is_empty(),
        "no preserve on an unavailable-list infra-hold"
    );
}

#[test]
fn asset_undecodable_unmatched_image_is_503_not_preserved() {
    // With an AVAILABLE list, an undecodable image that matches NOTHING is a can't-scan
    // HOLD ⇒ retryable 503, and must NOT be preserved (closes an unauthenticated
    // quarantine-fill via garbage /asset bytes once the list is live).
    let am = am_with(
        MockHashListSource::loaded(vec![], vec![])
            .refresh()
            .unwrap(),
    );
    let req = ModerateAssetRequest {
        kind: "image".into(),
        data: b64(b"definitely not a valid image"),
    };
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let err = moderate_asset_inner_preserving(&am, &req, 20 * 1024 * 1024, &q, at()).unwrap_err();
    assert_eq!(
        err.0,
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        "an undecodable, unmatched image is a can't-scan HOLD (retryable 503)"
    );
    assert!(
        q.lock().unwrap().is_empty(),
        "an undecodable (non-matched) image must NOT be preserved"
    );
}

#[test]
fn asset_undecodable_ownhash_match_is_preserved_even_when_ncmec_unavailable() {
    // R9-A + R9-B: an UNDECODABLE blob whose raw-bytes SHA-256 is a confirmed own-hash is
    // a GENUINE exact match (the SHA is taken over raw bytes BEFORE decode, and an
    // own-hash hit is definitive regardless of NCMEC availability). Its evidence MUST be
    // preserved — never dropped, never a `blocked` verdict with no evidence (B6).
    let bytes: &[u8] = b"undecodable but known-bad source bytes";
    let mut own = OwnHashList::new();
    own.add(Matcher::sha256(bytes));
    let am = AssetModerator::new(
        HashListSnapshot::unavailable(), // NCMEC down — the own-hash must still block
        own,
        31,
        TextScanList::launch_mock(),
    );
    let req = ModerateAssetRequest {
        kind: "image".into(),
        data: b64(bytes),
    };
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let resp = moderate_asset_inner_preserving(&am, &req, 20 * 1024 * 1024, &q, at()).unwrap();
    assert_eq!(
        resp.verdict, "blocked",
        "an own-hash exact match blocks even with NCMEC unavailable"
    );
    let mut guard = q.lock().unwrap();
    assert_eq!(
        guard.len(),
        1,
        "a genuine match's evidence is preserved even when the blob is undecodable (B6)"
    );
    assert_eq!(guard.category("case-0"), Some(Category::Csam));
    let got = guard
        .retrieve("case-0", Role::Reviewer, "tester", at())
        .unwrap();
    assert_eq!(
        got, bytes,
        "the original raw bytes are preserved as evidence"
    );
}
