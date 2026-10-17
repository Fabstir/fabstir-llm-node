//! Policy schema 3 profile tooling (D14, D18): `profile new` from PREDICTED
//! values, `profile check` (all six rows), `profile pin` (the capture only
//! confirms; the KMS key provider is the committed constant). `profile sign`
//! is in `test_tools_authority.rs`.

use super::fixtures::fixture;
use super::policy_fixture::kbs_in;
use fabstir_llm_node::kbs::tools_authority::{
    capture_identity, check_profile, env_names, new_profile, pin_profile, predict_compose_hash,
    Measurements, NewProfile, PHALA_KMS_KEY_PROVIDER,
};
use fabstir_llm_node::tee::host_profile::{parse_bundle, HostProfile};

/// An app-compose exactly as Phala serves it (compact, sorted, UTF-8), and
/// the hashes `predict_compose_hash.py` computes for it and for the new
/// compose + env names below (computed with the Python rule, 2026-10-17):
/// non-ASCII, a control character, quotes, tabs and a null exercise every
/// escaping difference that could exist between the two serialisers.
const SERVED: &str = r##"{"allowed_envs":["OLD"],"docker_compose_file":"old","features":["kms","tproxy-net"],"gateway_enabled":null,"kms_enabled":true,"manifest_version":2,"name":"fabstir-node","pre_launch_script":"#!/bin/sh\necho \"héllo\"\t\\done\u0001\n","public_logs":false,"runner":"docker-compose"}"##;
const SERVED_HASH: &str = "d5b382cc3212afe91d23f51c95aeb73e49d552401150051f53fe3809e691be00";
const PREDICTED: &str = "49a8a11766fb0fb801e6e87deb125924ae15a8412483f07c18f5e1c4510abd25";
const COMPOSE: &str = "services:\n  node:\n    image: \"ghcr.io/fabstir/node@sha256:abü\"\n";
const ENV_PASTE: &str = "TEE_MODEL_ID=abc\nHOST_PRIVATE_KEY=0xnotarealvalue\n\n";
const APP_ID: &str = "81acd506565cc9c0da4ed5acbb47ce9e3a35654f";

/// The committed bundle's one profile: run 1 boot 2's identity, both pins.
fn run1_profile() -> HostProfile {
    let b = parse_bundle(&fixture("authority-bundle.json"), "fixture").unwrap();
    b.set.set.profiles[0].clone()
}

fn measurements_of(p: &HostProfile) -> Measurements {
    Measurements {
        mrtd: p.cvm.mrtd.clone(),
        rtmr0: p.cvm.rtmr0.clone(),
        rtmr1: p.cvm.rtmr1.clone(),
        rtmr2: p.cvm.rtmr2.clone(),
    }
}

#[test]
fn profile_new_round_trips_through_profile_check_and_carries_floors() {
    let names = env_names(ENV_PASTE).unwrap();
    assert_eq!(
        names,
        ["TEE_MODEL_ID", "HOST_PRIVATE_KEY"],
        "names only, values dropped"
    );
    // A line that is not a name (an `export`, a secret's continuation) is
    // refused by number, never echoed.
    let e = env_names("A=1\nexport B=s3cr3t\n").unwrap_err();
    assert!(e.0.contains("line 2") && !e.0.contains("s3cr3t"), "{e}");
    let attestation = serde_json::json!({"instances": [{"compose_file": SERVED}]}).to_string();
    for reference in [attestation.clone(), format!("{SERVED}\n")] {
        let got = predict_compose_hash(reference.as_bytes(), COMPOSE, &names).unwrap();
        assert_eq!(
            got,
            (SERVED_HASH.into(), PREDICTED.into()),
            "the Python rule"
        );
    }
    let spaced = SERVED.replacen(r#""name":"#, r#""name": "#, 1);
    let e = predict_compose_hash(spaced.as_bytes(), COMPOSE, &names).unwrap_err();
    assert!(e.0.contains("no longer reproduces"), "{e}");

    let run1 = run1_profile();
    let n = || NewProfile {
        id: "phala/h200-next".into(),
        class: "phala-h200".into(),
        not_before: 1_800_000_000,
        expiry: 1_800_000_000 + 300 * 86_400,
        evidence: "A-24 gpu-16c-64g-1gpu; dstack 0.5.9".into(),
        hwmodels: vec!["GH100".into()],
        min_driver_version: Some("580.95.05".into()),
        min_vbios_version: None,
        measurements: measurements_of(&run1),
        os_image_digest: run1.cvm.os_image_hash.clone(),
        compose_hash: PREDICTED.into(),
    };
    let p = new_profile(n()).unwrap();
    let want = HostProfile {
        id: "phala/h200-next".into(),
        class: "phala-h200".into(),
        cvm: fabstir_llm_node::tee::host_profile::ProfileCvm {
            compose_hashes: vec![PREDICTED.into()],
            app_id: None,
            key_provider: None,
            ..run1.cvm.clone()
        },
        gpu: fabstir_llm_node::tee::host_profile::ProfileGpu {
            hwmodels: vec!["GH100".into()],
            min_driver_version: Some("580.95.05".into()),
            min_vbios_version: None,
        },
        not_before: 1_800_000_000,
        expiry: 1_800_000_000 + 300 * 86_400,
        evidence: "A-24 gpu-16c-64g-1gpu; dstack 0.5.9".into(),
    };
    assert_eq!(p, want, "mutation: drop a field");
    assert!(check_profile(
        &p,
        &measurements_of(&run1),
        &run1.cvm.os_image_hash,
        PREDICTED
    )
    .is_empty());
    assert!(new_profile(NewProfile {
        expiry: 1_800_000_000 + 401 * 86_400,
        ..n()
    })
    .is_err());

    // Through the binary: `profile new` writes the same profile; `profile check` passes.
    let d = tempfile::tempdir().unwrap();
    let put = |n: &str, b: &[u8]| std::fs::write(d.path().join(n), b).unwrap();
    put(
        "m.json",
        &serde_json::to_vec(&measurements_of(&run1)).unwrap(),
    );
    put("compose.yml", COMPOSE.as_bytes());
    put("att.json", attestation.as_bytes());
    put("env.txt", ENV_PASTE.as_bytes());
    let inputs = format!(
        "--measurements @m.json --os-image-digest {} --compose-file @compose.yml \
         --compose-reference @att.json --env-names @env.txt",
        run1.cvm.os_image_hash
    );
    let new = format!(
        "profile new --id phala/h200-next --class phala-h200 --not-before 1800000000 \
         --expiry {} --evidence A-24/cli --hwmodel GH100 --min-driver-version 580.95.05 \
         --min-vbios-version 96.00.DA.00.11 --out @p.json {inputs}",
        1_800_000_000u64 + 300 * 86_400
    );
    let out = kbs_in(d.path(), &new);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The platform fields the prediction inherits are printed for review.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("reference.pre_launch_script=")
            && stdout.contains("reference.kms_enabled=true"),
        "{stdout}"
    );
    let written = std::fs::read(d.path().join("p.json")).unwrap();
    let cli: HostProfile = serde_json::from_slice(&written).unwrap();
    let mut gpu = want.gpu.clone();
    gpu.min_vbios_version = Some("96.00.DA.00.11".into());
    assert_eq!(
        cli,
        HostProfile {
            evidence: "A-24/cli".into(),
            gpu,
            ..want
        },
        "a driver/vbios mix-up"
    );
    assert!(!String::from_utf8_lossy(&written).contains("notarealvalue"));
    let out = kbs_in(
        d.path(),
        &format!("profile check --profile @p.json {inputs}"),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("profile_check=ok"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // A differing prediction exits non-zero.
    let other = inputs.replace(&run1.cvm.os_image_hash, &flip(&run1.cvm.os_image_hash));
    let out = kbs_in(
        d.path(),
        &format!("profile check --profile @p.json {other}"),
    );
    assert!(!out.status.success() && String::from_utf8_lossy(&out.stderr).contains("os image"));
}

/// `s` with its first hex digit changed (stays valid hex).
fn flip(s: &str) -> String {
    let first = if s.starts_with('0') { "1" } else { "0" };
    format!("{first}{}", &s[1..])
}

#[test]
fn profile_check_refuses_each_differing_register_digest_and_compose() {
    let p = run1_profile();
    let (m, os, compose) = (
        measurements_of(&p),
        p.cvm.os_image_hash.clone(),
        p.cvm.compose_hashes[0].clone(),
    );
    assert!(check_profile(&p, &m, &os, &compose).is_empty());
    // (row, edit of the predicted inputs): each one differs in exactly one row.
    type Inputs = (Measurements, String, String);
    type Edit = (&'static str, fn(&mut Inputs));
    let cases: [Edit; 6] = [
        ("mrtd", |i| i.0.mrtd = flip(&i.0.mrtd)),
        ("rtmr0", |i| i.0.rtmr0 = flip(&i.0.rtmr0)),
        ("rtmr1", |i| i.0.rtmr1 = flip(&i.0.rtmr1)),
        ("rtmr2", |i| i.0.rtmr2 = flip(&i.0.rtmr2)),
        ("os image", |i| i.1 = flip(&i.1)),
        ("compose hash", |i| i.2 = flip(&i.2)),
    ];
    for (row, edit) in cases {
        let mut i = (m.clone(), os.clone(), compose.clone());
        edit(&mut i);
        let rows = check_profile(&p, &i.0, &i.1, &i.2);
        assert_eq!(rows.len(), 1, "{row}: {rows:?}");
        assert!(
            rows[0].starts_with(&format!("{row}:")),
            "mutation: compare five ({row}): {rows:?}"
        );
    }
}

#[test]
fn profile_pin_refuses_unless_every_capture_row_matches_and_values_match_and_defaults_key_provider_to_the_constant(
) {
    let id = capture_identity(&fixture("run1-boot2-request.json")).unwrap();
    // The constant is run 1's key-provider event, from the capture, not from itself.
    let event = hex::encode(id.replayed.key_provider.as_ref().unwrap());
    assert_eq!(
        PHALA_KMS_KEY_PROVIDER, event,
        "mutation: change the constant"
    );
    let verified: serde_json::Value =
        serde_json::from_slice(&fixture("run1-boot2-verified.json")).unwrap();
    assert_eq!(verified["events"]["key_provider"], PHALA_KMS_KEY_PROVIDER);
    let pinned_run1 = run1_profile();
    let mut unpinned = pinned_run1.clone();
    (unpinned.cvm.app_id, unpinned.cvm.key_provider) = (None, None);
    // Every row matches, the app id is the dashboard's: pinned with the constant.
    let ok = pin_profile(unpinned.clone(), &id, APP_ID, None).unwrap();
    assert_eq!(
        ok.profile, pinned_run1,
        "the migrated, pinned profile exactly"
    );
    assert!(ok.notice.is_none());
    // Each identity row, flipped in the profile, refuses by name.
    type Edit = (&'static str, fn(&mut HostProfile));
    let rows: [Edit; 6] = [
        ("mrtd", |p| p.cvm.mrtd = flip(&p.cvm.mrtd)),
        ("rtmr0", |p| p.cvm.rtmr0 = flip(&p.cvm.rtmr0)),
        ("rtmr1", |p| p.cvm.rtmr1 = flip(&p.cvm.rtmr1)),
        ("rtmr2", |p| p.cvm.rtmr2 = flip(&p.cvm.rtmr2)),
        ("os image", |p| {
            p.cvm.os_image_hash = flip(&p.cvm.os_image_hash)
        }),
        ("compose hash", |p| {
            p.cvm.compose_hashes = vec![flip(&p.cvm.compose_hashes[0])]
        }),
    ];
    for (row, edit) in rows {
        let mut p = unpinned.clone();
        edit(&mut p);
        let e = pin_profile(p, &id, APP_ID, None).unwrap_err();
        assert!(
            e.0.contains(&format!("{row}: capture")),
            "mutation: skip a row ({row}): {e}"
        );
    }
    let mut bad = id.clone();
    bad.replayed.rtmr3[0] ^= 1;
    assert!(pin_profile(unpinned.clone(), &bad, APP_ID, None)
        .unwrap_err()
        .0
        .contains("event log"));
    let e = pin_profile(unpinned.clone(), &id, &flip(APP_ID), None).unwrap_err();
    assert!(e.0.contains("app id: capture event"), "{e}");
    // A capture whose key provider differs from the constant refuses without
    // the override; with it, the override is pinned and STOP is printed.
    let mut other = id.clone();
    other.replayed.key_provider = Some(b"{\"name\":\"other\"}".to_vec());
    let e = pin_profile(unpinned.clone(), &other, APP_ID, None).unwrap_err();
    assert!(
        e.0.contains("key provider: capture event"),
        "mutation: accept a different key_provider: {e}"
    );
    let over = hex::encode(b"{\"name\":\"other\"}");
    let p = pin_profile(unpinned.clone(), &other, APP_ID, Some(&over)).unwrap();
    assert_eq!(p.profile.cvm.key_provider.as_deref(), Some(over.as_str()));
    assert!(p.notice.unwrap().starts_with("STOP"));
    assert!(pin_profile(pinned_run1, &id, APP_ID, None)
        .unwrap_err()
        .0
        .contains("already pinned"));
    let mut half = unpinned.clone();
    half.cvm.key_provider = Some(PHALA_KMS_KEY_PROVIDER.into());
    assert!(pin_profile(half, &id, APP_ID, None)
        .unwrap_err()
        .0
        .contains("already pinned"));
    // Through the binary: STOP goes to stderr on the override path only.
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("request.json"),
        fixture("run1-boot2-request.json"),
    )
    .unwrap();
    std::fs::write(
        d.path().join("u.json"),
        serde_json::to_vec(&unpinned).unwrap(),
    )
    .unwrap();
    for (n, over) in [("a.json", ""), ("b.json", PHALA_KMS_KEY_PROVIDER)] {
        let flag = if over.is_empty() {
            String::new()
        } else {
            format!("--key-provider-override {over}")
        };
        let cmd = format!(
            "profile pin --profile @u.json --capture @. --app-id {APP_ID} --out @{n} {flag}"
        );
        let out = kbs_in(d.path(), &cmd);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stderr).contains("STOP"),
            !over.is_empty()
        );
    }
}
