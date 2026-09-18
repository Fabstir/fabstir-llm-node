// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P3.2 — the HTTP policy and blob sources against a plain fake server
//! on loopback (the one place `http://` is allowed). Proven: URL templating and
//! the model-id check on the policy, bounded bodies on both, `encrypted_ref`
//! resolution, and the https-or-loopback rule.

use fabstir_llm_node::tee::http_sources::{HttpBlobSource, HttpPolicySource, MAX_POLICY_BYTES};
use fabstir_llm_node::tee::model_source::BlobSource;
use fabstir_llm_node::tee::policy::SignedModelPolicy;
use fabstir_llm_node::tee::policy_source::PolicySource;
use fabstir_llm_node::tee::types::{CcMode, CvmPolicy, GpuPolicy, Policy, TeeError};
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

type Handler = Arc<dyn Fn(&str) -> (StatusCode, Vec<u8>) + Send + Sync>;

async fn spawn_http(handler: Handler) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                break;
            };
            let handler = handler.clone();
            tokio::spawn(async move {
                let svc = service_fn(move |req: Request<Incoming>| {
                    let handler = handler.clone();
                    async move {
                        let (status, out) = handler(req.uri().path());
                        let mut resp = Response::builder().status(status);
                        if status.is_redirection() {
                            // Every 3xx points back at this server's /redirected.
                            resp = resp.header(
                                "location",
                                format!("http://127.0.0.1:{}/redirected", addr.port()),
                            );
                        }
                        Ok::<_, std::convert::Infallible>(
                            resp.body(Full::new(Bytes::from(out))).unwrap(),
                        )
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(tcp), svc)
                    .await;
            });
        }
    });
    addr
}

const MODEL: [u8; 32] = [0x5Au8; 32];

fn signed(model_id: [u8; 32]) -> SignedModelPolicy {
    SignedModelPolicy {
        policy: Policy {
            schema_version: 2,
            policy_version: 1,
            model_id: model_id,
            not_before: 0,
            expiry: u64::MAX - 1,
            cvm: CvmPolicy {
                mrtd: hex::encode([9u8; 48]),
                rtmr0: "00".repeat(48),
                rtmr1: "00".repeat(48),
                rtmr2: "00".repeat(48),
                os_image_hash: "00".repeat(32),
                compose_hash: "00".repeat(32),
                app_id: None,
                key_provider: None,
                require_td_debug_off: true,
                allowed_tcb_status: vec!["UpToDate".to_string()],
                allowed_advisory_ids: vec![],
            },
            gpu: GpuPolicy {
                allowed_hwmodels: vec!["H100".into()],
                require_cc_mode: Some(CcMode::On),
                require_secure_boot: true,
                require_debug_disabled: true,
                min_driver_version: None,
                min_vbios_version: None,
            },
        },
        encrypted_ref: "models/x.enc".into(),
        signer: "0x0000000000000000000000000000000000000000".into(),
        signature: vec![0u8; 65],
    }
}

#[tokio::test]
async fn policy_url_template_and_model_check() {
    let addr = spawn_http(Arc::new(|path| {
        // Serves the right policy at its own path and the WRONG model at /other.
        if path == format!("/policies/{}.json", hex::encode(MODEL)) {
            (StatusCode::OK, serde_json::to_vec(&signed(MODEL)).unwrap())
        } else if path == "/other" {
            (
                StatusCode::OK,
                serde_json::to_vec(&signed([1u8; 32])).unwrap(),
            )
        } else {
            (StatusCode::NOT_FOUND, b"nope".to_vec())
        }
    }))
    .await;
    let src = HttpPolicySource::new(
        &format!(
            "http://127.0.0.1:{}/policies/{{model_id}}.json",
            addr.port()
        ),
        Duration::from_secs(5),
    )
    .unwrap();
    let got = src.fetch_policy(MODEL).await.expect("templated fetch");
    assert_eq!(got.policy.model_id, MODEL);
    assert_eq!(got.encrypted_ref, "models/x.enc");
    // Unknown model → 404 → Fetch error, fail-closed.
    assert!(matches!(
        src.fetch_policy([7u8; 32]).await,
        Err(TeeError::Fetch(_))
    ));
    // A fixed URL that serves another model's policy is refused by the model check.
    let fixed = HttpPolicySource::new(
        &format!("http://127.0.0.1:{}/other", addr.port()),
        Duration::from_secs(5),
    )
    .unwrap();
    match fixed.fetch_policy(MODEL).await {
        Err(TeeError::VerificationFailed(m)) => assert!(m.contains("is for model"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn policy_body_is_bounded() {
    let addr = spawn_http(Arc::new(|_| {
        (StatusCode::OK, vec![b'{'; MAX_POLICY_BYTES + 1])
    }))
    .await;
    let src = HttpPolicySource::new(
        &format!("http://127.0.0.1:{}/p", addr.port()),
        Duration::from_secs(5),
    )
    .unwrap();
    match src.fetch_policy(MODEL).await {
        Err(TeeError::Fetch(m)) => assert!(m.contains("bound"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn blob_resolves_relative_and_absolute_refs_and_is_bounded() {
    let addr = spawn_http(Arc::new(|path| match path {
        "/blobs/models/x.enc" => (StatusCode::OK, vec![0xEEu8; 1000]),
        // A colon in a path segment is a path, not a scheme (round 24).
        "/blobs/llama-3.1:8b-q4.enc" => (StatusCode::OK, vec![0xBBu8; 7]),
        "/blobs/cid:bafy123" => (StatusCode::OK, vec![0xCCu8; 3]),
        "/blobs/abs.enc" => (StatusCode::OK, vec![0xAAu8; 10]),
        "/abs.enc" => (StatusCode::OK, vec![0xAAu8; 10]), // exists, but off-base
        "/blobs/big.enc" => (StatusCode::OK, vec![0u8; 5000]),
        _ => (StatusCode::NOT_FOUND, Vec::new()),
    }))
    .await;
    let base = format!("http://127.0.0.1:{}/blobs", addr.port());
    let src = HttpBlobSource::new(&base, 4096, Duration::from_secs(5)).unwrap();
    assert_eq!(src.get_file("models/x.enc").await.unwrap().len(), 1000);
    assert_eq!(src.get_file("/models/x.enc").await.unwrap().len(), 1000);
    assert_eq!(
        src.get_file("llama-3.1:8b-q4.enc").await.unwrap(),
        vec![0xBBu8; 7]
    );
    assert_eq!(src.get_file("cid:bafy123").await.unwrap(), vec![0xCCu8; 3]);
    // Relative means under the base: no climbing out, no query, no fragment
    // (round 28; the ref is unsigned).
    for bad in [
        "../abs.enc",
        "models/../../abs.enc",
        // The base directory itself is not an object (round 41).
        "",
        "   ",
        "/",
        "models/..",
        "models/x.enc?x=1",
        "models/x.enc#frag",
        // Encoded and backslash forms the URL parser normalises (round 30).
        "%2e%2e/%2e%2e/abs.enc",
        "models/%2E%2E/../abs.enc",
        "..\\..\\abs.enc",
        ".%2e/abs.enc",
        // Encoded slashes: one segment to the parser, a climb to a server that
        // decodes before dot-segment removal (round 44).
        "..%2F..%2Fabs.enc",
        "models%2f..%2f..%2fabs.enc",
        "models/x%20y.enc", // any percent-encoding is refused, not only slashes
    ] {
        match src.get_file(bad).await {
            Err(TeeError::Fetch(m)) => assert!(
                m.contains("resolve under") || m.contains("is empty"),
                "{bad:?}: {m}"
            ),
            other => panic!("{bad:?}: {other:?}"),
        }
    }
    // A `..` INSIDE a segment name is just a name.
    assert!(matches!(
        src.get_file("models/..x.enc").await,
        Err(TeeError::Fetch(m)) if m.contains("HTTP 404")
    ));
    // An absolute ref is accepted only UNDER the base path (round 38): the same
    // origin alone is not enough, an unsigned ref must not reach other objects.
    let abs = format!("http://127.0.0.1:{}/blobs/abs.enc", addr.port());
    assert_eq!(src.get_file(&abs).await.unwrap(), vec![0xAAu8; 10]);
    match src
        .get_file(&format!("http://127.0.0.1:{}/abs.enc", addr.port()))
        .await
    {
        Err(TeeError::Fetch(m)) => assert!(m.contains("resolve under"), "{m}"),
        other => panic!("off-base absolute ref must be refused: {other:?}"),
    }
    match src
        .get_file(&format!("http://127.0.0.1:{}/blobs/big.enc", addr.port()))
        .await
    {
        Err(TeeError::Fetch(m)) => assert!(m.contains("bound"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        src.get_file("missing.enc").await,
        Err(TeeError::Fetch(_))
    ));
}

#[tokio::test]
async fn redirects_are_not_followed() {
    // A 302 to a valid policy would be followed by a default client; here it
    // must surface as the 3xx itself (a redirect to http:// would otherwise
    // silently undo the https rule).
    let addr = spawn_http(Arc::new(|path| match path {
        "/redirected" => (StatusCode::OK, serde_json::to_vec(&signed(MODEL)).unwrap()),
        _ => (StatusCode::FOUND, Vec::new()),
    }))
    .await;
    let src = HttpPolicySource::new(
        &format!("http://127.0.0.1:{}/p", addr.port()),
        Duration::from_secs(5),
    )
    .unwrap();
    match src.fetch_policy(MODEL).await {
        Err(TeeError::Fetch(m)) => assert!(m.contains("HTTP 302"), "{m}"),
        other => panic!("{other:?}"),
    }
    let blobs = HttpBlobSource::new(
        &format!("http://127.0.0.1:{}", addr.port()),
        4096,
        Duration::from_secs(5),
    )
    .unwrap();
    match blobs.get_file("x.enc").await {
        Err(TeeError::Fetch(m)) => assert!(m.contains("HTTP 302"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn plaintext_is_allowed_only_to_loopback() {
    assert!(HttpPolicySource::new("http://127.0.0.1:1/p", Duration::from_secs(1)).is_ok());
    assert!(HttpPolicySource::new("http://localhost:1/p", Duration::from_secs(1)).is_ok());
    assert!(HttpPolicySource::new("http://[::1]:1/p", Duration::from_secs(1)).is_ok());
    assert!(HttpPolicySource::new("http://[::1]/p", Duration::from_secs(1)).is_ok());
    for bad in [
        "http://policies.example.com/p",
        "http://[::2]:1/p",
        "http://[::1].example.com/p",
        "http://localhost.example.com/p",
        "http://127.0.0.1.example.com/p",
        "http://evil.example.com/127.0.0.1/p",
        "http://evil.example.com/[::1]/p",
        "http://localhost:1@evil.example.com/p",
        "http://127.0.0.1@evil.example.com/p",
        "not a url",
    ] {
        assert!(
            matches!(
                HttpPolicySource::new(bad, Duration::from_secs(1)),
                Err(TeeError::Fetch(_))
            ),
            "{bad} must be refused"
        );
    }
    assert!(HttpBlobSource::new("https://s5.platformlessai.ai", 1, Duration::from_secs(1)).is_ok());
    // A base with a query or fragment would corrupt every joined ref: refused up front.
    for bad_base in [
        "https://s5.platformlessai.ai/bucket?token=abc",
        "https://s5.platformlessai.ai/bucket#x",
    ] {
        match HttpBlobSource::new(bad_base, 1, Duration::from_secs(1)) {
            Err(TeeError::Fetch(m)) => assert!(m.contains("no query or fragment"), "{m}"),
            other => panic!("{bad_base}: {other:?}"),
        }
    }
    assert!(matches!(
        HttpBlobSource::new("http://s5.platformlessai.ai", 1, Duration::from_secs(1)),
        Err(TeeError::Fetch(_))
    ));
    // An absolute plaintext encrypted_ref to a non-loopback host is refused too.
    let src =
        HttpBlobSource::new("https://s5.platformlessai.ai", 1, Duration::from_secs(1)).unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    assert!(matches!(
        rt.block_on(src.get_file("http://evil.example.com/x.enc")),
        Err(TeeError::Fetch(_))
    ));
    // And an absolute https encrypted_ref off the TEE_BLOB_URL origin is refused
    // before any connection: encrypted_ref is unsigned, so it must not be able
    // to send the node to an arbitrary host for a multi-GB download.
    for off_origin in [
        "https://evil.example.com/x.enc",
        "HTTPS://evil.example.com/x.enc", // scheme case must not demote it to a relative path
        "https://s5.platformlessai.ai:8443/x.enc",
        "https://s5.platformlessai.ai.evil.example.com/x.enc",
    ] {
        match rt.block_on(src.get_file(off_origin)) {
            Err(TeeError::Fetch(m)) => assert!(m.contains("origin"), "{off_origin}: {m}"),
            other => panic!("{off_origin}: {other:?}"),
        }
    }
    // A `scheme://host` reference in another scheme (the S5 source's `s5://…`)
    // is refused with a message naming the scheme, never appended to the base.
    for foreign in ["s5://blob", "S5://blob/x.enc", "ipfs://bafy123"] {
        match rt.block_on(src.get_file(foreign)) {
            Err(TeeError::Fetch(m)) => assert!(
                m.contains("HTTP blob source cannot fetch"),
                "{foreign}: {m}"
            ),
            other => panic!("{foreign}: {other:?}"),
        }
    }
    // Same origin spelled with the default port or different case still resolves
    // (it then fails on the network, not on the origin check).
    for same in [
        "https://S5.platformlessai.ai/x.enc",
        "HTTPS://s5.platformlessai.ai/x.enc",
        "https://s5.platformlessai.ai:443/x.enc",
    ] {
        match rt.block_on(src.get_file(same)) {
            Err(TeeError::Fetch(m)) => assert!(!m.contains("origin"), "{same}: {m}"),
            Ok(_) => {}
            other => panic!("{same}: {other:?}"),
        }
    }
}
