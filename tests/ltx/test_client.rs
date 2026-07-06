// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 3 ComfyUI client tests. No live ComfyUI: HTTP is exercised against a
//! local axum server; WS/history parsing is unit-tested on sample frames.

use fabstir_llm_node::ltx::client::{parse_history, parse_progress, ComfyClient, ExrRef, Progress};
use fabstir_llm_node::ltx::Graph;
use serde_json::json;

async fn spawn_prompt_server() -> String {
    use axum::{routing::post, Json, Router};
    let app = Router::new().route(
        "/prompt",
        post(|| async { Json(json!({ "prompt_id": "p-xyz" })) }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    format!("http://{addr}")
}

async fn spawn_upload_server() -> String {
    use axum::{routing::post, Json, Router};
    let app = Router::new().route(
        "/upload/image",
        post(|| async { Json(json!({ "name": "img_abc.png", "subfolder": "", "type": "input" })) }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    format!("http://{addr}")
}

#[tokio::test]
async fn test_upload_input_returns_stored_name() {
    let url = spawn_upload_server().await;
    let client = ComfyClient::new(&url).unwrap();
    // ComfyUI is authoritative on the stored name; the client returns whatever it assigns.
    let name = client
        .upload_input("egyptian_queen.png", b"\x89PNG\r\n\x1a\n".to_vec())
        .await
        .unwrap();
    assert_eq!(name, "img_abc.png");
}

#[test]
fn test_new_trims_trailing_slash() {
    let c = ComfyClient::new("http://localhost:8188/").unwrap();
    assert_eq!(c.endpoint(), "http://localhost:8188");
}

#[tokio::test]
async fn test_submit_returns_prompt_id() {
    let url = spawn_prompt_server().await;
    let client = ComfyClient::new(&url).unwrap();
    let graph = Graph(json!({ "1": { "class_type": "X", "inputs": {} } }));
    assert_eq!(client.submit(&graph).await.unwrap(), "p-xyz");
}

/// ComfyUI answers 200 + `prompt_id` even when SOME output nodes fail validation
/// (it executes only the valid subset, listing failures in `node_errors`) — the
/// exact shape of the live session-847 loss. A non-empty `node_errors` must be a
/// hard submit failure, before any GPU work.
#[tokio::test]
async fn test_submit_rejects_partial_graph_with_node_errors() {
    use axum::{routing::post, Json, Router};
    let app = Router::new().route(
        "/prompt",
        post(|| async {
            Json(json!({
                "prompt_id": "p-partial",
                "number": 7,
                "node_errors": { "692": { "errors": [{ "type": "return_type_mismatch" }] } }
            }))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let client = ComfyClient::new(&format!("http://{addr}")).unwrap();
    let graph = Graph(json!({ "1": { "class_type": "X", "inputs": {} } }));
    let err = client.submit(&graph).await.unwrap_err().to_string();
    assert!(err.contains("692"), "error names the failing node: {err}");
    assert!(err.contains("refusing the partial graph"), "{err}");
}

/// An explicitly EMPTY node_errors object (what ComfyUI sends on full success)
/// must not trip the guard.
#[tokio::test]
async fn test_submit_accepts_empty_node_errors() {
    use axum::{routing::post, Json, Router};
    let app = Router::new().route(
        "/prompt",
        post(|| async { Json(json!({ "prompt_id": "p-ok", "node_errors": {} })) }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let client = ComfyClient::new(&format!("http://{addr}")).unwrap();
    let graph = Graph(json!({ "1": { "class_type": "X", "inputs": {} } }));
    assert_eq!(client.submit(&graph).await.unwrap(), "p-ok");
}

#[test]
fn test_unique_client_id_per_job() {
    let a = ComfyClient::new("http://localhost:8188").unwrap();
    let b = ComfyClient::new("http://localhost:8188").unwrap();
    assert_ne!(a.client_id(), b.client_id());
    assert!(!a.client_id().is_empty());
}

#[test]
fn test_progress_events_parsed() {
    let exec = json!({ "type": "executing", "data": { "node": "6", "prompt_id": "p" } });
    assert_eq!(
        parse_progress(&exec),
        Some(Progress::Executing {
            node: Some("6".to_string())
        })
    );
    // node:null is ComfyUI's "prompt finished" signal.
    let done = json!({ "type": "executing", "data": { "node": null, "prompt_id": "p" } });
    assert_eq!(
        parse_progress(&done),
        Some(Progress::Executing { node: None })
    );
    let prog = json!({ "type": "progress", "data": { "value": 12, "max": 20 } });
    assert_eq!(
        parse_progress(&prog),
        Some(Progress::Progress { value: 12, max: 20 })
    );
    let executed = json!({ "type": "executed", "data": { "node": "8" } });
    assert_eq!(parse_progress(&executed), Some(Progress::Executed));
    // Unknown frame types are ignored.
    assert_eq!(parse_progress(&json!({ "type": "status" })), None);
}

#[test]
fn test_history_lists_outputs() {
    let body = json!({
        "p-1": { "outputs": { "8": { "images": [
            { "filename": "ltx_00001_.exr", "subfolder": "p-1", "type": "output" },
            { "filename": "ltx_00002_.exr", "subfolder": "p-1", "type": "output" }
        ] } } }
    });
    let refs = parse_history(&body, "p-1");
    assert_eq!(refs.len(), 2);
    assert_eq!(
        refs[0],
        ExrRef {
            filename: "ltx_00001_.exr".to_string(),
            subfolder: "p-1".to_string(),
            type_: "output".to_string(),
        }
    );
    // Unknown prompt id -> empty.
    assert!(parse_history(&body, "other").is_empty());
}

#[test]
fn test_history_lists_video_output() {
    // A SaveVideo/CreateVideo node reports its clip under a non-"images" bucket
    // (e.g. "gifs"); the parser must still collect it. The stray "animated" flag
    // array (booleans, no filename) must be ignored.
    let body = json!({
        "p-2": { "outputs": { "75": {
            "gifs": [ { "filename": "ltx_00001.mp4", "subfolder": "p-2", "type": "output" } ],
            "animated": [ true ]
        } } }
    });
    let refs = parse_history(&body, "p-2");
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].filename, "ltx_00001.mp4");
    assert_eq!(refs[0].type_, "output");
}

#[tokio::test]
async fn test_health_unreachable_is_false() {
    let client = ComfyClient::new("http://127.0.0.1:59998").unwrap();
    assert!(!client.health().await);
}
