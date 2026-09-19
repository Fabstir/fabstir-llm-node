//! Test harness: a TLS 1.3 fake origin on 127.0.0.1 (counting hits, serving
//! arbitrary status/headers/body per request), a fake PCCS built from the vendored
//! collateral, an axum router served over TLS with `ConnectInfo`, and the
//! `EgressClient` aimed at them through the §12 test hooks. The PKI comes from the
//! node's own broker-client fixture.

#[path = "../tee/kbs_fixture.rs"]
#[allow(dead_code)]
pub mod kbs_fixture;

pub use kbs_fixture::{pki, TestPki, HOST};

use dcap_qvl::QuoteCollateralV3;
use fabstir_llm_node::kbs::egress::{EgressClient, EgressOptions};
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// `(method, path-and-query, body) -> (status, headers, body)`.
pub type FakeHandler =
    Arc<dyn Fn(&str, &str, Vec<u8>) -> (u16, Vec<(String, String)>, Vec<u8>) + Send + Sync>;

pub struct Fake {
    pub addr: SocketAddr,
    /// Every request served: `(method, path-and-query)`.
    pub hits: Arc<Mutex<Vec<(String, String)>>>,
}

impl Fake {
    pub fn base(&self) -> String {
        format!("https://{HOST}:{}", self.addr.port())
    }
    pub fn hit_count(&self) -> usize {
        self.hits.lock().unwrap().len()
    }
    pub fn hits_matching(&self, needle: &str) -> usize {
        self.hits
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, p)| p.contains(needle))
            .count()
    }
}

fn tls_acceptor(p: &TestPki) -> tokio_rustls::TlsAcceptor {
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
    tokio_rustls::TlsAcceptor::from(Arc::new(cfg))
}

/// A TLS fake origin answering every request through `handler`, after `delay`.
pub async fn spawn_fake(p: &TestPki, handler: FakeHandler, delay: Duration) -> Fake {
    let acceptor = tls_acceptor(p);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let hits2 = hits.clone();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            let handler = handler.clone();
            let hits = hits2.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let svc = service_fn(move |req: Request<Incoming>| {
                    let handler = handler.clone();
                    let hits = hits.clone();
                    async move {
                        let method = req.method().to_string();
                        let pq = req
                            .uri()
                            .path_and_query()
                            .map(|x| x.to_string())
                            .unwrap_or_default();
                        let body = req.into_body().collect().await.unwrap().to_bytes().to_vec();
                        hits.lock().unwrap().push((method.clone(), pq.clone()));
                        if !delay.is_zero() {
                            tokio::time::sleep(delay).await;
                        }
                        let (status, headers, out) = handler(&method, &pq, body);
                        let mut resp = Response::builder().status(status);
                        for (k, v) in headers {
                            resp = resp.header(k, v);
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
    Fake { addr, hits }
}

/// An axum router served over TLS; every connection carries `ConnectInfo(peer)`.
pub async fn spawn_tls_axum(p: &TestPki, router: axum::Router) -> SocketAddr {
    let acceptor = tls_acceptor(p);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, peer)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            let router = router.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let svc = service_fn(move |mut req: Request<Incoming>| {
                    let mut r = router.clone();
                    req.extensions_mut()
                        .insert(axum::extract::ConnectInfo(peer));
                    async move { tower::Service::call(&mut r, req).await }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(tls), svc)
                    .await;
            });
        }
    });
    addr
}

/// The egress client aimed at `addr` as `HOST` with `p`'s root, allow-listing
/// `(HOST, port)` and any extra pairs; every extra host is pinned to the same fake.
pub fn egress_for(p: &TestPki, addr: SocketAddr, extra: &[(String, u16)]) -> EgressClient {
    let mut allowed = vec![(HOST.to_string(), addr.port())];
    allowed.extend_from_slice(extra);
    EgressClient::new(
        allowed,
        EgressOptions {
            extra_root_pem: Some(p.root_pem.clone().into_bytes()),
            resolve: Some((HOST.to_string(), addr)),
            resolve_extra: extra.iter().map(|(h, _)| (h.clone(), addr)).collect(),
        },
    )
    .unwrap()
}

/// Percent-encode as PCCS does for the `*-Issuer-Chain` headers.
pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A PCCS handler serving `collateral` exactly as dcap-qvl expects: PCK CRL as DER
/// with its issuer-chain header, TCB info and QE identity as `{..., signature}`
/// JSON with their chain headers, root CA CRL as hex. `overrides` replaces the
/// answer for any path containing its key.
pub fn pccs_handler(
    collateral: QuoteCollateralV3,
    overrides: Vec<(&'static str, (u16, Vec<(String, String)>, Vec<u8>))>,
) -> FakeHandler {
    Arc::new(move |_method, pq, _body| {
        for (needle, ans) in &overrides {
            if pq.contains(needle) {
                return ans.clone();
            }
        }
        let c = &collateral;
        if pq.contains("/pckcrl") {
            (
                200,
                vec![(
                    "SGX-PCK-CRL-Issuer-Chain".into(),
                    urlencode(&c.pck_crl_issuer_chain),
                )],
                c.pck_crl.clone(),
            )
        } else if pq.contains("/tcb?") {
            let body = format!(
                r#"{{"tcbInfo":{},"signature":"{}"}}"#,
                c.tcb_info,
                hex::encode(&c.tcb_info_signature)
            );
            (
                200,
                vec![(
                    "TCB-Info-Issuer-Chain".into(),
                    urlencode(&c.tcb_info_issuer_chain),
                )],
                body.into_bytes(),
            )
        } else if pq.contains("/qe/identity") {
            let body = format!(
                r#"{{"enclaveIdentity":{},"signature":"{}"}}"#,
                c.qe_identity,
                hex::encode(&c.qe_identity_signature)
            );
            (
                200,
                vec![(
                    "SGX-Enclave-Identity-Issuer-Chain".into(),
                    urlencode(&c.qe_identity_issuer_chain),
                )],
                body.into_bytes(),
            )
        } else if pq.contains("/rootcacrl") {
            (200, vec![], hex::encode(&c.root_ca_crl).into_bytes())
        } else {
            (404, vec![], b"no such path".to_vec())
        }
    })
}
