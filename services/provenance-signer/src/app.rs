// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! HTTP: three listeners (D12). Host API (authenticated), public repository (read-only, uniform 404, no
//! listing), internal index API (detector IP + shared secret). Request bodies are capped before parsing.
#![allow(clippy::result_large_err)] // the Err side is an axum Response, returned as is

use crate::service::{Fail, Service};
use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, HeaderValue, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use fabstir_provenance::contract::{self, ErrorBody, ErrorCode};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

pub type Svc = Arc<Service>;

fn error(code: ErrorCode, msg: &str) -> Response {
    let status = StatusCode::from_u16(code.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, axum::Json(ErrorBody::new(code, msg))).into_response()
}

fn fail(e: Fail) -> Response {
    error(e.0, &e.1)
}

/// `reserve_out_of_range` / `reserve_too_small` carry the current limits (contract section 8).
fn fail_with_limits(svc: &Svc, e: Fail) -> Response {
    if matches!(
        e.0,
        ErrorCode::ReserveOutOfRange | ErrorCode::ReserveTooSmall
    ) {
        let mut b = ErrorBody::new(e.0, &e.1);
        b.limits = Some(svc.limits());
        let status = StatusCode::from_u16(e.0.status()).unwrap_or(StatusCode::CONFLICT);
        return (status, axum::Json(b)).into_response();
    }
    fail(e)
}

fn not_found() -> Response {
    error(ErrorCode::NotFound, "not found")
}

fn header<'a>(h: &'a HeaderMap, name: &str) -> &'a str {
    h.get(name).and_then(|v| v.to_str().ok()).unwrap_or("")
}

/// Read the body under `cap`, authenticate. Returns (host, body).
async fn authed(svc: &Svc, req: Request<Body>, cap: usize) -> Result<(String, Vec<u8>), Response> {
    let method = req.method().as_str().to_string();
    let target = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| req.uri().path().to_string());
    let headers = req.headers().clone();
    if let Some(len) = headers
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
    {
        if len > cap {
            return Err(error(ErrorCode::BodyTooLarge, "body too large"));
        }
    }
    let body = to_bytes(req.into_body(), cap)
        .await
        .map_err(|_| error(ErrorCode::BodyTooLarge, "body too large"))?
        .to_vec();
    // ecrecover, keccak of up to 1 MiB and a SQLite write: off the async workers
    let (ts, nonce, sig) = (
        header(&headers, contract::HDR_TIMESTAMP).to_string(),
        header(&headers, contract::HDR_NONCE).to_string(),
        header(&headers, contract::HDR_SIGNATURE).to_string(),
    );
    let s2 = svc.clone();
    let (host, body) = blocking(move || {
        let h = s2.authenticate(&method, &target, &ts, &nonce, &sig, &body);
        h.map(|h| (h, body))
    })
    .await?
    .map_err(fail)?;
    Ok((host, body))
}

fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, Response> {
    serde_json::from_slice(body)
        .map_err(|_| error(ErrorCode::MalformedRequest, "malformed request"))
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, Response> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| error(ErrorCode::Internal, "internal"))
}

async fn limits(State(svc): State<Svc>) -> Response {
    axum::Json(svc.limits()).into_response()
}

async fn reserve(State(svc): State<Svc>, req: Request<Body>) -> Response {
    let (host, body) = match authed(&svc, req, contract::MAX_RESERVE_BODY).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let r: contract::ReserveRequest = match parse(&body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    match blocking(move || svc.reserve(&host, &r)).await {
        Ok(Ok(v)) => axum::Json(v).into_response(),
        Ok(Err(e)) => fail(e),
        Err(e) => e,
    }
}

fn signed_response(s: crate::service::Signed, epoch: u64, content_type: &'static str) -> Response {
    let mut r = (StatusCode::OK, s.manifest).into_response();
    let h = r.headers_mut();
    h.insert("content-type", HeaderValue::from_static(content_type));
    let set = |h: &mut HeaderMap, k: &'static str, v: String| {
        if let Ok(v) = HeaderValue::from_str(&v) {
            h.insert(k, v);
        }
    };
    set(
        h,
        contract::HDR_MANIFEST_ID,
        format!("urn:c2pa:{}", s.manifest_uuid),
    );
    set(h, contract::HDR_MANIFEST_SHA256, s.sha256);
    set(h, contract::HDR_CHAIN_EPOCH, epoch.to_string());
    set(
        h,
        contract::HDR_REPLAY,
        if s.replay { "1".into() } else { "0".into() },
    );
    if let Some(t) = s.tsa.and_then(|u| url_host(&u)) {
        set(h, contract::HDR_TSA, t);
    }
    r
}

fn url_host(u: &str) -> Option<String> {
    let rest = u.split("://").nth(1)?;
    Some(rest.split(['/', ':']).next()?.to_string())
}

async fn sign(State(svc): State<Svc>, req: Request<Body>) -> Response {
    let (host, body) = match authed(&svc, req, contract::MAX_SIGN_BODY).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let r: contract::SignRequest = match parse(&body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let epoch = svc.settings.chain_epoch;
    let s2 = svc.clone();
    match blocking(move || s2.sign_mp4(&host, &body, &r)).await {
        Ok(Ok(s)) => signed_response(s, epoch, "application/octet-stream"),
        Ok(Err(e)) => fail_with_limits(&svc, e),
        Err(e) => e,
    }
}

async fn sign_sequence(State(svc): State<Svc>, req: Request<Body>) -> Response {
    let (host, body) = match authed(&svc, req, contract::MAX_SEQUENCE_BODY).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let r: contract::SignSequenceRequest = match parse(&body) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let epoch = svc.settings.chain_epoch;
    match blocking(move || svc.sign_sequence(&host, &body, &r)).await {
        Ok(Ok(s)) => signed_response(s, epoch, "application/c2pa"),
        Ok(Err(e)) => fail(e),
        Err(e) => e,
    }
}

pub fn api_router(svc: Svc) -> Router {
    Router::new()
        .route("/v1/limits", get(limits))
        .route("/v1/reserve", post(reserve))
        .route("/v1/sign", post(sign))
        .route("/v1/sign-sequence", post(sign_sequence))
        .fallback(|| async { not_found() })
        // bounded work per box; slow clients are cut by the TLS proxy's header/body timeouts (deploy/)
        .layer(tower::limit::GlobalConcurrencyLimitLayer::new(64))
        .with_state(svc)
}

// ------------------------------------------------------------------------------------- public

fn public_bytes(bytes: Vec<u8>, content_type: &'static str) -> Response {
    let mut r = (StatusCode::OK, bytes).into_response();
    r.headers_mut()
        .insert("content-type", HeaderValue::from_static(content_type));
    r.headers_mut().insert(
        "cache-control",
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    r
}

async fn manifest(State(svc): State<Svc>, Path(id): Path<String>) -> Response {
    match blocking(move || svc.public_manifest(&id)).await {
        Ok(Some((bytes, _))) => public_bytes(bytes, "application/c2pa"),
        _ => not_found(),
    }
}

async fn frames_index(State(svc): State<Svc>, Path(id): Path<String>) -> Response {
    match blocking(move || svc.public_frames_index(&id)).await {
        Ok(Some(bytes)) => public_bytes(bytes, "application/json"),
        _ => not_found(),
    }
}

pub fn public_router(svc: Svc) -> Router {
    Router::new()
        .route("/v1/manifests/:id", get(manifest))
        .route("/v1/manifests/:id/frames-index", get(frames_index))
        .fallback(|| async { not_found() })
        .layer(tower::limit::GlobalConcurrencyLimitLayer::new(256))
        .with_state(svc)
}

// ------------------------------------------------------------------------------------- index

#[derive(Clone)]
pub struct IndexGate {
    pub secret: Arc<Vec<u8>>,
    pub allowed: Arc<Vec<IpAddr>>,
    /// At most this many matches run at once (brute force over the registry); more get 429.
    pub permits: Arc<tokio::sync::Semaphore>,
}

impl IndexGate {
    pub fn new(secret: Vec<u8>, allowed: Vec<IpAddr>) -> IndexGate {
        IndexGate {
            secret: Arc::new(secret),
            allowed: Arc::new(allowed),
            permits: Arc::new(tokio::sync::Semaphore::new(2)),
        }
    }
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub const MAX_INDEX_QUERY: usize = 8 * 1024 * 1024;

async fn index_match(
    State((svc, gate)): State<(Svc, IndexGate)>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request<Body>,
) -> Response {
    let ip = match peer.ip() {
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
        v4 => v4,
    };
    let given = req
        .headers()
        .get(contract::HDR_INDEX_SECRET)
        .map(|v| v.as_bytes().to_vec())
        .unwrap_or_default();
    if gate.secret.is_empty() || !gate.allowed.contains(&ip) || !ct_eq(&given, &gate.secret) {
        return not_found();
    }
    let body = match to_bytes(req.into_body(), MAX_INDEX_QUERY).await {
        Ok(b) => b.to_vec(),
        Err(_) => return error(ErrorCode::BodyTooLarge, "body too large"),
    };
    let Ok(_permit) = gate.permits.clone().try_acquire_owned() else {
        return error(ErrorCode::RateLimited, "busy");
    };
    match blocking(move || svc.index_match(&body)).await {
        Ok(Ok(m)) => axum::Json(serde_json::json!({ "matches": m })).into_response(),
        Ok(Err(e)) => fail(e),
        Err(e) => e,
    }
}

pub fn index_router(svc: Svc, gate: IndexGate) -> Router {
    Router::new()
        .route("/v1/index/match", post(index_match))
        .fallback(|| async { not_found() })
        .with_state((svc, gate))
}
