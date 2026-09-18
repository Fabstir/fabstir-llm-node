// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Shared harness for the HTTPS key-broker client tests: a throwaway CA per
//! test (rcgen), a TLS 1.3 fake broker on 127.0.0.1 driven by a closure, and
//! the pinned client aimed at it. Used by `test_kbs_http.rs` and
//! `test_kbs_http_release.rs`.

use fabstir_llm_node::tee::kbs_http::{ErrorBody, ErrorInner, HttpKeyBrokerClient};
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

pub const HOST: &str = "kbs.test";
pub const MODEL: [u8; 32] = [0xABu8; 32];
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
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let svc = service_fn(move |req: Request<Incoming>| {
                    let handler = handler.clone();
                    async move {
                        let path = req.uri().path().to_string();
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

pub fn err_body(kind: &str, detail: &str) -> Vec<u8> {
    serde_json::to_vec(&ErrorBody {
        error: ErrorInner {
            kind: kind.into(),
            detail: detail.into(),
        },
    })
    .unwrap()
}
