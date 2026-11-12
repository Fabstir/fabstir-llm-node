// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The three listeners over HTTP (tower oneshot): auth order, body caps, typed errors, no listing, the
//! index gate. Each test is named by the defect it catches.
mod common;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use base64::Engine;
use common::*;
use fabstir_provenance::contract::{self, sign_request};
use http_body_util::BodyExt;
use provenance_signer::app::{self, IndexGate};
use std::net::SocketAddr;
use tower::ServiceExt;

const SECRET: &[u8] = b"0123456789abcdef0123456789abcdef-index";

async fn send(
    router: axum::Router,
    req: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let r = router.oneshot(req).await.unwrap();
    let s = r.status();
    let h = r.headers().clone();
    (
        s,
        h,
        r.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
}

fn signed(
    e: &Env,
    key: &k256::ecdsa::SigningKey,
    path: &str,
    body: &[u8],
    nonce: [u8; 16],
) -> Request<Body> {
    let now = e.clock.load(std::sync::atomic::Ordering::SeqCst);
    let h = sign_request(key, "POST", path, now, nonce, body);
    Request::post(path)
        .header("content-type", "application/json")
        .header(contract::HDR_TIMESTAMP, h.timestamp)
        .header(contract::HDR_NONCE, h.nonce)
        .header(contract::HDR_SIGNATURE, h.signature)
        .body(Body::from(body.to_vec()))
        .unwrap()
}

fn code(body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body).unwrap()["error"]["code"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

fn reserve_body() -> Vec<u8> {
    serde_json::to_vec(&reserve_req("ltx-t2v-hdr", false)).unwrap()
}

#[tokio::test]
async fn refuses_unlisted_host() {
    let e = env();
    let (s, _, b) = send(
        app::api_router(e.svc.clone()),
        signed(&e, &other_key(), "/v1/reserve", &reserve_body(), [1; 16]),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(code(&b), "unauthorized");
    let (s, _, _) = send(
        app::api_router(e.svc.clone()),
        signed(&e, &host_key(), "/v1/reserve", &reserve_body(), [1; 16]),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::OK,
        "an unlisted caller must not burn the host's nonce"
    );
}

#[tokio::test]
async fn refuses_replayed_request() {
    let e = env();
    let (s, _, _) = send(
        app::api_router(e.svc.clone()),
        signed(&e, &host_key(), "/v1/reserve", &reserve_body(), [7; 16]),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, b) = send(
        app::api_router(e.svc.clone()),
        signed(&e, &host_key(), "/v1/reserve", &reserve_body(), [7; 16]),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(code(&b), "unauthorized");
}

#[tokio::test]
async fn stale_request_is_reported_as_stale() {
    let e = env();
    let req = signed(&e, &host_key(), "/v1/reserve", &reserve_body(), [3; 16]);
    e.advance(contract::WINDOW_SECS + 1);
    let (s, _, b) = send(app::api_router(e.svc.clone()), req).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(code(&b), "stale_request");
}

#[tokio::test]
async fn signature_covers_path_and_body() {
    let e = env();
    let mut req = signed(&e, &host_key(), "/v1/reserve", &reserve_body(), [4; 16]);
    *req.body_mut() = Body::from(serde_json::to_vec(&reserve_req("ltx-edit-hdr", false)).unwrap());
    let (s, _, _) = send(app::api_router(e.svc.clone()), req).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sign_over_http_returns_the_manifest_with_headers() {
    let e = env();
    let r = e.reserve("ltx-t2v-hdr", false);
    let p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    let (s, h, b) = send(
        app::api_router(e.svc.clone()),
        signed(&e, &host_key(), "/v1/sign", &p.body, [5; 16]),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&b));
    assert_eq!(b.len(), p.req.placeholder_len);
    assert_eq!(
        h[contract::HDR_MANIFEST_ID],
        format!("urn:c2pa:{}", r.params.manifest_uuid).as_str()
    );
    assert_eq!(h[contract::HDR_REPLAY], "0");
    assert_eq!(h[contract::HDR_TSA], "127.0.0.1");
    // the identical retry with a fresh nonce: same bytes, replay 1
    let (s, h, b2) = send(
        app::api_router(e.svc.clone()),
        signed(&e, &host_key(), "/v1/sign", &p.body, [6; 16]),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h[contract::HDR_REPLAY], "1");
    assert_eq!(b, b2);
}

#[tokio::test]
async fn oversized_body_is_413_before_parsing() {
    let e = env();
    let big = vec![b' '; contract::MAX_SIGN_BODY + 1];
    let (s, _, b) = send(
        app::api_router(e.svc.clone()),
        signed(&e, &host_key(), "/v1/sign", &big, [8; 16]),
    )
    .await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(code(&b), "body_too_large");
    let big = vec![b' '; contract::MAX_RESERVE_BODY + 1];
    let (s, _, _) = send(
        app::api_router(e.svc.clone()),
        signed(&e, &host_key(), "/v1/reserve", &big, [9; 16]),
    )
    .await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn truncated_or_garbage_bodies_get_typed_400s() {
    let e = env();
    let mut seed = 0x9e3779b97f4a7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let good = reserve_body();
    for i in 0..200u32 {
        let body: Vec<u8> = match i % 3 {
            0 => good[..(next() as usize % good.len())].to_vec(),
            1 => (0..(next() % 512)).map(|_| next() as u8).collect(),
            _ => {
                let mut g = good.clone();
                let k = next() as usize % g.len();
                g[k] = next() as u8;
                g
            }
        };
        let mut nonce = [0u8; 16];
        nonce[..4].copy_from_slice(&i.to_be_bytes());
        nonce[4] = 0xaa;
        let path = if i.is_multiple_of(2) {
            "/v1/reserve"
        } else {
            "/v1/sign"
        };
        let (s, _, b) = send(
            app::api_router(e.svc.clone()),
            signed(&e, &host_key(), path, &body, nonce),
        )
        .await;
        assert!(s.is_client_error() || s == StatusCode::OK, "case {i}: {s}");
        if s != StatusCode::OK {
            assert!(!code(&b).is_empty(), "case {i}: untyped error body");
        }
    }
}

#[tokio::test]
async fn repository_has_no_listing() {
    let e = env();
    let r = e.reserve("ltx-t2v-hdr", false);
    let p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap();
    let get = |u: &str| Request::get(u).body(Body::empty()).unwrap();
    let (s, h, b) = send(
        app::public_router(e.svc.clone()),
        get(&format!("/v1/manifests/{}", r.params.manifest_uuid)),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h["content-type"], "application/c2pa");
    assert!(!b.is_empty());
    let (_, _, nf) = send(
        app::public_router(e.svc.clone()),
        get("/v1/manifests/00000000-0000-4000-8000-000000000000"),
    )
    .await;
    for u in [
        "/",
        "/v1/manifests",
        "/v1/manifests/",
        "/v1",
        "/v1/manifests/*",
        "/v1/manifests/..%2F..%2Fsigner.sqlite",
        "/v1/manifests/urn:c2pa:",
        "/v1/index/match",
        "/v1/limits",
    ] {
        let (s, _, b) = send(app::public_router(e.svc.clone()), get(u)).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{u}");
        assert_eq!(b, nf, "{u}: 404s must be uniform");
    }
    for u in ["/", "/v1/manifests", "/v1/reservations", "/v1/audit"] {
        let (s, _, _) = send(app::api_router(e.svc.clone()), get(u)).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "api {u}");
    }
}

fn gate(ip: &str) -> IndexGate {
    IndexGate::new(SECRET.to_vec(), vec![ip.parse().unwrap()])
}

fn index_req(from: &str, secret: Option<&[u8]>, body: Vec<u8>) -> Request<Body> {
    let mut b = Request::post("/v1/index/match");
    if let Some(s) = secret {
        b = b.header(contract::HDR_INDEX_SECRET, s);
    }
    let mut r = b.body(Body::from(body)).unwrap();
    r.extensions_mut().insert(ConnectInfo::<SocketAddr>(
        format!("{from}:40000").parse().unwrap(),
    ));
    r
}

#[tokio::test]
async fn index_api_rejects_unauthenticated() {
    let e = env();
    let r = e.reserve("ltx-t2v-hdr", false);
    let p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    e.svc.sign_mp4(&e.host(), &p.body, &p.req).unwrap();
    let router = || app::index_router(e.svc.clone(), gate("10.0.0.5"));
    for (from, secret) in [
        ("10.0.0.5", None),
        (
            "10.0.0.5",
            Some(&b"wrong-secret-wrong-secret-wrong-secret!"[..]),
        ),
        ("10.0.0.6", Some(SECRET)),
        ("10.0.0.5", Some(&SECRET[..SECRET.len() - 1])),
    ] {
        let (s, _, b) = send(router(), index_req(from, secret, fingerprint())).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{from} {secret:?}");
        assert!(
            !String::from_utf8_lossy(&b).contains("urn:c2pa"),
            "no data without auth"
        );
    }
    let (s, _, b) = send(router(), index_req("10.0.0.5", Some(SECRET), fingerprint())).await;
    assert_eq!(s, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(
        v["matches"][0]["manifest"],
        format!("urn:c2pa:{}", r.params.manifest_uuid)
    );
    assert_eq!(v["matches"][0]["has_visual_input"], false);
}

#[tokio::test]
async fn limits_publish_the_reserve_range_and_versions() {
    let e = env();
    let (s, _, b) = send(
        app::api_router(e.svc.clone()),
        Request::get("/v1/limits").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let l: contract::Limits = serde_json::from_slice(&b).unwrap();
    assert_eq!(
        (l.reserve_min, l.reserve_max, l.reserve_recommended),
        (9_216, 16_384, 12_000)
    );
    assert_eq!(l.c2pa_crate, "0.90.22");
    assert_eq!(l.chain_epoch, 1);
}

#[tokio::test]
async fn reserve_out_of_range_carries_the_limits() {
    let e = env();
    let r = e.reserve("ltx-t2v-hdr", false);
    let mut p = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    p.req.reserve_size = 4_000;
    p.body = serde_json::to_vec(&p.req).unwrap();
    let (s, _, b) = send(
        app::api_router(e.svc.clone()),
        signed(&e, &host_key(), "/v1/sign", &p.body, [0x31; 16]),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["error"]["code"], "reserve_out_of_range");
    assert_eq!(v["limits"]["reserve_min"], 9_216);
}

#[tokio::test]
async fn garbage_inside_well_formed_sign_requests_gets_typed_errors() {
    // past the JSON parse: random archive and fingerprint bytes reach c2pa and the unpacker
    let e = env();
    let r = e.reserve("ltx-t2v-hdr", false);
    let good = node_sign_request(&r.reservation_id, &r.params, Some(&fingerprint()), 0);
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut seed = 0x2545f4914f6cdd1du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let archive = b64.decode(&good.req.builder_archive_b64).unwrap();
    let fp = fingerprint();
    for i in 0..60u32 {
        let mut req = good.req.clone();
        match i % 4 {
            0 => {
                let mut a = archive.clone();
                for _ in 0..8 {
                    let k = next() as usize % a.len();
                    a[k] = next() as u8;
                }
                req.builder_archive_b64 = b64.encode(a);
            }
            1 => {
                let n = next() as usize % archive.len();
                req.builder_archive_b64 = b64.encode(&archive[..n]);
            }
            2 => {
                let mut f = fp.clone();
                let k = next() as usize % f.len();
                f[k] = next() as u8;
                req.fingerprint_b64 = Some(b64.encode(f));
            }
            _ => {
                let mut a = b"\0\0\x10\0jumb".to_vec();
                a.extend((0..(next() % 4096)).map(|_| next() as u8));
                req.builder_archive_b64 = b64.encode(a);
            }
        }
        let body = serde_json::to_vec(&req).unwrap();
        let mut nonce = [0x5au8; 16];
        nonce[..4].copy_from_slice(&i.to_be_bytes());
        let (s, _, b) = send(
            app::api_router(e.svc.clone()),
            signed(&e, &host_key(), "/v1/sign", &body, nonce),
        )
        .await;
        assert!(
            s.is_client_error() || s == StatusCode::OK || s == StatusCode::INTERNAL_SERVER_ERROR,
            "case {i}: {s}"
        );
        if s != StatusCode::OK {
            assert!(!code(&b).is_empty(), "case {i}: untyped error body");
        }
        if s == StatusCode::OK {
            // the only acceptable success is the untouched request's replay or a mutation c2pa ignores
            assert!(e
                .svc
                .store
                .slot(&r.reservation_id, "mp4")
                .unwrap()
                .is_some());
        }
    }
}

#[tokio::test]
async fn frames_index_route_and_bare_manifest_over_http() {
    let e = env();
    let r = e.reserve("ltx-sdr2hdr-hdr", true);
    let g = r.sequence.clone().unwrap();
    let idx = fabstir_provenance::frames_index::build(
        fabstir_provenance::Class::Modified,
        &[("frame_000001.exr".into(), b"x".to_vec())],
    )
    .unwrap();
    let req = fabstir_provenance::contract::SignSequenceRequest {
        reservation_id: r.reservation_id.clone(),
        slot: "sequence".into(),
        frames_index_b64: base64::engine::general_purpose::STANDARD.encode(&idx),
    };
    e.svc
        .sign_sequence(&e.host(), &serde_json::to_vec(&req).unwrap(), &req)
        .unwrap();
    let get = |u: String| Request::get(u).body(Body::empty()).unwrap();
    let (s, h, b) = send(
        app::public_router(e.svc.clone()),
        get(format!("/v1/manifests/{}/frames-index", g.manifest_uuid)),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h["content-type"], "application/json");
    assert_eq!(b, idx);
    let (s, _, _) = send(
        app::public_router(e.svc.clone()),
        get(format!(
            "/v1/manifests/{}/frames-index",
            r.params.manifest_uuid
        )),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::NOT_FOUND,
        "an mp4 record has no frames index"
    );
}

#[tokio::test]
async fn reserve_too_small_is_a_409_with_limits_not_a_500() {
    // a reserve inside the configured range but too small for the real chain + TSA token
    let e = env_full(|good| vec![good.to_string()], |s| s.reserve_min = 500);
    let r = e.reserve("ltx-t2v-hdr", false);
    let ctx = c2pa::Context::new()
        .with_settings(fabstir_provenance::flow::settings(None, false))
        .unwrap()
        .with_signer(Box::new(fabstir_provenance::flow::ReserveOnly(1_000)));
    let fp = fingerprint();
    let digest = fabstir_provenance::fingerprint::digest(&fp);
    let (h, _) = fabstir_provenance::flow::prepare_mp4(
        ctx,
        &r.params,
        Some(&digest),
        &[],
        &fixture("tiny.mp4"),
        2_400,
    )
    .unwrap();
    let req = fabstir_provenance::contract::SignRequest {
        reservation_id: r.reservation_id.clone(),
        slot: "mp4".into(),
        reserve_size: 1_000,
        placeholder_len: h.placeholder_len,
        bmff_hash_sha256_hex: hex::encode(&h.bmff_hash),
        builder_archive_b64: base64::engine::general_purpose::STANDARD.encode(&h.archive),
        fingerprint_b64: Some(base64::engine::general_purpose::STANDARD.encode(&fp)),
    };
    let body = serde_json::to_vec(&req).unwrap();
    let (s, _, b) = send(
        app::api_router(e.svc.clone()),
        signed(&e, &host_key(), "/v1/sign", &body, [0x77; 16]),
    )
    .await;
    let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(s, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["error"]["code"], "reserve_too_small");
    assert!(v["limits"]["reserve_recommended"].is_number());
    assert!(e
        .svc
        .store
        .slot(&r.reservation_id, "mp4")
        .unwrap()
        .is_none());
}
