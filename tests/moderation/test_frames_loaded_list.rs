// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! WP-N2 Phase 2: end-to-end through the real seam with an ENV-LOADED operator
//! list (`IMPLEMENTATION-MODERATION-LISTS.md` §4 Phase 2). The existing
//! `benign_clears` already clears through the seam from a hand-built snapshot —
//! the new fact under test here is the LOADER (`from_env`) and the WIRING
//! (`ApiServer::new` → stored state → both builders). Helpers are deliberate
//! module-private copies (plan §2.1: no cross-imports between test files).

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use chrono::{DateTime, Utc};
use tower::util::ServiceExt;

use fabstir_llm_node::api::moderation::{
    moderate_asset_inner, moderate_frames_inner, ModerateAssetRequest, ModerateFramesRequest,
    MAX_ASSET_BYTES,
};
use fabstir_llm_node::api::server::{ApiConfig, ApiServer};
use fabstir_llm_node::moderation::csam::hashlist::ListState;
use fabstir_llm_node::moderation::csam::listfile::FramesMatchState;
use fabstir_llm_node::moderation::csam::matcher::Matcher;
use fabstir_llm_node::moderation::csam::pdq::compute_pdq_rgb;
use fabstir_llm_node::moderation::csam::quarantine::Quarantine;
use fabstir_llm_node::moderation::types::{Pdq256, Verdict};
use fabstir_llm_node::moderation::verdict_store::VerdictStore;

const LIST_VAR: &str = "MODERATION_LIST_FILE";
const OWN_VAR: &str = "MODERATION_OWNHASH_FILE";
const PDQ_VAR: &str = "MODERATION_PDQ_MAX_DISTANCE";

/// Restore-on-drop guard (copy of the test_listfile idiom): a failing assert
/// must not leak env into later tests in this serial binary.
struct EnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn new(vars: &[&'static str]) -> Self {
        let saved = vars.iter().map(|v| (*v, std::env::var(v).ok())).collect();
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (var, val) in &self.saved {
            match val {
                Some(v) => std::env::set_var(var, v),
                None => std::env::remove_var(var),
            }
        }
    }
}

/// Guard all three vars and start from a clean slate.
fn guarded_clean_env() -> EnvGuard {
    let guard = EnvGuard::new(&[LIST_VAR, OWN_VAR, PDQ_VAR]);
    std::env::remove_var(LIST_VAR);
    std::env::remove_var(OWN_VAR);
    std::env::remove_var(PDQ_VAR);
    guard
}

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

/// PDQ of a PNG via the SAME decode path the node uses (frames recompute PDQ
/// from pixels, so list entries must be derived from the in-node hash).
fn pdq_of(png: &[u8]) -> Pdq256 {
    let img = image::load_from_memory(png).unwrap();
    let rgb = img.to_rgb8();
    let (w, h) = (rgb.width(), rgb.height());
    compute_pdq_rgb(rgb.as_raw(), w, h).unwrap().hash
}

/// A PDQ hash at exactly Hamming distance `n` from `base` (copy of
/// test_matcher_pdq's module-private helper).
fn flip_bits(base: &Pdq256, n: usize) -> Pdq256 {
    let mut b = base.0;
    for i in 0..n {
        b[i / 8] ^= 1 << (i % 8);
    }
    Pdq256(b)
}

fn write_list(dir: &tempfile::TempDir, name: &str, lines: &str) -> std::path::PathBuf {
    let p = dir.path().join(name);
    std::fs::write(&p, lines).unwrap();
    p
}

fn frames_req(pngs: Vec<String>, source_sha256: Option<String>) -> ModerateFramesRequest {
    ModerateFramesRequest {
        ingest_token: "x".into(),
        task_id: "x".into(),
        keyframes_png_base64: pngs,
        source_sha256,
    }
}

fn state_tuple(
    st: &FramesMatchState,
) -> (
    fabstir_llm_node::moderation::csam::hashlist::HashListSnapshot,
    fabstir_llm_node::moderation::csam::ownhash::OwnHashList,
    u32,
) {
    (st.snapshot.clone(), st.ownhash.clone(), st.max_distance)
}

// --- 2.1.1: listed sourceSha256 blocks through a from_env-built state ---

#[test]
fn listed_source_sha_blocks_and_preserves_via_env_loaded_list() {
    let _guard = guarded_clean_env();
    let dir = tempfile::tempdir().unwrap();
    let listed = [0x5a; 32];
    let list = write_list(
        &dir,
        "list.txt",
        &format!("sha256:{}\n", hex::encode(listed)),
    );
    std::env::set_var(LIST_VAR, &list);
    let st = FramesMatchState::from_env().expect("must load");

    let store = VerdictStore::new();
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    // ≥1 benign keyframe: an empty keyframes array trips the 400 guard before
    // matching (frames.rs:88).
    let png = varied_png(64, 64);
    let req = frames_req(vec![b64(&png)], Some(hex::encode(listed)));
    let resp = moderate_frames_inner(11, &store, &q, state_tuple(&st), &req, at()).unwrap();
    assert_eq!(resp.verdict, "blocked");
    assert_eq!(
        resp.reason.as_deref(),
        Some("hash-list-match"),
        "the match sentinel VALUE is the wire contract (post-0.4)"
    );
    let rec = store.get(11).expect("verdict recorded");
    assert_eq!(rec.verdict, Verdict::Blocked);
    assert!(rec.is_genuine_hit(), "a list hit is genuine — preserves");
    assert_eq!(
        q.lock().unwrap().len(),
        1,
        "keyframes preserved in quarantine on a genuine hit"
    );
}

// --- 2.1.2: unlisted content CLEARS — the first cleared verdict from a
// from_env-built, production-shaped state ---

#[test]
fn unlisted_content_clears_via_env_loaded_list() {
    let _guard = guarded_clean_env();
    let dir = tempfile::tempdir().unwrap();
    let list = write_list(
        &dir,
        "list.txt",
        &format!("sha256:{}\n", hex::encode([0x5a; 32])),
    );
    std::env::set_var(LIST_VAR, &list);
    let st = FramesMatchState::from_env().expect("must load");

    let store = VerdictStore::new();
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let png = varied_png(64, 64);
    let req = frames_req(vec![b64(&png)], Some(hex::encode([0x77; 32])));
    let resp = moderate_frames_inner(12, &store, &q, state_tuple(&st), &req, at()).unwrap();
    assert_eq!(
        resp.verdict, "cleared",
        "a Loaded operator list must let unlisted content CLEAR"
    );
    assert_eq!(store.get(12).map(|r| r.verdict), Some(Verdict::Cleared));
    assert_eq!(q.lock().unwrap().len(), 0, "cleared preserves nothing");
}

// --- 2.1.3: PDQ boundary exactness through the loaded list ---

#[test]
fn pdq_boundary_31_blocks_32_clears_via_env_loaded_list() {
    let _guard = guarded_clean_env();
    let dir = tempfile::tempdir().unwrap();
    let png = varied_png(64, 64);
    let base = pdq_of(&png);

    // List an entry at distance exactly 31 from the keyframe's in-node PDQ.
    let list31 = write_list(
        &dir,
        "d31.txt",
        &format!("pdq:{}\n", hex::encode(flip_bits(&base, 31).0)),
    );
    std::env::set_var(LIST_VAR, &list31);
    let st = FramesMatchState::from_env().expect("must load");
    let store = VerdictStore::new();
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let resp = moderate_frames_inner(
        21,
        &store,
        &q,
        state_tuple(&st),
        &frames_req(vec![b64(&png)], None),
        at(),
    )
    .unwrap();
    assert_eq!(resp.verdict, "blocked", "distance 31 ≤ max_distance 31");

    // Distance 32 must NOT match: boundary is exact.
    let list32 = write_list(
        &dir,
        "d32.txt",
        &format!("pdq:{}\n", hex::encode(flip_bits(&base, 32).0)),
    );
    std::env::set_var(LIST_VAR, &list32);
    let st = FramesMatchState::from_env().expect("must load");
    let resp = moderate_frames_inner(
        22,
        &store,
        &q,
        state_tuple(&st),
        &frames_req(vec![b64(&png)], None),
        at(),
    )
    .unwrap();
    assert_eq!(resp.verdict, "cleared", "distance 32 > 31 must clear");
}

// --- 2.1.4: own-hash-only — blocked for listed, HELD (not cleared, not
// poisoned) for everything else. Pinned so nobody "fixes" the hold later ---

#[test]
fn ownhash_only_blocks_listed_and_holds_unlisted_without_poisoning() {
    let _guard = guarded_clean_env();
    let dir = tempfile::tempdir().unwrap();
    let listed = [0x5a; 32];
    let own = write_list(&dir, "own.txt", &format!("{}\n", hex::encode(listed)));
    std::env::set_var(OWN_VAR, &own);
    let st = FramesMatchState::from_env().expect("must load");

    let store = VerdictStore::new();
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let png = varied_png(64, 64);

    // Listed source: own-hash blocks regardless of snapshot state.
    let resp = moderate_frames_inner(
        31,
        &store,
        &q,
        state_tuple(&st),
        &frames_req(vec![b64(&png)], Some(hex::encode(listed))),
        at(),
    )
    .unwrap();
    assert_eq!(resp.verdict, "blocked");

    // Unlisted source: falls through to require_available() on Unavailable ⇒
    // 503 HOLD — with NO VerdictStore write and NO quarantine growth (the
    // retry-safety half of the §1 discovery).
    let before_q = q.lock().unwrap().len();
    let err = moderate_frames_inner(
        32,
        &store,
        &q,
        state_tuple(&st),
        &frames_req(vec![b64(&png)], Some(hex::encode([0x77; 32]))),
        at(),
    )
    .expect_err("own-hash alone can never clear");
    assert_eq!(err.0, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        store.get(32).is_none(),
        "a hold must not write a verdict (retry once a list lands must not be poisoned)"
    );
    assert_eq!(
        q.lock().unwrap().len(),
        before_q,
        "a hold must preserve nothing"
    );
}

// --- 2.1.5: no env vars ⇒ endpoint behaviour identical to today (the no-op proof) ---

#[test]
fn no_env_vars_is_exactly_todays_hold() {
    let _guard = guarded_clean_env();
    let st = FramesMatchState::from_env().expect("no env vars is never an error");
    let store = VerdictStore::new();
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let png = varied_png(64, 64);
    let err = moderate_frames_inner(
        41,
        &store,
        &q,
        state_tuple(&st),
        &frames_req(vec![b64(&png)], None),
        at(),
    )
    .expect_err("without a list everything HOLDs — today's behaviour");
    assert_eq!(err.0, StatusCode::SERVICE_UNAVAILABLE);
    assert!(store.get(41).is_none());
}

// --- 2.1.7: THE WIRING PIN — fails if server.rs's builder stays hardcoded ---

#[tokio::test]
async fn env_loaded_list_reaches_build_frames_match_state() {
    let _guard = guarded_clean_env();
    let dir = tempfile::tempdir().unwrap();
    let listed = [0x5a; 32];
    let list = write_list(
        &dir,
        "list.txt",
        &format!("sha256:{}\n", hex::encode(listed)),
    );
    std::env::set_var(LIST_VAR, &list);

    // The REAL constructor (route recorded in the build log: primary route,
    // no fallback setter needed — new() has no contract-env prerequisites).
    let config = ApiConfig {
        listen_addr: "127.0.0.1:0".to_string(),
        ..Default::default()
    };
    let server = ApiServer::new(config).await.expect("constructor");
    let (snapshot, _own, max_distance) = server.build_frames_match_state();
    assert_eq!(
        snapshot.state,
        ListState::Loaded,
        "the env-loaded list must reach the production builder — if this is \
         Unavailable, ApiServer::new never wired FramesMatchState"
    );
    assert!(snapshot.sha256.contains(&listed));
    assert_eq!(max_distance, 31);
}

// --- 2.1.6: asset-path parity on the env-built server (driven via the REAL
// build_asset_moderator — a hand-built AssetModerator would pass with the
// wiring missing) ---

#[tokio::test]
async fn asset_path_sees_the_same_env_loaded_list() {
    let _guard = guarded_clean_env();
    let dir = tempfile::tempdir().unwrap();
    let listed_png = varied_png(48, 48);
    let listed_sha = Matcher::sha256(&listed_png);
    let list = write_list(
        &dir,
        "list.txt",
        &format!("sha256:{}\n", hex::encode(listed_sha)),
    );
    std::env::set_var(LIST_VAR, &list);

    let config = ApiConfig {
        listen_addr: "127.0.0.1:0".to_string(),
        ..Default::default()
    };
    let server = ApiServer::new(config).await.expect("constructor");
    let am = server.build_asset_moderator();

    let blocked = moderate_asset_inner(
        &am,
        &ModerateAssetRequest {
            kind: "image".into(),
            data: b64(&listed_png),
        },
        MAX_ASSET_BYTES,
    )
    .expect("listed image is a 200 verdict");
    assert_eq!(
        blocked.verdict, "blocked",
        "asset path sees the loaded list"
    );
    // Independent discriminator: the non-preserving asset inner surfaces
    // fail-closed holds as "blocked" too, so pin the genuine-match reason —
    // an unwired (Unavailable) builder would say "moderation unavailable".
    assert_eq!(blocked.reason.as_deref(), Some("hash-list-match"));

    let other_png = varied_png(52, 52);
    let cleared = moderate_asset_inner(
        &am,
        &ModerateAssetRequest {
            kind: "image".into(),
            data: b64(&other_png),
        },
        MAX_ASSET_BYTES,
    )
    .expect("unlisted image is a 200 verdict");
    assert_eq!(
        cleared.verdict, "cleared",
        "C5 parity: both paths, one state"
    );
}

// --- 2.1.8: observability — degradation is visible, holds are counted ---

#[tokio::test]
async fn degraded_boot_surfaces_in_health_and_holds_move_the_counter() {
    let _guard = guarded_clean_env();
    let dir = tempfile::tempdir().unwrap();
    let bad = write_list(&dir, "broken.txt", "sha256:not-a-hash\n");
    std::env::set_var(LIST_VAR, &bad);

    let config = ApiConfig {
        listen_addr: "127.0.0.1:0".to_string(),
        ..Default::default()
    };
    let mut server = ApiServer::new(config)
        .await
        .expect("a broken list file must DEGRADE, never kill the node");

    // Rule 1a-i: assert the degradation STRING, not the status literal — a
    // test-built server also carries a "No P2P node" issue, so the overall
    // status reads "unhealthy", not "degraded".
    let health = server.health_check().await;
    let issues = health.issues.expect("issues present");
    assert!(
        issues
            .iter()
            .any(|i| i.contains("moderation list degraded")),
        "health must carry the degradation string, got: {issues:?}"
    );

    // Rule 1a-ii: a frames 503 HOLD moves moderation_holds_total.
    server.set_ingest_token(Some("secret".into()));
    server.record_task_job("task-h".into(), 51);
    let server = Arc::new(server);
    let before = server.moderation_metrics().snapshot().held;
    let app = ApiServer::create_router(server.clone());
    let png = varied_png(64, 64);
    let body = serde_json::to_string(&ModerateFramesRequest {
        ingest_token: "secret".into(),
        task_id: "task-h".into(),
        keyframes_png_base64: vec![b64(&png)],
        source_sha256: None,
    })
    .unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/moderate/frames")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        server.moderation_metrics().snapshot().held,
        before + 1,
        "the 503 hold must increment moderation_holds_total"
    );
}

// --- Acceptance §7.3's node half, endpoint-level: the DEPLOYMENT.md worked
// example through the REAL router on an env-built server — a listed source
// blocks, an unlisted one clears, no hand-built state anywhere ---

#[tokio::test]
async fn worked_example_listed_blocks_unlisted_clears_through_the_real_endpoint() {
    let _guard = guarded_clean_env();
    let dir = tempfile::tempdir().unwrap();
    let listed = [0x5a; 32];
    let list = write_list(
        &dir,
        "list.txt",
        &format!("sha256:{}\n", hex::encode(listed)),
    );
    std::env::set_var(LIST_VAR, &list);

    let config = ApiConfig {
        listen_addr: "127.0.0.1:0".to_string(),
        ..Default::default()
    };
    let mut server = ApiServer::new(config).await.expect("constructor");
    server.set_ingest_token(Some("secret".into()));
    server.record_task_job("task-b".into(), 61);
    server.record_task_job("task-c".into(), 62);
    let server = Arc::new(server);
    let png = varied_png(64, 64);

    let post = |task: &str, source: [u8; 32]| {
        let body = serde_json::to_string(&ModerateFramesRequest {
            ingest_token: "secret".into(),
            task_id: task.into(),
            keyframes_png_base64: vec![b64(&png)],
            source_sha256: Some(hex::encode(source)),
        })
        .unwrap();
        Request::builder()
            .method("POST")
            .uri("/v1/moderate/frames")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap()
    };

    let resp = ApiServer::create_router(server.clone())
        .oneshot(post("task-b", listed))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(v["verdict"], "blocked", "the listed test video blocks");
    assert_eq!(v["reason"], "hash-list-match");

    let resp = ApiServer::create_router(server.clone())
        .oneshot(post("task-c", [0x77; 32]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(v["verdict"], "cleared", "an unlisted one clears");
}

// --- PART-A §3.2 batching amendment (2026-07-31): multiple frames POSTs for
// one job compose safely — blocked is STICKY in the store, so batch order can
// never wash out a block. This is the node-side property the transcoder's
// chunked-POST contract relies on; pinned so nobody "fixes" it later ---

#[test]
fn batched_posts_compose_blocked_is_sticky() {
    let _guard = guarded_clean_env();
    let dir = tempfile::tempdir().unwrap();
    let listed = [0x5a; 32];
    let list = write_list(
        &dir,
        "list.txt",
        &format!("sha256:{}\n", hex::encode(listed)),
    );
    std::env::set_var(LIST_VAR, &list);
    let st = FramesMatchState::from_env().expect("must load");

    let store = VerdictStore::new();
    let q = Mutex::new(Quarantine::new(b"k".to_vec(), 90));
    let png = varied_png(64, 64);
    let job = 71u64;

    // Batch A: benign, unlisted source ⇒ 200 cleared (batch-local).
    let a = moderate_frames_inner(
        job,
        &store,
        &q,
        state_tuple(&st),
        &frames_req(vec![b64(&png)], Some(hex::encode([0x77; 32]))),
        at(),
    )
    .unwrap();
    assert_eq!(a.verdict, "cleared");

    // Batch B: LISTED source ⇒ 200 blocked; the store worsens.
    let b = moderate_frames_inner(
        job,
        &store,
        &q,
        state_tuple(&st),
        &frames_req(vec![b64(&png)], Some(hex::encode(listed))),
        at(),
    )
    .unwrap();
    assert_eq!(b.verdict, "blocked");

    // Batch C: benign again ⇒ the RESPONSE is batch-local cleared (why the
    // transcoder's client-side any-non-cleared-holds aggregation is mandatory,
    // PART-A §3.2 rule 3)…
    let c = moderate_frames_inner(
        job,
        &store,
        &q,
        state_tuple(&st),
        &frames_req(vec![b64(&png)], Some(hex::encode([0x88; 32]))),
        at(),
    )
    .unwrap();
    assert_eq!(c.verdict, "cleared", "per-batch responses are batch-local");

    // …but the STORE — which the node's completion gate reads — stays blocked:
    // set_if_not_downgrade rejects the cleared-over-blocked write.
    assert_eq!(
        store.get(job).map(|r| r.verdict),
        Some(Verdict::Blocked),
        "blocked must be sticky across later cleared batches"
    );
}
