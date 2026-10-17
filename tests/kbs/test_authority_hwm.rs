//! Policy schema 3 (V3.2), D10: the persisted per-root high-water mark against
//! rollback and counter burning, the write-once archive, the read-only tool
//! store, and the removed-profile ledger's persistence. Each test names the
//! mutation that turns it red.

use super::policy_fixture::{
    address_of, bundle, recording_policy, recording_profile, root_key, sign_doc, signer_key,
    test_model_id, write_bundle, NOW,
};
use fabstir_llm_node::kbs::authority::{archive_name, AuthorityStore, Hwm, HwmStore, MAX_JUMP};
use fabstir_llm_node::kbs::error::Kind;
use fabstir_llm_node::tee::host_profile::{AuthorityBundle, HostProfile};
use fabstir_llm_node::tee::types::Policy;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

pub(super) struct Dirs {
    _tmp: tempfile::TempDir,
    pub(super) auth: PathBuf,
    pub(super) hwm: PathBuf,
    pub(super) archive: PathBuf,
}

pub(super) fn dirs() -> Dirs {
    let tmp = tempfile::tempdir().unwrap();
    let d = Dirs {
        auth: tmp.path().join("public/authority"),
        hwm: tmp.path().join("hwm"),
        archive: tmp.path().join("archive"),
        _tmp: tmp,
    };
    for p in [&d.auth, &d.hwm, &d.archive] {
        std::fs::create_dir_all(p).unwrap();
    }
    d
}

pub(super) fn store(d: &Dirs) -> AuthorityStore {
    AuthorityStore::new(&d.auth, &d.hwm, &d.archive, false)
}

pub(super) fn policy() -> Policy {
    recording_policy(test_model_id(1), 1)
}

pub(super) fn root() -> String {
    address_of(&root_key())
}

pub(super) fn hwm(d: &Dirs) -> Option<Hwm> {
    HwmStore::new(&d.hwm, true).read(&root()).unwrap()
}

pub(super) fn serve(
    d: &Dirs,
    b: &AuthorityBundle,
) -> Result<(), fabstir_llm_node::kbs::error::KbsError> {
    write_bundle(&d.auth, b);
    store(d).load(&policy(), NOW).map(|_| ())
}

fn named(id: &str) -> HostProfile {
    HostProfile {
        id: id.into(),
        ..recording_profile()
    }
}

#[test]
fn hwm_is_raised_only_after_full_validation_including_window() {
    // Mutations: raise before the signature checks; raise for a not-yet-valid
    // manifest → the HWM moves on a refused bundle → red.
    let d = dirs();
    serve(&d, &bundle(1, 1, vec![recording_profile()])).unwrap();
    assert_eq!(hwm(&d).unwrap().epoch, 1);
    // A badly signed epoch 2 is refused and leaves the HWM at 1.
    let mut bad = bundle(1, 2, vec![recording_profile()]);
    bad.set.signatures[0][3] ^= 1;
    assert!(serve(&d, &bad).is_err());
    assert_eq!(
        hwm(&d).unwrap().epoch,
        1,
        "a refused set never raises the HWM"
    );
    // A manifest v2 that is not yet valid is refused and leaves manifest_version at 1.
    let mut early = bundle(2, 1, vec![recording_profile()]);
    early.manifest.manifest.not_before = NOW + 3600;
    early.manifest.signature = sign_doc(&early.manifest.manifest, &root_key());
    let e = serve(&d, &early).unwrap_err();
    assert!(e.detail.contains("outside its window"), "{e}");
    assert_eq!(
        hwm(&d).unwrap().manifest_version,
        1,
        "a not-yet-valid manifest never raises it"
    );
}

#[test]
fn equal_version_requires_equal_digest() {
    // Mutation: compare the counters only → a second, different body at the
    // same epoch is accepted (a signer could swap a set without an epoch bump).
    let d = dirs();
    serve(&d, &bundle(1, 3, vec![named("a")])).unwrap();
    serve(&d, &bundle(1, 3, vec![named("a")])).expect("the same body again is fine");
    let e = serve(&d, &bundle(1, 3, vec![named("b")])).unwrap_err();
    assert_eq!(e.kind, Kind::Verification);
    assert!(
        e.detail
            .contains("epoch 3 was already loaded with a different body"),
        "{e}"
    );
    // Same for the manifest.
    let mut m = bundle(1, 3, vec![named("a")]);
    m.manifest.manifest.expiry -= 1;
    m.manifest.signature = sign_doc(&m.manifest.manifest, &root_key());
    let e = serve(&d, &m).unwrap_err();
    assert!(
        e.detail
            .contains("manifest_version 1 was already loaded with a different body"),
        "{e}"
    );
}

#[test]
fn hwm_raise_refuses_below_and_holds_the_lock_across_compare_and_write() {
    // Direct `HwmStore::raise`. Mutations: drop the below-compare → `raise(5)`
    // succeeds; compare (read + compare + max) outside the lock → B completes
    // while A is parked and A then writes 7 over B's 8 → the file says 7.
    let d = dirs();
    let s = std::sync::Arc::new(HwmStore::new(&d.hwm, false));
    let h = |e: u64| Hwm {
        manifest_version: 1,
        manifest_digest: "aa".repeat(32),
        epoch: e,
        set_digest: format!("{e:02x}").repeat(32),
    };
    let r = root();
    s.raise(&r, &h(6)).unwrap();
    let e = s.raise(&r, &h(5)).unwrap_err();
    assert!(e.detail.contains("below the high-water mark"), "{e}");
    assert_eq!(
        s.read(&r).unwrap().unwrap().epoch,
        6,
        "the file still says 6"
    );

    let (parked_tx, parked_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let (sa, ra) = (s.clone(), r.clone());
    let a = std::thread::spawn(move || {
        sa.raise_with_hook(&ra, &h(7), || {
            parked_tx.send(()).unwrap();
            go_rx.recv().unwrap();
        })
    });
    parked_rx.recv().unwrap();
    let (b_done_tx, b_done_rx) = mpsc::channel();
    let (sb, rb) = (s.clone(), r.clone());
    let b = std::thread::spawn(move || {
        let out = sb.raise(&rb, &h(8));
        b_done_tx.send(()).unwrap();
        out
    });
    // Correct: B is blocked on the lock for the whole window.
    let b_finished_early = b_done_rx.recv_timeout(Duration::from_millis(1000)).is_ok();
    go_tx.send(()).unwrap();
    a.join().unwrap().unwrap();
    let _ = b.join().unwrap();
    assert_eq!(
        s.read(&r).unwrap().unwrap().epoch,
        8,
        "B finished early: {b_finished_early}"
    );
}

#[test]
fn jump_over_16_is_refused_except_first_load_for_epoch_and_for_manifest_version() {
    // Mutations: drop the epoch bound / the manifest_version bound / the
    // first-load exemption → red.
    let d = dirs();
    let far = 1 + MAX_JUMP + 1;
    serve(&d, &bundle(1, far, vec![recording_profile()])).expect("first load: no bound");
    let e = serve(
        &d,
        &bundle(1, 2 * far + MAX_JUMP, vec![recording_profile()]),
    )
    .unwrap_err();
    assert!(e.detail.contains("jumps more than 16"), "{e}");
    serve(&d, &bundle(1, far + MAX_JUMP, vec![recording_profile()])).expect("+16 is allowed");
    let e = serve(
        &d,
        &bundle(
            1 + MAX_JUMP as u32 + 1,
            far + MAX_JUMP,
            vec![recording_profile()],
        ),
    )
    .unwrap_err();
    assert!(e.detail.contains("manifest_version 18 jumps"), "{e}");
    assert_eq!(hwm(&d).unwrap().epoch, far + MAX_JUMP);
}

#[test]
fn hwm_survives_restart() {
    // Mutation: keep the HWM in memory only → a fresh store accepts epoch 1.
    let d = dirs();
    serve(&d, &bundle(1, 2, vec![recording_profile()])).unwrap();
    drop(store(&d)); // "restart": every store below is fresh
    let e = serve(&d, &bundle(1, 1, vec![recording_profile()])).unwrap_err();
    assert!(
        e.detail.contains("epoch 1 is below the high-water mark 2"),
        "{e}"
    );
}

#[test]
fn corrupt_hwm_is_a_fault_missing_is_created() {
    // Mutation: treat a corrupt file as missing → it is overwritten → a rollback reopens.
    let d = dirs();
    assert!(hwm(&d).is_none());
    serve(&d, &bundle(1, 1, vec![recording_profile()])).unwrap();
    assert!(hwm(&d).is_some(), "created on the first valid bundle");
    let f = d
        .hwm
        .join(format!("{}.json", root().trim_start_matches("0x")));
    std::fs::write(&f, b"{\"epoch\": ").unwrap();
    let e = serve(&d, &bundle(1, 1, vec![recording_profile()])).unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::Unavailable, 500), "{e}");
    assert!(e.detail.contains("corrupt"), "{e}");
    assert_eq!(
        std::fs::read(&f).unwrap(),
        b"{\"epoch\": ",
        "left for the operator"
    );
    // A fresh install (no HWM, no archive) creates it, owner-only. (Deleting
    // it next to a non-empty archive is a fault: see the lost-HWM test.)
    use std::os::unix::fs::PermissionsExt;
    let fresh = dirs();
    serve(&fresh, &bundle(1, 1, vec![recording_profile()])).unwrap();
    let mode = std::fs::metadata(hwm_file(&fresh))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn accepted_bundle_is_archived_once_per_body_pair() {
    // Mutations: skip the archive; key it on the file bytes (a co-signature-only
    // change would add a file).
    let d = dirs();
    // A store created before anything was loaded: its ledger cache is empty,
    // so loading the co-signed bundle through it reaches the archive step.
    let early = store(&d);
    let b1 = bundle(1, 1, vec![recording_profile()]);
    serve(&d, &b1).unwrap();
    let mut cosigned = b1.clone();
    cosigned
        .set
        .signatures
        .push(sign_doc(&cosigned.set.set, &signer_key(5)));
    write_bundle(&d.auth, &cosigned);
    early.load(&policy(), NOW).unwrap();
    serve(&d, &bundle(1, 2, vec![recording_profile()])).unwrap();
    // A manifest-only bump (same set) is a new body pair: its own entry.
    serve(&d, &bundle(2, 2, vec![recording_profile()])).unwrap();
    let dir = d.archive.join(root().trim_start_matches("0x"));
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(
        names[0].starts_with(&format!("{:020}-{:010}-", 1, 1)),
        "{names:?}"
    );
    assert!(
        names[1].starts_with(&format!("{:020}-{:010}-", 2, 1)),
        "{names:?}"
    );
    assert_eq!(names[2], archive_name(&hwm(&d).unwrap()));
}

#[test]
fn read_only_store_never_writes() {
    // Mutation: write anyway → the HWM file / archive appear.
    let d = dirs();
    write_bundle(&d.auth, &bundle(1, 4, vec![recording_profile()]));
    AuthorityStore::new(&d.auth, &d.hwm, &d.archive, true)
        .load(&policy(), NOW)
        .unwrap();
    assert!(hwm(&d).is_none(), "no HWM written");
    assert_eq!(
        std::fs::read_dir(&d.archive).unwrap().count(),
        0,
        "no archive written"
    );
    // ...but it still refuses a rollback against an HWM that exists.
    serve(&d, &bundle(1, 5, vec![recording_profile()])).unwrap();
    write_bundle(&d.auth, &bundle(1, 4, vec![recording_profile()]));
    let e = AuthorityStore::new(&d.auth, &d.hwm, &d.archive, true)
        .load(&policy(), NOW)
        .unwrap_err();
    assert!(e.detail.contains("below the high-water mark"), "{e}");
}

#[test]
fn removed_profile_ledger_survives_restart() {
    // Mutation: an in-memory ledger (the previous set remembered by the
    // process, not read from the archive) → after a restart the removed
    // profile is unknown → no hint.
    let d = dirs();
    serve(&d, &bundle(1, 1, vec![named("gone"), named("kept")])).unwrap();
    serve(&d, &bundle(1, 2, vec![named("kept")])).unwrap();
    let fresh = store(&d); // restart: rebuilt from the archive at construction
    let auth = fresh.load(&policy(), NOW).unwrap();
    let removed: Vec<_> = auth
        .removed
        .iter()
        .map(|r| (r.profile.id.clone(), r.epoch))
        .collect();
    assert_eq!(removed, vec![("gone".to_string(), 2)]);
    assert!(auth.removed[0]
        .archive_file
        .starts_with(&format!("{:020}-", 1)));
}

pub(super) fn hwm_file(d: &Dirs) -> PathBuf {
    d.hwm
        .join(format!("{}.json", root().trim_start_matches("0x")))
}
