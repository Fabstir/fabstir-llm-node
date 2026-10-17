// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Policy schema 3 (V3.1): the bound policy's rules and the host profile's
//! rules, one case per rule (D3, D5). Mutation for every case: drop that one
//! rule → the case validates → red.

use super::profile_fixture::{a_policy, a_profile, a_set, ROOT};
use fabstir_llm_node::tee::host_profile::{
    parse_signed_document, HostProfile, HostProfileSet, SignedHostProfileSet, MAX_PROFILE_LIFETIME,
    SET_DOC,
};
use fabstir_llm_node::tee::policy::SignedModelPolicy;
use fabstir_llm_node::tee::types::{
    Policy, TeeError, TeeResult, POLICY_DOC, POLICY_SCHEMA_VERSION,
};

/// (expected needle, the one rule-breaking mutation).
type Case<T> = (&'static str, fn(&mut T));

fn assert_refused(res: TeeResult<()>, needle: &str) {
    match res {
        Err(TeeError::VerificationFailed(m)) => {
            assert!(m.contains(needle), "want {needle:?} in: {m}")
        }
        other => panic!("want VerificationFailed containing {needle:?}, got {other:?}"),
    }
}

fn policy() -> Policy {
    a_policy([3u8; 32], 0, u64::MAX - 1, &["GH100"])
}

#[test]
fn policy_rules_one_at_a_time() {
    policy().validate().expect("the fixture policy is valid");
    let cases: Vec<Case<Policy>> = vec![
        ("allowed_classes is empty", |p| {
            p.host_authority.allowed_classes.clear()
        }),
        ("allowed_classes[1]", |p| {
            p.host_authority.allowed_classes.push("Bad_Class".into())
        }),
        ("allowed_classes[0]", |p| {
            p.host_authority.allowed_classes[0] = "-lead".into()
        }),
        ("allowed_hwmodels is empty", |p| {
            p.gpu.allowed_hwmodels.clear()
        }),
        ("min_threshold is 0", |p| p.host_authority.min_threshold = 0),
        ("host_authority.root", |p| {
            p.host_authority.root = ROOT.trim_start_matches("0x").into()
        }),
        ("host_authority.root", |p| {
            p.host_authority.root = ROOT.to_uppercase().replace("0X", "0x")
        }),
        (
            "require_app_id_pinned without require_key_provider_pinned",
            |p| p.cvm.require_app_id_pinned = true,
        ),
        ("allowed_tcb_status is empty", |p| {
            p.cvm.allowed_tcb_status.clear()
        }),
        ("gpu.min_driver_version", |p| {
            p.gpu.min_driver_version = Some("r580".into())
        }),
        ("doc", |p| p.doc = "fabstir-model-release-policy ".into()),
    ];
    for (needle, mutate) in cases {
        let mut p = policy();
        mutate(&mut p);
        assert_refused(p.validate(), needle);
    }
    // Pinning both is fine; pinning only the key provider is fine.
    let mut p = policy();
    (
        p.cvm.require_app_id_pinned,
        p.cvm.require_key_provider_pinned,
    ) = (true, true);
    p.validate().expect("both pins");
    p.cvm.require_app_id_pinned = false;
    p.validate().expect("key provider alone");
    // A policy JSON without `doc` is refused before the typed parse.
    let mut v = serde_json::to_value(SignedModelPolicy {
        policy: policy(),
        encrypted_ref: "x".into(),
        signer: String::new(),
        signature: vec![],
    })
    .unwrap();
    v["policy"].as_object_mut().unwrap().remove("doc");
    let res: TeeResult<SignedModelPolicy> = parse_signed_document(
        &serde_json::to_vec(&v).unwrap(),
        "policy",
        "/policy",
        POLICY_DOC,
        POLICY_SCHEMA_VERSION,
    );
    assert_refused(res.map(|_| ()), "no doc tag");
}

fn profile() -> HostProfile {
    a_profile([1u8; 48], &["GH100"])
}

fn set_of(profiles: Vec<HostProfile>) -> HostProfileSet {
    a_set(ROOT, 1, profiles)
}

#[test]
fn profile_rules_one_at_a_time() {
    set_of(vec![profile()])
        .validate()
        .expect("the fixture profile is valid");
    let cases: Vec<Case<HostProfile>> = vec![
        ("400 day", |p| {
            p.expiry = p.not_before + MAX_PROFILE_LIFETIME + 1
        }),
        ("window inverted", |p| p.expiry = p.not_before - 1),
        ("cvm.compose_hashes: empty", |p| {
            p.cvm.compose_hashes.clear()
        }),
        ("cvm.compose_hashes[1]", |p| {
            p.cvm.compose_hashes.push("AB".repeat(32))
        }),
        ("gpu.hwmodels: empty", |p| p.gpu.hwmodels.clear()),
        ("cvm.app_id", |p| {
            p.cvm.key_provider = Some("ab".into());
            p.cvm.app_id = Some("ab".repeat(21));
        }),
        ("cvm.key_provider", |p| {
            p.cvm.key_provider = Some("abc".into())
        }),
        ("cvm.key_provider", |p| {
            p.cvm.key_provider = Some(String::new())
        }),
        ("cvm.key_provider", |p| {
            p.cvm.key_provider = Some("ab".repeat(513))
        }),
        ("cvm.key_provider", |p| {
            p.cvm.key_provider = Some("0xab".into())
        }),
        ("pinned without cvm.key_provider", |p| {
            p.cvm.app_id = Some("ab".repeat(20))
        }),
        ("gpu.min_vbios_version", |p| {
            p.gpu.min_vbios_version = Some("96..00".into())
        }),
        ("gpu.min_driver_version", |p| {
            p.gpu.min_driver_version = Some("N/A".into())
        }),
        ("profile id", |p| p.id = "Upper/case".into()),
        ("profile id", |p| p.id = "a".repeat(129)),
        ("class", |p| p.class = "a/b".into()),
        ("cvm.mrtd", |p| p.cvm.mrtd.push('0')),
        ("cvm.rtmr0", |p| p.cvm.rtmr0.truncate(94)),
        ("cvm.rtmr2", |p| p.cvm.rtmr2 = "AB".repeat(48)),
        ("cvm.os_image_hash", |p| {
            p.cvm.os_image_hash = "0x".to_string() + &"0".repeat(62)
        }),
        ("evidence", |p| p.evidence = "e".repeat(257)),
        ("evidence", |p| p.evidence = "line\nbreak".into()),
    ];
    for (needle, mutate) in cases {
        let mut p = profile();
        mutate(&mut p);
        assert_refused(set_of(vec![p]).validate(), needle);
    }
    // The longest legal key provider (1024 hex) and window (400 days) pass.
    let mut p = profile();
    p.cvm.key_provider = Some("ab".repeat(512));
    p.cvm.app_id = Some("cd".repeat(20));
    p.expiry = p.not_before + MAX_PROFILE_LIFETIME;
    set_of(vec![p]).validate().expect("bounds are inclusive");
    // Set-level: duplicate id, more than 64 profiles, no profile at all.
    assert_refused(
        set_of(vec![profile(), profile()]).validate(),
        "appears twice",
    );
    let many: Vec<HostProfile> = (0..65)
        .map(|i| HostProfile {
            id: format!("p{i}"),
            ..profile()
        })
        .collect();
    assert_refused(set_of(many.clone()).validate(), "holds 65 profiles");
    set_of(many[..64].to_vec())
        .validate()
        .expect("64 is the maximum");
    assert_refused(set_of(vec![]).validate(), "holds 0 profiles");
    // A profile JSON with no `expiry` does not parse as a set at all.
    let mut v = serde_json::json!({ "set": set_of(vec![profile()]), "signatures": [] });
    v["set"]["profiles"][0]
        .as_object_mut()
        .unwrap()
        .remove("expiry");
    let bytes = serde_json::to_vec(&v).unwrap();
    let res = parse_signed_document::<SignedHostProfileSet>(&bytes, "set", "/set", SET_DOC, 1);
    assert_refused(res.map(|_| ()), "missing field `expiry`");
}
