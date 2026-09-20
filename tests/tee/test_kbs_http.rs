// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P3.1 — the HTTPS key-broker client against a fake broker behind a
//! real TLS 1.3 handshake. A throwaway CA is minted per test (rcgen); the
//! client pins it and nothing else, so what is proven here is exactly what
//! protects the node on the day: chain to the pinned root, hostname match,
//! TLS 1.3, bounded bodies, the frozen wire format, and the error mapping.
//! Harness in `kbs_fixture.rs`.

use super::kbs_fixture::{
    client, err_body, pki, spawn_tls_broker, DEK, HOST, MODEL, NONCE, TEST_MODEL,
};
use fabstir_llm_node::tee::kbs_http::{
    ChallengeRequest, ChallengeResponse, EvidenceWire, HttpKeyBrokerClient, RequestKeyRequest,
    RequestKeyResponse, WrappedKeyWire, MAX_BODY,
};
use fabstir_llm_node::tee::key_broker::KeyBrokerClient;
use fabstir_llm_node::tee::keywrap::{generate_ephemeral_keypair, unwrap_key, wrap_key};
use fabstir_llm_node::tee::mock::MockAttestationProvider;
use fabstir_llm_node::tee::provider::AttestationProvider;
use fabstir_llm_node::tee::types::{CcMode, Evidence, TeeError};
use hyper::StatusCode;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn challenge_and_request_key_round_trip_over_pinned_tls13() {
    let p = pki(HOST);
    // The fake broker checks the wire shapes and wraps DEK to the pk_att it sees.
    let addr = spawn_tls_broker(
        &p,
        Arc::new(|path, body| match path {
            "/v1/kbs/challenge" => {
                let req: ChallengeRequest = serde_json::from_slice(&body).unwrap();
                assert_eq!(req.model_id, hex::encode(MODEL));
                assert_eq!(hex::decode(&req.pk_att).unwrap().len(), 33);
                (
                    StatusCode::OK,
                    serde_json::to_vec(&ChallengeResponse {
                        nonce: hex::encode(NONCE),
                        ttl_seconds: 123,
                    })
                    .unwrap(),
                )
            }
            "/v1/kbs/request_key" => {
                let req: RequestKeyRequest = serde_json::from_slice(&body).unwrap();
                let ev = req.evidence.decode().expect("wire evidence decodes");
                assert_eq!(ev.nonce, NONCE);
                assert_eq!(
                    &ev.cpu_quote[32..64],
                    &NONCE,
                    "mock quote carries the nonce"
                );
                let wrapped = wrap_key(&DEK, &ev.pk_att).unwrap();
                (
                    StatusCode::OK,
                    serde_json::to_vec(&RequestKeyResponse {
                        wrapped_key: WrappedKeyWire::encode(&wrapped),
                        test_release: false,
                    })
                    .unwrap(),
                )
            }
            other => (StatusCode::NOT_FOUND, format!("no {other}").into_bytes()),
        }),
    )
    .await;
    let kbs = client(&p, addr);
    let (sk, pk) = generate_ephemeral_keypair();
    assert_eq!(
        kbs.challenge_nonce_ttl_seconds(),
        300,
        "default before any challenge"
    );
    let nonce = kbs.challenge(MODEL, &pk).await.expect("challenge");
    assert_eq!(nonce, NONCE);
    assert_eq!(
        kbs.challenge_nonce_ttl_seconds(),
        123,
        "the TTL the broker minted, not a constant"
    );
    let ev = MockAttestationProvider::new("H100", [9u8; 48], CcMode::On)
        .gather_evidence(nonce, &pk)
        .await
        .unwrap();
    let wrapped = kbs.request_key(MODEL, &ev).await.expect("request_key");
    assert_eq!(unwrap_key(&wrapped, &sk).unwrap(), DEK);
    assert!(!kbs.last_release_was_test());
}

#[tokio::test]
async fn evidence_wire_round_trips_byte_exact() {
    let ev = Evidence {
        gpu_report: br#"{"nonce":"11","evidence_list":[{"certificate":"YQ==","evidence":"Yg==","arch":"HOPPER"}],"arch":"HOPPER"}"#.to_vec(),
        cpu_quote: vec![4, 0, 2, 0, 0x81, 0, 0, 0, 0xFF],
        event_log: br#"[{"imr":3,"event":"compose-hash"}]"#.to_vec(),
        vm_config: br#"{"cpu_count":4}"#.to_vec(),
        image_measurement: [7u8; 48],
        pk_att: vec![2u8; 33],
        nonce: NONCE,
    };
    let wire = EvidenceWire::encode(&ev).unwrap();
    assert!(wire.gpu_report_b64.ends_with('=') || !wire.gpu_report_b64.is_empty());
    assert_eq!(wire.cpu_quote_hex, "04000200810000 00ff".replace(' ', ""));
    let back = wire.decode().unwrap();
    assert_eq!(back, ev, "encode/decode must be lossless on every field");
    // Wrong sizes are refused on decode (the broker side of the contract).
    let mut bad = wire.clone();
    bad.pk_att_hex = hex::encode([2u8; 65]);
    assert!(matches!(bad.decode(), Err(TeeError::Kbs(_))));
    let mut bad = wire.clone();
    bad.nonce_hex = "zz".into();
    assert!(matches!(bad.decode(), Err(TeeError::Kbs(_))));
}

#[tokio::test]
async fn test_release_flag_is_remembered_and_the_key_still_unwraps() {
    let p = pki(HOST);
    let addr = spawn_tls_broker(
        &p,
        Arc::new(|_, body| {
            let req: RequestKeyRequest = serde_json::from_slice(&body).unwrap();
            let ev = req.evidence.decode().unwrap();
            let wrapped = wrap_key(&DEK, &ev.pk_att).unwrap();
            (
                StatusCode::OK,
                serde_json::to_vec(&RequestKeyResponse {
                    wrapped_key: WrappedKeyWire::encode(&wrapped),
                    test_release: true,
                })
                .unwrap(),
            )
        }),
    )
    .await;
    // Accepted only because this client opted in (the CPU gate compose does) AND
    // the id carries the `t5t:` prefix (P4.5 witness rule).
    let kbs = client(&p, addr).with_accept_test_release(true);
    let (sk, pk) = generate_ephemeral_keypair();
    let ev = MockAttestationProvider::new("H100", [9u8; 48], CcMode::On)
        .gather_evidence(NONCE, &pk)
        .await
        .unwrap();
    let wrapped = kbs.request_key(TEST_MODEL, &ev).await.unwrap();
    assert_eq!(unwrap_key(&wrapped, &sk).unwrap(), DEK);
    assert!(
        kbs.last_release_was_test(),
        "a canned-mode release is labelled"
    );
}

#[tokio::test]
async fn broker_errors_map_to_the_contract_variants() {
    let p = pki(HOST);
    let addr = spawn_tls_broker(
        &p,
        Arc::new(|_, body| {
            let req: ChallengeRequest = serde_json::from_slice(&body).unwrap();
            // The fake broker chooses the error from the model id's first byte.
            match hex::decode(&req.model_id).unwrap()[0] {
                1 => (StatusCode::FORBIDDEN, err_body("freshness", "stale")),
                2 => (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    err_body("verification", "rtmr0 mismatch"),
                ),
                3 => (
                    StatusCode::NOT_FOUND,
                    err_body("no_provider", "unknown model"),
                ),
                4 => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    err_body("unavailable", "nras down"),
                ),
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    b"<html>oops</html>".to_vec(),
                ),
            }
        }),
    )
    .await;
    let kbs = client(&p, addr);
    let (_sk, pk) = generate_ephemeral_keypair();
    let call = |b: u8| {
        let mut m = [0u8; 32];
        m[0] = b;
        m
    };
    assert!(matches!(
        kbs.challenge(call(1), &pk).await,
        Err(TeeError::FreshnessFailure)
    ));
    match kbs.challenge(call(2), &pk).await {
        Err(TeeError::VerificationFailed(m)) => assert!(m.contains("rtmr0"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        kbs.challenge(call(3), &pk).await,
        Err(TeeError::VerificationFailed(_))
    ));
    match kbs.challenge(call(4), &pk).await {
        Err(TeeError::Kbs(m)) => assert!(m.contains("unavailable"), "{m}"),
        other => panic!("{other:?}"),
    }
    match kbs.challenge(call(9), &pk).await {
        Err(TeeError::Kbs(m)) => assert!(m.contains("500"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_server_certificate_from_another_ca_is_refused() {
    // Same hostname, valid chain to a DIFFERENT root: the node must not accept
    // it. This is the whole point of pinning with the built-in roots off.
    let trusted = pki(HOST);
    let other = pki(HOST);
    let addr = spawn_tls_broker(&other, Arc::new(|_, _| (StatusCode::OK, b"{}".to_vec()))).await;
    let kbs = client(&trusted, addr);
    let (_sk, pk) = generate_ephemeral_keypair();
    match kbs.challenge(MODEL, &pk).await {
        Err(TeeError::Kbs(m)) => assert!(
            m.to_lowercase().contains("certificate")
                || m.to_lowercase().contains("tls")
                || m.contains("error"),
            "{m}"
        ),
        other => panic!("a foreign CA must be refused, got {other:?}"),
    }
}

#[tokio::test]
async fn a_certificate_for_another_hostname_is_refused() {
    let p = pki("other.test");
    let addr = spawn_tls_broker(&p, Arc::new(|_, _| (StatusCode::OK, b"{}".to_vec()))).await;
    // Pins the right root, but the leaf names other.test while we speak to kbs.test.
    let kbs = client(&p, addr);
    let (_sk, pk) = generate_ephemeral_keypair();
    assert!(matches!(
        kbs.challenge(MODEL, &pk).await,
        Err(TeeError::Kbs(_))
    ));
}

#[tokio::test]
async fn oversized_bodies_are_refused_not_truncated() {
    let p = pki(HOST);
    let addr = spawn_tls_broker(
        &p,
        Arc::new(|_, _| (StatusCode::OK, vec![b' '; MAX_BODY + 1])),
    )
    .await;
    let kbs = client(&p, addr);
    let (_sk, pk) = generate_ephemeral_keypair();
    match kbs.challenge(MODEL, &pk).await {
        Err(TeeError::Kbs(m)) => assert!(m.contains("bound"), "{m}"),
        other => panic!("{other:?}"),
    }
    // A proxy's oversized 502 page: the bound still refuses it, and the log line
    // still says 502 (round 9: the status was lost behind the bound message).
    let addr = spawn_tls_broker(
        &p,
        Arc::new(|_, _| (StatusCode::BAD_GATEWAY, vec![b'<'; MAX_BODY + 1])),
    )
    .await;
    match client(&p, addr).challenge(MODEL, &pk).await {
        Err(TeeError::Kbs(m)) => {
            assert!(m.contains("bound"), "{m}");
            assert!(m.contains("502"), "{m}");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn redirects_are_not_followed() {
    // The broker never redirects; a 30x is reported as such, not chased (a
    // followed redirect would re-issue the POST elsewhere, or as a GET).
    let p = pki(HOST);
    let addr = spawn_tls_broker(
        &p,
        Arc::new(|_, _| (StatusCode::TEMPORARY_REDIRECT, Vec::new())),
    )
    .await;
    let kbs = client(&p, addr);
    let (_sk, pk) = generate_ephemeral_keypair();
    match kbs.challenge(MODEL, &pk).await {
        Err(TeeError::Kbs(m)) => assert!(m.contains("HTTP 307"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn plaintext_base_urls_are_refused_at_construction() {
    let p = pki(HOST);
    assert!(matches!(
        HttpKeyBrokerClient::new(
            "http://kbs.fabstir.net/v1/kbs",
            p.root_pem.as_bytes(),
            None,
            Duration::from_secs(1)
        ),
        Err(TeeError::Kbs(_))
    ));
    assert!(matches!(
        HttpKeyBrokerClient::new(
            "https://kbs.fabstir.net/v1/kbs",
            b"not a pem",
            None,
            Duration::from_secs(1)
        ),
        Err(TeeError::Kbs(_))
    ));
    // A chain file (more than one certificate) would widen the pin: refused.
    let two = format!("{}{}", p.root_pem, p.root_pem);
    match HttpKeyBrokerClient::new(
        "https://kbs.fabstir.net/v1/kbs",
        two.as_bytes(),
        None,
        Duration::from_secs(1),
    ) {
        Err(TeeError::Kbs(m)) => assert!(m.contains("exactly one certificate"), "{m}"),
        other => panic!("{other:?}"),
    }
}
