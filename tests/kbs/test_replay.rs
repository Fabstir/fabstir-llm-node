//! D22: replay of a real capture is the hardware-free regression gate. Run 1
//! boot 2 (a real, non-test entry) releases under the migrated v3 documents
//! through the prefilter, `check_policy` AND the independent `release_gate`
//! (D21); then each identity row, flipped in memory, refuses with the row
//! that names the profile. `replay_dir_harness` (`#[ignore]`; a harness, not a
//! gate) replays every capture dir under `KBS_REPLAY_DIR`.

use super::fixtures::{fixture, fixture_dir};
use super::policy_fixture::{sign_doc, signer_key};
use fabstir_llm_node::kbs::authority::Authority;
use fabstir_llm_node::kbs::nras_claims::GpuOutcome;
use fabstir_llm_node::kbs::routes::release_gate;
use fabstir_llm_node::kbs::tools_authority::{
    authority_check, capture_from, judge_capture, load_capture, Capture, CheckInputs,
};
use fabstir_llm_node::tee::host_profile::parse_bundle;
use fabstir_llm_node::tee::policy::SignedModelPolicy;
use fabstir_llm_node::tee::types::Policy;
use std::path::Path;

const PROVIDER: &str = "0x18abb9ca9428d272c8796158b1f5ac321855a5bd";

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedOutcome {
    now: u64,
    entry_test: bool,
    outcome: String,
    headline_profile: String,
}

fn documents(dir: Option<&Path>) -> (Policy, Authority, ExpectedOutcome) {
    let read = |name: &str, own: &str| match dir.map(|d| d.join(own)).filter(|p| p.exists()) {
        Some(p) => std::fs::read(p).unwrap(),
        None => fixture(name),
    };
    let signed: SignedModelPolicy =
        serde_json::from_slice(&read("v3-policy-signed.json", "policy.json")).unwrap();
    if dir.is_none() {
        signed.verify_signer(PROVIDER).unwrap();
    }
    let expected: ExpectedOutcome =
        serde_json::from_slice(&read("run1-boot2-expected.json", "expected.json")).unwrap();
    let bundle = parse_bundle(&read("authority-bundle.json", "bundle.json"), "bundle").unwrap();
    let auth = Authority::from_bundle(bundle, &signed.policy, expected.now).unwrap();
    (signed.policy, auth, expected)
}

/// (identity row, the in-memory flip, the start of the row it must produce).
type Flip = (&'static str, fn(&mut Capture), &'static str);

fn run1() -> Capture {
    capture_from(
        &fixture("run1-boot2-request.json"),
        &fixture("run1-boot2-verified.json"),
    )
    .unwrap()
}

#[test]
fn replay_run1_boot2_releases_then_each_identity_row_flip_refuses() {
    let (policy, auth, want) = documents(None);
    assert_eq!((want.outcome.as_str(), want.entry_test), ("release", false));
    let c = run1();
    let v = judge_capture(&c, &policy, &auth, want.entry_test, want.now).expect("run 1 releases");
    assert_eq!(v.profile.headline, want.headline_profile);
    assert_eq!(v.profile.all, std::slice::from_ref(&want.headline_profile));
    assert_eq!(
        (
            v.hwmodel.as_deref(),
            v.driver_version.as_deref(),
            v.tcb_status.as_str()
        ),
        (Some("GH100"), Some("580.95.05"), "UpToDate")
    );
    // The independent gate passes on its own, on a REAL entry, and is not a
    // pass-through: a disagreeing step-7 record refuses (D21, R4-adv 1).
    let gate = |v: &fabstir_llm_node::kbs::verify::Verified| {
        release_gate(
            &auth.manifest,
            &auth.set,
            want.now,
            &c.cpu,
            &c.replayed,
            &c.gpu,
            &policy,
            v,
            false,
        )
    };
    gate(&v).expect("the gate passes run 1");
    let mut wrong = v.clone();
    wrong.profile.headline = "phala/other".into();
    assert!(gate(&wrong).unwrap_err().detail.contains("disagreement"));

    let id = &want.headline_profile;
    let flips: [Flip; 9] = [
        ("mrtd", |c| c.cpu.mr_td[0] ^= 1, "mrtd: "),
        ("rtmr0", |c| c.cpu.rt_mr0[0] ^= 1, "rtmr0: "),
        ("rtmr1", |c| c.cpu.rt_mr1[0] ^= 1, "rtmr1: "),
        ("rtmr2", |c| c.cpu.rt_mr2[0] ^= 1, "rtmr2: "),
        (
            "os_image_hash",
            |c| c.replayed.os_image_hash[0] ^= 1,
            "os image: ",
        ),
        (
            "compose_hash",
            |c| c.replayed.compose_hash[0] ^= 1,
            "compose hash: ",
        ),
        (
            "app_id",
            |c| c.replayed.app_id.as_mut().unwrap()[0] ^= 1,
            "app id: ",
        ),
        (
            "key_provider",
            |c| c.replayed.key_provider.as_mut().unwrap()[0] ^= 1,
            "key provider: ",
        ),
        (
            "hwmodel",
            |c| {
                if let GpuOutcome::Real(f) = &mut c.gpu {
                    f.hwmodel = "GB999".into();
                }
            },
            "hwmodel: \"GB999\" not in profile ",
        ),
    ];
    for (row, flip, needle) in flips {
        let mut c = run1();
        flip(&mut c);
        let e = judge_capture(&c, &policy, &auth, false, want.now).unwrap_err();
        let named = e
            .detail
            .split("; ")
            .any(|r| r.starts_with(needle) && r.contains(&format!("profile {id}")));
        assert!(named, "mutation: skip row {row}: {}", e.detail);
    }
    // The prefilter runs too: a GPU wrapper nonce that is not the request's
    // refuses before step 7 (which never reads the wrapper).
    let mut c = run1();
    let mut g: serde_json::Value = serde_json::from_slice(&c.gpu_report).unwrap();
    g["nonce"] = "00".repeat(32).into();
    c.gpu_report = serde_json::to_vec(&g).unwrap();
    let e = judge_capture(&c, &policy, &auth, false, want.now).unwrap_err();
    assert!(
        e.detail.contains("gpu nonce (wrapper)"),
        "mutation: prefilter skipped: {}",
        e.detail
    );
}

#[test]
fn authority_check_replay_releases_run1_and_refuses_without_its_policy() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("request.json"),
        fixture("run1-boot2-request.json"),
    )
    .unwrap();
    std::fs::write(
        d.path().join("verified.json"),
        fixture("run1-boot2-verified.json"),
    )
    .unwrap();
    let (_, _, want) = documents(None);
    let bundle = fixture("authority-bundle.json");
    let run = |policies: Vec<(String, Vec<u8>)>| {
        authority_check(&CheckInputs {
            bundle: &bundle,
            bundle_name: "authority-bundle.json",
            policies,
            previous: None,
            warn_days: None,
            replays: vec![d.path().to_path_buf()],
            hwm_dir: None,
            now: want.now,
        })
    };
    let r = run(vec![("p".into(), fixture("v3-policy-signed.json"))]);
    assert!(r.failures.is_empty(), "{r:?}");
    assert!(
        r.lines
            .iter()
            .any(|l| l.contains("outcome=release profile=phala/run1-boot2")),
        "{r:?}"
    );
    let r = run(vec![]);
    assert!(
        r.failures
            .iter()
            .any(|f| f.contains("no --policy for its model")),
        "{r:?}"
    );
    // EVERY policy for the model is replayed, not the first: a second one the
    // bundle cannot meet is named by its version.
    let mut second = documents(None).0;
    (
        second.policy_version,
        second.host_authority.min_profile_epoch,
    ) = (4, 2);
    let (signed, _) = super::policy_fixture::sign(&second, &super::policy_fixture::signer());
    let both = vec![
        ("p".into(), fixture("v3-policy-signed.json")),
        ("q".into(), serde_json::to_vec(&signed).unwrap()),
    ];
    let r = run(both);
    assert!(
        r.lines
            .iter()
            .any(|l| l.contains("policy_version=3 outcome=release")),
        "{r:?}"
    );
    assert!(
        r.failures
            .iter()
            .any(|f| f.contains("(policy_version 4): refused")),
        "mutation: replay the first policy only: {r:?}"
    );
    assert_eq!(load_capture(d.path()).unwrap().expected, run1().expected);
}

#[test]
fn judge_capture_runs_the_gate_and_reads_every_verified_value_from_the_capture() {
    let (policy, auth, want) = documents(None);
    // The gate inside judge_capture is observable: a set whose raw signatures
    // no current signer made passes step 7 (it trusts the loader's count) and
    // is refused by the gate's own recount.
    let mut forged = auth.clone();
    forged.set.signatures = vec![sign_doc(&forged.set.set, &signer_key(9))];
    let e = judge_capture(&run1(), &policy, &forged, false, want.now).unwrap_err();
    assert!(
        e.detail.starts_with("release gate:"),
        "mutation: drop the gate: {}",
        e.detail
    );
    // verified.json must describe request.json's quote.
    let verified = || -> serde_json::Value {
        serde_json::from_slice(&fixture("run1-boot2-verified.json")).unwrap()
    };
    let request = fixture("run1-boot2-request.json");
    let mut v = verified();
    v["tdx"]["rt_mr1"] = "00".repeat(48).into();
    let e = capture_from(&request, &serde_json::to_vec(&v).unwrap()).unwrap_err();
    assert!(
        e.0.contains("does not describe") && e.0.contains("rt_mr1"),
        "{e}"
    );
    // Each verified value the replay trusts is read from the capture, not a default.
    let cases: [(&str, serde_json::Value, &str); 5] = [
        (
            "/tdx/tcb_status",
            "OutOfDate".into(),
            "tcb status: OutOfDate",
        ),
        (
            "/tdx/advisory_ids",
            serde_json::json!(["INTEL-SA-00837"]),
            "advisory",
        ),
        ("/gpu/secure_boot", false.into(), "secure boot: off"),
        ("/gpu/debug_disabled", false.into(), "gpu debug: enabled"),
        (
            "/gpu/driver_version",
            "540.0.0".into(),
            "driver version: 540.0.0 below floor 550.0.0",
        ),
    ];
    for (ptr, value, row) in cases {
        let mut v = verified();
        *v.pointer_mut(ptr).unwrap() = value;
        let c = capture_from(&request, &serde_json::to_vec(&v).unwrap()).unwrap();
        let e = judge_capture(&c, &policy, &auth, false, want.now).unwrap_err();
        assert!(
            e.detail.contains(row),
            "mutation: {ptr} not read: {}",
            e.detail
        );
    }
}

/// Replays every capture dir under `KBS_REPLAY_DIR` (each with `request.json`,
/// `verified.json`, `expected.json`, and optionally its own `policy.json` +
/// `bundle.json`; the committed documents otherwise). A harness, not a gate.
#[test]
#[ignore]
fn replay_dir_harness() {
    let Some(root) = std::env::var_os("KBS_REPLAY_DIR") else {
        eprintln!(
            "KBS_REPLAY_DIR is not set; nothing to replay (fixtures in {})",
            fixture_dir().display()
        );
        return;
    };
    let mut bad = Vec::new();
    let mut dirs: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    dirs.sort();
    for dir in dirs.into_iter().filter(|d| d.join("request.json").exists()) {
        let (policy, auth, want) = documents(Some(&dir));
        let got = load_capture(&dir).map_err(|e| e.0).and_then(|c| {
            judge_capture(&c, &policy, &auth, want.entry_test, want.now).map_err(|e| e.detail)
        });
        let ok = match (&got, want.outcome.as_str()) {
            (Ok(v), "release") => v.profile.headline == want.headline_profile,
            (Err(_), "refuse") => true,
            _ => false,
        };
        eprintln!(
            "{} {}: {:?}",
            if ok { "ok  " } else { "FAIL" },
            dir.display(),
            got.as_ref().map(|v| &v.profile.headline)
        );
        if !ok {
            bad.push(dir);
        }
    }
    assert!(bad.is_empty(), "{bad:?}");
}
