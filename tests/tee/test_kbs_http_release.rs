// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P3 converge round 3 — the two release-time policies of the broker
//! client: a test-keyring release is refused unless the node opted in
//! (`TEE_ACCEPT_TEST_RELEASE`, CPU gate compose only), and `request_key` has
//! its own, longer budget than `challenge` (a burned nonce cannot be retried).
//! Harness in `kbs_fixture.rs`.

use super::kbs_fixture::{
    client, client_with_timeout, pki, spawn_tls_broker, spawn_tls_broker_delayed, DEK, HOST, MODEL,
    NONCE,
};
use fabstir_llm_node::tee::kbs_http::{
    ChallengeResponse, RequestKeyRequest, RequestKeyResponse, WrappedKeyWire,
};
use fabstir_llm_node::tee::key_broker::KeyBrokerClient;
use fabstir_llm_node::tee::keywrap::{generate_ephemeral_keypair, wrap_key};
use fabstir_llm_node::tee::mock::MockAttestationProvider;
use fabstir_llm_node::tee::provider::AttestationProvider;
use fabstir_llm_node::tee::types::{CcMode, Evidence, TeeError};
use hyper::StatusCode;
use std::sync::Arc;
use std::time::Duration;

async fn evidence() -> Evidence {
    let (_sk, pk) = generate_ephemeral_keypair();
    MockAttestationProvider::new("H100", [9u8; 48], CcMode::On)
        .gather_evidence(NONCE, &pk)
        .await
        .unwrap()
}

/// A broker that answers every request_key with a TEST-keyring release.
fn test_keyring_broker() -> super::kbs_fixture::Handler {
    Arc::new(move |path, body| match path {
        "/v1/kbs/challenge" => (
            StatusCode::OK,
            serde_json::to_vec(&ChallengeResponse {
                nonce: hex::encode(NONCE),
                ttl_seconds: 300,
            })
            .unwrap(),
        ),
        _ => {
            let req: RequestKeyRequest = serde_json::from_slice(&body).unwrap();
            let ev = req.evidence.decode().unwrap();
            (
                StatusCode::OK,
                serde_json::to_vec(&RequestKeyResponse {
                    wrapped_key: WrappedKeyWire::encode(&wrap_key(&DEK, &ev.pk_att).unwrap()),
                    test_release: true,
                })
                .unwrap(),
            )
        }
    })
}

#[tokio::test]
async fn a_test_keyring_release_is_refused_unless_opted_in() {
    let p = pki(HOST);
    let addr = spawn_tls_broker(&p, test_keyring_broker()).await;
    let ev = evidence().await;
    // Default (the GPU compose): refused, key never decoded, nothing remembered.
    let kbs = client(&p, addr);
    match kbs.request_key(MODEL, &ev).await {
        Err(TeeError::Kbs(m)) => {
            assert!(m.contains("TEST keyring"), "{m}");
            assert!(m.contains("TEE_ACCEPT_TEST_RELEASE"), "{m}");
        }
        other => panic!("a test release must be refused by default: {other:?}"),
    }
    assert!(!kbs.last_release_was_test());
    // Explicit opt-out is the same as the default.
    assert!(client(&p, addr)
        .with_accept_test_release(false)
        .request_key(MODEL, &ev)
        .await
        .is_err());
    // Opted in (the CPU gate compose): accepted and labelled.
    let kbs = client(&p, addr).with_accept_test_release(true);
    kbs.request_key(MODEL, &ev).await.expect("opted-in release");
    assert!(kbs.last_release_was_test());
}

#[tokio::test]
async fn request_key_has_its_own_budget() {
    // The fake broker takes 1.5 s per request (async sleep, the runtime keeps
    // turning); the client's default budget is 300 ms. challenge times out;
    // request_key, given 10 s, succeeds.
    let p = pki(HOST);
    let addr =
        spawn_tls_broker_delayed(&p, test_keyring_broker(), Duration::from_millis(1500)).await;
    let ev = evidence().await;
    let kbs = client_with_timeout(&p, addr, Duration::from_millis(300))
        .with_request_key_timeout(Duration::from_secs(10))
        .with_accept_test_release(true);
    let (_sk, pk) = generate_ephemeral_keypair();
    match kbs.challenge(MODEL, &pk).await {
        Err(TeeError::Kbs(m)) => assert!(m.contains("timed out") || m.contains("timeout"), "{m}"),
        other => panic!("challenge must hit the 300 ms budget: {other:?}"),
    }
    kbs.request_key(MODEL, &ev)
        .await
        .expect("request_key runs under its own 10 s budget");
}

#[tokio::test]
async fn a_release_without_the_keyring_label_is_refused() {
    // Round 26: `test_release` is required on the wire. A broker build that omits
    // it must not be read as a real-keyring release.
    let p = pki(HOST);
    let addr = spawn_tls_broker(
        &p,
        Arc::new(|path, body| match path {
            "/v1/kbs/challenge" => (
                StatusCode::OK,
                serde_json::to_vec(&ChallengeResponse {
                    nonce: hex::encode(NONCE),
                    ttl_seconds: 300,
                })
                .unwrap(),
            ),
            _ => {
                let req: RequestKeyRequest = serde_json::from_slice(&body).unwrap();
                let ev = req.evidence.decode().unwrap();
                let wrapped = WrappedKeyWire::encode(&wrap_key(&DEK, &ev.pk_att).unwrap());
                // Hand-built body: `wrapped_key` only, no `test_release`.
                (
                    StatusCode::OK,
                    serde_json::to_vec(&serde_json::json!({ "wrapped_key": wrapped })).unwrap(),
                )
            }
        }),
    )
    .await;
    let ev = evidence().await;
    for kbs in [
        client(&p, addr),
        client(&p, addr).with_accept_test_release(true),
    ] {
        match kbs.request_key(MODEL, &ev).await {
            Err(TeeError::Kbs(m)) => assert!(m.contains("test_release"), "{m}"),
            other => panic!("an unlabelled release must be refused: {other:?}"),
        }
        assert!(!kbs.last_release_was_test());
    }
}
