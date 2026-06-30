// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 5.2 — POST /v1/moderate/asset (B8).
//!
//! Lives in the `moderation_tests` crate (not `tests/api/`) because the `api_tests`
//! crate has pre-existing, unrelated compile failures (test bitrot); per the
//! baseline-diff principle this new test runs in a clean crate.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use std::sync::Arc;
use tower::util::ServiceExt;

use fabstir_llm_node::api::moderation::{moderate_asset_inner, ModerateAssetRequest};
use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::moderation::asset::{AssetModerator, TextScanList};
use fabstir_llm_node::moderation::csam::hashlist::{HashListSnapshot, HashListSource};
use fabstir_llm_node::moderation::csam::matcher::Matcher;
use fabstir_llm_node::moderation::csam::mock_source::MockHashListSource;
use fabstir_llm_node::moderation::csam::ownhash::OwnHashList;

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
fn dto_roundtrip() {
    let req = ModerateAssetRequest {
        kind: "subtitle".into(),
        data: b64(b"hi"),
    };
    let back: ModerateAssetRequest =
        serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
    assert_eq!(back.kind, "subtitle");
}

#[test]
fn post_clean_returns_cleared() {
    let am = am_with(
        MockHashListSource::loaded(vec![], vec![])
            .refresh()
            .unwrap(),
    );
    let req = ModerateAssetRequest {
        kind: "subtitle".into(),
        data: b64(b"WEBVTT\n\n00:00.000 --> 00:01.000\nnormal text\n"),
    };
    let resp = moderate_asset_inner(&am, &req, 20 * 1024 * 1024).unwrap();
    assert_eq!(resp.verdict, "cleared");
}

#[test]
fn post_match_returns_blocked() {
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
    let resp = moderate_asset_inner(&am, &req, 20 * 1024 * 1024).unwrap();
    assert_eq!(resp.verdict, "blocked");
}

#[test]
fn oversize_rejected() {
    let am = am_with(HashListSnapshot::unavailable());
    let req = ModerateAssetRequest {
        kind: "image".into(),
        data: b64(&[0u8; 100]),
    };
    let err = moderate_asset_inner(&am, &req, 10).unwrap_err(); // max 10 bytes
    assert_eq!(err.0, StatusCode::PAYLOAD_TOO_LARGE);
}

#[test]
fn unknown_kind_rejected() {
    let am = am_with(HashListSnapshot::unavailable());
    let req = ModerateAssetRequest {
        kind: "nonsense".into(),
        data: b64(b"x"),
    };
    assert_eq!(
        moderate_asset_inner(&am, &req, 1000).unwrap_err().0,
        StatusCode::BAD_REQUEST
    );
}

#[test]
fn invalid_base64_rejected() {
    let am = am_with(
        MockHashListSource::loaded(vec![], vec![])
            .refresh()
            .unwrap(),
    );
    let req = ModerateAssetRequest {
        kind: "image".into(),
        data: "!!! not valid base64 !!!".into(),
    };
    assert_eq!(
        moderate_asset_inner(&am, &req, 20 * 1024 * 1024)
            .unwrap_err()
            .0,
        StatusCode::BAD_REQUEST
    );
}

#[test]
fn empty_asset_data_holds() {
    // Empty image bytes ⇒ undecodable ⇒ hold (never cleared).
    let am = am_with(
        MockHashListSource::loaded(vec![], vec![])
            .refresh()
            .unwrap(),
    );
    let req = ModerateAssetRequest {
        kind: "image".into(),
        data: b64(&[]),
    };
    let resp = moderate_asset_inner(&am, &req, 20 * 1024 * 1024).unwrap();
    assert_ne!(resp.verdict, "cleared", "empty/undecodable image must hold");
}

#[tokio::test]
async fn route_registered_and_serves_clean_subtitle() {
    // End-to-end through the production router: a clean subtitle ⇒ 200.
    let server = Arc::new(ApiServer::new_for_test());
    let app = ApiServer::create_router(server);
    let body = serde_json::to_string(&ModerateAssetRequest {
        kind: "subtitle".into(),
        data: b64(b"WEBVTT\n\n00:00.000 --> 00:01.000\nhello\n"),
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
}

#[tokio::test]
async fn route_image_on_unavailable_list_returns_503() {
    // PRODUCTION path: build_asset_moderator() installs an Unavailable NCMEC snapshot, so
    // an image POST to /v1/moderate/asset must HOLD as 503 (retryable infra hold) and
    // preserve nothing — NOT 200-blocked-and-preserved (the over-preserve fix).
    let server = Arc::new(ApiServer::new_for_test());
    let app = ApiServer::create_router(server);
    let body = serde_json::to_string(&ModerateAssetRequest {
        kind: "image".into(),
        data: b64(&png([7, 8, 9])),
    })
    .unwrap();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/moderate/asset")
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}
