//! Design §6 step 4 and §11: `prefilter` and `check_policy` row by row, every row
//! listed, the canned gate, the malformed `require_cc_mode`, and the attacker-shaped
//! event-log test (a quote whose `rt_mr3` = replay of a doctored log).

use super::policy_fixture::{
    event_log, gpu_payload, patched_quote, recording_policy, test_model_id,
};
use fabstir_llm_node::kbs::config::GpuEvidenceMode;
use fabstir_llm_node::kbs::cpu::{simulator, TdxEvidence};
use fabstir_llm_node::kbs::error::Kind;
use fabstir_llm_node::kbs::eventlog::{
    extract, parse, replay, replay_and_extract, select_runtime, RuntimeEvent, EV_COMPOSE_HASH,
    EV_SYSTEM_READY,
};
use fabstir_llm_node::kbs::nras_claims::{GpuFields, GpuOutcome};
use fabstir_llm_node::kbs::verify::{check_policy, prefilter, CcRecord, Expected};
use fabstir_llm_node::tee::types::{CcMode, Policy};
use serde_json::json;

const PK: [u8; 33] = [0x02; 33];
const NONCE: [u8; 32] = [0x77; 32];

fn expected(model_id: [u8; 32]) -> Expected {
    Expected {
        model_id,
        pk_att: PK,
        nonce: NONCE,
    }
}

fn good_gpu() -> GpuOutcome {
    GpuOutcome::Real(GpuFields {
        nonce: NONCE,
        hwmodel: "GH100 A01 GSP BROM".into(),
        secure_boot: true,
        debug_disabled: true,
        driver_version: "550.90.07".into(),
        vbios_version: "96.00.74.00.1a".into(),
    })
}

/// `good_gpu` with the signed pair varied: the broker derives `cc_mode` from
/// `secboot` and `dbgstat` exactly as `map_per_gpu` does.
fn gpu_with(secure_boot: bool, debug_disabled: bool) -> GpuOutcome {
    let GpuOutcome::Real(f) = good_gpu() else {
        unreachable!()
    };
    GpuOutcome::Real(GpuFields {
        secure_boot,
        debug_disabled,
        ..f
    })
}

fn good_cpu() -> TdxEvidence {
    simulator(&patched_quote(&PK, &NONCE)).unwrap()
}

fn setup() -> (Policy, Expected) {
    let id = test_model_id(1);
    (recording_policy(id, 1), expected(id))
}

#[test]
fn prefilter_passes_on_the_recording_and_lists_every_failing_row() {
    let (policy, exp) = setup();
    let q = patched_quote(&PK, &NONCE);
    let pf = prefilter(
        &policy,
        &exp,
        &q,
        &event_log(),
        &gpu_payload(&NONCE, None),
        GpuEvidenceMode::Real,
    )
    .unwrap();
    assert!(!pf.label_present);
    assert_eq!(
        hex::encode(pf.replayed.compose_hash),
        policy.cvm.compose_hash
    );

    // three independent failures → three rows (mutation: return at first failure → one)
    let mut bad_policy = policy.clone();
    bad_policy.cvm.mrtd = "00".repeat(48);
    bad_policy.cvm.compose_hash = "11".repeat(32);
    let e = prefilter(
        &bad_policy,
        &exp,
        &q,
        &event_log(),
        &gpu_payload(&[9u8; 32], None),
        GpuEvidenceMode::Real,
    )
    .unwrap_err();
    assert_eq!(e.kind, Kind::Verification);
    assert!(
        e.detail.contains("mrtd:")
            && e.detail.contains("compose hash")
            && e.detail.contains("gpu nonce (wrapper)"),
        "{e}"
    );
}

#[test]
fn prefilter_identity_nonce_and_register_rows() {
    let (policy, exp) = setup();
    let log = event_log();
    let gp = gpu_payload(&NONCE, None);
    // identity: report_data[..32] for another key
    let q = patched_quote(&[0x03; 33], &NONCE);
    let e = prefilter(&policy, &exp, &q, &log, &gp, GpuEvidenceMode::Real).unwrap_err();
    assert!(e.detail.contains("identity:"), "{e}");
    // nonce: report_data[32..] differs
    let q = patched_quote(&PK, &[1u8; 32]);
    let e = prefilter(&policy, &exp, &q, &log, &gp, GpuEvidenceMode::Real).unwrap_err();
    assert!(e.detail.contains("nonce:"), "{e}");
    // each register
    for (i, name) in ["rtmr0", "rtmr1", "rtmr2"].iter().enumerate() {
        let mut p = policy.clone();
        match i {
            0 => p.cvm.rtmr0 = "ab".repeat(48),
            1 => p.cvm.rtmr1 = "ab".repeat(48),
            _ => p.cvm.rtmr2 = "ab".repeat(48),
        }
        let e = prefilter(
            &p,
            &exp,
            &patched_quote(&PK, &NONCE),
            &log,
            &gp,
            GpuEvidenceMode::Real,
        )
        .unwrap_err();
        assert!(e.detail.contains(&format!("{name}:")), "{name}: {e}");
    }
    // an SGX quote → the "not a TDX quote" row
    let sgx = super::fixtures::fixture("dcap-sgx_quote.bin");
    let e = prefilter(&policy, &exp, &sgx, &log, &gp, GpuEvidenceMode::Real).unwrap_err();
    assert!(e.detail.contains("not a TDX quote"), "{e}");
}

#[test]
fn prefilter_gpu_payload_rows_and_canned_label_vs_mode() {
    let (policy, exp) = setup();
    let q = patched_quote(&PK, &NONCE);
    let log = event_log();
    // canned key present at all under real mode → refused
    for v in [json!(true), json!(false), json!("true")] {
        let e = prefilter(
            &policy,
            &exp,
            &q,
            &log,
            &gpu_payload(&NONCE, Some(v)),
            GpuEvidenceMode::Real,
        )
        .unwrap_err();
        assert!(e.detail.contains("canned evidence under real mode"), "{e}");
    }
    // canned mode requires the boolean true label
    let e = prefilter(
        &policy,
        &exp,
        &q,
        &log,
        &gpu_payload(&NONCE, None),
        GpuEvidenceMode::Canned,
    )
    .unwrap_err();
    assert!(e.detail.contains("canned mode requires"), "{e}");
    let e = prefilter(
        &policy,
        &exp,
        &q,
        &log,
        &gpu_payload(&NONCE, Some(json!("true"))),
        GpuEvidenceMode::Canned,
    )
    .unwrap_err();
    assert!(e.detail.contains("canned mode requires"), "{e}");
    let pf = prefilter(
        &policy,
        &exp,
        &q,
        &log,
        &gpu_payload(&NONCE, Some(json!(true))),
        GpuEvidenceMode::Canned,
    )
    .unwrap();
    assert!(pf.label_present);
    // shape: two evidence entries; missing arch; missing nonce
    let two = serde_json::to_vec(
        &json!({"nonce": hex::encode(NONCE), "evidence_list": [1, 2], "arch": "HOPPER"}),
    )
    .unwrap();
    assert!(
        prefilter(&policy, &exp, &q, &log, &two, GpuEvidenceMode::Real)
            .unwrap_err()
            .detail
            .contains("gpu count")
    );
    let no_arch =
        serde_json::to_vec(&json!({"nonce": hex::encode(NONCE), "evidence_list": [1]})).unwrap();
    assert!(
        prefilter(&policy, &exp, &q, &log, &no_arch, GpuEvidenceMode::Real)
            .unwrap_err()
            .detail
            .contains("arch missing")
    );
    let no_nonce = serde_json::to_vec(&json!({"evidence_list": [1], "arch": "HOPPER"})).unwrap();
    assert!(
        prefilter(&policy, &exp, &q, &log, &no_nonce, GpuEvidenceMode::Real)
            .unwrap_err()
            .detail
            .contains("wrapper")
    );
    assert!(
        prefilter(&policy, &exp, &q, &log, b"not json", GpuEvidenceMode::Real)
            .unwrap_err()
            .detail
            .contains("gpu payload")
    );
}

#[test]
fn attacker_shaped_log_with_a_matching_rt_mr3_is_refused_on_the_extraction_row() {
    // The genuine compose-hash is replaced by another value; a second compose-hash
    // equal to the pin is appended after system-ready; the quote's rt_mr3 is the
    // replay of that log (the attacker's own CVM would carry exactly that).
    let (policy, exp) = setup();
    let mut events = select_runtime(&parse(&event_log()).unwrap()).unwrap();
    let i = events
        .iter()
        .position(|e| e.event == EV_COMPOSE_HASH)
        .unwrap();
    let pin = events[i].payload.clone();
    events[i].payload = vec![0xEE; 32];
    events[i].served_digest = None;
    events.push(RuntimeEvent {
        event: EV_COMPOSE_HASH.into(),
        payload: pin,
        served_digest: None,
    });
    let doctored_rtmr3 = replay(&events).unwrap();
    // serialise the doctored log in the live (digest-less) form
    let wire: Vec<serde_json::Value> = events
        .iter()
        .map(|e| json!({"imr": 3, "event_type": 0x08000001u32, "digest": "", "event": e.event, "event_payload": hex::encode(&e.payload)}))
        .collect();
    let log = serde_json::to_vec(&wire).unwrap();
    let mut q = patched_quote(&PK, &NONCE);
    q[520..568].copy_from_slice(&doctored_rtmr3);
    let e = prefilter(
        &policy,
        &exp,
        &q,
        &log,
        &gpu_payload(&NONCE, None),
        GpuEvidenceMode::Real,
    )
    .unwrap_err();
    assert!(e.detail.contains("compose hash"), "{e}");
    assert!(
        !e.detail.contains("replayed rtmr3"),
        "the replay row PASSED; only extraction caught it: {e}"
    );
    // and the direct extractor agrees: first occurrence wins
    let r = extract(&events, doctored_rtmr3).unwrap();
    assert_eq!(r.compose_hash, [0xEE; 32]);
    let _ = (EV_SYSTEM_READY, replay_and_extract);
}

#[test]
fn check_policy_passes_on_the_recording() {
    let (policy, exp) = setup();
    let r = replay_and_extract(&event_log()).unwrap();
    let v = check_policy(&policy, &exp, &good_cpu(), &r, &good_gpu(), true).unwrap();
    assert_eq!(v.tcb_status, "Simulator");
    assert_eq!(v.hwmodel.as_deref(), Some("GH100 A01 GSP BROM"));
    assert_eq!(v.cc_mode, CcRecord::SignedNotDevTools);
    assert!(v.td_debug_off);
}

/// D14 superseded 2026-09-22 (Phala's answer to the G-6 question): a policy
/// asking for CC mode On is refused unless the SIGNED pair `secboot: true` +
/// `dbgstat` in the disabled family rules DevTools out, both read from the
/// EAT this broker verified against NVIDIA's JWKS for this nonce, rather than
/// from the node's own NVML reading. The pair does not separate On from Off
/// (G-6a); that stays with the measured collector.
#[test]
fn cc_mode_on_needs_the_signed_pair_that_rules_devtools_out() {
    let (mut policy, exp) = setup();
    policy.gpu.require_cc_mode = Some(fabstir_llm_node::tee::types::CcMode::On);
    // Belt and braces off, so the refusal can only come from the cc-mode row.
    policy.gpu.require_secure_boot = false;
    policy.gpu.require_debug_disabled = false;
    let r = replay_and_extract(&event_log()).unwrap();

    let v = check_policy(&policy, &exp, &good_cpu(), &r, &good_gpu(), true).unwrap();
    assert_eq!(v.cc_mode, CcRecord::SignedNotDevTools);

    // DevTools: the debug facilities are enabled.
    let e = check_policy(&policy, &exp, &good_cpu(), &r, &gpu_with(true, false), true).unwrap_err();
    assert!(
        e.detail.contains("cc mode") && e.detail.contains("debug disabled false"),
        "{e}"
    );
    // Secure boot off is not the On pair either.
    let e = check_policy(&policy, &exp, &good_cpu(), &r, &gpu_with(false, true), true).unwrap_err();
    assert!(e.detail.contains("cc mode"), "{e}");

    // A policy that does not ask for On records the state without refusing.
    // A TEST entry with no cc-mode row records the state without refusing.
    policy.gpu.require_cc_mode = None;
    let v = check_policy(&policy, &exp, &good_cpu(), &r, &gpu_with(true, false), true).unwrap();
    assert_eq!(v.cc_mode, CcRecord::SignedDevToolsOrNoSecureBoot);

    // A REAL entry with the same lax policy is refused at step 7, so the
    // capture carries the failing row (D11) instead of recording a pass that
    // step 8 then refuses.
    let e = check_policy(
        &policy,
        &exp,
        &good_cpu(),
        &r,
        &gpu_with(true, false),
        false,
    )
    .unwrap_err();
    assert!(
        e.detail.contains("cc mode") && e.detail.contains("whatever the policy asked"),
        "{e}"
    );
    // ... and the same real entry with the good pair still passes.
    let v = check_policy(&policy, &exp, &good_cpu(), &r, &good_gpu(), false).unwrap();
    assert_eq!(v.cc_mode, CcRecord::SignedNotDevTools);
}

#[test]
fn check_policy_rows_one_at_a_time() {
    let (policy, exp) = setup();
    let r = replay_and_extract(&event_log()).unwrap();
    let cpu = good_cpu();
    let gpu = good_gpu();
    let refuse = |p: &Policy, e: &Expected, c: &TdxEvidence, g: &GpuOutcome, needle: &str| {
        let err = check_policy(p, e, c, &r, g, true).unwrap_err();
        assert_eq!(err.kind, Kind::Verification);
        assert!(err.detail.contains(needle), "want {needle:?} in {err}");
    };
    // model id
    let mut e2 = exp.clone();
    e2.model_id = test_model_id(2);
    refuse(&policy, &e2, &cpu, &gpu, "model id");
    // identity / nonce
    let mut e2 = exp.clone();
    e2.pk_att = [0x03; 33];
    refuse(&policy, &e2, &cpu, &gpu, "identity:");
    let mut e2 = exp.clone();
    e2.nonce = [1; 32];
    refuse(&policy, &e2, &cpu, &gpu, "nonce:");
    // registers, event rows
    let mut p = policy.clone();
    p.cvm.mrtd = "00".repeat(48);
    refuse(&p, &exp, &cpu, &gpu, "mrtd:");
    let mut p = policy.clone();
    p.cvm.os_image_hash = "00".repeat(32);
    refuse(&p, &exp, &cpu, &gpu, "os image");
    let mut p = policy.clone();
    p.cvm.app_id = Some("00".repeat(20));
    refuse(&p, &exp, &cpu, &gpu, "app id");
    let mut p = policy.clone();
    p.cvm.key_provider = Some("00".repeat(10));
    refuse(&p, &exp, &cpu, &gpu, "key provider");
    let mut c2 = cpu.clone();
    c2.rt_mr3 = [0; 48];
    refuse(&policy, &exp, &c2, &gpu, "event log");
    // td debug, tcb status, advisories
    let mut c2 = cpu.clone();
    c2.td_debug = true;
    refuse(&policy, &exp, &c2, &gpu, "td debug");
    let mut c2 = cpu.clone();
    c2.tcb_status = "OutOfDate".into();
    refuse(&policy, &exp, &c2, &gpu, "tcb status");
    let mut c2 = cpu.clone();
    c2.advisory_ids = vec!["INTEL-SA-00001".into()];
    refuse(&policy, &exp, &c2, &gpu, "advisory");
    // gpu rows
    let mut p = policy.clone();
    p.gpu.allowed_hwmodels = vec!["PENDING-B3".into()];
    refuse(&p, &exp, &cpu, &gpu, "hwmodel");
    let mut p = policy.clone();
    p.gpu.min_driver_version = Some("560.0.0".into());
    refuse(&p, &exp, &cpu, &gpu, "driver version");
    let mut p = policy.clone();
    p.gpu.min_vbios_version = Some("97.0".into());
    refuse(&p, &exp, &cpu, &gpu, "vbios version");
    let GpuOutcome::Real(mut f) = gpu.clone() else {
        unreachable!()
    };
    f.secure_boot = false;
    refuse(
        &policy,
        &exp,
        &cpu,
        &GpuOutcome::Real(f.clone()),
        "secure boot",
    );
    f.secure_boot = true;
    f.debug_disabled = false;
    refuse(&policy, &exp, &cpu, &GpuOutcome::Real(f), "gpu debug");
    // malformed policy: require_cc_mode DevTools
    let mut p = policy.clone();
    p.gpu.require_cc_mode = Some(CcMode::DevTools);
    refuse(&p, &exp, &cpu, &gpu, "malformed");
    // a policy with require_cc_mode None passes
    let mut p = policy.clone();
    p.gpu.require_cc_mode = None;
    assert!(check_policy(&p, &exp, &cpu, &r, &gpu, true).is_ok());
}

#[test]
fn three_failing_rows_are_all_listed() {
    let (policy, exp) = setup();
    let r = replay_and_extract(&event_log()).unwrap();
    let mut cpu = good_cpu();
    cpu.td_debug = true;
    cpu.tcb_status = "OutOfDate".into();
    let mut p = policy.clone();
    p.gpu.allowed_hwmodels = vec!["PENDING-B3".into()];
    let e = check_policy(&p, &exp, &cpu, &r, &good_gpu(), true).unwrap_err();
    assert!(
        e.detail.contains("td debug")
            && e.detail.contains("tcb status")
            && e.detail.contains("hwmodel"),
        "{e}"
    );
}

#[test]
fn canned_tolerance_skips_gpu_rows_only_for_a_test_entry() {
    let (policy, exp) = setup();
    let r = replay_and_extract(&event_log()).unwrap();
    let v = check_policy(
        &policy,
        &exp,
        &good_cpu(),
        &r,
        &GpuOutcome::CannedTolerated,
        true,
    )
    .unwrap();
    assert_eq!(v.cc_mode, CcRecord::Canned);
    assert!(v.hwmodel.is_none());
    let e = check_policy(
        &policy,
        &exp,
        &good_cpu(),
        &r,
        &GpuOutcome::CannedTolerated,
        false,
    )
    .unwrap_err();
    assert!(e.detail.contains("test keyring entry"), "{e}");
}
