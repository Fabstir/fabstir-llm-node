// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! 8.62.1: a finished transcode waits for its output list instead of being held,
//! and a source CID without its storage network is refused before the sidecar.
//!
//! Live evidence (host2, 2026-11-04): sessions 1461 and 1462 reached progress 100
//! with a 121 MB output still uploading to S5 (about 25 s at ~5 MB/s). The node
//! gave up after 5 polls of 2 s, parsed "Transcoding in progress" as no outputs,
//! held the finished job unbilled and told the client MODERATION_UNAVAILABLE.
//! Sessions 1459 and 1460 sent a CID without "s5://": the sidecar dropped it
//! ("Invalid source CID") while its status said "in progress" for 30 minutes.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use axum::Router;
use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::api::transcode::handler::transcode_submit_handler;
use fabstir_llm_node::api::websocket::handlers::transcode::{
    completion_wait, handle_encrypted_transcode, output_wait_secs, source_cid_names_network,
    CompletionWait, OUTPUT_WAIT_SECS_DEFAULT,
};
use fabstir_llm_node::transcoder::TranscoderClient;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

const NOT_READY: &str = "Transcoding in progress";
const OUTPUTS: &str = r#"[{"id":1,"ext":"mp4","cid":"s5://urqYSH9H"}]"#;

// ---------------------------------------------------------------- completion wait

#[test]
fn test_outputs_listed_means_ready_at_once() {
    assert_eq!(
        completion_wait(OUTPUTS, Duration::ZERO, Duration::from_secs(120)),
        CompletionWait::Ready
    );
    // A genuine empty list is the sidecar's held signal: ready, and the
    // zero-output guard downstream holds it (unchanged).
    assert_eq!(
        completion_wait("[]", Duration::ZERO, Duration::from_secs(120)),
        CompletionWait::Ready
    );
}

#[test]
fn test_the_1462_case_keeps_waiting_past_ten_seconds() {
    // 8.62.0 gave up here (5 polls x 2 s) with the upload still running.
    let wait = Duration::from_secs(OUTPUT_WAIT_SECS_DEFAULT);
    assert_eq!(
        completion_wait(NOT_READY, Duration::from_secs(10), wait),
        CompletionWait::KeepWaiting
    );
    assert_eq!(
        completion_wait(NOT_READY, Duration::from_secs(30), wait),
        CompletionWait::KeepWaiting
    );
}

#[test]
fn test_gives_up_only_at_the_bound() {
    let wait = Duration::from_secs(120);
    assert_eq!(
        completion_wait(NOT_READY, Duration::from_secs(119), wait),
        CompletionWait::KeepWaiting
    );
    assert_eq!(
        completion_wait(NOT_READY, Duration::from_secs(120), wait),
        CompletionWait::GiveUp
    );
    assert_eq!(
        completion_wait(NOT_READY, Duration::from_secs(600), wait),
        CompletionWait::GiveUp
    );
}

#[test]
fn test_default_wait_covers_a_two_minute_upload_and_respects_the_proof_window() {
    // ~600 MB of output at the ~5 MB/s measured on host2.
    assert!(
        OUTPUT_WAIT_SECS_DEFAULT >= 120,
        "default wait too short for long outputs"
    );
    // With the default 300 s minimum proof window the default is kept.
    assert_eq!(output_wait_secs(OUTPUT_WAIT_SECS_DEFAULT, 300), 120);
    // At 100 % the last interim proof can be up to half the window old, so the
    // wait must end with margin before the window could lapse.
    for window in [60u64, 120, 300, 600, 3600] {
        let w = output_wait_secs(10_000, window);
        assert!(
            w + window / 2 < window || w == 10,
            "window {window}: wait {w} could outlast it"
        );
        assert!(w >= 10, "never below the 8.62.0 behaviour of 10 s");
    }
    // The probe-2b setting (60 s) keeps 8.62.0's 10 s.
    assert_eq!(output_wait_secs(OUTPUT_WAIT_SECS_DEFAULT, 60), 10);
}

// ---------------------------------------------------------------- source CID

#[test]
fn test_source_cid_must_name_its_network() {
    assert!(source_cid_names_network("s5://urqYSH2QNJOY"));
    assert!(source_cid_names_network("ipfs://bafy"));
    for bad in [
        "urqYSH2QNJOY",
        "uEiBkTest",
        "://urq",
        "s5://",
        "",
        "s5:/urq",
    ] {
        assert!(!source_cid_names_network(bad), "{bad:?} must be refused");
    }
}

fn key() -> [u8; 32] {
    [0xAA; 32]
}

fn decrypt_inner(envelope: &serde_json::Value, key: &[u8; 32]) -> serde_json::Value {
    let p = &envelope["payload"];
    let ct = hex::decode(p["ciphertextHex"].as_str().unwrap()).unwrap();
    let nonce_bytes = hex::decode(p["nonceHex"].as_str().unwrap()).unwrap();
    let aad = hex::decode(p["aadHex"].as_str().unwrap()).unwrap();
    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&nonce_bytes);
    let pt = fabstir_llm_node::crypto::decrypt_with_aead(&ct, &nonce, &aad, key).unwrap();
    serde_json::from_slice(&pt).unwrap()
}

/// A server WITH a sidecar client (pointed at a dead port), so a request gets
/// past "sidecar not configured" to the new check.
async fn server_with_sidecar() -> ApiServer {
    let server = ApiServer::new_for_test();
    let client = TranscoderClient::new("http://127.0.0.1:9", "test-jwt").expect("client");
    server.set_transcoder_client(Arc::new(client)).await;
    server
}

fn ws_request(cid: &str) -> serde_json::Value {
    json!({"action": "transcode", "sourceCid": cid,
           "mediaFormats": [{"id": 1, "ext": "mp4", "vcodec": "h264_nvenc"}], "isEncrypted": true})
}

#[tokio::test]
async fn test_ws_bare_cid_refused_before_the_sidecar() {
    let server = server_with_sidecar().await;
    let (ack, task) = handle_encrypted_transcode(
        &server,
        &ws_request("urqYSH2QNJOY"),
        &key(),
        "s",
        None,
        None,
    )
    .await;
    assert!(task.is_none(), "nothing may reach the sidecar");
    let inner = decrypt_inner(&ack, &key());
    assert_eq!(inner["type"], "transcode_error");
    assert_eq!(inner["error"]["code"], "VALIDATION_FAILED");
    assert!(
        inner["error"]["message"]
            .as_str()
            .unwrap()
            .contains("s5://"),
        "the message names the fix"
    );
}

#[tokio::test]
async fn test_ws_prefixed_cid_passes_the_check() {
    // Control: the same server and request with "s5://" must NOT be refused by
    // the new check (it fails later, at the dead sidecar, with another code).
    let server = server_with_sidecar().await;
    let (ack, _) = handle_encrypted_transcode(
        &server,
        &ws_request("s5://urqYSH2QNJOY"),
        &key(),
        "s",
        None,
        None,
    )
    .await;
    let inner = decrypt_inner(&ack, &key());
    assert_ne!(
        inner["error"]["code"], "VALIDATION_FAILED",
        "a valid CID was refused: {inner}"
    );
}

#[tokio::test]
async fn test_http_bare_cid_400_before_the_sidecar() {
    let server = Arc::new(server_with_sidecar().await);
    let app = Router::new()
        .route("/v1/transcode", post(transcode_submit_handler))
        .with_state(server);
    let body = json!({"sourceCid": "urqYSH2QNJOY", "mediaFormats": [{"id": 1, "ext": "mp4"}]});
    let resp = app
        .oneshot(
            Request::post("/v1/transcode")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 16)
        .await
        .unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("s5://"));
}
