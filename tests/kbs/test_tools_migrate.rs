//! Policy schema 3 (D14, F10, F12, L2): `policy migrate-v2` against run 1's
//! real pinned v2 policy, and the `--authority` cross-check of `policy sign`
//! and `seal`.

use super::fixtures::fixture;
use super::policy_fixture::{address_of, kbs_in, sign, sign_doc, signer};
use fabstir_llm_node::kbs::tools_authority::{
    check_policy_against_bundle, migrate_v2, MigrateArgs,
};
use fabstir_llm_node::tee::host_profile::parse_bundle;
use fabstir_llm_node::tee::policy::SignedModelPolicy;
use fabstir_llm_node::tee::types::Policy;

const RUN1_PROVIDER: &str = "0x3d66986d29160af27409b4bc847b567e3f665b1c";
const FIXTURE_ROOT: &str = "0xd84fbf720e8a90fabf5fc1ca07f20be85b508420";

/// The arguments the committed fixtures were migrated with (README).
fn args() -> MigrateArgs {
    MigrateArgs {
        expected_provider: RUN1_PROVIDER.into(),
        root: FIXTURE_ROOT.into(),
        class: "phala-h200".into(),
        id: "phala/run1-boot2".into(),
        policy_not_before: 1_788_220_800,
        policy_expiry: 1_924_992_000,
        profile_not_before: 1_788_220_800,
        profile_expiry: 1_819_756_800,
        policy_version: 3,
        min_manifest_version: 1,
        min_threshold: 1,
        min_profile_epoch: 1,
        require_pins: true,
        evidence: "run 1 boot 2 (2026-09-23); migrated from gpu-day/policy-pinned.signed.json"
            .into(),
    }
}

fn fixture_policy() -> SignedModelPolicy {
    serde_json::from_slice(&fixture("v3-policy-signed.json")).unwrap()
}

/// A synthetic v2 signed policy: run 1's body edited by `edit`, signed by the
/// throw-away test key (B2).
fn synthetic_v2(edit: impl FnOnce(&mut serde_json::Value)) -> (Vec<u8>, String) {
    let mut v: serde_json::Value =
        serde_json::from_slice(&fixture("run1-policy-pinned-v2-signed.json")).unwrap();
    edit(&mut v["policy"]);
    let sk = signer();
    v["signature"] = serde_json::json!(sign_doc(&v["policy"], &sk));
    (serde_json::to_vec(&v).unwrap(), address_of(&sk))
}

#[test]
fn migrate_v2_verifies_the_provider_signature_requires_key_provider_with_app_id_and_splits_fields()
{
    let v2: serde_json::Value =
        serde_json::from_slice(&fixture("run1-policy-pinned-v2-signed.json")).unwrap();
    let (policy, profile) =
        migrate_v2(&fixture("run1-policy-pinned-v2-signed.json"), &args()).unwrap();
    // The floor moves to the PROFILE; the v3 policy's floors are None (D15).
    assert_eq!(profile.gpu.min_driver_version.as_deref(), Some("550.0.0"));
    assert_eq!(profile.gpu.min_vbios_version, None);
    assert_eq!(
        (
            &policy.gpu.min_driver_version,
            &policy.gpu.min_vbios_version
        ),
        (&None, &None),
        "mutation: leave the floor on the policy"
    );
    // hwmodels to both lists; compose to a one-element list; pins as present.
    assert_eq!(policy.gpu.allowed_hwmodels, ["GH100"]);
    assert_eq!(
        profile.gpu.hwmodels,
        ["GH100"],
        "mutation: copy empty hwmodels"
    );
    let c = &v2["policy"]["cvm"];
    assert_eq!(
        profile.cvm.compose_hashes,
        [c["compose_hash"].as_str().unwrap()]
    );
    for (got, key) in [
        (&profile.cvm.mrtd, "mrtd"),
        (&profile.cvm.rtmr0, "rtmr0"),
        (&profile.cvm.rtmr1, "rtmr1"),
        (&profile.cvm.rtmr2, "rtmr2"),
        (&profile.cvm.os_image_hash, "os_image_hash"),
    ] {
        assert_eq!(got, c[key].as_str().unwrap(), "{key}");
    }
    assert_eq!(profile.cvm.app_id.as_deref(), c["app_id"].as_str());
    assert_eq!(
        profile.cvm.key_provider.as_deref(),
        c["key_provider"].as_str()
    );
    // Separate windows (R3-5); pins required as asked.
    assert_eq!(
        (policy.not_before, policy.expiry),
        (1_788_220_800, 1_924_992_000)
    );
    assert_eq!(
        (profile.not_before, profile.expiry),
        (1_788_220_800, 1_819_756_800)
    );
    assert!(policy.cvm.require_app_id_pinned && policy.cvm.require_key_provider_pinned);
    // The committed fixtures are exactly this migration.
    assert_eq!(policy, fixture_policy().policy);
    let b = parse_bundle(&fixture("authority-bundle.json"), "fixture").unwrap();
    assert_eq!(profile, b.set.set.profiles[0]);
    let unpinned = MigrateArgs {
        require_pins: false,
        ..args()
    };
    let (p, _) = migrate_v2(&fixture("run1-policy-pinned-v2-signed.json"), &unpinned).unwrap();
    assert!(!p.cvm.require_app_id_pinned && !p.cvm.require_key_provider_pinned);

    // The signature is checked before any field is trusted (F12).
    let wrong = MigrateArgs {
        expected_provider: address_of(&signer()),
        ..args()
    };
    let e = migrate_v2(&fixture("run1-policy-pinned-v2-signed.json"), &wrong).unwrap_err();
    assert!(
        e.0.contains("not the expected provider"),
        "mutation: skip signature: {e}"
    );
    let mut tampered = v2.clone();
    tampered["policy"]["gpu"]["min_driver_version"] = "1.0.0".into();
    let e = migrate_v2(&serde_json::to_vec(&tampered).unwrap(), &args()).unwrap_err();
    assert!(e.0.contains("not the expected provider"), "{e}");
    // L2: app_id pinned without key_provider is refused (a synthetic v2, B2).
    let (alone, by) = synthetic_v2(|p| p["cvm"]["key_provider"] = serde_json::Value::Null);
    let e = migrate_v2(
        &alone,
        &MigrateArgs {
            expected_provider: by.clone(),
            ..args()
        },
    )
    .unwrap_err();
    assert!(
        e.0.contains("without key_provider"),
        "mutation: allow app_id alone: {e}"
    );
    let (neither, by) = synthetic_v2(|p| {
        p["cvm"]["key_provider"] = serde_json::Value::Null;
        p["cvm"]["app_id"] = serde_json::Value::Null;
    });
    let (_, prof) = migrate_v2(
        &neither,
        &MigrateArgs {
            expected_provider: by,
            ..args()
        },
    )
    .unwrap();
    assert!(prof.cvm.app_id.is_none() && prof.cvm.key_provider.is_none());
    // Run 1 has both GPU booleans true: a synthetic v2 tells them (and the
    // other copied rows) apart.
    let (mixed, by) = synthetic_v2(|p| {
        p["gpu"]["require_debug_disabled"] = false.into();
        p["gpu"]["require_cc_mode"] = serde_json::Value::Null;
        p["cvm"]["require_td_debug_off"] = false.into();
        p["cvm"]["allowed_tcb_status"] = serde_json::json!(["UpToDate", "SWHardeningNeeded"]);
        p["cvm"]["allowed_advisory_ids"] = serde_json::json!(["INTEL-SA-00837"]);
    });
    let (p, _) = migrate_v2(
        &mixed,
        &MigrateArgs {
            expected_provider: by,
            ..args()
        },
    )
    .unwrap();
    assert!(
        p.gpu.require_secure_boot && !p.gpu.require_debug_disabled,
        "a swapped GPU row"
    );
    assert_eq!(p.gpu.require_cc_mode, None);
    assert!(!p.cvm.require_td_debug_off);
    assert_eq!(p.cvm.allowed_tcb_status, ["UpToDate", "SWHardeningNeeded"]);
    assert_eq!(p.cvm.allowed_advisory_ids, ["INTEL-SA-00837"]);
    // A v3 input is not a v2 one.
    let e = migrate_v2(&fixture("v3-policy-signed.json"), &args()).unwrap_err();
    assert!(e.0.contains("not a schema-2"), "{e}");
}

#[test]
fn policy_sign_and_seal_refuse_a_root_or_minimum_or_class_that_does_not_match_the_bundle() {
    let b = parse_bundle(&fixture("authority-bundle.json"), "fixture").unwrap();
    let good = fixture_policy().policy;
    check_policy_against_bundle(&good, &b).unwrap();
    type Edit = (&'static str, fn(&mut Policy));
    let cases: [Edit; 5] = [
        ("!= the policy's host_authority.root", |p| {
            p.host_authority.root = "0x00000000000000000000000000000000000000aa".into()
        }),
        ("min_manifest_version 2", |p| {
            p.host_authority.min_manifest_version = 2
        }),
        ("min_threshold 2", |p| p.host_authority.min_threshold = 2),
        ("min_profile_epoch 2", |p| {
            p.host_authority.min_profile_epoch = 2
        }),
        ("share no class", |p| {
            p.host_authority.allowed_classes = vec!["other".into()]
        }),
    ];
    for (row, edit) in cases {
        let mut p = good.clone();
        edit(&mut p);
        let e = check_policy_against_bundle(&p, &b).unwrap_err();
        assert!(e.0.contains(row), "mutation: skip ({row}): {e}");
    }
    // Through the binary: `policy sign --authority` and `seal --authority`
    // refuse before writing anything; the matching policy signs and seals.
    let d = tempfile::tempdir().unwrap();
    let f = |n: &str| d.path().join(n);
    let sk = signer();
    std::fs::write(f("bundle.json"), fixture("authority-bundle.json")).unwrap();
    std::fs::write(f("key"), "0x".to_string() + &hex::encode(sk.to_bytes())).unwrap();
    std::fs::write(f("dek"), hex::encode([7u8; 32])).unwrap();
    std::fs::write(f("model.gguf"), b"tiny model").unwrap();
    let mut off = good.clone();
    off.host_authority.min_profile_epoch = 2;
    for (name, policy) in [("good", &good), ("off", &off)] {
        std::fs::write(
            f(&format!("{name}.json")),
            serde_json::to_vec(policy).unwrap(),
        )
        .unwrap();
        let (signed, _) = sign(policy, &sk);
        let signed = serde_json::to_vec(&signed).unwrap();
        std::fs::write(f(&format!("{name}-signed.json")), signed).unwrap();
        let cmd = format!(
            "policy sign --policy @{name}.json --key-file @key --encrypted-ref qwen.enc \
             --authority @bundle.json --out @{name}-out.json"
        );
        let out = kbs_in(d.path(), &cmd);
        assert_eq!(
            out.status.success(),
            name == "good",
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(f(&format!("{name}-out.json")).exists(), name == "good");
        let cmd = format!(
            "seal --model @model.gguf --dek-file @dek --model-id {} --policy @{name}-signed.json \
             --provider {} --authority @bundle.json --out @{name}.enc",
            hex::encode(policy.model_id),
            address_of(&sk)
        );
        let out = kbs_in(d.path(), &cmd);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.success(), name == "good", "{stderr}");
        assert!(
            name == "good" || stderr.contains("min_profile_epoch 2"),
            "{stderr}"
        );
        assert_eq!(f(&format!("{name}.enc")).exists(), name == "good");
    }
}
