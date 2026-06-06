// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 7.2 — moderation observability counters (§8 #7).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use std::sync::Arc;
use tower::util::ServiceExt;

use fabstir_llm_node::api::moderation::ModerateAssetRequest;
use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::moderation::types::Verdict;
use fabstir_llm_node::monitoring::moderation_metrics::ModerationMetrics;

#[test]
fn counters_record_and_snapshot() {
    let m = ModerationMetrics::new();
    m.record_verdict(Verdict::Cleared);
    m.record_verdict(Verdict::Blocked);
    m.record_verdict(Verdict::Blocked);
    m.record_verdict(Verdict::Flagged);
    m.record_held();
    m.record_match();
    m.record_report_filed();
    let s = m.snapshot();
    assert_eq!(s.cleared, 1);
    assert_eq!(s.blocked, 2);
    assert_eq!(s.flagged, 1);
    assert_eq!(s.held, 1);
    assert_eq!(s.matches, 1);
    assert_eq!(s.reports_filed, 1);
}

#[tokio::test]
async fn asset_endpoint_increments_metrics() {
    // A cleared /moderate/asset call must increment the shared counter.
    let server = Arc::new(ApiServer::new_for_test());
    let app = ApiServer::create_router(Arc::clone(&server));
    let body = serde_json::to_string(&ModerateAssetRequest {
        kind: "subtitle".into(),
        data: base64::engine::general_purpose::STANDARD.encode(b"WEBVTT\n\nclean line\n"),
    })
    .unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/moderate/asset")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        server.moderation_metrics().snapshot().cleared,
        1,
        "a cleared asset must increment the cleared counter"
    );
}

#[tokio::test]
async fn metrics_endpoint_exposes_moderation_counters() {
    // §8 #7: the moderation counters must be VISIBLE at /metrics (not just incremented
    // in-process). Drive a cleared asset, then scrape /metrics and assert the counters are
    // both present and reflect the increment (R10-1: /metrics was a hardcoded stub).
    let server = Arc::new(ApiServer::new_for_test());
    let app = ApiServer::create_router(Arc::clone(&server));
    let body = serde_json::to_string(&ModerateAssetRequest {
        kind: "subtitle".into(),
        data: base64::engine::general_purpose::STANDARD.encode(b"WEBVTT\n\nclean line\n"),
    })
    .unwrap();
    let post = Request::builder()
        .method("POST")
        .uri("/v1/moderate/asset")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(post).await.unwrap().status(),
        StatusCode::OK
    );

    let get = Request::builder()
        .method("GET")
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(get).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(
        text.contains("moderation_verdicts_total"),
        "the moderation verdict counters must be exposed at /metrics"
    );
    assert!(
        text.contains("moderation_matches_total"),
        "the Track-1 matches counter must be exposed at /metrics"
    );
    assert!(
        text.contains("moderation_holds_total"),
        "the fail-closed holds counter must be exposed at /metrics"
    );
    assert!(
        text.contains("moderation_verdicts_total{outcome=\"cleared\"} 1"),
        "the cleared increment must be visible at /metrics; body was:\n{text}"
    );
}
