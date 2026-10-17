// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 1.3 — MockAttestationProvider + DefaultVerifier (tasks 1.3.1–1.3.3).
//!
//! Each reject test asserts the *specific* rejection reason (not merely "is_err")
//! so a verifier that errors for the wrong reason cannot false-green.

use super::profile_fixture::{a_policy, a_profile, now_unix};
use fabstir_llm_node::tee::host_profile::{HostProfile, MAX_PROFILE_LIFETIME};
use fabstir_llm_node::tee::mock::MockAttestationProvider;
use fabstir_llm_node::tee::provider::AttestationProvider;
use fabstir_llm_node::tee::types::{
    sha256_32, CcMode, Claims, Evidence, GpuReportFields, Policy, TeeError, REPORT_DATA_LEN,
};
use fabstir_llm_node::tee::verifier::{AttestationVerifier, DefaultVerifier};

const MEAS: [u8; 48] = [9u8; 48];
const OTHER_MEAS: [u8; 48] = [0xAAu8; 48];
const PK_ATT: [u8; 33] = [2u8; 33];
const NONCE: [u8; 32] = [7u8; 32];

fn valid_policy() -> Policy {
    a_policy([1u8; 32], 0, now_unix() + 3600, &["H100"])
}

fn profile() -> HostProfile {
    a_profile(MEAS, &["H100"])
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
        .verify(&ev, &valid_policy(), &profile(), NONCE)
        .expect("valid evidence should be accepted");
    assert!(claims.measurement_verified);
    assert_eq!(claims.gpu_report_hash, sha256_32(&ev.gpu_report));
    // verified_at is stamped during verify() — must be a sane, recent timestamp.
    assert!(claims.verified_at >= before && claims.verified_at <= now_unix() + 5);
}

#[tokio::test]
async fn rejects_wrong_mrtd() {
    // The mock's image_measurement stands in for MRTD; policy.cvm.mrtd pins it.
    let p = MockAttestationProvider::new("H100", OTHER_MEAS, CcMode::On);
    let ev = gather(&p, NONCE).await;
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), &profile(), NONCE),
        "mrtd",
    );
}

#[tokio::test]
async fn rejects_disallowed_hwmodel() {
    let p = MockAttestationProvider::new("H200", MEAS, CcMode::On);
    let ev = gather(&p, NONCE).await;
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), &profile(), NONCE),
        "hwmodel",
    );
}

#[tokio::test]
async fn rejects_cc_off() {
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::Off);
    let ev = gather(&p, NONCE).await;
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), &profile(), NONCE),
        "cc mode off",
    );
}

#[tokio::test]
async fn rejects_tcb_status_outside_the_allow_list() {
    // Policy v2: an exact TCB status allow-list, not an age in days.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On).with_tcb_status("OutOfDate");
    let ev = gather(&p, NONCE).await;
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), &profile(), NONCE),
        "tcb status",
    );
}

#[tokio::test]
async fn rejects_nonce_mismatch() {
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let ev = gather(&p, NONCE).await;
    let wrong_nonce = [8u8; 32];
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), &profile(), wrong_nonce),
        "nonce",
    );
}

#[tokio::test]
async fn rejects_expired_policy() {
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let ev = gather(&p, NONCE).await;
    let mut policy = valid_policy();
    policy.expiry = now_unix() - 1;
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &policy, &profile(), NONCE),
        "expired",
    );
}

#[tokio::test]
async fn rejects_not_yet_valid() {
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let ev = gather(&p, NONCE).await;
    let mut policy = valid_policy();
    policy.not_before = now_unix() + 3600;
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &policy, &profile(), NONCE),
        "not yet valid",
    );
}

#[tokio::test]
async fn rejects_td_debug_when_the_policy_requires_it_off() {
    // Policy v2: the TD DEBUG attribute replaces "production TCB".
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On).with_td_debug_off(false);
    let ev = gather(&p, NONCE).await;
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), &profile(), NONCE),
        "td debug",
    );
}

#[tokio::test]
async fn rejects_cpu_quote_too_short() {
    // Check #2 (fail-closed guard that also prevents a slice panic on `[..64]`).
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let mut ev = gather(&p, NONCE).await;
    ev.cpu_quote.truncate(32);
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), &profile(), NONCE),
        "cpu_quote too short",
    );
}

#[tokio::test]
async fn rejects_gpu_report_decode_failure() {
    // A gpu_report that is not valid bincode for GpuReportFields must fail closed
    // at decode. Under the identity ‖ nonce layout report_data does not depend on
    // the GPU bytes, so the CPU half stays genuine and the decode step is reached.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let mut ev = gather(&p, NONCE).await;
    ev.gpu_report = vec![0xFFu8; 4]; // too short to be a valid GpuReportFields
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), &profile(), NONCE),
        "gpu report decode",
    );
}

#[tokio::test]
async fn mock_evidence_carries_the_nonce_on_both_halves() {
    // The mock echoes nonce + pk_att, pins the measurement, and (Phase 5 P2.2)
    // puts the challenge nonce on BOTH halves: report_data[32..64] and the GPU
    // evidence's own nonce field, exactly as the real collector does.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let ev = gather(&p, NONCE).await;
    assert_eq!(ev.nonce, NONCE);
    assert_eq!(ev.pk_att, PK_ATT.to_vec());
    assert_eq!(ev.image_measurement, MEAS);
    assert_eq!(ev.cpu_quote.len(), REPORT_DATA_LEN);
    assert_eq!(&ev.cpu_quote[32..], &NONCE);
    let fields: GpuReportFields = bincode::deserialize(&ev.gpu_report).unwrap();
    assert_eq!(fields.nonce, NONCE);
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
    // Schema 3: a profile window covering the policy's, so the policy's decides.
    let mut prof = profile();
    (prof.not_before, prof.expiry) = (0, MAX_PROFILE_LIFETIME);

    assert!(
        DefaultVerifier
            .verify_at(&ev, &policy, &prof, NONCE, 1_000)
            .is_ok(),
        "now == not_before must be accepted"
    );
    assert!(
        DefaultVerifier
            .verify_at(&ev, &policy, &prof, NONCE, 2_000)
            .is_ok(),
        "now == expiry must be accepted"
    );
    assert_verification_failed(
        DefaultVerifier.verify_at(&ev, &policy, &prof, NONCE, 999),
        "not yet valid",
    );
    assert_verification_failed(
        DefaultVerifier.verify_at(&ev, &policy, &prof, NONCE, 2_001),
        "expired",
    );
}

#[tokio::test]
async fn accepts_cc_off_when_not_required() {
    // Policy-gated check #7: a policy with NO cc requirement accepts any mode.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::Off);
    let ev = gather(&p, NONCE).await;
    let mut policy = valid_policy();
    policy.gpu.require_cc_mode = None;
    assert!(
        DefaultVerifier
            .verify(&ev, &policy, &profile(), NONCE)
            .is_ok(),
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
        DefaultVerifier.verify(&ev, &valid_policy(), &profile(), NONCE),
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
    devtools_policy.gpu.require_cc_mode = Some(CcMode::DevTools);
    assert!(
        DefaultVerifier
            .verify(&ev, &devtools_policy, &profile(), NONCE)
            .is_ok(),
        "a policy naming DevTools should accept a DevTools report"
    );

    // ...and that same policy must NOT then accept a protected GPU silently
    // passing for something else: the match is exact in both directions.
    let on = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let ev_on = gather(&on, NONCE).await;
    assert_verification_failed(
        DefaultVerifier.verify(&ev_on, &devtools_policy, &profile(), NONCE),
        "cc mode on",
    );
}

#[tokio::test]
async fn accepts_td_debug_when_the_policy_does_not_require_it_off() {
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On).with_td_debug_off(false);
    let ev = gather(&p, NONCE).await;
    let mut policy = valid_policy();
    policy.cvm.require_td_debug_off = false;
    assert!(DefaultVerifier
        .verify(&ev, &policy, &profile(), NONCE)
        .is_ok());
}

#[tokio::test]
async fn tcb_status_allow_list_is_exact_and_widenable() {
    // Widening is a signed policy change: SWHardeningNeeded passes only once listed.
    let p =
        MockAttestationProvider::new("H100", MEAS, CcMode::On).with_tcb_status("SWHardeningNeeded");
    let ev = gather(&p, NONCE).await;
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), &profile(), NONCE),
        "tcb status",
    );
    let mut policy = valid_policy();
    policy
        .cvm
        .allowed_tcb_status
        .push("SWHardeningNeeded".into());
    assert!(DefaultVerifier
        .verify(&ev, &policy, &profile(), NONCE)
        .is_ok());
}

#[tokio::test]
async fn gpu_secure_boot_debug_and_version_floors() {
    let good = || MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let ev = gather(&good().with_secure_boot(false), NONCE).await;
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), &profile(), NONCE),
        "secure boot",
    );
    let ev = gather(&good().with_debug_disabled(false), NONCE).await;
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &valid_policy(), &profile(), NONCE),
        "gpu debug",
    );
    // Version floors: dotted numeric compare; 580.95.05 >= 580.95 passes, > fails.
    let ev = gather(&good().with_driver_version("580.95.05"), NONCE).await;
    let mut policy = valid_policy();
    policy.gpu.min_driver_version = Some("580.95".into());
    assert!(DefaultVerifier
        .verify(&ev, &policy, &profile(), NONCE)
        .is_ok());
    policy.gpu.min_driver_version = Some("581.0".into());
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &policy, &profile(), NONCE),
        "driver",
    );
    let ev = gather(&good().with_vbios_version("96.00.9f.00.01"), NONCE).await;
    let mut policy = valid_policy();
    policy.gpu.min_vbios_version = Some("96.00.a0".into());
    assert_verification_failed(
        DefaultVerifier.verify(&ev, &policy, &profile(), NONCE),
        "vbios",
    );
}

#[tokio::test]
async fn policy_form_is_validated_and_never_coerced() {
    // Expert condition 3 (2026-09-17): a 0x prefix, upper case or a wrong length
    // is refused; nothing is silently rewritten (the signer signed these bytes).
    // Schema 3: the measurements moved to the profile, which verify() validates
    // as strictly as the policy.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let ev = gather(&p, NONCE).await;
    type Mutation = Box<dyn Fn(&mut Policy, &mut HostProfile)>;
    let cases: Vec<(Mutation, &str)> = vec![
        (
            Box::new(|_, pr| pr.cvm.mrtd = format!("0x{}", pr.cvm.mrtd)),
            "cvm.mrtd",
        ),
        (
            Box::new(|_, pr| pr.cvm.rtmr1 = "AB".repeat(48)),
            "cvm.rtmr1",
        ),
        (
            Box::new(|_, pr| pr.cvm.compose_hashes[0].truncate(63)),
            "cvm.compose_hashes[0]",
        ),
        (Box::new(|pol, _| pol.schema_version = 2), "schema_version"),
        (
            Box::new(|pol, _| pol.cvm.allowed_tcb_status.clear()),
            "allowed_tcb_status",
        ),
        (
            Box::new(|pol, _| pol.gpu.allowed_hwmodels.clear()),
            "allowed_hwmodels",
        ),
    ];
    for (mutate, needle) in cases {
        let (mut policy, mut prof) = (valid_policy(), profile());
        mutate(&mut policy, &mut prof);
        assert_verification_failed(DefaultVerifier.verify(&ev, &policy, &prof, NONCE), needle);
    }
}

#[tokio::test]
async fn rejects_on_broken_clock_even_if_never_expires() {
    // now == u64::MAX is now_unix()'s broken-clock sentinel: must fail closed even for a
    // never-expiring (expiry == u64::MAX) policy.
    let p = MockAttestationProvider::new("H100", MEAS, CcMode::On);
    let ev = gather(&p, NONCE).await;
    let mut policy = valid_policy();
    policy.expiry = u64::MAX;
    let prof = profile();
    assert_verification_failed(
        DefaultVerifier.verify_at(&ev, &policy, &prof, NONCE, u64::MAX),
        "clock",
    );
}
