//! Policy schema 3 (V3.2), D10 recovery paths: a lost or stale high-water
//! mark, a crash between the archive write and the HWM write, an archive that
//! cannot be listed, and the invariant that every HWM has its archived bundle.
//! Split from `test_authority_hwm.rs` (400-line cap). Each test names the
//! mutation that turns it red.

use super::policy_fixture::{bundle, recording_profile, sign_doc, signer_key};
use super::test_authority_hwm::{dirs, hwm, hwm_file, root, serve, store, Dirs};
use fabstir_llm_node::kbs::authority::{archive_name, Hwm};
use fabstir_llm_node::kbs::error::Kind;
use fabstir_llm_node::tee::host_profile::{document_digest, AuthorityBundle};
use std::os::unix::fs::PermissionsExt;

#[test]
fn a_lost_hwm_with_an_archive_is_a_fault_and_a_malformed_digest_is_corrupt() {
    // The HWM file vanished (deleted, or a restore without hwm/) while the
    // archive shows bundles were accepted: NOT a first load (which would
    // re-admit epoch 1 with no jump bound). Mutation: drop the archive check.
    let d = dirs();
    serve(&d, &bundle(1, 5, vec![recording_profile()])).unwrap();
    std::fs::remove_file(hwm_file(&d)).unwrap();
    let e = serve(&d, &bundle(1, 1, vec![recording_profile()])).unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::Unavailable, 500), "{e}");
    assert!(
        e.detail.contains("HWM was lost") && e.detail.contains(&format!("{:020}-", 5)),
        "{e}"
    );
    assert!(hwm(&d).is_none(), "nothing was re-created");
    // A parseable HWM whose digest is not 64 hex is corrupt (fail closed), not
    // a different body. Mutation: accept any string as a digest.
    std::fs::write(
        hwm_file(&d),
        br#"{"manifest_version":1,"manifest_digest":"ab","epoch":5,"set_digest":"ab"}"#,
    )
    .unwrap();
    let e = serve(&d, &bundle(1, 5, vec![recording_profile()])).unwrap_err();
    assert!(e.detail.contains("is corrupt: a digest"), "{e}");
}

#[test]
fn every_hwm_has_its_archived_bundle() {
    // The archive is written inside the HWM's critical section BEFORE the
    // HWM: a failed archive write leaves the HWM unmoved, and a failed HWM
    // write removes the archive file it just created. Mutations: write the
    // HWM first; keep the archive file after a failed HWM write.
    use std::os::unix::fs::PermissionsExt;
    let d = dirs();
    serve(&d, &bundle(1, 1, vec![recording_profile()])).unwrap();
    // The archive cannot be written: the root's archive dir is a FILE.
    let adir = d.archive.join(root().trim_start_matches("0x"));
    std::fs::rename(&adir, d.archive.join("moved")).unwrap();
    std::fs::write(&adir, b"not a dir").unwrap();
    let e = serve(&d, &bundle(1, 2, vec![recording_profile()])).unwrap_err();
    assert_eq!((e.kind, e.status), (Kind::Unavailable, 500), "{e}");
    assert_eq!(hwm(&d).unwrap().epoch, 1, "the HWM did not move");
    std::fs::remove_file(&adir).unwrap();
    std::fs::rename(d.archive.join("moved"), &adir).unwrap();
    // The HWM cannot be written: hwm/ is read-only. The archive file created
    // for epoch 2 is removed again, so the archive never runs ahead.
    std::fs::set_permissions(&d.hwm, std::fs::Permissions::from_mode(0o500)).unwrap();
    let e = serve(&d, &bundle(1, 2, vec![recording_profile()])).unwrap_err();
    std::fs::set_permissions(&d.hwm, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!((e.kind, e.status), (Kind::Unavailable, 500), "{e}");
    let epochs: Vec<String> = std::fs::read_dir(&adir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy()[..20].to_string())
        .collect();
    assert_eq!(
        epochs,
        vec![format!("{:020}", 1)],
        "no epoch-2 archive left behind"
    );
    serve(&d, &bundle(1, 2, vec![recording_profile()])).expect("retry succeeds");
    assert_eq!(hwm(&d).unwrap().epoch, 2);
}

#[test]
fn a_crash_orphan_of_the_same_bundle_is_a_first_load_but_any_other_entry_is_not() {
    // A crash between a first load's archive write and its HWM write leaves
    // exactly that bundle's archive file. Loading the same bundle again is not
    // a rollback, so it proceeds. Mutation: refuse any archive entry → the root
    // is wedged for good after such a crash.
    let d = dirs();
    let b = bundle(1, 4, vec![recording_profile()]);
    let h = Hwm {
        manifest_version: 1,
        manifest_digest: hex::encode(document_digest(&b.manifest.manifest).unwrap()),
        epoch: 4,
        set_digest: hex::encode(document_digest(&b.set.set).unwrap()),
    };
    let adir = d.archive.join(root().trim_start_matches("0x"));
    std::fs::create_dir_all(&adir).unwrap();
    std::fs::write(adir.join(archive_name(&h)), serde_json::to_vec(&b).unwrap()).unwrap();
    serve(&d, &b).expect("the orphan is the bundle being loaded");
    assert_eq!(hwm(&d).unwrap(), h);
    // An orphan of ANOTHER bundle (a newer epoch) is the lost-HWM case.
    std::fs::remove_file(hwm_file(&d)).unwrap();
    let e = serve(&d, &bundle(1, 3, vec![recording_profile()])).unwrap_err();
    assert!(e.detail.contains("HWM was lost"), "{e}");
}

#[test]
fn an_archive_that_cannot_be_listed_counts_as_an_archive() {
    // Fail closed. Mutation: treat any listing error as "no archive" → a lost
    // HWM beside an unlistable (0300) archive is taken as a first load and an
    // old epoch is re-admitted.
    let d = dirs();
    serve(&d, &bundle(1, 5, vec![recording_profile()])).unwrap();
    std::fs::remove_file(hwm_file(&d)).unwrap();
    let adir = d.archive.join(root().trim_start_matches("0x"));
    std::fs::set_permissions(&adir, std::fs::Permissions::from_mode(0o300)).unwrap();
    let r = serve(&d, &bundle(1, 2, vec![recording_profile()]));
    std::fs::set_permissions(&adir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let e = r.unwrap_err();
    assert!(e.detail.contains("cannot be listed"), "{e}");
    assert!(hwm(&d).is_none(), "no HWM was written at epoch 2");
}

/// The HWM a bundle stands for.
fn hwm_of(b: &AuthorityBundle) -> Hwm {
    Hwm {
        manifest_version: b.manifest.manifest.manifest_version,
        manifest_digest: hex::encode(document_digest(&b.manifest.manifest).unwrap()),
        epoch: b.set.set.epoch,
        set_digest: hex::encode(document_digest(&b.set.set).unwrap()),
    }
}

/// What a raise interrupted between its archive write and its HWM write leaves.
fn orphan(d: &Dirs, b: &AuthorityBundle, name: &str) {
    let adir = d.archive.join(root().trim_start_matches("0x"));
    std::fs::create_dir_all(&adir).unwrap();
    std::fs::write(adir.join(name), serde_json::to_vec(b).unwrap()).unwrap();
}

#[test]
fn an_interrupted_raise_is_completed_at_start_and_a_forged_entry_is_not() {
    // A crash between the archive write and the HWM write leaves exactly one
    // entry above the HWM: `serve` completes the raise (re-verified from the
    // archive) instead of refusing to start for ever. Mutations: refuse
    // instead of rolling forward; skip the re-verification; skip the
    // name-vs-contents check.
    let d = dirs();
    serve(&d, &bundle(1, 1, vec![recording_profile()])).unwrap();
    let b2 = bundle(1, 2, vec![recording_profile()]);
    orphan(&d, &b2, &archive_name(&hwm_of(&b2)));
    let done = store(&d).reconcile_with_archive().unwrap();
    assert_eq!(done.len(), 1, "{done:?}");
    assert_eq!(
        hwm(&d).unwrap(),
        hwm_of(&b2),
        "rolled forward to the archived entry"
    );
    store(&d).reconcile_with_archive().expect("consistent now");
    // First load interrupted: no HWM, one entry.
    let d = dirs();
    let b = bundle(1, 3, vec![recording_profile()]);
    orphan(&d, &b, &archive_name(&hwm_of(&b)));
    store(&d).reconcile_with_archive().unwrap();
    assert_eq!(hwm(&d).unwrap(), hwm_of(&b));
    // An entry named for one bundle holding another refuses to start.
    let d = dirs();
    serve(&d, &bundle(1, 1, vec![recording_profile()])).unwrap();
    orphan(
        &d,
        &bundle(1, 3, vec![recording_profile()]),
        &archive_name(&hwm_of(&b2)),
    );
    let e = store(&d).reconcile_with_archive().unwrap_err();
    assert!(e.detail.contains("is not named for its contents"), "{e}");
    assert_eq!(hwm(&d).unwrap().epoch, 1, "nothing rolled forward");
    // An entry whose set no current signer signed refuses to start.
    let d = dirs();
    serve(&d, &bundle(1, 1, vec![recording_profile()])).unwrap();
    let mut bad = b2.clone();
    bad.set.signatures = vec![sign_doc(&bad.set.set, &signer_key(5))];
    orphan(&d, &bad, &archive_name(&hwm_of(&b2)));
    let e = store(&d).reconcile_with_archive().unwrap_err();
    assert!(e.detail.contains("does not verify"), "{e}");
    // An entry whose MANIFEST the root did not sign refuses to start.
    let d = dirs();
    serve(&d, &bundle(1, 1, vec![recording_profile()])).unwrap();
    let mut bad = b2.clone();
    bad.manifest.signature = sign_doc(&bad.manifest.manifest, &signer_key(5));
    orphan(&d, &bad, &archive_name(&hwm_of(&b2)));
    let e = store(&d).reconcile_with_archive().unwrap_err();
    assert!(e.detail.contains("does not verify"), "{e}");
}

#[test]
fn several_entries_above_the_hwm_walk_forward_step_by_step_and_a_stray_file_refuses() {
    // A lost or stale HWM (a backup restore) with several archived entries
    // above it: ACCEPTING it would re-admit the epochs in between and refusing
    // to start leaves no safe recovery, so `serve` walks the HWM forward entry
    // by entry under `decide`. Mutations: refuse instead; jump straight to the
    // newest entry (the +16 bound refuses 1 → 20, the walk 1 → 10 → 20 does
    // not); skip the per-step bound.
    let d = dirs();
    serve(&d, &bundle(1, 1, vec![recording_profile()])).unwrap();
    let old = std::fs::read(hwm_file(&d)).unwrap();
    for e in [10, 20] {
        let b = bundle(1, e, vec![recording_profile()]);
        orphan(&d, &b, &archive_name(&hwm_of(&b)));
    }
    let done = store(&d).reconcile_with_archive().unwrap();
    assert_eq!(done.len(), 2, "{done:?}");
    assert!(done[1].contains("2 entries above the HWM"), "{done:?}");
    assert_eq!(hwm(&d).unwrap().epoch, 20, "landed on the newest state");
    // The same walk from a restored (stale) HWM after a real chain.
    std::fs::write(hwm_file(&d), &old).unwrap();
    store(&d).reconcile_with_archive().unwrap();
    assert_eq!(hwm(&d).unwrap().epoch, 20);
    // A single step over +16 still refuses (and keeps the HWM).
    let d = dirs();
    serve(&d, &bundle(1, 1, vec![recording_profile()])).unwrap();
    let b = bundle(1, 30, vec![recording_profile()]);
    orphan(&d, &b, &archive_name(&hwm_of(&b)));
    let e = store(&d).reconcile_with_archive().unwrap_err();
    assert!(e.detail.contains("jumps more than 16"), "{e}");
    assert_eq!(hwm(&d).unwrap().epoch, 1);
    // A stray .json beside the entries refuses, naming it.
    let d = dirs();
    serve(&d, &bundle(1, 1, vec![recording_profile()])).unwrap();
    let adir = d.archive.join(root().trim_start_matches("0x"));
    std::fs::write(adir.join("notes.json"), b"{}").unwrap();
    let e = store(&d).reconcile_with_archive().unwrap_err();
    assert!(e.detail.contains("unexpected file notes.json"), "{e}");
    // Only serve reconciles: a read-only (tool) store refuses, even on a
    // consistent store.
    let clean = dirs();
    serve(&clean, &bundle(1, 1, vec![recording_profile()])).unwrap();
    let ro = fabstir_llm_node::kbs::authority::AuthorityStore::new(
        &clean.auth,
        &clean.hwm,
        &clean.archive,
        true,
    );
    let e = ro.reconcile_with_archive().unwrap_err();
    assert!(e.detail.contains("read-only store cannot reconcile"), "{e}");
}
