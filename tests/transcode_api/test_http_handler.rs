use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use axum::Router;
use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::api::transcode::handler::{
    transcode_status_handler, transcode_submit_handler,
};
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;

fn test_app() -> Router {
    let server = Arc::new(ApiServer::new_for_test());
    Router::new()
        .route("/v1/transcode", post(transcode_submit_handler))
        .route("/v1/transcode/:task_id", get(transcode_status_handler))
        .with_state(server)
}

#[tokio::test]
async fn test_http_transcode_missing_cid_400() {
    let app = test_app();
    let body = json!({"sourceCid": "", "mediaFormats": [{"id": 1, "ext": "mp4"}]});
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcode")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_http_transcode_empty_formats_400() {
    let app = test_app();
    let body = json!({"sourceCid": "uEiBkTest", "mediaFormats": []});
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcode")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_http_transcode_no_sidecar_503() {
    let app = test_app();
    let body = json!({
        "sourceCid": "uEiBkTest",
        "mediaFormats": [{"id": 1, "ext": "mp4"}]
    });
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcode")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn test_http_transcode_status_endpoint() {
    let app = test_app();
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/transcode/test-task-123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn test_http_transcode_with_preview_percent_503() {
    let app = test_app();
    let body = json!({
        "sourceCid": "uEiBkTestHls",
        "mediaFormats": [{"id": 1, "ext": "mp4", "vcodec": "av1_nvenc", "hls": true, "hls_time": 6}],
        "isGpu": true,
        "isEncrypted": true,
        "previewPercent": 20
    });
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/transcode")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
