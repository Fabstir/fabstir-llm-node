// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P3 — `AttestedLoad` lifecycle: what happens to the tmpfs plaintext
//! on every way out of the process. Fixture from `test_orchestration.rs`.

use super::test_orchestration::{fixture, fixture_with_plaintext_len, good_provider};
use fabstir_llm_node::tee::live::AttestedLoad;
use fabstir_llm_node::tee::orchestration::prepare_attested_model;
use std::sync::Arc;

#[tokio::test]
async fn attested_load_drop_purges_the_plaintext() {
    // P3 converge round 5: a `?` return from main after the decrypt (P2P/API
    // start failure) must not leave the weights on tmpfs; Drop does the wipe.
    let f = fixture();
    let prepared = prepare_attested_model(
        &f.loader,
        &f.source,
        &f.providers,
        &f.s5,
        &f.kbs,
        &good_provider(),
        f.model_id,
        None,
    )
    .await
    .unwrap();
    let path = prepared.path.clone();
    let load = AttestedLoad {
        path: path.clone(),
        prepared,
        loader: Arc::new(f.loader),
        test_release: false,
    };
    assert!(path.exists(), "decrypted plaintext is on disk while held");
    // Round 8: Drop must UNLINK, never overwrite — a generation may still hold
    // the mapping (an open handle stands in for it here).
    let mut open = std::fs::File::open(&path).unwrap();
    drop(load);
    assert!(
        !path.exists(),
        "dropping the AttestedLoad must remove the plaintext's path"
    );
    let mut still = Vec::new();
    std::io::Read::read_to_end(&mut open, &mut still).unwrap();
    assert_eq!(
        still, f.plaintext,
        "a mapping still open after Drop sees the real bytes, not zeros"
    );
}

#[tokio::test]
async fn attested_load_detach_unlinks_without_overwriting() {
    // P3 converge round 7: the serving-node shutdown path removes the PATH but
    // leaves the bytes untouched for any mapping still open; Drop afterwards is
    // a no-op, not an error.
    let f = fixture();
    let prepared = prepare_attested_model(
        &f.loader,
        &f.source,
        &f.providers,
        &f.s5,
        &f.kbs,
        &good_provider(),
        f.model_id,
        None,
    )
    .await
    .unwrap();
    let path = prepared.path.clone();
    let load = AttestedLoad {
        path: path.clone(),
        prepared,
        loader: Arc::new(f.loader),
        test_release: false,
    };
    // An open handle stands in for llama.cpp's mmap: the bytes must stay real.
    let mut open = std::fs::File::open(&path).unwrap();
    load.detach_for_exit().expect("unlink");
    assert!(!path.exists(), "the path is gone");
    let mut still = Vec::new();
    std::io::Read::read_to_end(&mut open, &mut still).unwrap();
    assert_eq!(
        still, f.plaintext,
        "an open mapping still sees the real weights"
    );
    load.detach_for_exit().expect("idempotent");
    drop(load); // release() on a missing file is Ok
}

#[test]
fn live_path_requires_a_tmpfs_decrypt_dir() {
    // Round 12: the serving-node shutdown unlinks without overwriting, which is
    // only safe where the pages die with the mount. A persistent dir is refused
    // at boot; /dev/shm (tmpfs in this container) passes.
    use fabstir_llm_node::tee::model_source::{is_tmpfs, EncryptedModelLoader};
    use fabstir_llm_node::tee::types::TeeError;
    // The negative half needs a disk-backed dir; the OS tempdir may itself be
    // tmpfs on some hosts, so use cargo's target tmpdir and skip if even that
    // is tmpfs (the positive half already skips symmetrically).
    let disk = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    if is_tmpfs(disk.path()) {
        eprintln!(
            "skipping the persistent-dir half: {} is tmpfs",
            disk.path().display()
        );
        return;
    }
    let loader = EncryptedModelLoader::new(disk.path()).with_tee_enabled(true);
    loader
        .verify_decrypt_dir()
        .expect("the plain check only warns");
    match loader.require_tmpfs_decrypt_dir() {
        Err(TeeError::VerificationFailed(m)) => assert!(m.contains("tmpfs"), "{m}"),
        other => panic!("a non-tmpfs decrypt dir must be refused on the live path: {other:?}"),
    }
    let shm = std::path::Path::new("/dev/shm");
    if is_tmpfs(shm) {
        let dir = shm.join(format!("tee-req-tmpfs-{}", std::process::id()));
        let loader = EncryptedModelLoader::new(&dir).with_tee_enabled(true);
        loader
            .require_tmpfs_decrypt_dir()
            .expect("/dev/shm is tmpfs");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn filesystem_type_takes_the_effective_mount_on_an_over_mount() {
    // Round 17: the tmpfs answer is now a security gate. Two /proc/mounts
    // entries for one mount point are effective in list order (the later one
    // is what a path sees), and a deeper mount point still beats a shallower.
    use fabstir_llm_node::tee::model_source::filesystem_type_from_mounts;
    use std::path::Path;
    let canon = Path::new("/dev/shm/tee/model.gguf");
    let tmpfs_then_disk = "overlay / overlay rw 0 0\n\
shm /dev/shm tmpfs rw,nosuid 0 0\n\
/dev/sda1 /dev/shm ext4 rw 0 0\n";
    assert_eq!(
        filesystem_type_from_mounts(canon, tmpfs_then_disk).as_deref(),
        Some("ext4"),
        "the over-mount (later entry) is the effective filesystem"
    );
    let disk_then_tmpfs = "overlay / overlay rw 0 0\n\
/dev/sda1 /dev/shm ext4 rw 0 0\n\
shm /dev/shm tmpfs rw,nosuid 0 0\n";
    assert_eq!(
        filesystem_type_from_mounts(canon, disk_then_tmpfs).as_deref(),
        Some("tmpfs")
    );
    // Depth still wins over order: a deeper persistent mount under /dev/shm.
    let deeper = "shm /dev/shm tmpfs rw 0 0\n\
/dev/sdb1 /dev/shm/tee ext4 rw 0 0\n\
overlay / overlay rw 0 0\n";
    assert_eq!(
        filesystem_type_from_mounts(canon, deeper).as_deref(),
        Some("ext4")
    );
    assert_eq!(filesystem_type_from_mounts(canon, ""), None);
}

#[tokio::test]
async fn cancelling_the_load_during_the_hash_step_purges_the_plaintext() {
    // P3 converge round 23: a stop signal racing the boot drops the load future.
    // The only awaits AFTER the decrypt are the hash step's file reads, and at
    // that point no `AttestedLoad` exists yet, so the orchestration's own guard
    // must purge the plaintext on cancellation. A 4 MiB plaintext gives the
    // hash step dozens of awaits to be aborted at.
    let f = fixture_with_plaintext_len(4 * 1024 * 1024);
    let dir = f._dir.path().to_path_buf();
    let expected = {
        use sha2::Digest;
        format!("{:x}", sha2::Sha256::digest(&f.plaintext))
    };
    let f = Arc::new(f);
    let task = {
        let f = Arc::clone(&f);
        tokio::spawn(async move {
            prepare_attested_model(
                &f.loader,
                &f.source,
                &f.providers,
                &f.s5,
                &f.kbs,
                &good_provider(),
                f.model_id,
                Some(&expected),
            )
            .await
            .map(|p| p.path)
        })
    };
    // Wait for the decrypted file to appear (the decrypt itself is synchronous;
    // the first await after it is the hash step), then cancel.
    let plaintext_file = loop {
        let files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.path())
            .collect();
        if let Some(p) = files.into_iter().next() {
            break p;
        }
        if task.is_finished() {
            panic!("the load finished before the test could cancel it; enlarge the plaintext");
        }
        tokio::task::yield_now().await;
    };
    task.abort();
    assert!(
        task.await.unwrap_err().is_cancelled(),
        "the load was cancelled"
    );
    assert!(
        !plaintext_file.exists(),
        "a load cancelled after the decrypt must purge its plaintext ({})",
        plaintext_file.display()
    );
    assert_eq!(
        std::fs::read_dir(&dir).unwrap().count(),
        0,
        "nothing left in the decrypt dir"
    );
}

#[tokio::test]
async fn dropping_the_boxed_load_future_after_a_stop_purges_the_plaintext() {
    // P3 converge round 24 (kept after round 46 retired the race in main): a
    // caller that cancels the load by dropping its future gets the plaintext
    // purged by the orchestration guard. That only holds if what is dropped IS
    // the future: `Box::pin` here, never `pin!` (whose `drop` drops a
    // reference). "The decrypted file appeared" stands in for the stop.
    let f = Arc::new(fixture_with_plaintext_len(4 * 1024 * 1024));
    let dir = f._dir.path().to_path_buf();
    let expected = {
        use sha2::Digest;
        format!("{:x}", sha2::Sha256::digest(&f.plaintext))
    };
    let load_f = Arc::clone(&f);
    let mut load = Box::pin(async move {
        prepare_attested_model(
            &load_f.loader,
            &load_f.source,
            &load_f.providers,
            &load_f.s5,
            &load_f.kbs,
            &good_provider(),
            load_f.model_id,
            Some(&expected),
        )
        .await
        .map(|p| p.path)
    });
    let stop = async {
        loop {
            let first = std::fs::read_dir(&dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .find(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .map(|e| e.path());
            if let Some(p) = first {
                return p;
            }
            tokio::task::yield_now().await;
        }
    };
    let outcome = tokio::select! {
        r = &mut load => Err(r),
        p = stop => Ok(p),
    };
    let plaintext_file = match outcome {
        Ok(p) => p,
        Err(r) => panic!("the load finished before the stop fired: {r:?}"),
    };
    assert!(
        plaintext_file.exists(),
        "still there while the future lives"
    );
    drop(load);
    assert!(
        !plaintext_file.exists(),
        "dropping the boxed load future must purge the plaintext"
    );
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
}

#[tokio::test]
async fn unlink_live_plaintexts_reaches_a_file_no_one_holds_yet() {
    // Round 36: the watchdog's emergency exit must find a plaintext that exists
    // while no `AttestedLoad` names it (decrypt in progress, hash step). The
    // loader registers each file before decrypting; here it is held by the
    // returned PreparedModel only, which stands in for "not yet handed over".
    let f = fixture();
    let prepared = prepare_attested_model(
        &f.loader,
        &f.source,
        &f.providers,
        &f.s5,
        &f.kbs,
        &good_provider(),
        f.model_id,
        None,
    )
    .await
    .unwrap();
    assert!(prepared.path.exists());
    assert_eq!(f.loader.unlink_live_plaintexts(), 1, "one live plaintext");
    assert!(!prepared.path.exists(), "unlinked");
    assert_eq!(f.loader.unlink_live_plaintexts(), 0, "idempotent");
    // After a normal hand-over and Drop the set is empty too.
    let f2 = fixture();
    let prepared = prepare_attested_model(
        &f2.loader,
        &f2.source,
        &f2.providers,
        &f2.s5,
        &f2.kbs,
        &good_provider(),
        f2.model_id,
        None,
    )
    .await
    .unwrap();
    let path = prepared.path.clone();
    let loader = Arc::new(f2.loader);
    drop(AttestedLoad {
        path,
        prepared,
        loader: Arc::clone(&loader),
        test_release: false,
    });
    assert_eq!(
        loader.unlink_live_plaintexts(),
        0,
        "Drop already removed it"
    );
}

#[tokio::test]
async fn no_plaintext_can_be_created_after_the_emergency_unlink() {
    // Round 50: the unlink pass and a decrypt's `create_new` are serialised
    // under one lock with a `stopping` flag, so a load racing the stop cannot
    // leave a file behind: after the pass, the loader refuses to create.
    let f = fixture();
    assert_eq!(f.loader.unlink_live_plaintexts(), 0);
    let err = prepare_attested_model(
        &f.loader,
        &f.source,
        &f.providers,
        &f.s5,
        &f.kbs,
        &good_provider(),
        f.model_id,
        None,
    )
    .await
    .expect_err("a loader that is stopping must not create a plaintext");
    assert!(err.to_string().contains("stop in progress"), "{err}");
    assert_eq!(
        std::fs::read_dir(f._dir.path()).unwrap().count(),
        0,
        "nothing on disk"
    );
}
