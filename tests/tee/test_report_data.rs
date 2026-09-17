// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P2.2 — the `report_data = sha256(pk_att) ‖ nonce` layout and the
//! shared-nonce cross-binding, on the mock provider + `DefaultVerifier`.
//! Split from `test_verify.rs` (400-line cap). The old golden-vector test for
//! the retired `sha256(pk_att ‖ sha256(gpu_report) ‖ nonce)` layout was deleted
//! rather than updated so nothing stayed green across the change.

use fabstir_llm_node::tee::mock::MockAttestationProvider;
use fabstir_llm_node::tee::provider::AttestationProvider;
use fabstir_llm_node::tee::types::{
    report_data, report_data_identity, CcMode, Claims, Evidence, GpuReportFields, Policy, TeeError,
    REPORT_DATA_LEN,
};
use fabstir_llm_node::tee::verifier::{AttestationVerifier, DefaultVerifier};
use std::time::{SystemTime, UNIX_EPOCH};

const MEAS: [u8; 48] = [9u8; 48];
const PK_ATT: [u8; 33] = [2u8; 33];
const NONCE: [u8; 32] = [7u8; 32];

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn valid_policy() -> Policy {
    Policy {
        policy_version: 1,
        allowed_skus: vec!["H100".to_string()],
        expected_measurement: MEAS,
        require_cc_mode: Some(CcMode::On),
        require_production_tcb: true,
        max_tcb_age_days: 30,
        not_before: 0,
        expiry: now_unix() + 3600,
        model_id: [1u8; 32],
    }
}

async fn gather(p: &MockAttestationProvider, nonce: [u8; 32]) -> Evidence {
    p.gather_evidence(nonce, &PK_ATT)
        .await
        .expect("mock gather_evidence")
}

fn assert_verification_failed(res: Result<Claims, TeeError>, needle: &str) {
    match res {
        Err(TeeError::VerificationFailed(msg)) => assert!(
            msg.to_lowercase().contains(needle),
            "expected '{needle}' in VerificationFailed, got: {msg}"
        ),
        other => panic!("expected VerificationFailed containing '{needle}', got {other:?}"),
    }
}

#[tokio::test]
async fn rejects_gpu_evidence_from_another_challenge() {
    // The split-attestation forgery under the identity ‖ nonce layout: a genuine
    // CPU quote for THIS challenge paired with genuine GPU evidence collected for
    // ANOTHER challenge (another session, box, or a replay). Both halves verify
    // in isolation; only the shared nonce ties them, so the GPU half's nonce must
    // be checked against the issued one.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let mut ev = gather(&p, NONCE).await;
    let other = gather(&p, [8u8; 32]).await;
    ev.gpu_report = other.gpu_report; // CPU half still says NONCE; GPU half says [8; 32]
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), NONCE),
        "gpu evidence nonce",
    );
}

#[tokio::test]
async fn rejects_gpu_evidence_nonce_tamper() {
    // Single-byte mutation inside the GPU evidence's nonce (the mutation check the
    // Phase-5 gate A-7 asks for): decode, flip one byte, re-encode. Everything
    // else about the evidence is untouched and the CPU half is genuine.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let mut ev = gather(&p, NONCE).await;
    let mut fields: GpuReportFields = bincode::deserialize(&ev.gpu_report).unwrap();
    fields.nonce[31] ^= 0x01;
    ev.gpu_report = bincode::serialize(&fields).unwrap();
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), NONCE),
        "gpu evidence nonce",
    );
}

#[tokio::test]
async fn rejects_report_data_identity_mismatch() {
    // The substituted-key attack: a genuine quote whose identity half commits to
    // PK_ATT, presented with a different pk_att to wrap the DEK to. The verifier
    // recomputes sha256(ev.pk_att) and compares with the (signed) identity half.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let mut ev = gather(&p, NONCE).await;
    ev.pk_att = vec![3u8; 33];
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), NONCE),
        "identity",
    );
}

#[tokio::test]
async fn rejects_report_data_nonce_tamper() {
    // The nonce half of the signed report_data must equal the issued nonce
    // byte-for-byte; one flipped byte inside [32..64] is a different challenge.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let mut ev = gather(&p, NONCE).await;
    ev.cpu_quote[40] ^= 0x01;
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), NONCE),
        "report_data nonce",
    );
}

#[tokio::test]
async fn accepts_a_nonce_whose_first_byte_looks_like_json() {
    // Converge round 2 (2026-09-17): bincode writes the 32-byte nonce at byte 0
    // of the mock gpu_report, so a nonce starting 0x7B ('{') must not be
    // mistaken for real nvidia_payload JSON. 1-in-256 flake if it were.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let mut nonce = NONCE;
    nonce[0] = b'{';
    let ev = gather(&p, nonce).await;
    DefaultVerifier
        .verify(&ev, &valid_policy(), nonce)
        .expect("a nonce beginning 0x7B is ordinary mock evidence");
}

#[tokio::test]
async fn real_nvidia_payload_is_refused_as_needing_the_phase5_verifier() {
    // The message that stops someone wiring the real provider into the mock
    // broker from chasing a "decode error".
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let mut ev = gather(&p, NONCE).await;
    ev.gpu_report = br#"{"nonce":"07","evidence_list":[{"certificate":"x","evidence":"y","arch":"HOPPER"}],"arch":"HOPPER"}"#.to_vec();
    // Real-shaped: a multi-KB TDX quote whose first bytes are the DCAP header, not
    // report_data. The Phase-5 message must win over the identity check.
    ev.cpu_quote = vec![0x04, 0x00, 0x02, 0x00, 0x81, 0x00, 0x00, 0x00]
        .into_iter()
        .chain(std::iter::repeat(0x5Au8).take(4000))
        .collect();
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), NONCE),
        "phase-5 verifier",
    );
}

#[test]
fn report_data_layout_is_identity_then_nonce() {
    // Pins the Phase-5 report_data layout (expert decision 2026-09-17, matching
    // Phala's dstack node): `sha256(pk_att) ‖ nonce`, 64 bytes, nonce in the
    // clear, no domain tag, no hash over the GPU evidence. Recomputed here with
    // sha2 directly so the helper cannot drift; the identity half is also
    // frozen as a vector computed OUTSIDE this crate (python hashlib, 2026-09-17).
    // This test replaces `cross_bind_construction_is_exact`, which pinned the
    // retired `sha256(pk_att ‖ sha256(gpu_report) ‖ nonce)` layout; that test was
    // deleted rather than updated so nothing stays green across the change.
    use sha2::{Digest, Sha256};
    let pk_att = [0x02u8; 33];
    let nonce = [0x11u8; 32];

    let rd = report_data(&pk_att, &nonce);
    assert_eq!(rd.len(), REPORT_DATA_LEN);
    assert_eq!(&rd[..32], Sha256::digest(pk_att).as_slice());
    assert_eq!(&rd[32..], &nonce);
    assert_eq!(rd[..32], report_data_identity(&pk_att));
    assert_eq!(
        hex::encode(report_data_identity(&pk_att)),
        "7f2f54ff94459f3ac4d19d3219ce6ef06868eb8c72e6d84cc358bc769b23113a",
    );

    // Each half is bound to exactly one input.
    assert_ne!(report_data(&[0x03u8; 33], &nonce)[..32], rd[..32]);
    assert_eq!(report_data(&[0x03u8; 33], &nonce)[32..], rd[32..]);
    assert_eq!(report_data(&pk_att, &[0x22u8; 32])[..32], rd[..32]);
    assert_ne!(report_data(&pk_att, &[0x22u8; 32])[32..], rd[32..]);
}
