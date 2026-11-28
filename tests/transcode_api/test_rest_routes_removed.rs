// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! NP1.3: the unauthenticated transcode REST routes are gone.
//!
//! `GET /v1/transcode/:task_id` returned task outputs that the WebSocket path sends only
//! encrypted, and `POST /v1/transcode` started a transcode with no payment gate. Neither the SDK
//! nor the orchestrator calls them; both use only `GET /v1/transcode/capacity`, which stays.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use fabstir_llm_node::api::server::ApiServer;
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;

async fn call(req: Request<Body>) -> (StatusCode, Vec<u8>) {
    let server = Arc::new(ApiServer::new_for_test());
    let resp = ApiServer::create_router(server).oneshot(req).await.unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), 1 << 16)
        .await
        .unwrap()
        .to_vec();
    (status, body)
}

#[tokio::test]
async fn np1_3_get_transcode_status_is_gone() {
    let (status, _) = call(
        Request::get("/v1/transcode/test-task-123")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn np1_3_post_transcode_is_gone() {
    let body = json!({"sourceCid": "s5://uEiBkTest", "mediaFormats": [{"id": 1, "ext": "mp4"}]});
    let (status, _) = call(
        Request::post("/v1/transcode")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn np1_3_transcode_capacity_still_answers() {
    let (status, body) = call(
        Request::get("/v1/transcode/capacity")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["sidecarConnected"], false);
}
