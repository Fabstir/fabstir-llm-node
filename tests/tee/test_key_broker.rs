// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 3.2 — attestation-gated key broker: challenge → evidence → wrapped DEK,
//! with one-time-use freshness nonces (Option A) and policy verification.

use fabstir_llm_node::tee::key_broker::{KeyBrokerClient, NodeAttestationClient};
use fabstir_llm_node::tee::keywrap::generate_ephemeral_keypair;
use fabstir_llm_node::tee::mock::{MockAttestationProvider, MockKeyBroker};
use fabstir_llm_node::tee::provider::AttestationProvider;
use fabstir_llm_node::tee::types::{CcMode,Policy, TeeError};
use std::collections::HashMap;

const SKU: &str = "H100";
const MEASUREMENT: [u8; 48] = [0x42u8; 48];

fn test_policy(model_id: [u8; 32]) -> Policy {
    Policy {
        policy_version: 1,
        allowed_skus: vec![SKU.to_string()],
        expected_measurement: MEASUREMENT,
        require_cc_mode: Some(CcMode::On),
        require_production_tcb: true,
        max_tcb_age_days: 30,
        not_before: 0,
        expiry: u64::MAX - 1,
        model_id,
    }
}

fn good_provider() -> MockAttestationProvider {
    MockAttestationProvider::new(SKU, MEASUREMENT, CcMode::On)
}

fn broker(model_id: [u8; 32], dek: [u8; 32]) -> MockKeyBroker {
    MockKeyBroker::new(HashMap::from([(model_id, (dek, test_policy(model_id)))]))
}

#[tokio::test]
async fn obtain_dek_happy_path_matches_dek() {
    let (model_id, dek) = ([1u8; 32], [2u8; 32]);
    let kbs = broker(model_id, dek);
    let got = NodeAttestationClient::obtain_dek(&good_provider(), &kbs, model_id)
        .await
        .expect("obtain_dek");
    assert_eq!(got, dek, "the node recovers exactly the model's DEK");
}

#[tokio::test]
async fn obtain_dek_rejects_unknown_model() {
    let kbs = broker([1u8; 32], [2u8; 32]);
    let err = NodeAttestationClient::obtain_dek(&good_provider(), &kbs, [9u8; 32])
        .await
        .expect_err("unknown model must be refused");
    assert!(matches!(err, TeeError::NoProviderBound(_)), "got {err:?}");
}

#[tokio::test]
async fn request_key_rejects_tampered_evidence() {
    let (model_id, dek) = ([1u8; 32], [2u8; 32]);
    let kbs = broker(model_id, dek);
    let provider = good_provider();
    let nonce = kbs.challenge(model_id).await.unwrap();
    let (_sec, pk_att) = generate_ephemeral_keypair();
    let mut ev = provider.gather_evidence(nonce, &pk_att).await.unwrap();
    ev.gpu_report[0] ^= 0x01; // breaks the cross-binding
    let err = kbs
        .request_key(model_id, &ev)
        .await
        .expect_err("tampered evidence must be refused");
    assert!(
        matches!(err, TeeError::VerificationFailed(_)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn request_key_rejects_unissued_nonce() {
    let (model_id, dek) = ([1u8; 32], [2u8; 32]);
    let kbs = broker(model_id, dek);
    let provider = good_provider();
    let (_sec, pk_att) = generate_ephemeral_keypair();
    // A nonce never minted by challenge() — challenge-bound freshness must reject it.
    let ev = provider.gather_evidence([7u8; 32], &pk_att).await.unwrap();
    let err = kbs
        .request_key(model_id, &ev)
        .await
        .expect_err("unissued nonce must be refused");
    assert!(matches!(err, TeeError::FreshnessFailure), "got {err:?}");
}

#[tokio::test]
async fn request_key_rejects_stale_nonce() {
    let (model_id, dek) = ([1u8; 32], [2u8; 32]);
    // TTL = 0 → an issued nonce goes stale as soon as the clock ticks past its second.
    let kbs = broker(model_id, dek).with_ttl(0);
    let provider = good_provider();
    let nonce = kbs.challenge(model_id).await.unwrap();
    let (_sec, pk_att) = generate_ephemeral_keypair();
    let ev = provider.gather_evidence(nonce, &pk_att).await.unwrap();
    // Cross at least one whole second so now_unix() > issued_at + ttl(0).
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let err = kbs
        .request_key(model_id, &ev)
        .await
        .expect_err("stale nonce must be refused");
    assert!(matches!(err, TeeError::FreshnessFailure), "got {err:?}");
}

#[tokio::test]
async fn request_key_rejects_reused_nonce_one_time_use() {
    let (model_id, dek) = ([1u8; 32], [2u8; 32]);
    let kbs = broker(model_id, dek);
    let provider = good_provider();
    let nonce = kbs.challenge(model_id).await.unwrap();
    let (_sec, pk_att) = generate_ephemeral_keypair();
    let ev = provider.gather_evidence(nonce, &pk_att).await.unwrap();
    kbs.request_key(model_id, &ev).await.expect("first use ok"); // consumes the nonce
    let err = kbs
        .request_key(model_id, &ev)
        .await
        .expect_err("re-using a one-time nonce must be refused");
    assert!(matches!(err, TeeError::FreshnessFailure), "got {err:?}");
}
