// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Shared harness for the HTTPS key-broker client tests: a throwaway CA per
//! test (rcgen), a TLS 1.3 fake broker on 127.0.0.1 driven by a closure, and
//! the pinned client aimed at it. Used by `test_kbs_http.rs` and
//! `test_kbs_http_release.rs`.

use fabstir_llm_node::tee::kbs_http::{
    ErrorBody, ErrorInner, HttpKeyBrokerClient, RequestKeyRequest, RequestKeyResponse,
    WrappedKeyWire,
};
use fabstir_llm_node::tee::keywrap::wrap_key;
use fabstir_llm_node::tee::types::TEST_ID_PREFIX;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

pub const HOST: &str = "kbs.test";
/// A REAL-class model id (no `t5t:` prefix).
pub const MODEL: [u8; 32] = [0xABu8; 32];
/// A TEST-class model id: `t5t:` + 28 bytes (P4.5 witness rule).
pub const TEST_MODEL: [u8; 32] = {
    let mut m = [0xABu8; 32];
    m[0] = TEST_ID_PREFIX[0];
    m[1] = TEST_ID_PREFIX[1];
    m[2] = TEST_ID_PREFIX[2];
    m[3] = TEST_ID_PREFIX[3];
    m
};
/// `(method, path)` of every request a hit-recording broker served.
pub type Hits = Arc<std::sync::Mutex<Vec<(String, String)>>>;
pub const NONCE: [u8; 32] = [0x11u8; 32];
pub const DEK: [u8; 32] = [0xCDu8; 32];

/// A CA and a leaf for `host`, as PEM root + DER chain/key for the server.
pub struct TestPki {
    pub root_pem: String,
    pub leaf_der: Vec<u8>,
    pub key_der: Vec<u8>,
}

pub fn pki(host: &str) -> TestPki {
    let mut ca = rcgen::CertificateParams::new(Vec::<String>::new());
    ca.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
    ca.distinguished_name = rcgen::DistinguishedName::new();
    ca.distinguished_name
        .push(rcgen::DnType::CommonName, "Test KBS Root");
    let ca = rcgen::Certificate::from_params(ca).unwrap();
    let mut leaf = rcgen::CertificateParams::new(vec![host.to_string()]);
    leaf.distinguished_name = rcgen::DistinguishedName::new();
    leaf.distinguished_name
        .push(rcgen::DnType::CommonName, host);
    let leaf = rcgen::Certificate::from_params(leaf).unwrap();
    TestPki {
        root_pem: ca.serialize_pem().unwrap(),
        leaf_der: leaf.serialize_der_with_signer(&ca).unwrap(),
        key_der: leaf.serialize_private_key_der(),
    }
}

/// `(path, request body) -> (status, response body)`.
pub type Handler = Arc<dyn Fn(&str, Vec<u8>) -> (StatusCode, Vec<u8>) + Send + Sync>;

/// A TLS 1.3 fake broker on 127.0.0.1; returns its address. A 3xx status gets
/// a `location` header so redirect handling can be exercised.
pub async fn spawn_tls_broker(p: &TestPki, handler: Handler) -> SocketAddr {
    spawn_tls_broker_delayed(p, handler, Duration::ZERO).await
}

/// As [`spawn_tls_broker`], but every request is answered only after `delay`
/// (an async sleep, so a slow broker never blocks the test runtime's workers).
pub async fn spawn_tls_broker_delayed(
    p: &TestPki,
    handler: Handler,
    delay: Duration,
) -> SocketAddr {
    spawn_tls_broker_full(p, handler, delay, None).await
}

/// As [`spawn_tls_broker`], recording `(method, path)` of every request into
/// `hits` (P4.5: the method is what tells a `GET /info` from a `POST`).
pub async fn spawn_tls_broker_with_hits(p: &TestPki, handler: Handler, hits: Hits) -> SocketAddr {
    spawn_tls_broker_full(p, handler, Duration::ZERO, Some(hits)).await
}

async fn spawn_tls_broker_full(
    p: &TestPki,
    handler: Handler,
    delay: Duration,
    hits: Option<Hits>,
) -> SocketAddr {
    let certs = vec![rustls::Certificate(p.leaf_der.clone())];
    let key = rustls::PrivateKey(p.key_der.clone());
    let cfg = rustls::ServerConfig::builder()
        .with_safe_default_cipher_suites()
        .with_safe_default_kx_groups()
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(cfg));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            let handler = handler.clone();
            let hits = hits.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let svc = service_fn(move |req: Request<Incoming>| {
                    let handler = handler.clone();
                    let hits = hits.clone();
                    async move {
                        let path = req.uri().path().to_string();
                        if let Some(h) = &hits {
                            h.lock()
                                .unwrap()
                                .push((req.method().to_string(), path.clone()));
                        }
                        let body = req.into_body().collect().await.unwrap().to_bytes().to_vec();
                        if !delay.is_zero() {
                            tokio::time::sleep(delay).await;
                        }
                        let (status, out) = handler(&path, body);
                        let mut resp = Response::builder()
                            .status(status)
                            .header("content-type", "application/json");
                        if status.is_redirection() {
                            resp = resp.header("location", "/v1/kbs/challenge");
                        }
                        Ok::<_, std::convert::Infallible>(
                            resp.body(Full::new(Bytes::from(out))).unwrap(),
                        )
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(tls), svc)
                    .await;
            });
        }
    });
    addr
}

/// A TLS 1.3 listener that answers each connection with the RAW bytes of
/// `responses[i]` (the i-th connection) after reading the request head, then
/// closes: for shapes hyper will not produce, such as a 200 whose declared body
/// is longer than what is sent (a reset mid-stream). Records one hit per
/// connection.
pub async fn spawn_tls_raw(p: &TestPki, responses: Vec<Vec<u8>>, hits: Hits) -> SocketAddr {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let certs = vec![rustls::Certificate(p.leaf_der.clone())];
    let key = rustls::PrivateKey(p.key_der.clone());
    let cfg = rustls::ServerConfig::builder()
        .with_safe_default_cipher_suites()
        .with_safe_default_kx_groups()
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(cfg));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut i = 0usize;
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                break;
            };
            let Ok(mut tls) = acceptor.accept(tcp).await else {
                continue;
            };
            let raw = responses.get(i).cloned().unwrap_or_default();
            i += 1;
            // read the request head (enough to record the method and path)
            let mut buf = vec![0u8; 4096];
            let n = tls.read(&mut buf).await.unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]);
            let mut words = head.split_whitespace();
            hits.lock().unwrap().push((
                words.next().unwrap_or("").to_string(),
                words.next().unwrap_or("").to_string(),
            ));
            let _ = tls.write_all(&raw).await;
            let _ = tls.shutdown().await;
        }
    });
    addr
}

/// The pinned client for `p`'s root, resolving `HOST` to `addr`.
pub fn client(p: &TestPki, addr: SocketAddr) -> HttpKeyBrokerClient {
    client_with_timeout(p, addr, Duration::from_secs(5))
}

pub fn client_with_timeout(
    p: &TestPki,
    addr: SocketAddr,
    timeout: Duration,
) -> HttpKeyBrokerClient {
    HttpKeyBrokerClient::new(
        &format!("https://{HOST}:{}/v1/kbs", addr.port()),
        p.root_pem.as_bytes(),
        Some((HOST.to_string(), addr)),
        timeout,
    )
    .expect("client")
}

/// A broker answering `GET /v1/kbs/info` with `body` (any status/body pair) and
/// nothing else (404). The `/info` arm sits BEFORE the catch-all on purpose: a
/// `RequestKeyRequest`-parsing catch-all would panic the server task on a GET.
pub fn info_broker(status: StatusCode, body: Vec<u8>) -> Handler {
    Arc::new(move |path, _| match path {
        "/v1/kbs/info" => (status, body.clone()),
        _ => (StatusCode::NOT_FOUND, b"no such route".to_vec()),
    })
}

/// A broker that answers every `challenge` with `NONCE` and every `request_key`
/// with `DEK` wrapped to the evidence's `pk_att`, labelled `test_release` as given.
pub fn release_broker(test_release: bool) -> Handler {
    Arc::new(move |path, body| match path {
        "/v1/kbs/challenge" => (
            StatusCode::OK,
            serde_json::to_vec(&fabstir_llm_node::tee::kbs_http::ChallengeResponse {
                nonce: hex::encode(NONCE),
                ttl_seconds: 300,
            })
            .unwrap(),
        ),
        "/v1/kbs/request_key" => {
            let req: RequestKeyRequest = serde_json::from_slice(&body).unwrap();
            let ev = req.evidence.decode().unwrap();
            (
                StatusCode::OK,
                serde_json::to_vec(&RequestKeyResponse {
                    wrapped_key: WrappedKeyWire::encode(&wrap_key(&DEK, &ev.pk_att).unwrap()),
                    test_release,
                })
                .unwrap(),
            )
        }
        _ => (StatusCode::NOT_FOUND, b"no such route".to_vec()),
    })
}

pub fn err_body(kind: &str, detail: &str) -> Vec<u8> {
    serde_json::to_vec(&ErrorBody {
        error: ErrorInner {
            kind: kind.into(),
            detail: detail.into(),
        },
    })
    .unwrap()
}
