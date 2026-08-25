// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 1.3 — MockAttestationProvider + DefaultVerifier (tasks 1.3.1–1.3.3).
//!
//! Each reject test asserts the *specific* rejection reason (not merely "is_err")
//! so a verifier that errors for the wrong reason cannot false-green.

use fabstir_llm_node::tee::mock::MockAttestationProvider;
use fabstir_llm_node::tee::provider::AttestationProvider;
use fabstir_llm_node::tee::types::{
    cross_bind_report_data, sha256_32, CcMode, Claims, Evidence, Policy, TeeError,
};
use fabstir_llm_node::tee::verifier::{AttestationVerifier, DefaultVerifier};
use std::time::{SystemTime, UNIX_EPOCH};

const MEAS: [u8; 48] = [9u8; 48];
const OTHER_MEAS: [u8; 48] = [0xAAu8; 48];
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
async fn accepts_valid() {
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let ev = gather(&p, NONCE).await;
    let before = now_unix();
    let claims = DefaultVerifier
        .verify(&ev, &valid_policy(), NONCE)
        .expect("valid evidence should be accepted");
    assert!(claims.measurement_verified);
    assert_eq!(claims.gpu_report_hash, sha256_32(&ev.gpu_report));
    // verified_at is stamped during verify() — must be a sane, recent timestamp.
    assert!(claims.verified_at >= before && claims.verified_at <= now_unix() + 5);
}

#[tokio::test]
async fn rejects_wrong_measurement() {
    let p = MockAttestationProvider::new("H100", OTHER_MEAS, CcMode::On);
    let ev = gather(&p, NONCE).await;
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), NONCE),
        "measurement",
    );
}

#[tokio::test]
async fn rejects_disallowed_sku() {
    let p = MockAttestationProvider::new("H200", MEAS, CcMode::On);
    let ev = gather(&p, NONCE).await;
    assert_verification_failed(DefaultVerifier.verify(&ev, &valid_policy(), NONCE), "sku");
}

#[tokio::test]
async fn rejects_cc_off() {
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::Off);
    let ev = gather(&p, NONCE).await;
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), NONCE),
        "cc mode off",
    );
}

#[tokio::test]
async fn rejects_stale_tcb() {
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On).with_tcb_age_days(60);
    let ev = gather(&p, NONCE).await;
    assert_verification_failed(DefaultVerifier.verify(&ev, &valid_policy(), NONCE), "tcb");
}

#[tokio::test]
async fn rejects_nonce_mismatch() {
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let ev = gather(&p, NONCE).await;
    let wrong_nonce = [8u8; 32];
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), wrong_nonce),
        "nonce",
    );
}

#[tokio::test]
async fn rejects_cross_bind_mismatch() {
    // A genuine CPU quote from p1 paired with a different (also genuine) GPU
    // report — the classic split-attestation forgery. Both reports are valid in
    // isolation; only the pairing is wrong, so cross-binding must catch it.
    let p1 = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let p2 = MockAttestationProvider::new("H100", MEAS, CcMode::On).with_tcb_age_days(5);
    let mut ev = gather(&p1, NONCE).await;
    let ev2 = gather(&p2, NONCE).await;
    ev.gpu_report = ev2.gpu_report; // swap report; ev.cpu_quote still commits to p1's
    assert_verification_failed(DefaultVerifier.verify(&ev, &valid_policy(), NONCE), "cross");
}

#[tokio::test]
async fn rejects_expired_policy() {
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let ev = gather(&p, NONCE).await;
    let mut policy = valid_policy();
    policy.expiry = now_unix() - 1;
    assert_verification_failed(DefaultVerifier.verify(&ev, &policy, NONCE), "expired");
}

#[tokio::test]
async fn rejects_not_yet_valid() {
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let ev = gather(&p, NONCE).await;
    let mut policy = valid_policy();
    policy.not_before = now_unix() + 3600;
    assert_verification_failed(DefaultVerifier.verify(&ev, &policy, NONCE), "not yet valid");
}

#[tokio::test]
async fn rejects_non_production_tcb() {
    // Check #8: debug/non-production CPU TCB must be rejected when the policy requires it.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On).with_production_tcb(false);
    let ev = gather(&p, NONCE).await;
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), NONCE),
        "non-production tcb",
    );
}

#[tokio::test]
async fn rejects_cpu_quote_too_short() {
    // Check #2 (fail-closed guard that also prevents a slice panic on `[..64]`).
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let mut ev = gather(&p, NONCE).await;
    ev.cpu_quote.truncate(32);
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), NONCE),
        "cpu_quote too short",
    );
}

#[tokio::test]
async fn rejects_gpu_report_decode_failure() {
    // Check #4: a gpu_report that is not valid bincode for GpuReportFields must fail
    // closed at decode — even with a cross-binding deliberately recomputed to match it.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let mut ev = gather(&p, NONCE).await;
    ev.gpu_report = vec![0xFFu8; 4]; // too short to be a valid GpuReportFields
    let grh = sha256_32(&ev.gpu_report);
    let rd = cross_bind_report_data(&ev.pk_att, &grh, &ev.nonce);
    let mut quote = vec![0u8; 64];
    quote[..32].copy_from_slice(&rd);
    ev.cpu_quote = quote; // cross-binding now passes, so we reach the decode step
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), NONCE),
        "gpu report decode",
    );
}

#[tokio::test]
async fn rejects_nonzero_report_data_padding() {
    // Check #3 zero-pad half: report_data[32..64] must be zero.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let mut ev = gather(&p, NONCE).await;
    ev.cpu_quote[40] = 0x01; // a byte inside the [32..64] zero-pad region
    assert_verification_failed(DefaultVerifier.verify(&ev, &valid_policy(), NONCE), "cross");
}

#[tokio::test]
async fn mock_echoes_nonce_and_pk_att() {
    // Task 1.3.1: the mock echoes the nonce + pk_att (and pins measurement / quote length).
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let ev = gather(&p, NONCE).await;
    assert_eq!(ev.nonce, NONCE);
    assert_eq!(ev.pk_att, PK_ATT.to_vec());
    assert_eq!(ev.image_measurement, MEAS);
    assert_eq!(ev.cpu_quote.len(), 64);
}

#[tokio::test]
async fn validity_window_is_inclusive() {
    // Deterministic boundary test of `not_before <= now <= expiry` (inclusive) via
    // verify_at(now). The plan defines the window as inclusive at both ends; revocation
    // is expressed by pushing expiry into the past, not by excluding the exact second.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let ev = gather(&p, NONCE).await;
    let mut policy = valid_policy();
    policy.not_before = 1_000;
    policy.expiry = 2_000;

    assert!(
        DefaultVerifier
            .verify_at(&ev, &policy, NONCE, 1_000)
            .is_ok(),
        "now == not_before must be accepted"
    );
    assert!(
        DefaultVerifier
            .verify_at(&ev, &policy, NONCE, 2_000)
            .is_ok(),
        "now == expiry must be accepted"
    );
    assert_verification_failed(
        DefaultVerifier.verify_at(&ev, &policy, NONCE, 999),
        "not yet valid",
    );
    assert_verification_failed(
        DefaultVerifier.verify_at(&ev, &policy, NONCE, 2_001),
        "expired",
    );
}

#[tokio::test]
async fn accepts_cc_off_when_not_required() {
    // Policy-gated check #7: a policy with NO cc requirement accepts any mode.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::Off);
    let ev = gather(&p, NONCE).await;
    let mut policy = valid_policy();
    policy.require_cc_mode = None;
    assert!(
        DefaultVerifier.verify(&ev, &policy, NONCE).is_ok(),
        "CcMode::Off should be accepted when require_cc_mode is None"
    );
}

#[tokio::test]
async fn rejects_devtools_when_the_policy_requires_on() {
    // THE reason CcMode is not a bool. `devtools` enables the CC APIs and
    // attests, with the memory protections DISABLED. Under the old
    // `cc_on: bool` this state was inexpressible, so a real report parser
    // would have mapped "not off" to true and released the dataset key to an
    // unprotected GPU while `require_cc_on` read as though it had done its job.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::DevTools);
    let ev = gather(&p, NONCE).await;
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), NONCE),
        "cc mode devtools",
    );
}

#[tokio::test]
async fn devtools_is_accepted_only_when_named_explicitly() {
    // A policy MAY accept devtools, but only by naming it. There is no way to
    // reach it by relaxing a flag, which is what made the boolean dangerous:
    // the unsafe state must be spelled out, never arrived at by loosening.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::DevTools);
    let ev = gather(&p, NONCE).await;

    let mut devtools_policy = valid_policy();
    devtools_policy.require_cc_mode = Some(CcMode::DevTools);
    assert!(
        DefaultVerifier.verify(&ev, &devtools_policy, NONCE).is_ok(),
        "a policy naming DevTools should accept a DevTools report"
    );

    // ...and that same policy must NOT then accept a protected GPU silently
    // passing for something else: the match is exact in both directions.
    let on = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let ev_on = gather(&on, NONCE).await;
    assert_verification_failed(
        DefaultVerifier.verify(&ev_on, &devtools_policy, NONCE),
        "cc mode on",
    );
}

#[tokio::test]
async fn accepts_non_production_tcb_when_not_required() {
    // Policy-gated check #8: require_production_tcb=false must ACCEPT a debug-TCB report.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On).with_production_tcb(false);
    let ev = gather(&p, NONCE).await;
    let mut policy = valid_policy();
    policy.require_production_tcb = false;
    assert!(
        DefaultVerifier.verify(&ev, &policy, NONCE).is_ok(),
        "production_tcb=false should be accepted when require_production_tcb=false"
    );
}

#[tokio::test]
async fn accepts_tcb_age_equal_to_max() {
    // Check #9 inclusive boundary: tcb_age_days == max_tcb_age_days is accepted (`>` check).
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On).with_tcb_age_days(30);
    let ev = gather(&p, NONCE).await;
    assert!(
        DefaultVerifier.verify(&ev, &valid_policy(), NONCE).is_ok(),
        "tcb_age == max should be accepted"
    );
}

#[tokio::test]
async fn rejects_tcb_age_one_over_max() {
    // Check #9 exact boundary: max + 1 is rejected.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On).with_tcb_age_days(31);
    let ev = gather(&p, NONCE).await;
    assert_verification_failed(DefaultVerifier.verify(&ev, &valid_policy(), NONCE), "tcb");
}

#[tokio::test]
async fn rejects_on_broken_clock_even_if_never_expires() {
    // now == u64::MAX is now_unix()'s broken-clock sentinel: must fail closed even for a
    // never-expiring (expiry == u64::MAX) policy.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let ev = gather(&p, NONCE).await;
    let mut policy = valid_policy();
    policy.expiry = u64::MAX;
    assert_verification_failed(
        DefaultVerifier.verify_at(&ev, &policy, NONCE, u64::MAX),
        "clock",
    );
}

#[test]
fn cross_bind_construction_is_exact() {
    // Pins the canonical cross-binding construction (task 1.3.1): the commitment
    // is exactly sha256(pk_att ‖ sha256(gpu_report) ‖ nonce) — no domain tag, no
    // length prefixes, this field order. Independently recomputed via sha2 so the
    // helper cannot silently change shape (which would break Phase-5 / SDK parity).
    use sha2::{Digest, Sha256};
    let pk_att = [0x02u8; 33];
    let gpu_report = b"example-gpu-report".to_vec();
    let nonce = [0x11u8; 32];

    let gpu_report_hash = sha256_32(&gpu_report);
    let mut h = Sha256::new();
    h.update(pk_att);
    h.update(gpu_report_hash);
    h.update(nonce);
    let mut expected = [0u8; 32];
    expected.copy_from_slice(&h.finalize());

    assert_eq!(
        cross_bind_report_data(&pk_att, &gpu_report_hash, &nonce),
        expected,
        "cross-binding must be sha256(pk_att ‖ gpu_report_hash ‖ nonce)"
    );

    // Frozen golden vector (task 1.1.1/1.3.1) — pins the exact bytes so any drift,
    // even one shared by the in-test recomputation, breaks cross-impl (Phase-5/SDK)
    // parity. Inputs above: pk_att=[0x02;33], gpu_report=b"example-gpu-report",
    // nonce=[0x11;32].
    assert_eq!(
        hex::encode(cross_bind_report_data(&pk_att, &gpu_report_hash, &nonce)),
        "91e6f9443984e47d198c619cceecd5ec75ecbd729bf251742ab843c2be672448",
    );

    // Every input is bound: changing any single one flips the commitment.
    assert_ne!(
        cross_bind_report_data(&[0x03u8; 33], &gpu_report_hash, &nonce),
        expected
    );
    assert_ne!(
        cross_bind_report_data(&pk_att, &sha256_32(b"different-report"), &nonce),
        expected
    );
    assert_ne!(
        cross_bind_report_data(&pk_att, &gpu_report_hash, &[0x22u8; 32]),
        expected
    );
}
