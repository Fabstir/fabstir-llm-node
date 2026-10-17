//! The V3.3 commands through the real binary, in ceremony order (D14, D16,
//! D22): migrate run 1's real v2 policy, sign a 2-of-2 manifest, assemble and
//! co-sign a set, assemble the bundle (HOME ledger), sign the policy against
//! it, check it (replaying run 1's real capture, an empty broker data dir),
//! then epoch 2 with `--previous`; and the refusals the runbook relies on.
//! Every library half is tested in its own file; this proves the wiring.

use super::fixtures::fixture;
use super::policy_fixture::{address_of, kbs_in, kbs_in_env, root_key, signer, signer_key};
use fabstir_llm_node::kbs::tools_authority::{migrate_v2, MigrateArgs};
use fabstir_llm_node::tee::host_profile::{
    AuthorityBundle, HostProfile, SignedHostProfileSet, MANIFEST_DOC,
};
use fabstir_llm_node::tee::types::Policy;

const RUN1_PROVIDER: &str = "0x3d66986d29160af27409b4bc847b567e3f665b1c";
const APP_ID: &str = "81acd506565cc9c0da4ed5acbb47ce9e3a35654f";

fn ok(out: &std::process::Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn the_ceremony_runs_through_the_binary_and_its_check_releases_run1() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let d = tempfile::tempdir().unwrap();
    let put = |n: &str, b: &[u8]| {
        if let Some(parent) = d.path().join(n).parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(d.path().join(n), b).unwrap()
    };
    let key = |sk: &k256::ecdsa::SigningKey| hex::encode(sk.to_bytes()).into_bytes();
    let (root, provider) = (root_key(), signer());
    put("root.key", &key(&root));
    put("s0.key", &key(&signer_key(0)));
    put("s1.key", &key(&signer_key(1)));
    put("provider.key", &key(&provider));
    put("v2.json", &fixture("run1-policy-pinned-v2-signed.json"));
    put("cap/request.json", &fixture("run1-boot2-request.json"));
    put("cap/verified.json", &fixture("run1-boot2-verified.json"));
    std::fs::create_dir_all(d.path().join("data/hwm")).unwrap();
    std::fs::create_dir_all(d.path().join("data/archive")).unwrap();
    let r = address_of(&root);

    // 1. migrate-v2: the binary writes exactly what the library computes
    //    (every flag wired to its field; the two windows apart).
    let (pnb, pex, fnb, fex) = (
        now - 86_400,
        now + 1000 * 86_400,
        now - 3600,
        now + 300 * 86_400,
    );
    let out = kbs_in(
        d.path(),
        &format!(
            "policy migrate-v2 --in @v2.json --expected-provider {RUN1_PROVIDER} --root {r} \
         --class phala-h200 --id phala/run1-boot2 --policy-not-before {pnb} --policy-expiry {pex} \
         --profile-not-before {fnb} --profile-expiry {fex} --policy-version 3 \
         --min-manifest-version 1 --min-threshold 2 --min-profile-epoch 1 --require-pins true \
         --evidence cli --policy-out @policy.json --profile-out @profile.json"
        ),
    );
    ok(&out);
    let want = migrate_v2(
        &fixture("run1-policy-pinned-v2-signed.json"),
        &MigrateArgs {
            expected_provider: RUN1_PROVIDER.into(),
            root: r.clone(),
            class: "phala-h200".into(),
            id: "phala/run1-boot2".into(),
            policy_not_before: pnb,
            policy_expiry: pex,
            profile_not_before: fnb,
            profile_expiry: fex,
            policy_version: 3,
            min_manifest_version: 1,
            min_threshold: 2,
            min_profile_epoch: 1,
            require_pins: true,
            evidence: "cli".into(),
        },
    )
    .unwrap();
    let read = |n: &str| std::fs::read(d.path().join(n)).unwrap();
    let got_policy: Policy = serde_json::from_slice(&read("policy.json")).unwrap();
    let got_profile: HostProfile = serde_json::from_slice(&read("profile.json")).unwrap();
    assert_eq!((got_policy, got_profile), want, "migrate-v2 wiring");
    let again = kbs_in(
        d.path(),
        &format!(
            "policy migrate-v2 --in @v2.json --expected-provider {RUN1_PROVIDER} --root {r} \
         --class phala-h200 --id x --policy-not-before 1 --policy-expiry 2 --profile-not-before 1 \
         --profile-expiry 2 --policy-version 3 --min-manifest-version 1 --min-threshold 1 \
         --min-profile-epoch 1 --require-pins false --evidence e --policy-out @policy.json \
         --profile-out @other.json"
        ),
    );
    assert!(!again.status.success(), "never overwrites");

    // 2. a 2-of-2 manifest, signed by the root.
    let signers = [address_of(&signer_key(0)), address_of(&signer_key(1))];
    let manifest = serde_json::json!({"doc": MANIFEST_DOC, "schema_version": 1, "root": r,
        "manifest_version": 1, "signers": signers, "threshold": 2, "not_before": now - 86_400,
        "expiry": now + 700 * 86_400});
    put("manifest.json", manifest.to_string().as_bytes());
    ok(&kbs_in(
        d.path(),
        "authority sign --manifest @manifest.json --key-file @root.key --first --out @m.json",
    ));

    // 3-5. set epoch 1, co-signed; the bundle; the ledger under HOME.
    let set_sign = |set: &str, out: &str| {
        let a = format!(
            "profile sign --set @{set} --key-file @s0.key --manifest @m.json --out @{out}.a"
        );
        ok(&kbs_in(d.path(), &a));
        let b = format!(
            "profile sign --set @{set} --key-file @s1.key --manifest @m.json --append @{out}.a --out @{out}"
        );
        let stdout = ok(&kbs_in(d.path(), &b));
        assert!(stdout.contains("signatures=2"), "{stdout}");
    };
    let home = d.path().join("home");
    let asm = |args: &str| ok(&kbs_in_env(d.path(), args, &[("HOME", &home)]));
    ok(&kbs_in(d.path(), &format!(
        "profile assemble --root {r} --epoch 1 --issued-at {} --first --profile @profile.json --out @set1.json",
        now - 60
    )));
    set_sign("set1.json", "set1-signed.json");
    let stdout = asm("authority assemble --manifest-signed @m.json --set-signed @set1-signed.json --first --out @b1.json");
    assert!(
        stdout.contains(&format!(
            "ledger={}",
            home.join(".fabstir-authority").display()
        )),
        "{stdout}"
    );
    let b1: AuthorityBundle = serde_json::from_slice(&read("b1.json")).unwrap();
    assert_eq!(b1.set.signatures.len(), 2);

    // 6. the policy, signed against the bundle it will be judged under.
    ok(&kbs_in(d.path(), "policy sign --policy @policy.json --key-file @provider.key --encrypted-ref qwen.enc --authority @b1.json --out @policy-signed.json"));

    // 7. the pre-deploy dry run: run 1's real capture releases; never loaded (exit 3).
    let check = kbs_in(d.path(), "authority check --bundle @b1.json --policy @policy-signed.json --replay @cap --hwm-dir @data/hwm");
    let stdout = String::from_utf8_lossy(&check.stdout);
    assert_eq!(check.status.code(), Some(3), "{stdout}");
    assert!(
        stdout.contains("outcome=release profile=phala/run1-boot2")
            && stdout.contains("never loaded"),
        "{stdout}"
    );

    // 8. epoch 2 with --previous: the set, the bundle, and the check's step rule.
    ok(&kbs_in(d.path(), &format!(
        "profile assemble --root {r} --epoch 2 --issued-at {} --previous @b1.json --profile @profile.json --out @set2.json",
        now - 30
    )));
    set_sign("set2.json", "set2-signed.json");
    asm("authority assemble --manifest-signed @m.json --set-signed @set2-signed.json --previous @b1.json --out @b2.json");
    let stdout = ok(&kbs_in(
        d.path(),
        "authority check --bundle @b2.json --previous @b1.json",
    ));
    assert!(
        stdout.contains("step_rule=ok") && stdout.contains("result=ok"),
        "{stdout}"
    );
    // The ledger refuses epoch 2 again with another body (re-signed at another time).
    ok(&kbs_in(d.path(), &format!(
        "profile assemble --root {r} --epoch 2 --issued-at {} --previous @b1.json --profile @profile.json --out @set2b.json",
        now - 20
    )));
    set_sign("set2b.json", "set2b-signed.json");
    let e = kbs_in_env(d.path(), "authority assemble --manifest-signed @m.json --set-signed @set2b-signed.json --previous @b1.json --out @b2b.json", &[("HOME", &home)]);
    assert!(
        !e.status.success() && String::from_utf8_lossy(&e.stderr).contains("already assembled")
    );
    assert!(!d.path().join("b2b.json").exists());

    // 9. profile pin through the binary: the out-of-band app id is pinned,
    //    a differing one refuses and writes nothing.
    let mut unpinned: HostProfile = serde_json::from_slice(&read("profile.json")).unwrap();
    (unpinned.cvm.app_id, unpinned.cvm.key_provider) = (None, None);
    put("unpinned.json", &serde_json::to_vec(&unpinned).unwrap());
    ok(&kbs_in(d.path(), &format!("profile pin --profile @unpinned.json --capture @cap --app-id {APP_ID} --out @pinned.json")));
    let pinned: HostProfile = serde_json::from_slice(&read("pinned.json")).unwrap();
    let migrated: HostProfile = serde_json::from_slice(&read("profile.json")).unwrap();
    assert_eq!(
        pinned, migrated,
        "the pins the v2 policy carried, re-derived out of band"
    );
    let wrong = format!("1{}", &APP_ID[1..]);
    let e = kbs_in(
        d.path(),
        &format!(
            "profile pin --profile @unpinned.json --capture @cap --app-id {wrong} --out @bad.json"
        ),
    );
    assert!(!e.status.success() && String::from_utf8_lossy(&e.stderr).contains("app id"));
    assert!(!d.path().join("bad.json").exists());
    let signed: SignedHostProfileSet = serde_json::from_slice(&read("set1-signed.json")).unwrap();
    assert_eq!(signed.set.profiles, [migrated]);
}
