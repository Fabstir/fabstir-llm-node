// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P3 converge round 2 — which roots the policy/blob fetch client
//! trusts. Its own test binary because the native-store scenario sets
//! `SSL_CERT_FILE` for the process (rustls-native-certs reads it), which must
//! not race the rest of `tee_tests`. One test, three phases in a fixed order:
//!
//! 1. a throwaway CA is refused outright (nothing trusts it);
//! 2. the same CA in `SSL_CERT_FILE` (= the container's store) is honoured —
//!    the `rustls-tls-native-roots` feature, without which an LE "ISRG Root YE"
//!    installed via `update-ca-certificates` would be invisible to the node;
//! 3. a different CA passed with `with_extra_root` (the private broker root
//!    from `TEE_KBS_CA_FILE`) is honoured too.

use fabstir_llm_node::tee::http_sources::HttpBlobSource;
use fabstir_llm_node::tee::model_source::BlobSource;
use fabstir_llm_node::tee::types::TeeError;
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

struct Pki {
    root_pem: String,
    leaf_der: Vec<u8>,
    key_der: Vec<u8>,
}

/// A fresh CA and a leaf for the IP literal 127.0.0.1 (no DNS in play).
fn pki(name: &str) -> Pki {
    let mut ca = rcgen::CertificateParams::new(Vec::<String>::new());
    ca.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
    ca.distinguished_name = rcgen::DistinguishedName::new();
    ca.distinguished_name.push(rcgen::DnType::CommonName, name);
    let ca = rcgen::Certificate::from_params(ca).unwrap();
    let mut leaf = rcgen::CertificateParams::new(vec!["127.0.0.1".to_string()]);
    leaf.distinguished_name = rcgen::DistinguishedName::new();
    leaf.distinguished_name
        .push(rcgen::DnType::CommonName, "127.0.0.1");
    let leaf = rcgen::Certificate::from_params(leaf).unwrap();
    Pki {
        root_pem: ca.serialize_pem().unwrap(),
        leaf_der: leaf.serialize_der_with_signer(&ca).unwrap(),
        key_der: leaf.serialize_private_key_der(),
    }
}

/// A TLS server on 127.0.0.1 answering every GET with `body`.
async fn spawn_tls(p: &Pki, body: &'static [u8]) -> SocketAddr {
    let cfg = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::Certificate(p.leaf_der.clone())],
            rustls::PrivateKey(p.key_der.clone()),
        )
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
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let svc = service_fn(move |_req: Request<Incoming>| async move {
                    Ok::<_, std::convert::Infallible>(
                        Response::builder()
                            .status(StatusCode::OK)
                            .body(Full::new(Bytes::from_static(body)))
                            .unwrap(),
                    )
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(tls), svc)
                    .await;
            });
        }
    });
    addr
}

fn source(addr: SocketAddr) -> HttpBlobSource {
    HttpBlobSource::new(
        &format!("https://127.0.0.1:{}", addr.port()),
        1024,
        Duration::from_secs(5),
    )
    .unwrap()
}

#[tokio::test]
async fn fetch_client_trusts_native_store_and_extra_root_but_nothing_else() {
    let native = pki("Test Native Root");
    let private = pki("Test Private Broker Root");
    let native_addr = spawn_tls(&native, b"native").await;
    let private_addr = spawn_tls(&private, b"private").await;

    // 1. Nothing trusts either throwaway CA: refused at the handshake.
    for addr in [native_addr, private_addr] {
        match source(addr).get_file("x.enc").await {
            Err(TeeError::Fetch(m)) => assert!(
                m.contains("certificate") || m.contains("Unknown"),
                "expected a TLS trust failure, got: {m}"
            ),
            other => panic!("an untrusted CA must be refused: {other:?}"),
        }
    }

    // 2. The native store (here: SSL_CERT_FILE, as rustls-native-certs reads
    //    it) now holds the first CA: honoured. The variable is cleared before
    //    the file is removed at the end.
    let dir = std::env::temp_dir().join(format!("tee-fetch-roots-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = dir.join("native-root.pem");
    std::fs::write(&store, native.root_pem.as_bytes()).unwrap();
    std::env::set_var("SSL_CERT_FILE", &store);
    assert_eq!(
        source(native_addr).get_file("x.enc").await.unwrap(),
        b"native".to_vec(),
        "a root in the container's store must be trusted (rustls-tls-native-roots)"
    );
    // ...and that store does NOT vouch for the other CA.
    assert!(
        matches!(
            source(private_addr).get_file("x.enc").await,
            Err(TeeError::Fetch(_))
        ),
        "the native store must not make an unrelated CA trusted"
    );

    // 3. The private broker root, added explicitly: honoured for its own server
    //    while the native root keeps working alongside it.
    let with_root = |addr: SocketAddr| {
        source(addr)
            .with_extra_root(private.root_pem.as_bytes())
            .unwrap()
    };
    assert_eq!(
        with_root(private_addr).get_file("x.enc").await.unwrap(),
        b"private".to_vec()
    );
    assert_eq!(
        with_root(native_addr).get_file("x.enc").await.unwrap(),
        b"native".to_vec()
    );
    // A PEM with no certificate in it is an error, not "no extra root".
    assert!(matches!(
        source(private_addr).with_extra_root(b"not a pem"),
        Err(TeeError::Fetch(_))
    ));
    // 4. An UNUSABLE OS store (SSL_CERT_FILE pointing at a missing file makes
    //    rustls-native-certs fail, and reqwest then fails the client build): a
    //    client with no extra root cannot be built, but one carrying the
    //    private broker root falls back to that root alone and still reaches
    //    the private server. The attested boot never depends on OS-store health.
    std::env::set_var("SSL_CERT_FILE", dir.join("does-not-exist.pem"));
    match HttpBlobSource::new(
        &format!("https://127.0.0.1:{}", private_addr.port()),
        1024,
        Duration::from_secs(5),
    ) {
        Err(TeeError::Fetch(m)) => assert!(m.contains("OS root store unusable"), "{m}"),
        other => panic!("no extra root and an unusable OS store must fail the build: {other:?}"),
    }
    let with_root_only = HttpBlobSource::new_with_extra_root(
        &format!("https://127.0.0.1:{}", private_addr.port()),
        1024,
        Duration::from_secs(5),
        private.root_pem.as_bytes(),
    )
    .expect(
        "the production constructor survives an unusable OS store when it has the private root",
    );
    assert_eq!(
        with_root_only.get_file("x.enc").await.unwrap(),
        b"private".to_vec(),
        "the private root alone carries the fetch when the OS store is unusable"
    );
    // 5. A store that LOADS but holds no parseable certificate: the probe says
    //    Ok, reqwest's own build then fails ("zero valid certificates"); the
    //    production constructor must still fall back to the private root.
    let corrupt = dir.join("corrupt-store.pem");
    std::fs::write(
        &corrupt,
        b"-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n",
    )
    .unwrap();
    std::env::set_var("SSL_CERT_FILE", &corrupt);
    match HttpBlobSource::new(
        &format!("https://127.0.0.1:{}", private_addr.port()),
        1024,
        Duration::from_secs(5),
    ) {
        Err(TeeError::Fetch(m)) => assert!(m.contains("build http client"), "{m}"),
        other => panic!("no extra root and a corrupt OS store must fail the build: {other:?}"),
    }
    let with_root_only = HttpBlobSource::new_with_extra_root(
        &format!("https://127.0.0.1:{}", private_addr.port()),
        1024,
        Duration::from_secs(5),
        private.root_pem.as_bytes(),
    )
    .expect("the production constructor survives a corrupt OS store when it has the private root");
    assert_eq!(
        with_root_only.get_file("x.enc").await.unwrap(),
        b"private".to_vec()
    );
    // The store file is deleted only after the variable no longer points at it:
    // rustls-native-certs errors on a missing SSL_CERT_FILE, which would break any
    // client built later in this process.
    std::env::remove_var("SSL_CERT_FILE");
    let _ = std::fs::remove_dir_all(&dir);
}
