// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P5.5 (design §7) — the plaintext's home: the pure LUKS rule and
//! its one real-path arm, the start-up sweep, disk-mode unlink-only purges,
//! and the directory-disjointness refusals of `require_decrypt_dir`.

use super::streaming_fixture::*;
use fabstir_llm_node::tee::model_source::EncryptedModelLoader;
use fabstir_llm_node::tee::plaintext_home::{home_rule, is_own_plaintext_name};
use fabstir_llm_node::tee::types::TeeError;
use std::path::Path;
use std::sync::{Arc, Mutex};

const M: [u8; 32] = [0x11u8; 32];
const H1: [u8; 32] = [0x21u8; 32];
const DEK1: [u8; 32] = [0x31u8; 32];

fn refused(r: Result<(), TeeError>, needle: &str) {
    match r {
        Err(TeeError::VerificationFailed(m)) => {
            assert!(m.contains(needle), "`{needle}` not in: {m}")
        }
        other => panic!("expected a refusal containing `{needle}`, got {other:?}"),
    }
}

#[test]
fn disk_home_rule_accepts_only_a_luks_backed_directory() {
    let dir = Path::new("/var/lib/fabstir/plaintext");
    let luks = "CRYPT-LUKS2-0123-data".to_string();
    let lvm = "LVM-abcdef".to_string();
    let verity = "CRYPT-VERITY-9999-root".to_string();
    // Disk mode.
    home_rule(true, dir, "ext4", Some(&luks), &[]).expect("LUKS directly");
    home_rule(true, dir, "ext4", Some(&lvm), std::slice::from_ref(&luks)).expect("LVM on LUKS");
    refused(
        home_rule(true, dir, "ext4", Some(&lvm), &[]),
        "not on a LUKS device",
    );
    refused(
        home_rule(true, dir, "ext4", Some(&lvm), std::slice::from_ref(&lvm)),
        "not on a LUKS device",
    );
    refused(
        home_rule(true, dir, "ext4", Some(&verity), &[]),
        "not on a LUKS device",
    );
    refused(
        home_rule(true, dir, "ext4", None, &[]),
        "plain block device",
    );
    refused(home_rule(true, dir, "tmpfs", None, &[]), "is RAM");
    refused(
        home_rule(true, dir, "overlay", None, &[]),
        "put it on a volume",
    );
    // tmpfs mode: today's rule.
    home_rule(false, dir, "tmpfs", None, &[]).expect("tmpfs");
    refused(home_rule(false, dir, "ext4", Some(&luks), &[]), "not tmpfs");

    // The real path: disk mode with the volume under /dev/shm (RAM on every
    // host) and the sibling container dir → reaches the home rule → "is RAM".
    let shm = Path::new("/dev/shm");
    if !fabstir_llm_node::tee::model_source::is_tmpfs(shm) {
        eprintln!("skipping the real-path arm: /dev/shm is not tmpfs here");
        return;
    }
    let vol = shm.join(format!("tee-home-{}", std::process::id()));
    std::fs::create_dir_all(&vol).unwrap();
    let l = EncryptedModelLoader::new(&vol)
        .with_tee_enabled(true)
        .with_plaintext_volume(&vol)
        .with_disk_mode(true);
    let r = l.require_decrypt_dir();
    let _ = std::fs::remove_dir_all(&vol);
    let mut sibling = vol.as_os_str().to_owned();
    sibling.push(".containers");
    let _ = std::fs::remove_dir_all(sibling);
    refused(r, "is RAM");
}

#[test]
fn own_plaintext_names_are_exactly_fresh_path_and_the_probe() {
    assert!(is_own_plaintext_name(&format!(
        "{}.{}.gguf",
        "a".repeat(64),
        "b".repeat(16)
    )));
    assert!(is_own_plaintext_name(".tee-write-probe.deadbeef"));
    assert!(!is_own_plaintext_name("model.gguf"));
    assert!(!is_own_plaintext_name(&format!(
        "{}.{}.gguf",
        "a".repeat(63),
        "b".repeat(16)
    )));
    assert!(!is_own_plaintext_name(&format!(
        "{}.{}.bin",
        "a".repeat(64),
        "b".repeat(16)
    )));
    assert!(!is_own_plaintext_name("notes.txt"));
}

#[tokio::test]
async fn disk_mode_purge_is_unlink_only_and_tmpfs_mode_zeroes() {
    for disk in [true, false] {
        let d = dirs();
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let pt = plaintext(5000, 4);
        let blobs = SwappableBlobs::new(Arc::clone(&log), REF, seal(&pt, &DEK1, M, H1));
        let kbs = LoggingBroker::new(Arc::clone(&log), M, DEK1);
        let mut l = loader(&d);
        if disk {
            // Builder order must not matter: the home is derived.
            l = l.with_disk_mode(true).with_plaintext_volume(&d.vol);
            assert_eq!(
                l.decrypt_dir(),
                d.vol.as_path(),
                "disk mode: the home IS the volume"
            );
            assert!(l.disk_mode());
        } else {
            assert!(
                !loader(&d).with_disk_mode(true).disk_mode(),
                "the flag without a volume is not disk mode"
            );
        }
        let p = l
            .prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H1))
            .await
            .unwrap();
        let link = d._tmp.path().join(format!("link-{disk}"));
        std::fs::hard_link(&p, &link).unwrap();
        l.release(&M, &H1);
        l.evict_unreferenced();
        assert!(!p.exists(), "the path is gone in both modes");
        let content = std::fs::read(&link).unwrap();
        assert_eq!(content.len(), pt.len(), "length unchanged");
        if disk {
            assert_eq!(
                content, pt,
                "disk mode: unlink only, the link's content intact"
            );
        } else {
            assert!(
                content.iter().all(|b| *b == 0),
                "tmpfs mode: zeroed then unlinked"
            );
        }
    }
}

#[test]
fn the_startup_sweep_removes_only_the_nodes_own_files() {
    let d = dirs();
    let l = loader(&d).with_plaintext_volume(&d.vol);
    let own = |n: u8| format!("{}.{}.gguf", hex::encode([n; 32]), hex::encode([n; 8]));
    let notes = d.decrypt.join("notes.txt");
    std::fs::write(&notes, b"keep me").unwrap();
    // Every own file is hard-linked first: the sweep unlinks, never overwrites.
    let v1 = d.vol.join(own(1));
    std::fs::write(&v1, vec![0xAAu8; 3000]).unwrap();
    let v1_link = d._tmp.path().join("v1.link");
    std::fs::hard_link(&v1, &v1_link).unwrap();
    // The decrypt dir: two own files (hard-linked), the probe, a symlink with
    // an own name pointing at notes.txt, notes.txt, a subdirectory.
    let g1 = d.decrypt.join(own(2));
    let g2 = d.decrypt.join(own(3));
    std::fs::write(&g1, vec![0xBBu8; 2000]).unwrap();
    std::fs::write(&g2, vec![0xCCu8; 1000]).unwrap();
    let g1_link = d._tmp.path().join("g1.link");
    let g2_link = d._tmp.path().join("g2.link");
    std::fs::hard_link(&g1, &g1_link).unwrap();
    std::fs::hard_link(&g2, &g2_link).unwrap();
    let probe = d.decrypt.join(".tee-write-probe.x");
    std::fs::write(&probe, vec![0xDDu8; 100]).unwrap();
    std::os::unix::fs::symlink(&notes, d.decrypt.join(own(4))).unwrap();
    std::fs::create_dir(d.decrypt.join("sub")).unwrap();
    // The container dir: a cache file (kept), two parts (removed).
    std::fs::write(d.containers.join("k.enc"), vec![1u8; 10]).unwrap();
    std::fs::write(d.containers.join("k.enc.deadbeef.part"), vec![2u8; 500]).unwrap();
    std::fs::write(d.containers.join("other.enc.1234abcd.part"), vec![3u8; 700]).unwrap();

    let (files, bytes) = l.sweep_leftovers().expect("sweep");
    assert_eq!(files, 7, "1 on the volume + 4 in the decrypt dir + 2 parts");
    assert_eq!(
        bytes,
        3000 + 2000 + 1000 + 100 + 500 + 700,
        "symlink counts 0 bytes"
    );
    assert!(!v1.exists() && !g1.exists() && !g2.exists() && !probe.exists());
    assert!(!d.decrypt.join(own(4)).exists(), "the symlink is unlinked");
    assert_eq!(std::fs::read(&notes).unwrap(), b"keep me", "never followed");
    assert!(d.decrypt.join("sub").is_dir(), "a directory is left alone");
    assert_eq!(
        names(&d.containers),
        vec!["k.enc".to_string()],
        "the cache stays, parts go"
    );
    assert_eq!(
        std::fs::read(&v1_link).unwrap(),
        vec![0xAAu8; 3000],
        "volume: unlink only"
    );
    // The sweep is unlink-only EVERYWHERE (code-review round 2): a file matched
    // by name may be another live process's mapped weights, and an overwrite
    // under a mapping is what P3 forbids; the links still see the real bytes.
    assert_eq!(std::fs::read(&g1_link).unwrap(), vec![0xBBu8; 2000]);
    assert_eq!(std::fs::read(&g2_link).unwrap(), vec![0xCCu8; 1000]);
}

#[test]
fn container_dir_inside_or_around_the_decrypt_dir_is_refused() {
    let d = dirs();
    let both = |r: Result<(), TeeError>| {
        let m = match r {
            Err(TeeError::VerificationFailed(m)) => m,
            other => panic!("expected a disjointness refusal, got {other:?}"),
        };
        assert!(
            m.contains("container dir") && m.contains("decrypt dir"),
            "{m}"
        );
    };
    both(
        EncryptedModelLoader::new(&d.decrypt)
            .with_container_dir(d.decrypt.join("c"))
            .require_decrypt_dir(),
    );
    both(
        EncryptedModelLoader::new(&d.decrypt)
            .with_container_dir(d.decrypt.parent().unwrap())
            .require_decrypt_dir(),
    );
    both(
        EncryptedModelLoader::new(&d.decrypt)
            .with_container_dir(&d.decrypt)
            .require_decrypt_dir(),
    );
    match EncryptedModelLoader::new(&d.decrypt)
        .with_container_dir(&d.containers)
        .with_plaintext_volume(d.containers.join("v"))
        .require_decrypt_dir()
    {
        Err(TeeError::VerificationFailed(m)) => assert!(m.contains("plaintext volume"), "{m}"),
        other => panic!("{other:?}"),
    }
    match EncryptedModelLoader::new(&d.decrypt)
        .with_container_dir(&d.containers)
        .with_disk_mode(true)
        .require_decrypt_dir()
    {
        Err(TeeError::VerificationFailed(m)) => assert!(m.contains("TEE_PLAINTEXT_VOLUME"), "{m}"),
        other => panic!("{other:?}"),
    }
    // The sibling default passes step (1): whatever comes back is the home
    // rule's answer (tmpfs or not), never the disjointness message.
    let r = EncryptedModelLoader::new(&d.decrypt).require_decrypt_dir();
    if let Err(TeeError::VerificationFailed(m)) = &r {
        assert!(!m.contains("container dir"), "{m}");
    }
    // A trailing slash still yields the SIBLING, never `<home>/.containers`.
    let slashed = format!("{}/", d.decrypt.display());
    assert_eq!(
        EncryptedModelLoader::new(&slashed).container_dir(),
        d._tmp.path().join("decrypt.containers").as_path()
    );
}
