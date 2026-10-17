//! Policy schema 3 (D10, D14, D17): `authority check`, offline and read-only:
//! `--warn-days`, `--hwm-dir` (served != HWM, lost HWM, read-only), the policy
//! rows (window, zero candidates) and the D6 file name.

use super::policy_fixture::{
    recording_policy, recording_profile, sign, signer, test_model_id, Profile,
};
use super::tools_fixture::{bundle, check, manifest, set, walkdir, FAR};
use fabstir_llm_node::kbs::authority::{HwmStore, MAX_JUMP};
use fabstir_llm_node::kbs::tools_authority::bundle_hwm;
use fabstir_llm_node::tee::host_profile::{AuthorityBundle, HostProfileSet};

#[test]
fn authority_check_warn_days_flags_expiries_within_the_window() {
    let now = 2_000_000_000u64;
    let day = 86_400;
    let profile = |id: &str, expires_in: u64| Profile {
        id: id.into(),
        not_before: now - day,
        expiry: now + expires_in * day,
        ..recording_profile()
    };
    let s = HostProfileSet {
        profiles: vec![
            profile("soon", 20),
            profile("later", 100),
            Profile {
                not_before: now - 30 * day,
                expiry: now - day,
                ..profile("gone", 0)
            },
        ],
        ..set(1, "a")
    };
    let b = bundle(manifest(1, now + 10 * day), s);
    let mut policy = recording_policy(test_model_id(9), 1);
    (policy.not_before, policy.expiry) = (now - day, now + 5 * day);
    let (signed, _) = sign(&policy, &signer());
    let policy_bytes = serde_json::to_vec(&signed).unwrap();
    let run = |days: Option<u64>| {
        check(&b, now, |i| {
            i.warn_days = days;
            i.policies = vec![("p.json".into(), policy_bytes.clone())];
        })
    };
    let r = run(Some(30));
    assert!(r.failures.is_empty(), "{r:?}");
    let w = r.warnings.join("\n");
    for needle in [
        "manifest v1 expires",
        "profile soon expires",
        "policy p.json",
        "profile gone expired at",
    ] {
        assert!(w.contains(needle), "{needle}: {w}");
    }
    assert!(!w.contains("profile later"), "{w}");
    assert_eq!(r.warnings.len(), 4, "{w}");
    // Without the window only the expired profile is reported (hosts on it
    // are refused today): every day, until it leaves the set.
    for quiet in [Some(3), None] {
        let r = run(quiet);
        assert_eq!(
            r.warnings.len(),
            1,
            "mutation: ignore the threshold ({quiet:?}): {r:?}"
        );
        assert!(r.warnings[0].contains("profile gone expired"), "{r:?}");
    }
}

#[test]
fn authority_check_is_read_only_and_reports_served_not_equal_hwm() {
    let data = tempfile::tempdir().unwrap();
    let (hwm_dir, archive) = (data.path().join("hwm"), data.path().join("archive"));
    std::fs::create_dir_all(&hwm_dir).unwrap();
    std::fs::create_dir_all(&archive).unwrap();
    let b1 = bundle(manifest(1, FAR), set(1, "a"));
    let b2 = bundle(manifest(1, FAR), set(2, "b"));
    let root = b1.manifest.manifest.root.clone();
    let snapshot = || {
        let mut v = walkdir(data.path());
        v.sort();
        v
    };
    let with_hwm = |b: &AuthorityBundle| check(b, 1, |i| i.hwm_dir = Some(hwm_dir.clone()));
    // No HWM, no archive: never loaded (a warning), and nothing is created.
    let before = snapshot();
    let r = with_hwm(&b1);
    assert!(
        r.failures.is_empty() && r.warnings[0].contains("never loaded"),
        "{r:?}"
    );
    assert_eq!(snapshot(), before, "mutation: write (an HWM appeared)");
    // HWM at b1 (written the broker's way): b1 is served; b2 is not loaded yet.
    HwmStore::new(&hwm_dir, false)
        .raise(&root, &bundle_hwm(&b1).unwrap())
        .unwrap();
    let before = snapshot();
    let r = with_hwm(&b1);
    assert!(
        r.failures.is_empty() && r.lines.iter().any(|l| l.starts_with("hwm=served")),
        "{r:?}"
    );
    let r = with_hwm(&b2);
    assert!(
        r.failures
            .iter()
            .any(|f| f.contains("served != hwm") && f.contains("not loaded it yet")),
        "mutation: drop the report: {r:?}"
    );
    assert_eq!(snapshot(), before, "mutation: write (the HWM moved to b2)");
    // A fork: the HWM's numbers with another body is refused, not "served".
    let fork = bundle(manifest(1, FAR), set(1, "fork"));
    assert!(
        with_hwm(&fork)
            .failures
            .iter()
            .any(|f| f.contains("served != hwm") && f.contains("refuses")),
        "mutation: compare the numbers only"
    );
    // HWM at b2: a served b1 is a rollback the broker refuses.
    HwmStore::new(&hwm_dir, false)
        .raise(&root, &bundle_hwm(&b2).unwrap())
        .unwrap();
    let r = with_hwm(&b1);
    assert!(
        r.failures
            .iter()
            .any(|f| f.contains("served != hwm") && f.contains("refuses")),
        "{r:?}"
    );
    // A jump past the bound is named by the broker's own rule.
    let far = bundle(manifest(1, FAR), set(2 + MAX_JUMP + 1, "far"));
    assert!(with_hwm(&far)
        .failures
        .iter()
        .any(|f| f.contains("refuses")));
    // No HWM beside a non-empty archive: lost (the store's rule, read-only too).
    std::fs::remove_file(hwm_dir.join(format!("{}.json", &root[2..]))).unwrap();
    let entry = archive.join(&root[2..]);
    std::fs::create_dir_all(&entry).unwrap();
    std::fs::write(entry.join("x.json"), b"{}").unwrap();
    let r = with_hwm(&b2);
    assert!(
        r.failures.iter().any(|f| f.contains("HWM was lost")),
        "{r:?}"
    );
    // Only the served bundle's own archive entry: an interrupted first load.
    std::fs::remove_file(entry.join("x.json")).unwrap();
    let name = fabstir_llm_node::kbs::authority::archive_name(&bundle_hwm(&b2).unwrap());
    std::fs::write(entry.join(name), b"{}").unwrap();
    let r = with_hwm(&b2);
    assert!(
        r.failures.is_empty()
            && r.warnings
                .iter()
                .any(|w| w.contains("interrupted first load")),
        "{r:?}"
    );
    // A wrong --hwm-dir is a refusal, never "never loaded".
    let typo = check(&b2, 1, |i| i.hwm_dir = Some(data.path().join("hwn")));
    assert!(
        typo.failures
            .iter()
            .any(|f| f.contains("is not a directory")),
        "{typo:?}"
    );
    let lone = tempfile::tempdir().unwrap();
    std::fs::create_dir(lone.path().join("hwm")).unwrap();
    let r = check(&b2, 1, |i| i.hwm_dir = Some(lone.path().join("hwm")));
    assert!(
        r.failures.iter().any(|f| f.contains("no archive/")),
        "{r:?}"
    );
}

#[test]
fn authority_check_refuses_an_expired_policy_or_no_candidate_and_names_a_misnamed_file() {
    // The recording profile's window is around the real clock.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let b = bundle(manifest(1, FAR), set(1, "a"));
    let policy_check = |edit: &dyn Fn(&mut fabstir_llm_node::tee::types::Policy)| {
        let mut p = recording_policy(test_model_id(9), 1);
        (p.not_before, p.expiry) = (now - 86_400, now + 86_400);
        edit(&mut p);
        let bytes = serde_json::to_vec(&sign(&p, &signer()).0).unwrap();
        check(&b, now, |i| i.policies = vec![("p.json".into(), bytes)])
    };
    let ok = policy_check(&|_| {});
    assert!(ok.failures.is_empty() && ok.warnings.is_empty(), "{ok:?}");
    // An expired policy refuses (the daily timer); one signed ahead (the T-1
    // seal) only warns: the node refuses it until then.
    let r = policy_check(&|p| p.expiry = now - 1);
    assert!(
        r.failures.iter().any(|f| f.contains("expired at")),
        "mutation: drop the expiry row: {r:?}"
    );
    let r = policy_check(&|p| p.not_before = now + 10);
    assert!(
        r.failures.is_empty() && r.warnings.iter().any(|w| w.contains("not valid before")),
        "{r:?}"
    );
    // No candidate: an outage is a refusal; unpinned profiles only (the
    // two-boot first epoch, D18) is a warning.
    let r = policy_check(&|p| p.host_authority.allowed_classes = vec!["other".into()]);
    assert!(
        r.failures.iter().any(|f| f.contains("nothing releases")),
        "mutation: warn only: {r:?}"
    );
    // ... but not for a staged epoch checked ahead of its policy (T-1).
    let r = policy_check(&|p| {
        p.not_before = now + 10;
        p.host_authority.allowed_classes = vec!["other".into()];
    });
    assert!(
        r.failures.is_empty() && r.warnings.iter().any(|w| w.contains("opens at")),
        "mutation: refuse a staged epoch: {r:?}"
    );
    let mut unpinned = recording_profile();
    (unpinned.cvm.app_id, unpinned.cvm.key_provider) = (None, None);
    let b2 = bundle(
        manifest(1, FAR),
        HostProfileSet {
            profiles: vec![unpinned],
            ..set(1, "a")
        },
    );
    let mut p = recording_policy(test_model_id(9), 1);
    (p.not_before, p.expiry) = (now - 86_400, now + 86_400);
    (
        p.cvm.require_app_id_pinned,
        p.cvm.require_key_provider_pinned,
    ) = (true, true);
    let bytes = serde_json::to_vec(&sign(&p, &signer()).0).unwrap();
    let r = check(&b2, now, |i| i.policies = vec![("p.json".into(), bytes)]);
    assert!(
        r.failures.is_empty() && r.warnings.iter().any(|w| w.contains("two-boot")),
        "{r:?}"
    );
    // D6: a served file is named for its root.
    let r = check(&b, now, |i| {
        i.bundle_name = "00000000000000000000000000000000000000aa.json"
    });
    assert!(
        r.failures.iter().any(|f| f.contains("named for root")),
        "mutation: skip the name check: {r:?}"
    );
}
