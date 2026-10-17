//! Policy schema 3 (V3.2): the broker's pure checks over candidate profiles
//! (D7–D9): every-row reporting over a list, the closest candidate, the
//! excluded/removed hints, the prefilter/step-7 split, profile floors and the
//! canned skip. Each test names the mutation that turns it red.

use super::policy_fixture::{
    authority_for, bundle, event_log, gpu_payload, patched_quote, recording_policy,
    recording_profile, test_model_id, HWMODEL, NOW,
};
use fabstir_llm_node::kbs::authority::{Authority, Removed};
use fabstir_llm_node::kbs::config::GpuEvidenceMode;
use fabstir_llm_node::kbs::cpu::{simulator, TdxEvidence};
use fabstir_llm_node::kbs::error::{KbsError, Kind};
use fabstir_llm_node::kbs::eventlog::{replay_and_extract, Replayed};
use fabstir_llm_node::kbs::nras_claims::{GpuFields, GpuOutcome};
use fabstir_llm_node::kbs::verify::{check_policy, prefilter, Expected, Verified};
use fabstir_llm_node::tee::host_profile::HostProfile;
use fabstir_llm_node::tee::types::Policy;

const PK: [u8; 33] = [0x02; 33];
const NONCE: [u8; 32] = [0x77; 32];

fn expected() -> Expected {
    Expected {
        model_id: test_model_id(1),
        pk_att: PK,
        nonce: NONCE,
    }
}

fn policy() -> Policy {
    recording_policy(test_model_id(1), 1)
}

fn cpu() -> TdxEvidence {
    simulator(&patched_quote(&PK, &NONCE)).unwrap()
}

fn events() -> Replayed {
    replay_and_extract(&event_log()).unwrap()
}

fn gpu(driver: &str) -> GpuOutcome {
    GpuOutcome::Real(GpuFields {
        nonce: NONCE,
        hwmodel: HWMODEL.into(),
        secure_boot: true,
        debug_disabled: true,
        driver_version: driver.into(),
        vbios_version: "96.00.74.00.1a".into(),
    })
}

fn auth(policy: &Policy, profiles: Vec<HostProfile>) -> Authority {
    authority_for(policy, &bundle(1, 1, profiles), NOW)
}

fn check(policy: &Policy, a: &Authority, g: &GpuOutcome, test: bool) -> Result<Verified, KbsError> {
    check_policy(policy, a, &expected(), &cpu(), &events(), g, test, NOW)
}

fn pre(policy: &Policy, a: &Authority) -> Result<(), KbsError> {
    prefilter(
        policy,
        a,
        &expected(),
        &patched_quote(&PK, &NONCE),
        &event_log(),
        &gpu_payload(&NONCE, None),
        GpuEvidenceMode::Real,
        NOW,
    )
    .map(|_| ())
}

fn named(id: &str) -> HostProfile {
    HostProfile {
        id: id.into(),
        ..recording_profile()
    }
}

#[test]
fn authoritative_check_rechecks_identity_on_verified_values() {
    // The prefilter saw a matching (unverified) quote; the VERIFIED registers
    // differ. Mutation: step 7 trusts the prefilter's survivors → releases.
    let p = policy();
    let a = auth(&p, vec![recording_profile()]);
    pre(&p, &a).expect("the unverified quote matches");
    let mut verified_cpu = cpu();
    verified_cpu.rt_mr1[0] ^= 1;
    let e = check_policy(
        &p,
        &a,
        &expected(),
        &verified_cpu,
        &events(),
        &gpu("550.90.07"),
        true,
        NOW,
    )
    .unwrap_err();
    assert!(e.detail.contains("rtmr1: "), "{e}");
    let v = check(&p, &a, &gpu("550.90.07"), true).unwrap();
    assert_eq!(v.profile.headline, recording_profile().id);
}

#[test]
fn closest_candidate_named_with_rows_and_counts() {
    // "a" fails three rows, "b" fails one: the closest is "b" although "a" is
    // first by id; "c" and "d" tie on one row each with "b" → "b" (by id).
    // Mutation: report the first candidate / break ties otherwise.
    let p = policy();
    let mut a3 = named("a");
    a3.cvm.mrtd = "00".repeat(48);
    a3.cvm.rtmr0 = "00".repeat(48);
    a3.cvm.os_image_hash = "00".repeat(32);
    let mut d1 = named("d");
    d1.cvm.rtmr2 = "00".repeat(48);
    let mut b1 = named("b");
    b1.cvm.rtmr1 = "00".repeat(48);
    let mut c1 = named("c");
    c1.cvm.compose_hashes = vec!["00".repeat(32)];
    let a = auth(&p, vec![a3, d1, c1, b1]);
    let e = check(&p, &a, &gpu("550.90.07"), true).unwrap_err();
    assert_eq!(e.kind, Kind::Verification);
    let rows: Vec<&str> = e.detail.split("; ").collect();
    assert_eq!(
        rows[0], "no profile matches: closest is profile b (1 failing row(s))",
        "{e}"
    );
    assert!(
        rows[1].starts_with("rtmr1: ") && rows[1].contains("!= profile b"),
        "{e}"
    );
    assert!(
        rows[2].starts_with("candidates=4 of 4 (expired=0, class_filtered=0, unpinned=0)"),
        "{e}"
    );
    // The prefilter prints the same headline.
    let e = pre(&p, &a).unwrap_err();
    assert!(e.detail.contains("closest is profile b"), "{e}");
}

#[test]
fn excluded_but_matching_profile_is_named_per_reason() {
    // Every excluded profile whose identity rows all hold is named with its
    // reason; the only candidate does not match. Mutation: drop any one hint list.
    let mut p = policy();
    let mut expired = named("old");
    expired.expiry = NOW - 1;
    expired.not_before = NOW - 2;
    let mut other_class = named("cls");
    other_class.class = "other".into();
    let mut unpinned = named("unp");
    (unpinned.cvm.app_id, unpinned.cvm.key_provider) = (None, None);
    let mut miss = named("miss");
    miss.cvm.mrtd = "00".repeat(48);
    (
        p.cvm.require_app_id_pinned,
        p.cvm.require_key_provider_pinned,
    ) = (true, true);
    let mut a = auth(&p, vec![expired, other_class, unpinned, miss]);
    a.removed = vec![Removed {
        profile: named("gone"),
        epoch: 1,
        archive_file: "00000000000000000000-0000000001-aa-bb.json".into(),
    }];
    let e = check(&p, &a, &gpu("550.90.07"), true).unwrap_err();
    let missing: Vec<String> = [
        format!("matches old but expired at {}", NOW - 1),
        "matches cls but class other is not allowed by the policy".into(),
        "matches unp but the policy requires a pinned app id".into(),
        "matches gone, present in the archived bundle 00000000000000000000-0000000001-aa-bb.json but absent from epoch 1".into(),
    ].into_iter().filter(|h| !e.detail.contains(h.as_str())).collect();
    assert!(missing.is_empty(), "missing HINTS {missing:?} in {e}");
    assert!(
        e.detail
            .contains("candidates=1 of 4 (expired=1, class_filtered=1, unpinned=1)"),
        "{e}"
    );
    // A non-matching excluded profile is NOT named.
    assert!(!e.detail.contains("matches miss"), "{e}");
}

#[test]
fn zero_candidates_names_the_reason() {
    // Mutation: a generic "no candidate" with no counts or hints.
    let mut p = policy();
    (
        p.cvm.require_app_id_pinned,
        p.cvm.require_key_provider_pinned,
    ) = (true, true);
    let mut unp = named("only");
    (unp.cvm.app_id, unp.cvm.key_provider) = (None, None);
    let a = auth(&p, vec![unp]);
    let e = pre(&p, &a).unwrap_err();
    assert!(
        e.detail
            .contains("no profile matches: no candidate profile"),
        "{e}"
    );
    assert!(
        e.detail
            .contains("candidates=0 of 1 (expired=0, class_filtered=0, unpinned=1)"),
        "{e}"
    );
    assert!(
        e.detail
            .contains("matches only but the policy requires a pinned app id"),
        "{e}"
    );
}

#[test]
fn profile_floor_applies_independently_and_malformed_floor_refuses() {
    // Two matching profiles, each with its own driver floor: BOTH apply
    // (no stricter-of). Mutations: skip a matched profile's floors; skip a
    // malformed floor instead of refusing.
    let p = policy();
    let mut lo = named("lo");
    lo.gpu.min_driver_version = Some("550.0".into());
    let mut hi = named("hi");
    hi.gpu.min_driver_version = Some("560.0".into());
    let a = auth(&p, vec![lo.clone(), hi]);
    let e = check(&p, &a, &gpu("555.0"), true).unwrap_err();
    assert!(
        e.detail
            .contains("driver version: 555.0 below floor 560.0 of profile hi"),
        "{e}"
    );
    assert!(!e.detail.contains("of profile lo"), "{e}");
    check(&p, &auth(&p, vec![lo.clone()]), &gpu("555.0"), true).expect("lo alone passes");
    // A malformed floor (bypassing the loader's form check) refuses.
    let mut a = auth(&p, vec![lo]);
    a.set.set.profiles[0].gpu.min_vbios_version = Some("zz".into());
    let e = check(&p, &a, &gpu("555.0"), true).unwrap_err();
    assert!(
        e.detail
            .contains("vbios version: 96.00.74.00.1a below floor zz of profile lo"),
        "{e}"
    );
}

#[test]
fn canned_test_entry_skips_all_gpu_rows_including_profile_floors() {
    // Mutation: apply a profile floor (or the hwmodel row) under canned evidence.
    let p = policy();
    let mut hi = named("hi");
    hi.gpu.min_driver_version = Some("999.0".into());
    hi.gpu.hwmodels = vec!["NOT-THIS-GPU".into()];
    let a = auth(&p, vec![hi]);
    let v = check(&p, &a, &GpuOutcome::CannedTolerated, true).unwrap();
    assert_eq!(v.profile.headline, "hi");
    // ...and never for a real entry.
    let e = check(&p, &a, &GpuOutcome::CannedTolerated, false).unwrap_err();
    assert!(
        e.detail
            .contains("canned GPU evidence tolerated only for a test keyring entry"),
        "{e}"
    );
}

#[test]
fn hwmodel_outside_profile_or_policy_refuses_after_nras() {
    // The verified hwmodel must be in the profile's list AND the policy's.
    // Mutation: drop either list.
    let mut p = policy();
    let mut prof = recording_profile();
    prof.gpu.hwmodels.push("GB100".into());
    let a = auth(&p, vec![prof.clone()]);
    let gb = GpuOutcome::Real(GpuFields {
        hwmodel: "GB100".into(),
        ..match gpu("550.90.07") {
            GpuOutcome::Real(f) => f,
            _ => unreachable!(),
        }
    });
    let e = check(&p, &a, &gb, true).unwrap_err();
    assert!(
        e.detail.contains(r#"hwmodel: "GB100" not in policy"#),
        "{e}"
    );
    p.gpu.allowed_hwmodels.push("GB100".into());
    check(&p, &auth(&p, vec![prof]), &gb, true).expect("in both lists");
    let a = auth(&p, vec![recording_profile()]);
    let e = check(&p, &a, &gb, true).unwrap_err();
    assert!(
        e.detail.contains(r#"hwmodel: "GB100" not in profile"#),
        "{e}"
    );
    // The prefilter could not know the GPU: it passed both.
    pre(&p, &a).expect("hwmodel is deferred to step 7");
}
