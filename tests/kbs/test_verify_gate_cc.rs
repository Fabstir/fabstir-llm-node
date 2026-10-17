//! The CC half of the independent second gate (design §6 step 8, D4/D14a),
//! moved from `test_routes.rs` (400-line cap) and adapted to the schema-3
//! `release_gate` signature (D21). The authority half sees the pristine
//! recording documents here, so every positive case proves the WHOLE gate
//! passes and every refusal is the CC half's.

use super::policy_fixture::{
    bundle, event_log, patched_quote, real_model_id, recording_policy, recording_profile, NOW,
};
use fabstir_llm_node::kbs::cpu::{simulator, TdxEvidence};
use fabstir_llm_node::kbs::error::Kind;
use fabstir_llm_node::kbs::eventlog::{replay_and_extract, Replayed};
use fabstir_llm_node::kbs::nras_claims::{GpuFields, GpuOutcome};
use fabstir_llm_node::kbs::verify::{check_policy, CcRecord, Expected, Verified};
use fabstir_llm_node::tee::host_profile::AuthorityBundle;
use fabstir_llm_node::tee::types::Policy;

const PK: [u8; 33] = [0x02; 33];
const NONCE: [u8; 32] = [0x77; 32];

fn policy() -> Policy {
    recording_policy(real_model_id(1), 1)
}

fn cpu() -> TdxEvidence {
    simulator(&patched_quote(&PK, &NONCE)).unwrap()
}

fn events() -> Replayed {
    replay_and_extract(&event_log()).unwrap()
}

/// Step 7 on the pristine documents: the profile record the gate is handed.
fn step7(p: &Policy, b: &AuthorityBundle) -> Verified {
    let a = super::policy_fixture::authority_for(p, b, NOW);
    let exp = Expected {
        model_id: real_model_id(1),
        pk_att: PK,
        nonce: NONCE,
    };
    let gpu = GpuOutcome::Real(GpuFields {
        nonce: NONCE,
        hwmodel: super::policy_fixture::HWMODEL.into(),
        secure_boot: true,
        debug_disabled: true,
        driver_version: "580.95.05".into(),
        vbios_version: "96.00.74.00.1a".into(),
    });
    check_policy(p, &a, &exp, &cpu(), &events(), &gpu, false, NOW).unwrap()
}

#[test]
fn release_gate_is_its_own_check() {
    // Moved from test_routes.rs (400-line cap) with the schema-3 gate
    // signature: the CC half runs first; the authority half (D21) sees the
    // pristine recording documents, so every positive case still proves the
    // whole gate passes.
    let (b, p, cpu, r) = (
        bundle(1, 1, vec![recording_profile()]),
        policy(),
        cpu(),
        events(),
    );
    let release_gate = |v: &Verified, g: &GpuOutcome, test: bool| {
        fabstir_llm_node::kbs::routes::release_gate(
            &b.manifest,
            &b.set,
            NOW,
            &cpu,
            &r,
            g,
            &p,
            v,
            test,
        )
    };
    let verified_of = |cc: CcRecord| Verified {
        tcb_status: "Simulator".into(),
        advisory_ids: vec![],
        hwmodel: None,
        driver_version: None,
        vbios_version: None,
        cc_mode: cc,
        td_debug_off: true,
        profile: step7(&p, &b).profile,
    };
    let gpu_of = |secure_boot: bool, debug_disabled: bool| {
        GpuOutcome::Real(GpuFields {
            nonce: [7u8; 32],
            hwmodel: "GH100 A01 GSP BROM".into(),
            secure_boot,
            debug_disabled,
            driver_version: "580.95.05".into(),
            vbios_version: "96.00.74.00.1a".into(),
        })
    };
    let good = gpu_of(true, true);

    // Canned tolerance: a test entry may, a real entry may not.
    let canned = verified_of(CcRecord::Canned);
    assert!(release_gate(&canned, &GpuOutcome::CannedTolerated, true).is_ok());
    let e = release_gate(&canned, &GpuOutcome::CannedTolerated, false).unwrap_err();
    assert!(e.detail.contains("canned tolerance"), "{e}");

    // The ordinary real release.
    let real = verified_of(CcRecord::SignedNotDevTools);
    assert!(release_gate(&real, &good, false).is_ok());

    // D14a: the gate is independent of the policy, so claims that leave
    // DevTools open refuse a real release even when no policy row asked.
    let e = release_gate(
        &verified_of(CcRecord::SignedDevToolsOrNoSecureBoot),
        &gpu_of(true, false),
        false,
    )
    .unwrap_err();
    assert!(
        e.detail.contains("rule DevTools out")
            && e.detail.contains("secboot true")
            && e.detail.contains("debug disabled false"),
        "the gate must name the failing claims: {e}"
    );
    assert!(
        release_gate(
            &verified_of(CcRecord::SignedDevToolsOrNoSecureBoot),
            &gpu_of(true, false),
            true
        )
        .is_ok(),
        "a test entry may"
    );

    // The gate derives from `map_per_gpu`'s OWN output, so a step-7 bug that
    // recorded the good verdict over bad claims is still refused here, and the
    // BAD GPU leads: no key can go to it whatever the broker did. The step-7
    // bug is named in the same line rather than replacing it.
    let e = release_gate(&real, &gpu_of(true, false), false).unwrap_err();
    assert!(
        e.detail.contains("claims do not rule DevTools out")
            && e.detail.contains("debug disabled false")
            && e.detail.contains("also recorded signed-not-devtools"),
        "a bad GPU must lead, with the step-7 disagreement named too: {e}"
    );
    // ... and with step 7 faithful, the same GPU gives the same lead without
    // the disagreement clause, so that clause cannot be unconditional text.
    let e = release_gate(
        &verified_of(CcRecord::SignedDevToolsOrNoSecureBoot),
        &gpu_of(true, false),
        false,
    )
    .unwrap_err();
    assert!(
        e.detail.contains("claims do not rule DevTools out") && !e.detail.contains("also recorded"),
        "a faithful record must not be reported as a disagreement: {e}"
    );

    // ... and the mirror: good claims, a bad recorded verdict. Mutation that
    // proves this arm can fail: delete the `verified.cc_mode != expected`
    // agreement block (the claims pass, so the release would go out).
    let e = release_gate(
        &verified_of(CcRecord::SignedDevToolsOrNoSecureBoot),
        &good,
        false,
    )
    .unwrap_err();
    assert!(
        e.detail
            .contains("recorded signed-devtools-or-no-secure-boot")
            && e.detail.contains("disagreement inside the broker")
            && !e.detail.contains("claims do not rule DevTools out"),
        "a broker disagreement must not read as a bad GPU: {e}"
    );

    // Every refusal is a `verification` (403), never an `unavailable` (500):
    // none of them will pass on a later attempt, so none may tell the reader
    // to come back later. Mutation: change any arm back to `KbsError::fault`.
    for e in [
        release_gate(&canned, &GpuOutcome::CannedTolerated, false).unwrap_err(),
        release_gate(&real, &GpuOutcome::CannedTolerated, false).unwrap_err(),
        release_gate(&canned, &good, false).unwrap_err(),
        release_gate(
            &verified_of(CcRecord::SignedDevToolsOrNoSecureBoot),
            &good,
            false,
        )
        .unwrap_err(),
        release_gate(&real, &gpu_of(true, false), false).unwrap_err(),
    ] {
        assert_eq!(
            (e.kind, e.status),
            (Kind::Verification, 403),
            "release-gate refusals must be terminal, not a retryable outage: {e}"
        );
    }

    // Both record-over-evidence contradictions are broker bugs, not bad GPUs.
    let e = release_gate(&real, &GpuOutcome::CannedTolerated, false).unwrap_err();
    assert!(
        e.detail.contains("evidence was canned")
            && e.detail.contains("disagreement inside the broker"),
        "{e}"
    );
    let e = release_gate(&canned, &good, false).unwrap_err();
    assert!(
        e.detail.contains("recorded canned")
            && e.detail.contains("disagreement inside the broker")
            && !e.detail.contains("canned tolerance"),
        "a Canned record over real evidence must read as a broker bug: {e}"
    );

    // The claims arm has its own failure mode: a GPU whose claims leave
    // DevTools open, with step 7 recording that state faithfully, is refused
    // as a bad GPU rather than as a broker bug. Mutation that proves it:
    // delete the `!matches!(derived, SignedNotDevTools)` block.
    let e = release_gate(
        &verified_of(CcRecord::SignedDevToolsOrNoSecureBoot),
        &gpu_of(false, true),
        false,
    )
    .unwrap_err();
    assert!(
        e.detail.contains("do not rule DevTools out")
            && e.detail.contains("secboot false")
            && !e.detail.contains("disagreement inside the broker"),
        "{e}"
    );
}
