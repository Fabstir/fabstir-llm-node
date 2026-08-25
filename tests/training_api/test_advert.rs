// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The pre-escrow training advert and the tokenizer it pins, through the REAL
//! router: `GET /v1/training/advert` and `GET /v1/training/tokenizer`.
//!
//! The load-bearing row here is `advert_hash_matches_what_the_tokenizer_route_
//! serves`. A client counts before escrow and must count with exactly the bytes
//! the host bills with; it does that by hashing what it fetches and comparing
//! against the advert. If those two ever disagree, every conforming client
//! refuses to count and training is dead in the water, so the agreement is
//! pinned rather than assumed.

use std::sync::Arc;

use fabstir_llm_node::api::server::ApiServer;
use sha2::{Digest, Sha256};
use tower::util::ServiceExt;

use super::support::{
    fixture, make_deps, model_id, passing_snapshot, CountBehaviour, MockSessions, ScanBehaviour,
};

/// Wire a training deps set onto a fresh server. `tokenizer: false` models a
/// host that serves adapters but was never given a tokenizer.
async fn wired_server_opt(tokenizer: bool) -> Arc<ApiServer> {
    let server = Arc::new(ApiServer::new_for_test());
    let fx = fixture(None).await;
    let mut h = make_deps(
        &fx,
        MockSessions {
            snapshot: Ok(passing_snapshot()),
            model: model_id(0xAA),
            dispute: 30,
        },
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    if !tokenizer {
        h.deps.tokenizer = None;
    }
    server.set_training_deps(Arc::new(h.deps)).await;
    server
}

/// Wire a healthy training deps set onto a fresh server.
async fn wired_server() -> Arc<ApiServer> {
    let server = Arc::new(ApiServer::new_for_test());
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        MockSessions {
            snapshot: Ok(passing_snapshot()),
            model: model_id(0xAA),
            dispute: 30,
        },
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    server.set_training_deps(Arc::new(h.deps)).await;
    server
}

struct Res {
    status: u16,
    etag: Option<String>,
    cache_control: Option<String>,
    body: Vec<u8>,
}

async fn get(server: Arc<ApiServer>, uri: &str, if_none_match: Option<&str>) -> Res {
    let mut req = axum::http::Request::builder().uri(uri);
    if let Some(inm) = if_none_match {
        req = req.header(axum::http::header::IF_NONE_MATCH, inm);
    }
    let response = ApiServer::create_router(server)
        .oneshot(req.body(axum::body::Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status().as_u16();
    let header = |n: axum::http::HeaderName| {
        response
            .headers()
            .get(n)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let etag = header(axum::http::header::ETAG);
    let cache_control = header(axum::http::header::CACHE_CONTROL);
    let body = axum::body::to_bytes(response.into_body(), 1 << 26)
        .await
        .unwrap()
        .to_vec();
    Res {
        status,
        etag,
        cache_control,
        body,
    }
}

#[tokio::test]
async fn both_routes_404_when_training_is_disabled() {
    let server = Arc::new(ApiServer::new_for_test());
    assert_eq!(
        get(server.clone(), "/v1/training/advert", None)
            .await
            .status,
        404
    );
    assert_eq!(
        get(server, "/v1/training/tokenizer", None).await.status,
        404,
        "a disabled host must not serve a tokenizer it will never bill against"
    );
}

#[tokio::test]
async fn advert_publishes_the_fields_the_ltx_bundle_cannot_carry() {
    let res = get(wired_server().await, "/v1/training/advert", None).await;
    assert_eq!(res.status, 200);
    let v: serde_json::Value = serde_json::from_slice(&res.body).unwrap();

    for field in ["tokenizerSha256", "baseServingModelId", "alphas"] {
        assert!(
            v["template"].get(field).is_some(),
            "{field} missing; this is the whole reason the advert exists: {v}"
        );
    }
    assert!(v["template"]["alphas"].is_array(), "{v}");
    // The client needs these to build a session at all.
    assert!(
        v["modelId"].as_str().is_some_and(|s| s.starts_with("0x")),
        "{v}"
    );
    assert!(
        v["pricePerToken"].is_string(),
        "decimal string, per billing.json: {v}"
    );
    assert!(
        v["template"]["sliceTokens"].is_number(),
        "counts stay numeric: {v}"
    );
    assert_eq!(v["tokenizer"]["url"], "/v1/training/tokenizer");
}

#[tokio::test]
async fn advert_hash_matches_what_the_tokenizer_route_serves() {
    let server = wired_server().await;
    let advert = get(server.clone(), "/v1/training/advert", None).await;
    let v: serde_json::Value = serde_json::from_slice(&advert.body).unwrap();
    let advertised = v["tokenizer"]["sha256"].as_str().unwrap().to_string();

    let served = get(server, "/v1/training/tokenizer", None).await;
    assert_eq!(served.status, 200);
    let actual = format!(
        "0x{}",
        hex::encode(<[u8; 32]>::from(Sha256::digest(&served.body)))
    );

    assert_eq!(
        advertised, actual,
        "a client that verifies the fetch against the advert would refuse these bytes"
    );
    assert_eq!(
        v["tokenizer"]["bytes"].as_u64().unwrap() as usize,
        served.body.len(),
        "advertised length disagrees with what was served"
    );
}

#[tokio::test]
async fn tokenizer_is_cacheable_and_revalidates() {
    let server = wired_server().await;
    let first = get(server.clone(), "/v1/training/tokenizer", None).await;
    assert_eq!(first.status, 200);
    assert!(!first.body.is_empty());
    let etag = first
        .etag
        .clone()
        .expect("a strong ETag is required to revalidate");
    assert!(etag.starts_with('"'), "strong, not weak: {etag}");
    let cc = first.cache_control.clone().unwrap_or_default();
    assert!(cc.contains("immutable"), "content is pin-addressed: {cc}");

    // Exact match revalidates to 304 with no body.
    let again = get(server.clone(), "/v1/training/tokenizer", Some(&etag)).await;
    assert_eq!(
        again.status, 304,
        "a repeat client must not re-download ~12 MB"
    );
    assert!(again.body.is_empty(), "304 carries no body");

    // A LIST containing our ETag also revalidates: a conforming client may send
    // several, and matching on string equality would force a re-download.
    let list = format!("\"deadbeef\", {etag}");
    assert_eq!(
        get(server.clone(), "/v1/training/tokenizer", Some(&list))
            .await
            .status,
        304,
        "If-None-Match is a list, not a single value"
    );

    // A non-matching ETag must serve the bytes, not a spurious 304.
    let stale = get(server, "/v1/training/tokenizer", Some("\"nope\"")).await;
    assert_eq!(stale.status, 200);
    assert_eq!(stale.body, first.body);
}

#[tokio::test]
async fn a_host_with_no_tokenizer_still_serves_adapters() {
    // The regression this pins: making the tokenizer a wiring REQUIREMENT left
    // `training_deps` as None, so serve-back answered "training is not enabled
    // on this node" on a host whose only job was to serve adapters. Serve-back
    // counts nothing, so it must not depend on a counting asset.
    let server = wired_server_opt(false).await;

    let capacity = get(server.clone(), "/v1/training/capacity", None).await;
    assert_eq!(
        capacity.status, 200,
        "training must stay ENABLED without a tokenizer, or serve-back dies with it"
    );

    // The advert still works, and says plainly that counting is unavailable.
    let advert = get(server.clone(), "/v1/training/advert", None).await;
    assert_eq!(advert.status, 200);
    let v: serde_json::Value = serde_json::from_slice(&advert.body).unwrap();
    assert_eq!(v["tokenizer"]["available"], false, "{v}");
    assert_eq!(v["tokenizer"]["reason"], "notServed", "{v}");
    // The template pin survives: it describes the template, not this host.
    assert!(v["template"]["tokenizerSha256"].as_str().is_some(), "{v}");
    // ...and the serve-back pin is untouched, which is what this host IS for.
    assert!(
        v["template"]["baseServingModelId"].as_str().is_some(),
        "{v}"
    );

    // 503, not 404: the route exists, it has nothing verified to serve.
    let tok = get(server, "/v1/training/tokenizer", None).await;
    assert_eq!(tok.status, 503, "404 would read as 'wrong URL' to a client");
}
