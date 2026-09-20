// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P3 — `AttestedLoad` lifecycle: what happens to the tmpfs plaintext
//! on every way out of the process. Fixture from `test_orchestration.rs`.

use super::test_orchestration::{fixture, good_provider};
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
    // The live gate is `require_decrypt_dir` (P5.5): in tmpfs mode it is
    // today's rule plus the container-dir checks and the sweep.
    let loader = EncryptedModelLoader::new(disk.path().join("decrypt")).with_tee_enabled(true);
    loader
        .verify_decrypt_dir()
        .expect("the plain check only warns");
    match loader.require_decrypt_dir() {
        Err(TeeError::VerificationFailed(m)) => assert!(m.contains("tmpfs"), "{m}"),
        other => panic!("a non-tmpfs decrypt dir must be refused on the live path: {other:?}"),
    }
    let shm = std::path::Path::new("/dev/shm");
    if is_tmpfs(shm) {
        let dir = shm.join(format!("tee-req-tmpfs-{}", std::process::id()));
        let loader = EncryptedModelLoader::new(&dir).with_tee_enabled(true);
        let r = loader.require_decrypt_dir();
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(format!("{}.containers", dir.display()));
        r.expect("/dev/shm is tmpfs");
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

/// A `KeyBrokerClient` that parks `request_key` on a gate and says when the
/// load has reached it: the one-poll property below needs the future stopped
/// at a known await, then exactly one more poll.
struct GatedBroker<'a> {
    inner: &'a fabstir_llm_node::tee::mock::MockKeyBroker,
    gate: Arc<tokio::sync::Notify>,
    reached: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait::async_trait]
impl fabstir_llm_node::tee::key_broker::KeyBrokerClient for GatedBroker<'_> {
    async fn challenge(
        &self,
        model_id: [u8; 32],
        pk_att: &[u8],
    ) -> fabstir_llm_node::tee::types::TeeResult<[u8; 32]> {
        self.inner.challenge(model_id, pk_att).await
    }
    async fn request_key(
        &self,
        model_id: [u8; 32],
        ev: &fabstir_llm_node::tee::types::Evidence,
    ) -> fabstir_llm_node::tee::types::TeeResult<fabstir_llm_node::tee::types::WrappedKey> {
        self.reached
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.gate.notified().await;
        self.inner.request_key(model_id, ev).await
    }
}

#[tokio::test]
async fn the_load_publishes_or_purges_inside_one_poll() {
    // P5.5 (design S2): the digest comes out of the decrypt's tee, so there is
    // NO await after the plaintext exists: the future publishes or purges it
    // inside one poll. (This replaces the two P3 cancellation tests, which
    // aborted at the hash step's file reads; that await stretch is gone.) An
    // await between `create_new` and `cache_publish` would leave a file on
    // disk with the future `Pending`, which neither arm below accepts.
    use std::future::Future;
    use std::task::{Context, Poll};
    let f = Arc::new(fixture());
    let dir = f._dir.path().join("decrypt");
    let expected = {
        use sha2::Digest;
        format!("{:x}", sha2::Sha256::digest(&f.plaintext))
    };
    let gate = Arc::new(tokio::sync::Notify::new());
    let reached = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let broker = GatedBroker {
        inner: &f.kbs,
        gate: Arc::clone(&gate),
        reached: Arc::clone(&reached),
    };
    let provider = good_provider();
    let mut load = Box::pin(prepare_attested_model(
        &f.loader,
        &f.source,
        &f.providers,
        &f.s5,
        &broker,
        &provider,
        f.model_id,
        Some(&expected),
    ));
    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    let files = |dir: &std::path::Path| -> usize {
        std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                    .count()
            })
            .unwrap_or(0)
    };
    // Drive the future to the gate (every earlier step is immediately ready).
    let mut polls = 0;
    while !reached.load(std::sync::atomic::Ordering::SeqCst) {
        polls += 1;
        assert!(polls < 1000, "the load never reached request_key");
        match load.as_mut().poll(&mut cx) {
            Poll::Ready(r) => panic!("finished before the gate: {r:?}"),
            Poll::Pending => tokio::task::yield_now().await,
        }
    }
    assert_eq!(files(&dir), 0, "no plaintext before the DEK");
    gate.notify_one();
    match load.as_mut().poll(&mut cx) {
        Poll::Ready(Ok(p)) => {
            assert!(p.path.exists(), "published inside the poll that decrypted");
            f.loader.release(&p.model_id, &p.policy_hash);
            f.loader.evict_unreferenced();
            assert_eq!(files(&dir), 0, "released + evicted: purged");
        }
        Poll::Ready(Err(e)) => panic!("the load failed: {e}"),
        Poll::Pending => {
            assert_eq!(
                files(&dir),
                0,
                "a Pending future must not have a plaintext on disk (an await after create_new)"
            );
            drop(load);
            assert_eq!(files(&dir), 0);
        }
    }
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
        std::fs::read_dir(f._dir.path().join("decrypt"))
            .unwrap()
            .count(),
        0,
        "nothing on disk"
    );
}
