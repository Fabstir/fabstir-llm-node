// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Policy schema 3 (V3.1): the pure profile matcher the broker uses (D7, D8,
//! D12) and the node's mock verifier against a profile (D13). Split from
//! `test_verify.rs` (400-line cap). Each test names the mutation that turns it
//! red.

use super::profile_fixture::{a_policy, a_profile, now_unix, CLASS};
use fabstir_llm_node::tee::host_profile::{
    match_profile, matching_ids, HostProfile, HwModel, Observed,
};
use fabstir_llm_node::tee::mock::MockAttestationProvider;
use fabstir_llm_node::tee::provider::AttestationProvider;
use fabstir_llm_node::tee::types::{CcMode, Claims, Policy, TeeError};
use fabstir_llm_node::tee::verifier::DefaultVerifier;

const ID: &str = "test/profile-1";

/// A profile whose every identity row has a distinct value and both pins set.
fn pinned_profile() -> HostProfile {
    let mut p = a_profile([0x11; 48], &["GH100"]);
    p.cvm.rtmr0 = "22".repeat(48);
    p.cvm.rtmr1 = "33".repeat(48);
    p.cvm.rtmr2 = "44".repeat(48);
    p.cvm.os_image_hash = "55".repeat(32);
    p.cvm.compose_hashes = vec!["66".repeat(32), "67".repeat(32)];
    p.cvm.app_id = Some("77".repeat(20));
    p.cvm.key_provider = Some("88".repeat(10));
    p
}

/// What a host matching `p` shows.
fn observed_for(p: &HostProfile) -> Observed {
    let b48 = |h: &str| -> [u8; 48] { hex::decode(h).unwrap().try_into().unwrap() };
    let b32 = |h: &str| -> [u8; 32] { hex::decode(h).unwrap().try_into().unwrap() };
    Observed {
        mr_td: b48(&p.cvm.mrtd),
        rt_mr0: b48(&p.cvm.rtmr0),
        rt_mr1: b48(&p.cvm.rtmr1),
        rt_mr2: b48(&p.cvm.rtmr2),
        compose_hash: b32(&p.cvm.compose_hashes[1]),
        os_image_hash: b32(&p.cvm.os_image_hash),
        app_id: p.cvm.app_id.as_deref().map(|h| hex::decode(h).unwrap()),
        key_provider: p
            .cvm
            .key_provider
            .as_deref()
            .map(|h| hex::decode(h).unwrap()),
        hwmodel: HwModel::Known(p.gpu.hwmodels[0].clone()),
    }
}

/// The policy allows GB100 too, so flipping the GPU to GB100 fails ONLY the
/// profile's list.
fn policy_hwmodels() -> Vec<String> {
    vec!["GH100".into(), "GB100".into()]
}

type Flip = fn(&mut Observed);

/// One flip per identity row, with the row prefix it must produce.
fn flips() -> Vec<(&'static str, Flip)> {
    vec![
        ("mrtd: ", |o| o.mr_td[0] ^= 1),
        ("rtmr0: ", |o| o.rt_mr0[0] ^= 1),
        ("rtmr1: ", |o| o.rt_mr1[0] ^= 1),
        ("rtmr2: ", |o| o.rt_mr2[0] ^= 1),
        ("compose hash: ", |o| o.compose_hash[0] ^= 1),
        ("os image: ", |o| o.os_image_hash[0] ^= 1),
        ("app id: ", |o| o.app_id.as_mut().unwrap()[0] ^= 1),
        ("key provider: ", |o| {
            o.key_provider.as_mut().unwrap()[0] ^= 1
        }),
        ("hwmodel: ", |o| o.hwmodel = HwModel::Known("GB100".into())),
    ]
}

#[test]
fn match_profile_rows_one_at_a_time() {
    // Mutation: drop any one compare → its flip yields no row → red.
    let p = pinned_profile();
    let pristine = observed_for(&p);
    assert!(match_profile(&p, &pristine, &policy_hwmodels()).is_empty());
    for (prefix, flip) in flips() {
        let mut o = pristine.clone();
        flip(&mut o);
        let rows = match_profile(&p, &o, &policy_hwmodels());
        assert_eq!(rows.len(), 1, "{prefix}: {rows:?}");
        assert!(rows[0].starts_with(prefix), "{rows:?}");
        assert!(rows[0].contains(&format!("profile {ID}")), "{rows:?}");
    }
    // A pinned event that is absent fails too.
    let mut o = pristine.clone();
    o.app_id = None;
    let rows = match_profile(&p, &o, &policy_hwmodels());
    assert_eq!(
        rows,
        vec![format!("app id: event absent, profile {ID} pins one")]
    );
    // An unpinned profile accepts any app id / key provider.
    let mut unpinned = p.clone();
    (unpinned.cvm.app_id, unpinned.cvm.key_provider) = (None, None);
    let mut o = pristine.clone();
    (o.app_id, o.key_provider) = (Some(vec![1]), None);
    assert!(match_profile(&unpinned, &o, &policy_hwmodels()).is_empty());
}

#[test]
fn match_profile_reports_every_failing_row() {
    // Mutation: return on the first failing row → 1 row, not 9.
    let p = pinned_profile();
    let mut o = observed_for(&p);
    for (_, flip) in flips() {
        flip(&mut o);
    }
    let rows = match_profile(&p, &o, &policy_hwmodels());
    assert_eq!(rows.len(), 9, "{rows:#?}");
    for ((prefix, _), row) in flips().iter().zip(&rows) {
        assert!(row.starts_with(prefix), "{row}");
    }
}

#[test]
fn hwmodel_row_names_which_list_failed() {
    // Mutation: merge the two rows into one that does not say which list.
    let mut p = pinned_profile();
    p.gpu.hwmodels = vec!["GH100".into(), "XX100".into()];
    let row = |got: &str, policy: &[&str]| {
        let mut o = observed_for(&p);
        o.hwmodel = HwModel::Known(got.into());
        let policy: Vec<String> = policy.iter().map(|s| s.to_string()).collect();
        match_profile(&p, &o, &policy)
    };
    // In the policy, not the profile.
    let r = row("GB100", &["GH100", "GB100"]);
    assert_eq!(
        r,
        vec![format!(
            r#"hwmodel: "GB100" not in profile {ID} ["GH100", "XX100"]"#
        )]
    );
    // In the profile, not the policy.
    let r = row("XX100", &["GH100"]);
    assert_eq!(
        r,
        vec![r#"hwmodel: "XX100" not in policy ["GH100"]"#.to_string()]
    );
    // In neither: both rows.
    let r = row("ZZ1", &["GH100"]);
    assert_eq!(r.len(), 2, "{r:?}");
    assert!(
        r[0].contains("not in profile") && r[1].contains("not in policy"),
        "{r:?}"
    );
}

#[test]
fn hwmodel_row_skipped_only_for_canned_test_entry() {
    // Mutation: skip the hwmodel row always → the Known case yields nothing.
    let p = pinned_profile();
    let mut o = observed_for(&p);
    o.hwmodel = HwModel::Known("bogus".into());
    assert_eq!(match_profile(&p, &o, &policy_hwmodels()).len(), 2);
    o.hwmodel = HwModel::CannedTestEntry;
    assert!(match_profile(&p, &o, &policy_hwmodels()).is_empty());
    // The prefilter runs before NRAS: the row is deferred, not passed for good
    // (step 7 re-runs it on the verified value).
    o.hwmodel = HwModel::NotYetKnown;
    assert!(match_profile(&p, &o, &policy_hwmodels()).is_empty());
}

#[test]
fn several_matches_headline_is_first_by_id() {
    // Mutation: keep candidate order / take the last → headline is not "a".
    let base = pinned_profile();
    let named = |id: &str| HostProfile {
        id: id.into(),
        ..base.clone()
    };
    let mut miss = named("0-misses");
    miss.cvm.mrtd = "99".repeat(48);
    let (c, a, b) = (named("c"), named("a"), named("b"));
    let o = observed_for(&base);
    let ids = matching_ids(&[&c, &miss, &a, &b], &o, &policy_hwmodels());
    assert_eq!(ids, vec!["a", "b", "c"]);
    assert_eq!(ids.first().map(String::as_str), Some("a"), "headline");
}

// ---- the node's mock path (D13) ----

const MEAS: [u8; 48] = [9u8; 48];
const PK_ATT: [u8; 33] = [2u8; 33];
const NONCE: [u8; 32] = [7u8; 32];

fn verify(gpu: &str, policy: &Policy, profile: &HostProfile, now: u64) -> Result<Claims, TeeError> {
    let p = MockAttestationProvider::new(gpu, MEAS, CcMode::On).with_driver_version("580.95.05");
    let ev = futures::executor::block_on(p.gather_evidence(NONCE, &PK_ATT)).unwrap();
    DefaultVerifier.verify_at(&ev, policy, profile, NONCE, now)
}

#[test]
fn mock_verifier_compares_mrtd_and_hwmodel_against_the_profile_and_window_and_class() {
    // Mutation per case: drop that compare (mrtd, profile hwmodel, policy
    // hwmodel, profile window, class, pin requirement, profile floor) → it
    // releases → red.
    let now = now_unix();
    let policy = a_policy([1u8; 32], 0, u64::MAX - 1, &["H100", "H200"]);
    let good = a_profile(MEAS, &["H100", "H200"]);
    verify("H100", &policy, &good, now).expect("the good host releases");
    let refused = |gpu: &str, pol: &Policy, prof: &HostProfile, at: u64, needle: &str| match verify(
        gpu, pol, prof, at,
    ) {
        Err(TeeError::VerificationFailed(m)) => assert!(m.contains(needle), "{needle}: {m}"),
        other => panic!("{needle}: want VerificationFailed, got {other:?}"),
    };
    // mrtd against the PROFILE.
    let mut p = good.clone();
    p.cvm.mrtd = hex::encode([8u8; 48]);
    refused("H100", &policy, &p, now, "mrtd: ");
    // hwmodel against the profile's list (the policy allows H200).
    let mut p = good.clone();
    p.gpu.hwmodels = vec!["H100".into()];
    refused("H200", &policy, &p, now, "not in profile");
    // hwmodel against the policy's list (the profile allows H200).
    let mut pol = policy.clone();
    pol.gpu.allowed_hwmodels = vec!["H100".into()];
    refused("H200", &pol, &good, now, "not in policy");
    // The profile's own window, inclusive at both ends.
    verify("H100", &policy, &good, good.not_before).expect("not_before inclusive");
    verify("H100", &policy, &good, good.expiry).expect("expiry inclusive");
    refused(
        "H100",
        &policy,
        &good,
        good.expiry + 1,
        "outside its window",
    );
    refused(
        "H100",
        &policy,
        &good,
        good.not_before - 1,
        "outside its window",
    );
    // A class the policy does not allow.
    let mut p = good.clone();
    p.class = format!("{CLASS}-other");
    refused("H100", &policy, &p, now, "is not allowed by the policy");
    // The policy requires pins; the profile has none.
    let mut pol = policy.clone();
    (
        pol.cvm.require_app_id_pinned,
        pol.cvm.require_key_provider_pinned,
    ) = (true, true);
    refused("H100", &pol, &good, now, "requires a pinned app id");
    // The key provider alone (the app id pin is optional on its own).
    pol.cvm.require_app_id_pinned = false;
    refused("H100", &pol, &good, now, "requires a pinned key provider");
    // The profile's floor applies on top of the policy's (which has none).
    let mut p = good.clone();
    p.gpu.min_driver_version = Some("581.0".into());
    refused(
        "H100",
        &policy,
        &p,
        now,
        "below profile test/profile-1 minimum 581.0",
    );
    // ...and its VBIOS floor too.
    let mut p = good.clone();
    p.gpu.min_vbios_version = Some("99.0".into());
    refused(
        "H100",
        &policy,
        &p,
        now,
        "vbios 96.00.9f.00.01 below profile",
    );
    // Independently, not merged: a policy floor above the profile's still
    // refuses (a `profile.or(policy)` merge would pass 580.95.05 here).
    let mut pol = policy.clone();
    pol.gpu.min_driver_version = Some("581.0".into());
    let mut p = good.clone();
    p.gpu.min_driver_version = Some("580.0".into());
    refused("H100", &pol, &p, now, "below policy minimum 581.0");
}
