// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 9 handler tests: validation/availability seam + encrypted envelope and
//! inner-message shapes (the full S5/chain pipeline is GPU-acceptance scope).
use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::api::websocket::handlers::ltx::{
    build_ltx_error, handle_encrypted_ltx_generate, ltx_complete_inner, ltx_progress_inner,
};
use fabstir_llm_node::ltx::{ComfyClient, FrameManifest, Resolution, TemplateStore};
use serde_json::{json, Value};
use std::sync::Arc;

fn key() -> [u8; 32] {
    [0x11; 32]
}
fn comfy() -> Arc<ComfyClient> {
    Arc::new(ComfyClient::new("http://127.0.0.1:8188").unwrap())
}
fn store_hash() -> (Arc<TemplateStore>, String) {
    let store = TemplateStore::new(concat!(env!("CARGO_MANIFEST_DIR"), "/templates")).unwrap();
    let hash = store.template_hash("ltx-t2v-hdr").unwrap().to_string();
    (Arc::new(store), hash)
}

/// Decrypt an `encrypted_response` envelope back to its inner JSON.
fn decrypt_envelope(resp: &Value, session_key: &[u8; 32]) -> Value {
    let p = &resp["payload"];
    let ct = hex::decode(p["ciphertextHex"].as_str().unwrap()).unwrap();
    let nb = hex::decode(p["nonceHex"].as_str().unwrap()).unwrap();
    let aad = hex::decode(p["aadHex"].as_str().unwrap()).unwrap();
    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&nb);
    let pt = fabstir_llm_node::crypto::decrypt_with_aead(&ct, &nonce, &aad, session_key).unwrap();
    serde_json::from_slice(&pt).unwrap()
}

fn valid_job(hash: &str) -> Value {
    json!({
        "action": "ltx_generate",
        "requestId": "r1",
        "templateId": "ltx-t2v-hdr",
        "templateHash": hash,
        "prompt": "a cat in a hat",
        "seed": "42",
        "frames": 121,
        "fps": 24,
        "resolution": { "w": 1280, "h": 720 },
        "lora": "ltx-iclora-hdr@v1",
        "output": "exr-sequence"
    })
}

/// A store carrying the M1a (v2) allow-list, plus the i2v templateHash.
fn store_i2v() -> (Arc<TemplateStore>, String) {
    let store = TemplateStore::new(concat!(env!("CARGO_MANIFEST_DIR"), "/templates")).unwrap();
    let hash = store.template_hash("ltx-i2v-hdr").unwrap().to_string();
    (Arc::new(store), hash)
}

/// A real capability CID (from the M0 interop fixture) — a valid single input
/// image, 302144 plaintext bytes (well under `imageMaxBytes`).
fn fixture_capability_cid() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/ltx/capability-fixture.json"
    );
    let v: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    v["capabilityCid"].as_str().unwrap().to_string()
}

/// A well-formed `0xae` envelope whose plaintextCID length field claims
/// `claimed_len` bytes. The pre-fetch gate reads only this length, so an
/// oversize reject needs no real allocation — 34 MB trips `imageMaxBytes`
/// (32 MiB), 200 MB trips `videoMaxBytes` (128 MiB).
fn oversize_capability_cid(claimed_len: u64) -> String {
    let mut env = vec![0xaeu8, 0xa6, 18, 0x1f];
    env.extend_from_slice(&[0u8; 32]); // ct_hash
    env.extend_from_slice(&[0u8; 32]); // key
    env.extend_from_slice(&[0u8; 4]); // padding
    env.push(0x26);
    env.push(0x1f);
    env.extend_from_slice(&[0u8; 32]); // pt_hash
    let mut sle = claimed_len.to_le_bytes().to_vec();
    while sle.len() > 1 && *sle.last().unwrap() == 0 {
        sle.pop();
    }
    env.extend_from_slice(&sle);
    format!(
        "u{}",
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &env)
    )
}

fn i2v_job(hash: &str, images: Value) -> Value {
    json!({
        "action": "ltx_generate",
        "requestId": "r-i2v",
        "templateId": "ltx-i2v-hdr",
        "templateHash": hash,
        "prompt": "egyptian royal walking forward through desert",
        "seed": "60540193790228",
        "frames": 126,
        "fps": 25,
        "resolution": { "w": 1280, "h": 720 },
        "lora": "ltx-iclora-hdr@v1",
        "output": "exr-sequence",
        "images": images
    })
}

#[tokio::test]
async fn test_i2v_accepted_ack() {
    let server = ApiServer::new_for_test();
    let k = key();
    let (store, hash) = store_i2v();
    server.set_ltx_client(comfy()).await;
    server.set_ltx_template_store(store).await;
    let job = i2v_job(&hash, json!([fixture_capability_cid()]));
    let (resp, task) =
        handle_encrypted_ltx_generate(&server, &job, &k, "sess-i2v", Some(9), None).await;
    assert!(task.is_some(), "valid one-image i2v job is accepted");
    let inner = decrypt_envelope(&resp, &k);
    assert_eq!(inner["type"], "ltx_accepted");
    assert_eq!(inner["allowListVersion"], 7, "v7 allow-list echoed");
}

#[tokio::test]
async fn test_i2v_wrong_image_count() {
    let server = ApiServer::new_for_test();
    let k = key();
    let (store, hash) = store_i2v();
    server.set_ltx_client(comfy()).await;
    server.set_ltx_template_store(store).await;
    // i2v expects 1 image; supply 0 -> fail closed BEFORE a slot is taken.
    let job = i2v_job(&hash, json!([]));
    let (resp, task) =
        handle_encrypted_ltx_generate(&server, &job, &k, "sess-i2v-c", Some(9), None).await;
    assert!(task.is_none());
    let inner = decrypt_envelope(&resp, &k);
    assert_eq!(inner["error"]["code"], "VALIDATION_FAILED");
}

#[tokio::test]
async fn test_i2v_oversize_image_rejected() {
    let server = ApiServer::new_for_test();
    let k = key();
    let (store, hash) = store_i2v();
    server.set_ltx_client(comfy()).await;
    server.set_ltx_template_store(store).await;
    let job = i2v_job(&hash, json!([oversize_capability_cid(34_000_000)]));
    let (resp, task) =
        handle_encrypted_ltx_generate(&server, &job, &k, "sess-i2v-o", Some(9), None).await;
    assert!(task.is_none(), "oversize image rejected pre-escrow");
    let inner = decrypt_envelope(&resp, &k);
    assert_eq!(inner["error"]["code"], "VALIDATION_FAILED");
}

#[tokio::test]
async fn test_validation_unknown_template() {
    let server = ApiServer::new_for_test();
    let k = key();
    let (store, hash) = store_hash();
    server.set_ltx_client(comfy()).await;
    server.set_ltx_template_store(store).await;
    let mut job = valid_job(&hash);
    job["templateId"] = json!("nope");
    let (resp, task) =
        handle_encrypted_ltx_generate(&server, &job, &k, "sess-1", Some(1), None).await;
    assert!(task.is_none());
    let inner = decrypt_envelope(&resp, &k);
    assert_eq!(inner["error"]["code"], "VALIDATION_FAILED");
}

#[tokio::test]
async fn test_no_sidecar_503() {
    let server = ApiServer::new_for_test();
    let k = key();
    let (_store, hash) = store_hash();
    let job = valid_job(&hash);
    let (resp, task) =
        handle_encrypted_ltx_generate(&server, &job, &k, "sess-2", Some(1), None).await;
    assert!(task.is_none());
    let inner = decrypt_envelope(&resp, &k);
    assert_eq!(inner["error"]["code"], "SIDECAR_UNAVAILABLE");
}

#[tokio::test]
async fn test_accepted_ack() {
    let server = ApiServer::new_for_test();
    let k = key();
    let (store, hash) = store_hash();
    server.set_ltx_client(comfy()).await;
    server.set_ltx_template_store(store).await;
    let job = valid_job(&hash);
    let (resp, task) =
        handle_encrypted_ltx_generate(&server, &job, &k, "sess-3", Some(7), None).await;
    assert!(task.is_some(), "accepted job returns a background task");
    let inner = decrypt_envelope(&resp, &k);
    assert_eq!(inner["type"], "ltx_accepted");
    assert_eq!(inner["status"], "processing");
    assert!(inner.get("allowListVersion").is_some());
    assert_eq!(inner["requestId"], "r1");
}

#[test]
fn test_progress_has_stage() {
    let v = ltx_progress_inner("generating", 42, Some("r1"));
    assert_eq!(v["type"], "ltx_progress");
    assert_eq!(v["stage"], "generating");
    assert_eq!(v["pct"], 42);
    assert_eq!(v["requestId"], "r1");
}

#[test]
fn test_complete_shape() {
    let manifest = FrameManifest {
        frame_count: 1,
        fps: 24,
        resolution: Resolution { w: 1280, h: 720 },
        colour_encoding: "linear-HDR-from-LogC3".into(),
        frame_hashes: vec!["0x00".into()],
        merkle_root: "0xabc".into(),
    };
    let frames = vec!["uCAP1".to_string(), "uCAP2".to_string()];
    let v = ltx_complete_inner(
        "uOUT",
        "uPROOF",
        &frames,
        &manifest,
        111514,
        "5",
        Some("r1"),
    );
    assert_eq!(v["outputCID"], "uOUT");
    assert_eq!(v["proofCID"], "uPROOF");
    assert!(v["frames"].is_array());
    assert!(v["manifest"].is_object());
    assert!(
        v["manifest"].get("frameCount").is_some(),
        "keyless manifest serialised"
    );
    assert!(
        v["manifest"].get("capabilityCIDs").is_none(),
        "manifest carries no keys/caps"
    );
    assert_eq!(v["billing"]["unit"], "megapixel-frame");
    assert_eq!(v["billing"]["tokens"], 111514);
    assert_eq!(v["requestId"], "r1");
}

#[test]
fn test_error_no_proof() {
    let k = key();
    let resp = build_ltx_error("GENERATION_FAILED", "boom", &k, "sess-x", None, None);
    let inner = decrypt_envelope(&resp, &k);
    assert_eq!(inner["type"], "ltx_error");
    assert_eq!(inner["error"]["code"], "GENERATION_FAILED");
    assert!(
        inner.get("proofCID").is_none(),
        "error path submits no proof"
    );
}

#[tokio::test]
async fn test_out_of_bounds_rejected() {
    let server = ApiServer::new_for_test();
    let k = key();
    let (store, hash) = store_hash();
    server.set_ltx_client(comfy()).await;
    server.set_ltx_template_store(store).await;
    let mut job = valid_job(&hash);
    job["frames"] = json!(99_999); // > the allow-list bounds max (751)
    let (resp, task) =
        handle_encrypted_ltx_generate(&server, &job, &k, "sess-c", Some(1), None).await;
    assert!(task.is_none());
    let inner = decrypt_envelope(&resp, &k);
    assert_eq!(inner["error"]["code"], "VALIDATION_FAILED");
}

/// Drive one `frames`/`fps` combo through the full handler against the live v5
/// bundle. Fresh server each call so the single VRAM permit never leaks across
/// cases. Returns `(accepted, inner_json)`.
async fn submit_frames_fps(frames: u32, fps: u32) -> (bool, Value) {
    let server = ApiServer::new_for_test();
    let k = key();
    let (store, hash) = store_hash();
    server.set_ltx_client(comfy()).await;
    server.set_ltx_template_store(store).await;
    let mut job = valid_job(&hash);
    job["frames"] = json!(frames);
    job["fps"] = json!(fps);
    let (resp, task) =
        handle_encrypted_ltx_generate(&server, &job, &k, "sess-dur", Some(1), None).await;
    let inner = decrypt_envelope(&resp, &k);
    (task.is_some(), inner)
}

#[tokio::test]
async fn test_duration_accepts_valid_matrix() {
    // 5..=15 s clips across the corrected fps set [24,25,48,50], each landing on
    // fps·secs + 1 frames within the v5 bounds {121,751}.
    for (frames, fps) in [
        (121u32, 24u32),
        (361, 24),
        (126, 25),
        (241, 48),
        (251, 50),
        (751, 50),
    ] {
        let (accepted, inner) = submit_frames_fps(frames, fps).await;
        assert!(accepted, "{frames}@{fps} should be accepted: {inner:?}");
    }
}

#[tokio::test]
async fn test_duration_rejects_matrix() {
    // Bounds-level rejects (frame min/max, fps membership) — 151@30 proves the
    // dropped 30 fps is gone from the v5 bundle. Generic bounds message.
    for (frames, fps) in [(97u32, 24u32), (757, 50), (151, 30)] {
        let (accepted, inner) = submit_frames_fps(frames, fps).await;
        assert!(!accepted, "{frames}@{fps} must reject");
        assert_eq!(inner["error"]["code"], "VALIDATION_FAILED");
    }
    // validate_duration divisibility rejects (in range, not a whole second).
    for (frames, fps) in [(200u32, 24u32), (122, 24)] {
        let (accepted, inner) = submit_frames_fps(frames, fps).await;
        assert!(!accepted, "{frames}@{fps} must reject");
        assert_eq!(inner["error"]["code"], "VALIDATION_FAILED");
        let msg = inner["error"]["message"].as_str().unwrap();
        assert!(
            msg.contains("whole"),
            "divisibility msg for {frames}@{fps}: {msg}"
        );
    }
    // validate_duration range rejects (2.4 s < 5 s; 16 s > 15 s).
    for (frames, fps) in [(121u32, 50u32), (385, 24)] {
        let (accepted, inner) = submit_frames_fps(frames, fps).await;
        assert!(!accepted, "{frames}@{fps} must reject");
        assert_eq!(inner["error"]["code"], "VALIDATION_FAILED");
        let msg = inner["error"]["message"].as_str().unwrap();
        assert!(msg.contains("range"), "range msg for {frames}@{fps}: {msg}");
    }
}

#[tokio::test]
async fn test_capacity_when_full() {
    // new_for_test sizes the VRAM semaphore at 1 permit.
    let server = ApiServer::new_for_test();
    let k = key();
    let (store, hash) = store_hash();
    server.set_ltx_client(comfy()).await;
    server.set_ltx_template_store(store).await;
    let job = valid_job(&hash);
    // First job acquires the single permit (held inside the returned task).
    let (_ack1, task1) =
        handle_encrypted_ltx_generate(&server, &job, &k, "sess-a", Some(1), None).await;
    assert!(task1.is_some());
    // Second job (different session, so the per-session rate limiter is not the
    // gate): the permit is gone -> CAPACITY. Keep task1 alive to hold the permit.
    let (resp2, task2) =
        handle_encrypted_ltx_generate(&server, &job, &k, "sess-b", Some(2), None).await;
    assert!(task2.is_none());
    let inner = decrypt_envelope(&resp2, &k);
    assert_eq!(inner["error"]["code"], "CAPACITY");
    drop(task1);
}

// ---------------------------------------------------------------------------
// BL3 iclora: one reference image + one control video across the seam.
// ---------------------------------------------------------------------------

/// A store carrying the v6 allow-list, plus the iclora templateHash.
fn store_iclora() -> (Arc<TemplateStore>, String) {
    let store = TemplateStore::new(concat!(env!("CARGO_MANIFEST_DIR"), "/templates")).unwrap();
    let hash = store.template_hash("ltx-iclora-hdr").unwrap().to_string();
    (Arc::new(store), hash)
}

fn iclora_job(hash: &str, images: Value, videos: Value) -> Value {
    json!({
        "action": "ltx_generate",
        "requestId": "r-iclora",
        "templateId": "ltx-iclora-hdr",
        "templateHash": hash,
        "prompt": "restyle the control clip as a hand-painted cartoon child",
        "seed": "60540193790228",
        "frames": 126,
        "fps": 25,
        "resolution": { "w": 768, "h": 512 },
        "lora": "ltx-iclora-hdr@v1",
        "output": "exr-sequence",
        "images": images,
        "videos": videos
    })
}

#[tokio::test]
async fn test_iclora_accepted_ack() {
    let server = ApiServer::new_for_test();
    let k = key();
    let (store, hash) = store_iclora();
    server.set_ltx_client(comfy()).await;
    server.set_ltx_template_store(store).await;
    // 1 reference image + 1 control video (the capability envelope is
    // media-agnostic; format is enforced on the decrypted bytes post-accept).
    let job = iclora_job(
        &hash,
        json!([fixture_capability_cid()]),
        json!([fixture_capability_cid()]),
    );
    let (resp, task) =
        handle_encrypted_ltx_generate(&server, &job, &k, "sess-iclora", Some(9), None).await;
    assert!(
        task.is_some(),
        "valid 1-image+1-video iclora job is accepted"
    );
    let inner = decrypt_envelope(&resp, &k);
    assert_eq!(inner["type"], "ltx_accepted");
    assert_eq!(inner["allowListVersion"], 7, "v7 allow-list echoed");
}

#[tokio::test]
async fn test_iclora_wrong_video_count() {
    let server = ApiServer::new_for_test();
    let k = key();
    let (store, hash) = store_iclora();
    server.set_ltx_client(comfy()).await;
    server.set_ltx_template_store(store).await;
    // iclora expects 1 video; supply 0 -> fail closed BEFORE a slot is taken.
    let job = iclora_job(&hash, json!([fixture_capability_cid()]), json!([]));
    let (resp, task) =
        handle_encrypted_ltx_generate(&server, &job, &k, "sess-iclora-c", Some(9), None).await;
    assert!(task.is_none());
    let inner = decrypt_envelope(&resp, &k);
    assert_eq!(inner["error"]["code"], "VALIDATION_FAILED");
    assert!(
        inner["error"]["message"]
            .as_str()
            .unwrap()
            .contains("video"),
        "reject names the video count: {}",
        inner["error"]["message"]
    );
}

#[tokio::test]
async fn test_iclora_oversize_video_rejected() {
    let server = ApiServer::new_for_test();
    let k = key();
    let (store, hash) = store_iclora();
    server.set_ltx_client(comfy()).await;
    server.set_ltx_template_store(store).await;
    // 34 MB claims fit under videoMaxBytes (128 MiB) — so claim 200 MB.
    let big = oversize_capability_cid(200_000_000);
    let job = iclora_job(&hash, json!([fixture_capability_cid()]), json!([big]));
    let (resp, task) =
        handle_encrypted_ltx_generate(&server, &job, &k, "sess-iclora-o", Some(9), None).await;
    assert!(task.is_none(), "oversize video rejected pre-escrow");
    let inner = decrypt_envelope(&resp, &k);
    assert_eq!(inner["error"]["code"], "VALIDATION_FAILED");
}
