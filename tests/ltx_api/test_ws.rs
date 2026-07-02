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
    job["frames"] = json!(99_999); // > the allow-list bounds max (257)
    let (resp, task) =
        handle_encrypted_ltx_generate(&server, &job, &k, "sess-c", Some(1), None).await;
    assert!(task.is_none());
    let inner = decrypt_envelope(&resp, &k);
    assert_eq!(inner["error"]["code"], "VALIDATION_FAILED");
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
