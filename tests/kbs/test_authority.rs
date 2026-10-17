//! Policy schema 3 (V3.2): the authority bundle loader (D3 peek, D6, D7, D12)
//! and the canonical-bytes golden (D23, under the `kbs` feature where
//! `preserve_order` makes declaration order visible). Each test names the
//! mutation that turns it red.

use super::policy_fixture::{
    address_of, bundle, recording_policy, recording_profile, root_key, sign, sign_doc, signer,
    signer_key, test_model_id, write_bundle, NOW,
};
use fabstir_llm_node::kbs::authority::AuthorityStore;
use fabstir_llm_node::kbs::error::{KbsError, Kind};
use fabstir_llm_node::kbs::policy_file::{load_policy, FilePolicySource};
use fabstir_llm_node::tee::host_profile::{HostProfile, SignedAuthorityManifest};
use fabstir_llm_node::tee::policy_source::ProviderRegistry;
use fabstir_llm_node::tee::types::Policy;
use std::path::PathBuf;

struct Env {
    _tmp: tempfile::TempDir,
    auth: PathBuf,
    hwm: PathBuf,
    archive: PathBuf,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let e = Env {
        auth: tmp.path().join("authority"),
        hwm: tmp.path().join("hwm"),
        archive: tmp.path().join("archive"),
        _tmp: tmp,
    };
    std::fs::create_dir_all(&e.auth).unwrap();
    e
}

fn load(e: &Env, policy: &Policy, now: u64) -> Result<(), KbsError> {
    AuthorityStore::new(&e.auth, &e.hwm, &e.archive, false)
        .load(policy, now)
        .map(|_| ())
}

fn policy() -> Policy {
    recording_policy(test_model_id(1), 1)
}

fn assert_kind(r: Result<(), KbsError>, kind: Kind, needle: &str) {
    match r {
        Err(e) => {
            assert_eq!(e.kind, kind, "{e}");
            assert!(e.detail.contains(needle), "want {needle:?} in {e}");
        }
        Ok(()) => panic!("want {kind:?} containing {needle:?}, got Ok"),
    }
}

#[tokio::test]
async fn file_policy_source_peek_refuses_schema_2_as_verification() {
    // Mutation: remove the peek in `FilePolicySource` → a schema-2 file is a
    // serde failure → `Fetch` → `fault` (500, retried for ever).
    let tmp = tempfile::tempdir().unwrap();
    let id = test_model_id(1);
    let sk = signer();
    let (signed, provider) = sign(&recording_policy(id, 1), &sk);
    let mut v = serde_json::to_value(&signed).unwrap();
    let body = v["policy"].as_object_mut().unwrap();
    body.remove("doc");
    body.remove("host_authority");
    body.insert("schema_version".into(), 2.into());
    std::fs::write(
        tmp.path().join(format!("{}.json", hex::encode(id))),
        serde_json::to_vec(&v).unwrap(),
    )
    .unwrap();
    let src = FilePolicySource::new(tmp.path());
    let reg = ProviderRegistry::new().with_provider(id, provider);
    let e = load_policy(&src, &reg, id, 1).await.unwrap_err();
    assert_eq!(e.kind, Kind::Verification, "{e}");
    assert!(e.detail.contains("schema_version 2"), "{e}");
    // Not JSON at all stays a fault (a half-copied file).
    std::fs::write(
        tmp.path().join(format!("{}.json", hex::encode(id))),
        b"{\"pol",
    )
    .unwrap();
    let e = load_policy(&src, &reg, id, 1).await.unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::Unavailable, 500), "{e}");
}

#[test]
fn bundle_loader_peek_refuses_schema_2_as_verification() {
    // Mutation: parse the bundle typed without the peek → a schema-2 manifest
    // body is a serde failure → `fault`.
    let e = env();
    let mut v = serde_json::to_value(bundle(1, 1, vec![recording_profile()])).unwrap();
    v["manifest"]["manifest"]["schema_version"] = 2.into();
    v["manifest"]["manifest"]["legacy"] = true.into();
    let key = address_of(&root_key()).trim_start_matches("0x").to_string();
    std::fs::write(
        e.auth.join(format!("{key}.json")),
        serde_json::to_vec(&v).unwrap(),
    )
    .unwrap();
    assert_kind(
        load(&e, &policy(), NOW),
        Kind::Verification,
        "schema_version 2",
    );
}

#[test]
fn bundle_missing_is_no_provider_unparsable_is_fault() {
    // Mutation: swap the two kinds.
    let e = env();
    assert_kind(
        load(&e, &policy(), NOW),
        Kind::NoProvider,
        "no authority bundle for root",
    );
    let key = address_of(&root_key()).trim_start_matches("0x").to_string();
    std::fs::write(e.auth.join(format!("{key}.json")), b"{\"manifest\": {").unwrap();
    let r = load(&e, &policy(), NOW);
    assert_kind(r.clone(), Kind::Unavailable, "not JSON");
    assert_eq!(r.unwrap_err().status, 500);
    // Over the 256 KiB size rule: permanent, a refusal (mutation: fault).
    std::fs::write(
        e.auth.join(format!("{key}.json")),
        vec![b' '; 256 * 1024 + 1],
    )
    .unwrap();
    assert_kind(
        load(&e, &policy(), NOW),
        Kind::Verification,
        "over 262144 bytes",
    );
}

#[test]
fn file_name_must_equal_root() {
    // A bundle of ANOTHER root stored under this policy's root's file name.
    // Mutation: drop the manifest-root == policy-root equality → it loads.
    let e = env();
    let other_root = signer_key(9);
    let mut b = bundle(1, 1, vec![recording_profile()]);
    b.manifest.manifest.root = address_of(&other_root);
    b.manifest.signature = sign_doc(&b.manifest.manifest, &other_root);
    b.set.set.root = address_of(&other_root);
    b.set.signatures = vec![sign_doc(&b.set.set, &signer_key(0))];
    let key = address_of(&root_key()).trim_start_matches("0x").to_string();
    std::fs::write(
        e.auth.join(format!("{key}.json")),
        serde_json::to_vec(&b).unwrap(),
    )
    .unwrap();
    assert_kind(
        load(&e, &policy(), NOW),
        Kind::Verification,
        "host_authority.root",
    );
}

fn manifest_v2(signers: Vec<String>) -> SignedAuthorityManifest {
    let mut m = bundle(2, 1, vec![]).manifest.manifest;
    m.signers = signers;
    SignedAuthorityManifest {
        signature: sign_doc(&m, &root_key()),
        manifest: m,
    }
}

#[test]
fn set_signed_by_a_signer_removed_in_manifest_v2_is_refused_and_manifest_below_hwm_is_refused() {
    // Mutations: count signatures without the CURRENT manifest; drop the HWM
    // compare on the manifest.
    let e = env();
    let s1 = address_of(&signer_key(1));
    let mut b = bundle(1, 1, vec![recording_profile()]);
    write_bundle(&e.auth, &b);
    load(&e, &policy(), NOW).unwrap();
    // Manifest v2 removes signer 0 and appoints signer 1; the set is still signed by 0 only.
    b.manifest = manifest_v2(vec![s1.clone()]);
    write_bundle(&e.auth, &b);
    assert_kind(
        load(&e, &policy(), NOW),
        Kind::Verification,
        "0 distinct current signer",
    );
    // Signed by 1 → loads; now v1 (which still names 0) is a rollback.
    b.set.signatures = vec![sign_doc(&b.set.set, &signer_key(1))];
    write_bundle(&e.auth, &b);
    load(&e, &policy(), NOW).unwrap();
    let old = bundle(1, 1, vec![recording_profile()]);
    write_bundle(&e.auth, &old);
    assert_kind(
        load(&e, &policy(), NOW),
        Kind::Verification,
        "manifest_version 1 is below the high-water mark 2",
    );
}

#[test]
fn transition_set_cosigned_by_incoming_signer_verifies_under_v1_and_v2_with_unchanged_policy_hash()
{
    // Acceptance 6: adding a second signer is a manifest bump with no outage
    // and no reseal. Mutation: refuse a non-current co-signature → the
    // transition set fails under v1.
    let e = env();
    let p = policy();
    let (signed_before, _) = sign(&p, &signer());
    let mut b = bundle(1, 1, vec![recording_profile()]);
    b.set.signatures.push(sign_doc(&b.set.set, &signer_key(1))); // the incoming signer
    write_bundle(&e.auth, &b);
    load(&e, &p, NOW).expect("under manifest v1 (signer 1 not yet current: ignored)");
    let mut m2 = bundle(2, 1, vec![]).manifest.manifest;
    m2.signers = vec![address_of(&signer_key(0)), address_of(&signer_key(1))];
    m2.threshold = 2;
    b.manifest = SignedAuthorityManifest {
        signature: sign_doc(&m2, &root_key()),
        manifest: m2,
    };
    write_bundle(&e.auth, &b);
    load(&e, &p, NOW).expect("under manifest v2 (both current, threshold 2)");
    let (signed_after, _) = sign(&p, &signer());
    assert_eq!(
        signed_before.policy_hash().unwrap(),
        signed_after.policy_hash().unwrap(),
        "the bound policy did not change: no reseal"
    );
}

#[test]
fn windows_are_inclusive_at_both_ends_and_issued_at_in_future_is_refused() {
    // Mutations: an exclusive manifest or profile window; drop the issued_at rule.
    let e = env();
    let mut b = bundle(1, 1, vec![recording_profile()]);
    b.manifest.manifest.not_before = NOW;
    b.manifest.manifest.expiry = NOW + 10;
    b.manifest.signature = sign_doc(&b.manifest.manifest, &root_key());
    write_bundle(&e.auth, &b);
    load(&e, &policy(), NOW).expect("now == manifest.not_before");
    load(&e, &policy(), NOW + 10).expect("now == manifest.expiry");
    assert_kind(
        load(&e, &policy(), NOW + 11),
        Kind::Verification,
        "outside its window",
    );
    assert_kind(
        load(&e, &policy(), NOW - 1),
        Kind::Verification,
        "outside its window",
    );
    // A profile exactly at its own bounds is a candidate; one second out is not.
    let e = env();
    let p = recording_profile();
    write_bundle(&e.auth, &bundle(1, 1, vec![p.clone()]));
    let s = AuthorityStore::new(&e.auth, &e.hwm, &e.archive, false);
    for at in [p.not_before, p.expiry] {
        assert_eq!(
            s.load(&policy(), at).unwrap().candidates.list,
            vec![0],
            "at {at}"
        );
    }
    for at in [p.not_before - 1, p.expiry + 1] {
        assert_eq!(
            s.load(&policy(), at).unwrap().candidates.expired,
            vec![0],
            "at {at}"
        );
    }
    // issued_at more than 300 s ahead of now.
    let e = env();
    let mut b = bundle(1, 2, vec![recording_profile()]);
    b.set.set.issued_at = NOW + 301;
    b.set.signatures = vec![sign_doc(&b.set.set, &signer_key(0))];
    write_bundle(&e.auth, &b);
    assert_kind(
        load(&e, &policy(), NOW),
        Kind::Verification,
        "is in the future",
    );
    let e = env();
    b.set.set.issued_at = NOW + 300;
    b.set.signatures = vec![sign_doc(&b.set.set, &signer_key(0))];
    write_bundle(&e.auth, &b);
    load(&e, &policy(), NOW).expect("300 s of skew is tolerated");
}

#[test]
fn policy_minimums_enforced() {
    // Mutation: drop any one of the three minimums.
    let e = env();
    write_bundle(&e.auth, &bundle(1, 1, vec![recording_profile()]));
    let wrong: Vec<String> = [
        (
            "min_manifest_version 2",
            (|p: &mut Policy| p.host_authority.min_manifest_version = 2) as fn(&mut Policy),
        ),
        ("min_threshold 2", |p| p.host_authority.min_threshold = 2),
        ("min_profile_epoch 2", |p| {
            p.host_authority.min_profile_epoch = 2
        }),
    ]
    .into_iter()
    .filter_map(|(needle, set_min)| {
        let mut p = policy();
        set_min(&mut p);
        match load(&e, &p, NOW) {
            Err(k) if k.kind == Kind::Verification && k.detail.contains(needle) => None,
            other => Some(format!("MIN[{needle}] {other:?}")),
        }
    })
    .collect();
    assert!(wrong.is_empty(), "{wrong:#?}");
    load(&e, &policy(), NOW).expect("at the minimums");
}

#[test]
fn require_pinned_excludes_unpinned_profiles() {
    // Mutation: ignore `require_app_id_pinned` / `require_key_provider_pinned`.
    let e = env();
    let pinned = recording_profile();
    let mut no_app = HostProfile {
        id: "no-app".into(),
        ..pinned.clone()
    };
    no_app.cvm.app_id = None;
    let mut none = HostProfile {
        id: "none".into(),
        ..pinned.clone()
    };
    (none.cvm.app_id, none.cvm.key_provider) = (None, None);
    write_bundle(&e.auth, &bundle(1, 1, vec![pinned, no_app, none]));
    let s = AuthorityStore::new(&e.auth, &e.hwm, &e.archive, false);
    let mut p = policy();
    (
        p.cvm.require_app_id_pinned,
        p.cvm.require_key_provider_pinned,
    ) = (true, true);
    let a = s.load(&p, NOW).unwrap();
    assert_eq!(
        (a.candidates.list.clone(), a.candidates.unpinned.clone()),
        (vec![0], vec![1, 2])
    );
    (
        p.cvm.require_app_id_pinned,
        p.cvm.require_key_provider_pinned,
    ) = (false, true);
    let a = s.load(&p, NOW).unwrap();
    assert_eq!(
        (a.candidates.list.clone(), a.candidates.unpinned.clone()),
        (vec![0, 1], vec![2])
    );
    (
        p.cvm.require_app_id_pinned,
        p.cvm.require_key_provider_pinned,
    ) = (false, false);
    assert_eq!(s.load(&p, NOW).unwrap().candidates.list, vec![0, 1, 2]);
}

#[test]
fn a_policy_minimum_never_holds_the_roots_hwm_back() {
    // Two models share one root. Model B demands threshold 2; the authority
    // publishes epoch 2 at threshold 1. B refuses it, but the bundle is valid
    // for the ROOT, so the HWM moves and a re-served epoch 1 is refused for
    // every model. Mutation: apply the minimums before the HWM raise → epoch 1
    // is accepted again for model A.
    let e = env();
    let a = policy();
    let mut b = policy();
    b.host_authority.min_threshold = 2;
    write_bundle(&e.auth, &bundle(1, 1, vec![recording_profile()]));
    load(&e, &a, NOW).unwrap();
    write_bundle(&e.auth, &bundle(1, 2, vec![recording_profile()]));
    assert_kind(load(&e, &b, NOW), Kind::Verification, "min_threshold 2");
    write_bundle(&e.auth, &bundle(1, 1, vec![recording_profile()]));
    assert_kind(
        load(&e, &a, NOW),
        Kind::Verification,
        "epoch 1 is below the high-water mark 2",
    );
}
