// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Policy schema 3 (V3.1): the documents' schema refusal, the bound policy's
//! hash coverage, the manifest rules, set signature counting and the root
//! equalities (`docs/development/IMPLEMENTATION-POLICY-V3-HOST-PROFILES.md`
//! D3–D6). Each test names the mutation that turns it red.

use super::profile_fixture::{
    a_bundle, a_manifest, a_policy, a_profile, a_set, address_of, key, sign_doc,
};
use fabstir_llm_node::tee::host_profile::{
    parse_bundle, verify_manifest, verify_set, AuthorityManifest, SignedAuthorityManifest,
    SignedHostProfileSet,
};
use fabstir_llm_node::tee::policy::SignedModelPolicy;
use fabstir_llm_node::tee::types::{CcMode, Policy, TeeError, TeeResult};

/// (name, mutation of the bundle JSON, expected needle).
type JsonCase = (
    &'static str,
    Box<dyn Fn(&mut serde_json::Value)>,
    &'static str,
);
/// (expected needle, mutation of the manifest).
type ManifestCase = (&'static str, Box<dyn Fn(&mut AuthorityManifest)>);

fn assert_refused<T: std::fmt::Debug>(res: TeeResult<T>, needle: &str) {
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
fn validate_refuses_schema_version_2_at_struct_level() {
    // Mutation: accept 2 (any one of the three documents) → red.
    let mut p = policy();
    p.schema_version = 2;
    assert_refused(p.validate(), "schema_version 2");
    let (bundle, _) = a_bundle(vec![a_profile([1u8; 48], &["GH100"])]);
    let mut m = bundle.manifest.manifest.clone();
    m.schema_version = 2;
    assert_refused(m.validate(), "schema_version 2");
    let mut s = bundle.set.set.clone();
    s.schema_version = 2;
    assert_refused(s.validate(), "schema_version 2");
    // And the wrong document type at struct level, for all three.
    let mut p = policy();
    p.doc = "fabstir-host-profile-set".into();
    assert_refused(p.validate(), "doc");
    m = bundle.manifest.manifest.clone();
    m.doc = "fabstir-model-release-policy".into();
    assert_refused(m.validate(), "doc");
    s = bundle.set.set.clone();
    s.doc = "fabstir-host-authority-manifest".into();
    assert_refused(s.validate(), "doc");
}

#[test]
fn bundle_json_with_wrong_or_missing_doc_or_schema_is_refused_as_verification_not_fault() {
    // The manifest/set half of the D3 peek (the policy half is
    // `schema_2_json_is_refused_as_verification_not_fault`). Mutation: drop the
    // peek of either body → the typed parse fails on the missing/odd field
    // with a schema message that does not name the doc, or accepts it.
    let (bundle, _) = a_bundle(vec![a_profile([1u8; 48], &["GH100"])]);
    let good = serde_json::to_value(&bundle).unwrap();
    parse_bundle(&serde_json::to_vec(&good).unwrap(), "bundle").expect("pristine bundle parses");
    let cases: Vec<JsonCase> = vec![
        (
            "manifest doc wrong",
            Box::new(|v| v["manifest"]["manifest"]["doc"] = "fabstir-host-profile-set".into()),
            "/manifest/manifest doc",
        ),
        (
            "manifest doc missing",
            Box::new(|v| {
                drop(
                    v["manifest"]["manifest"]
                        .as_object_mut()
                        .unwrap()
                        .remove("doc"),
                )
            }),
            "no doc tag",
        ),
        (
            "manifest schema 2",
            Box::new(|v| v["manifest"]["manifest"]["schema_version"] = 2.into()),
            "schema_version 2",
        ),
        (
            "set doc wrong",
            Box::new(|v| v["set"]["set"]["doc"] = "fabstir-model-release-policy".into()),
            "/set/set doc",
        ),
        (
            "set doc missing",
            Box::new(|v| drop(v["set"]["set"].as_object_mut().unwrap().remove("doc"))),
            "no doc tag",
        ),
        (
            "set schema absent",
            Box::new(|v| {
                drop(
                    v["set"]["set"]
                        .as_object_mut()
                        .unwrap()
                        .remove("schema_version"),
                )
            }),
            "schema_version absent",
        ),
        (
            "unknown field",
            Box::new(|v| v["set"]["set"]["profiles"][0]["cvm"]["extra"] = 1.into()),
            "extra",
        ),
    ];
    for (name, mutate, needle) in cases {
        let mut v = good.clone();
        mutate(&mut v);
        assert_refused(
            parse_bundle(&serde_json::to_vec(&v).unwrap(), "bundle"),
            needle,
        );
        let _ = name;
    }
    assert!(matches!(
        parse_bundle(b"{\"manifest\":", "bundle"),
        Err(TeeError::Fetch(_))
    ));
}

#[test]
fn policy_hash_covers_every_schema_3_field() {
    // Every leaf of the bound policy, every Option Some, perturbed one at a
    // time, must move policy_hash; and the table must cover every leaf the
    // canonical JSON carries. Mutation: skip-serialising any field → its
    // perturbation no longer moves the hash (or the leaf count drops) → red.
    let mut base = policy();
    base.gpu.min_driver_version = Some("580.95.05".into());
    base.gpu.min_vbios_version = Some("96.00.9f.00.01".into());
    base.cvm.allowed_advisory_ids = vec!["INTEL-SA-00837".into()];
    type Perturb = fn(&mut Policy);
    let table: Vec<(&str, Perturb)> = vec![
        ("doc", |p| p.doc.push('x')),
        ("schema_version", |p| p.schema_version += 1),
        ("policy_version", |p| p.policy_version += 1),
        ("model_id", |p| p.model_id[0] ^= 1),
        ("not_before", |p| p.not_before += 1),
        ("expiry", |p| p.expiry -= 1),
        ("cvm.require_td_debug_off", |p| {
            p.cvm.require_td_debug_off ^= true
        }),
        ("cvm.allowed_tcb_status", |p| {
            p.cvm.allowed_tcb_status.push("OutOfDate".into())
        }),
        ("cvm.allowed_advisory_ids", |p| {
            p.cvm.allowed_advisory_ids.clear()
        }),
        ("cvm.require_app_id_pinned", |p| {
            p.cvm.require_app_id_pinned ^= true
        }),
        ("cvm.require_key_provider_pinned", |p| {
            p.cvm.require_key_provider_pinned ^= true
        }),
        ("gpu.allowed_hwmodels", |p| {
            p.gpu.allowed_hwmodels.push("GB100".into())
        }),
        ("gpu.require_cc_mode", |p| {
            p.gpu.require_cc_mode = Some(CcMode::DevTools)
        }),
        ("gpu.require_secure_boot", |p| {
            p.gpu.require_secure_boot ^= true
        }),
        ("gpu.require_debug_disabled", |p| {
            p.gpu.require_debug_disabled ^= true
        }),
        ("gpu.min_driver_version", |p| {
            p.gpu.min_driver_version = Some("581.0".into())
        }),
        ("gpu.min_vbios_version", |p| {
            p.gpu.min_vbios_version = Some("96.00.a0".into())
        }),
        ("host_authority.root", |p| {
            p.host_authority.root.replace_range(2..3, "b")
        }),
        ("host_authority.min_manifest_version", |p| {
            p.host_authority.min_manifest_version += 1
        }),
        ("host_authority.min_threshold", |p| {
            p.host_authority.min_threshold += 1
        }),
        ("host_authority.min_profile_epoch", |p| {
            p.host_authority.min_profile_epoch += 1
        }),
        ("host_authority.allowed_classes", |p| {
            p.host_authority.allowed_classes.push("b".into())
        }),
    ];
    let hash = |p: &Policy| {
        SignedModelPolicy {
            policy: p.clone(),
            encrypted_ref: String::new(),
            signer: String::new(),
            signature: vec![],
        }
        .policy_hash()
        .unwrap()
    };
    let h0 = hash(&base);
    for (field, perturb) in &table {
        let mut p = base.clone();
        perturb(&mut p);
        assert_ne!(hash(&p), h0, "perturbing {field} must move policy_hash");
    }
    // Leaves = scalar values and arrays (an array is one leaf here).
    fn leaves(v: &serde_json::Value) -> usize {
        match v {
            serde_json::Value::Object(m) => m.values().map(leaves).sum(),
            _ => 1,
        }
    }
    assert_eq!(leaves(&serde_json::to_value(&base).unwrap()), table.len());
}

fn signed_manifest(m: &AuthorityManifest, sk: &k256::ecdsa::SigningKey) -> SignedAuthorityManifest {
    SignedAuthorityManifest {
        signature: sign_doc(m, sk),
        manifest: m.clone(),
    }
}

#[test]
fn manifest_rules_one_at_a_time() {
    // Mutation: drop any one rule → its case verifies → red.
    let root_sk = key();
    let root = address_of(&root_sk);
    let signers: Vec<String> = (0..3).map(|_| address_of(&key())).collect();
    let good = a_manifest(&root, &signers, 2);
    verify_manifest(&signed_manifest(&good, &root_sk), &root).expect("good manifest");
    let cases: Vec<ManifestCase> = vec![
        (
            "may not also be a signer",
            Box::new(|m| m.signers.push(m.root.clone())),
        ),
        ("threshold 0", Box::new(|m| m.threshold = 0)),
        ("threshold 4", Box::new(|m| m.threshold = 4)),
        (
            "listed twice",
            Box::new(|m| m.signers.push(m.signers[0].clone())),
        ),
        (
            "lowercase hex",
            Box::new(|m| m.signers[1] = m.signers[1].to_uppercase().replace("0X", "0x")),
        ),
        ("signers is empty", Box::new(|m| m.signers.clear())),
        (
            "window inverted",
            Box::new(|m| (m.not_before, m.expiry) = (10, 9)),
        ),
        (
            "manifest root: expected 0x + 40",
            Box::new(|m| m.root.push('0')),
        ),
    ];
    for (needle, mutate) in cases {
        let mut m = good.clone();
        mutate(&mut m);
        assert_refused(
            verify_manifest(&signed_manifest(&m, &root_sk), &m.root.clone()),
            needle,
        );
    }
    // Recovered signer != manifest.root: signed by a signer key instead.
    let other = key();
    assert_refused(
        verify_manifest(&signed_manifest(&good, &other), &root),
        &format!("manifest signer {} != manifest root", address_of(&other)),
    );
}

#[test]
fn set_signatures_at_threshold_two_of_three() {
    let root_sk = key();
    let root = address_of(&root_sk);
    let sks: Vec<_> = (0..3).map(|_| key()).collect();
    let addrs: Vec<String> = sks.iter().map(address_of).collect();
    let manifest = a_manifest(&root, &addrs, 2);
    let set = a_set(&root, 1, vec![a_profile([1u8; 48], &["GH100"])]);
    let outsider = key();
    let sig = |sk| sign_doc(&set, sk);
    let signed = |sigs: Vec<Vec<u8>>| SignedHostProfileSet {
        set: set.clone(),
        signatures: sigs,
    };
    // Two current signers → ok, both counted, nothing ignored.
    let c = verify_set(&signed(vec![sig(&sks[0]), sig(&sks[2])]), &manifest).unwrap();
    assert_eq!(c.signers, vec![addrs[0].clone(), addrs[2].clone()]);
    assert!(c.ignored.is_empty());
    // One current + one outsider → count 1 → refused [mutation: count the outsider].
    assert_refused(
        verify_set(&signed(vec![sig(&sks[0]), sig(&outsider)]), &manifest),
        "1 distinct current signer",
    );
    // Two current + one outsider → ok, outsider IGNORED and reported, not refused
    // [mutation: refuse a non-signer → red; drop `ignored` → red].
    // The outsider signs twice: listed once [mutation: drop the ignored dedupe].
    let c = verify_set(
        &signed(vec![
            sig(&sks[0]),
            sig(&outsider),
            sig(&sks[1]),
            sig(&outsider),
        ]),
        &manifest,
    )
    .unwrap();
    assert_eq!(c.signers.len(), 2);
    assert_eq!(c.ignored, vec![address_of(&outsider)]);
    // The same key twice counts once → refused [mutation: count duplicates].
    assert_refused(
        verify_set(&signed(vec![sig(&sks[0]), sig(&sks[0])]), &manifest),
        "1 distinct current signer",
    );
    // 17 signatures (two valid, the rest duplicates) → refused [mutation: drop the cap].
    let mut many = vec![sig(&sks[0]), sig(&sks[1])];
    many.extend((0..15).map(|_| sig(&sks[0])));
    assert_refused(
        verify_set(&signed(many.clone()), &manifest),
        "17 signatures",
    );
    // Exactly 16 is the maximum, accepted [mutation: `>` → `>=` on the cap].
    many.pop();
    verify_set(&signed(many), &manifest).expect("16 signatures");
    assert_refused(verify_set(&signed(vec![]), &manifest), "0 signatures");
    // A signature that does not recover refuses the set even beside two good
    // ones [mutation: skip it instead].
    let mut bad = sig(&sks[2]);
    bad[64] = 9;
    assert_refused(
        verify_set(&signed(vec![sig(&sks[0]), sig(&sks[1]), bad]), &manifest),
        "signature 2 does not recover",
    );
}

#[test]
fn root_equalities_one_at_a_time() {
    // D6: manifest.root == set.root == policy root == recovered manifest
    // signer (the file name is the broker's, V3.2). Mutation: drop any one.
    let (bundle, keys) = a_bundle(vec![a_profile([1u8; 48], &["GH100"])]);
    let root = keys.root_address();
    verify_manifest(&bundle.manifest, &root).expect("pristine");
    verify_set(&bundle.set, &bundle.manifest.manifest).expect("pristine");
    // (1) A validly signed set whose root is another authority's.
    let other_root = address_of(&key());
    let set = a_set(&other_root, 1, bundle.set.set.profiles.clone());
    let foreign = SignedHostProfileSet {
        signatures: vec![sign_doc(&set, &keys.signers[0])],
        set,
    };
    assert_refused(
        verify_set(&foreign, &bundle.manifest.manifest),
        "profile set root",
    );
    // (2) A policy that trusts another root than the manifest's.
    let mut p = policy();
    p.host_authority.root = other_root;
    assert_refused(
        verify_manifest(&bundle.manifest, &p.host_authority.root),
        "!= the policy's host_authority.root",
    );
    // (3) The manifest re-signed by a signer key: recovered != manifest.root.
    let resigned = signed_manifest(&bundle.manifest.manifest, &keys.signers[0]);
    assert_refused(verify_manifest(&resigned, &root), "!= manifest root");
}
