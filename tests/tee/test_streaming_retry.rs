// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P5.5 (design §7, `test_streaming_load.rs`, second file: the
//! integration-test 400-line cap) — the once-retry and what evicts what, the
//! prune, the truncated container, and the digest from the decrypt's tee.

use super::streaming_fixture::*;
use fabstir_llm_node::tee::container::HEADER_LEN;
use fabstir_llm_node::tee::container_cache::ContainerOutcome;
use fabstir_llm_node::tee::orchestration::prepare_attested_model;
use fabstir_llm_node::tee::types::TeeError;
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const M: [u8; 32] = [0x11u8; 32];
const H1: [u8; 32] = [0x21u8; 32];
const DEK1: [u8; 32] = [0x31u8; 32];
const DEK2: [u8; 32] = [0x32u8; 32];
const WRAP: Duration = Duration::from_secs(20);

fn fresh_log() -> Log {
    Arc::new(Mutex::new(Vec::new()))
}

#[tokio::test]
async fn a_corrupt_cache_that_passes_its_header_is_replaced_once() {
    tokio::time::timeout(WRAP, async {
        // A PLANTED under DEK1 for (M, H); the broker releases DEK2 and the
        // blob serves B under DEK2 with the same header: A fails at chunk 0.
        let d = dirs();
        let log = fresh_log();
        plant(&d, &seal(&plaintext(5000, 1), &DEK1, M, H1));
        let pt_b = plaintext(5000, 2);
        let b = seal(&pt_b, &DEK2, M, H1);
        let blobs = SwappableBlobs::new(Arc::clone(&log), REF, b.clone());
        let kbs = LoggingBroker::new(Arc::clone(&log), M, DEK2);
        let l = loader(&d);
        let p = l
            .prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H1))
            .await
            .expect("the once-retry replaces the corrupt cache");
        assert_eq!(log_of(&log), vec!["challenge", "get_file_to"]);
        assert_eq!(blobs.get_file_to_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            kbs.challenges.load(Ordering::SeqCst),
            1,
            "no second challenge"
        );
        assert_eq!(l.last_container_outcome(), Some(ContainerOutcome::Retried));
        assert_eq!(std::fs::read(cache_file(&d)).unwrap(), b, "cache = B");
        assert_eq!(std::fs::read(&p).unwrap(), pt_b);
    })
    .await
    .expect("hung");
}

#[tokio::test]
async fn an_io_error_never_evicts_the_cache_on_either_attempt() {
    tokio::time::timeout(WRAP, async {
        // (a) a valid cache, a read-only decrypt dir → Io, cache kept, no download.
        let d = dirs();
        let log = fresh_log();
        let a = seal(&plaintext(5000, 1), &DEK1, M, H1);
        plant(&d, &a);
        let blobs = SwappableBlobs::new(Arc::clone(&log), REF, a.clone());
        let kbs = LoggingBroker::new(Arc::clone(&log), M, DEK1);
        let l = loader(&d);
        std::fs::set_permissions(&d.decrypt, std::fs::Permissions::from_mode(0o500)).unwrap();
        let err = l
            .prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H1))
            .await
            .expect_err("create_new in a read-only dir");
        std::fs::set_permissions(&d.decrypt, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(err, TeeError::Io(_)), "{err:?}");
        assert_eq!(std::fs::read(cache_file(&d)).unwrap(), a, "cache kept");
        assert_eq!(blobs.get_file_to_calls.load(Ordering::SeqCst), 0);

        // (b) A corrupt-but-decodable cached; the re-download's side effect
        // makes the decrypt dir read-only → the second attempt's Io is
        // returned as itself, never "twice", and the fresh B stays cached.
        let d = dirs();
        let log = fresh_log();
        let mut corrupt = a.clone();
        let last = corrupt.len() - 1;
        corrupt[last] ^= 0xFF;
        plant(&d, &corrupt);
        let b = seal(&plaintext(5000, 2), &DEK1, M, H1);
        let blobs = SwappableBlobs::new(Arc::clone(&log), REF, b.clone());
        let ro = d.decrypt.clone();
        *blobs.on_fetch.lock().unwrap() = Some(Box::new(move || {
            std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o500)).unwrap();
        }));
        let kbs = LoggingBroker::new(Arc::clone(&log), M, DEK1);
        let l = loader(&d);
        let err = l
            .prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H1))
            .await
            .expect_err("the second attempt hits a read-only dir");
        std::fs::set_permissions(&d.decrypt, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(err, TeeError::Io(_)), "{err:?}");
        assert!(!err.to_string().contains("twice"), "{err}");
        assert_eq!(
            std::fs::read(cache_file(&d)).unwrap(),
            b,
            "the fresh B is kept"
        );
        assert_eq!(log_of(&log), vec!["challenge", "get_file_to"]);
    })
    .await
    .expect("hung");
}

#[tokio::test]
async fn a_bad_fresh_container_refuses_once_and_a_bad_fresh_retry_names_both() {
    tokio::time::timeout(WRAP, async {
        // No cache; garbage → one Crypto, no "twice", cache removed.
        let d = dirs();
        let log = fresh_log();
        let blobs = SwappableBlobs::new(Arc::clone(&log), REF, vec![7u8; 4000]);
        let kbs = LoggingBroker::new(Arc::clone(&log), M, DEK1);
        let l = loader(&d);
        let err = l
            .prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H1))
            .await
            .expect_err("garbage is refused");
        assert!(matches!(err, TeeError::Crypto(_)), "{err:?}");
        assert!(!err.to_string().contains("twice"));
        assert_eq!(
            l.last_container_outcome(),
            Some(ContainerOutcome::Miss),
            "a failed load still reports how the container step went"
        );
        assert!(!cache_file(&d).exists(), "a fresh failure is deleted");
        assert!(regular_files(&d.decrypt).is_empty());
        assert_eq!(blobs.get_file_to_calls.load(Ordering::SeqCst), 1);

        // A corrupt-but-decodable cached + garbage served → "twice".
        let d = dirs();
        let log = fresh_log();
        let mut corrupt = seal(&plaintext(5000, 1), &DEK1, M, H1);
        let last = corrupt.len() - 1;
        corrupt[last] ^= 0xFF;
        plant(&d, &corrupt);
        let blobs = SwappableBlobs::new(Arc::clone(&log), REF, vec![7u8; 4000]);
        let kbs = LoggingBroker::new(Arc::clone(&log), M, DEK1);
        let l = loader(&d);
        let err = l
            .prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H1))
            .await
            .expect_err("refused twice");
        match &err {
            TeeError::Fetch(m) => assert!(m.contains("twice"), "{m}"),
            other => panic!("expected Fetch, got {other:?}"),
        }
        assert!(!cache_file(&d).exists());
        assert!(regular_files(&d.decrypt).is_empty());
    })
    .await
    .expect("hung");
}

#[tokio::test]
async fn the_prune_removes_other_keys_and_keeps_the_current_keys_files() {
    tokio::time::timeout(WRAP, async {
        let d = dirs();
        let log = fresh_log();
        let notes = d._tmp.path().join("notes.txt");
        std::fs::write(&notes, b"keep me").unwrap();
        let key = fabstir_llm_node::tee::container_cache::cache_key(REF);
        std::fs::write(d.containers.join("other.enc"), b"x").unwrap();
        std::fs::write(d.containers.join("other.enc.1234abcd.part"), b"x").unwrap();
        std::os::unix::fs::symlink(&notes, d.containers.join("x.enc")).unwrap();
        std::fs::create_dir(d.containers.join("d.enc")).unwrap();
        let same_key_part = d.containers.join(format!("{key}.enc.deadbeef.part"));
        std::fs::write(&same_key_part, b"x").unwrap();
        let blobs = SwappableBlobs::new(
            Arc::clone(&log),
            REF,
            seal(&plaintext(5000, 1), &DEK1, M, H1),
        );
        let kbs = LoggingBroker::new(Arc::clone(&log), M, DEK1);
        let l = loader(&d);
        l.prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H1))
            .await
            .unwrap();
        let mut expect = vec![
            "d.enc".to_string(),
            format!("{key}.enc"),
            format!("{key}.enc.deadbeef.part"),
        ];
        expect.sort();
        assert_eq!(names(&d.containers), expect);
        assert_eq!(
            std::fs::read(&notes).unwrap(),
            b"keep me",
            "symlink unlinked, never followed"
        );
    })
    .await
    .expect("hung");
}

#[tokio::test]
async fn a_truncated_container_purges_the_partial_plaintext_and_the_fresh_cache() {
    tokio::time::timeout(WRAP, async {
        let d = dirs();
        let log = fresh_log();
        let mut sealed = seal(&plaintext(5000, 1), &DEK1, M, H1);
        // Cut inside chunk 3 (chunks are CHUNK + 16 bytes after the header).
        sealed.truncate(HEADER_LEN + 2 * (CHUNK as usize + 16) + 100);
        let blobs = SwappableBlobs::new(Arc::clone(&log), REF, sealed);
        let kbs = LoggingBroker::new(Arc::clone(&log), M, DEK1);
        let l = loader(&d);
        let err = l
            .prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H1))
            .await
            .expect_err("a truncated container is refused");
        assert!(matches!(err, TeeError::Crypto(_)), "{err:?}");
        assert!(
            regular_files(&d.decrypt).is_empty(),
            "partial plaintext purged"
        );
        assert!(names(&d.containers).is_empty(), "fresh cache removed");
    })
    .await
    .expect("hung");
}

#[tokio::test]
async fn the_digest_from_the_tee_equals_the_plaintext_sha256() {
    tokio::time::timeout(WRAP, async {
        let d = dirs();
        let log = fresh_log();
        let pt = plaintext(7000, 9);
        let blobs = SwappableBlobs::new(Arc::clone(&log), REF, seal(&pt, &DEK1, M, H1));
        let kbs = LoggingBroker::new(Arc::clone(&log), M, DEK1);
        let l = loader(&d);
        let (p, digest, _) = l
            .prepare_encrypted_model_with_digest(&blobs, &kbs, &good_provider(), &spec(M, H1))
            .await
            .unwrap();
        let want: [u8; 32] = Sha256::digest(std::fs::read(&p).unwrap()).into();
        assert_eq!(digest, want);
        assert_eq!(digest, <[u8; 32]>::from(Sha256::digest(&pt)));
        let (_, again, _) = l
            .prepare_encrypted_model_with_digest(&blobs, &kbs, &good_provider(), &spec(M, H1))
            .await
            .unwrap();
        assert_eq!(again, digest, "an in-process cache hit carries the digest");

        // Through the orchestration, on a FRESH loader: a wrong expected hash
        // is ModelHashMismatch whose `got` is the tee's digest, plaintext purged.
        let f = super::test_orchestration::fixture();
        let wrong = format!("{:x}", Sha256::digest(b"another model"));
        let err = prepare_attested_model(
            &f.loader,
            &f.source,
            &f.providers,
            &f.s5,
            &f.kbs,
            &super::test_orchestration::good_provider(),
            f.model_id,
            Some(&wrong),
        )
        .await
        .expect_err("mismatch");
        match err {
            TeeError::ModelHashMismatch { expected, got } => {
                assert_eq!(expected, wrong);
                assert_eq!(got, format!("{:x}", Sha256::digest(&f.plaintext)));
            }
            other => panic!("{other:?}"),
        }
        assert!(
            regular_files(&f._dir.path().join("decrypt")).is_empty(),
            "purged"
        );
    })
    .await
    .expect("hung");
}

#[tokio::test]
async fn two_concurrent_loads_of_different_keys_both_complete() {
    tokio::time::timeout(WRAP, async {
        // The prune (S1d) keeps every IN-FLIGHT key's files, so overlapping
        // loads of two different refs in one loader never unlink each other's
        // `.part` or fresh `.enc` (code-review round 5).
        let d = dirs();
        let log = fresh_log();
        let pt_a = plaintext(5000, 1);
        let pt_b = plaintext(5000, 2);
        let m_b = [0x12u8; 32];
        let blobs = SwappableBlobs::new(Arc::clone(&log), REF, seal(&pt_a, &DEK1, M, H1));
        blobs.insert("models/other.enc", seal(&pt_b, &DEK2, m_b, H1));
        // Interleave at the point where load A's fresh `.enc` exists and load
        // B's prune runs: without the in-flight set, B would unlink A's file.
        blobs.yield_after_fetch.store(true, Ordering::SeqCst);
        let kbs_a = LoggingBroker::new(Arc::clone(&log), M, DEK1);
        let kbs_b = LoggingBroker::new(Arc::clone(&log), m_b, DEK2);
        let l = loader(&d);
        let spec_b = fabstir_llm_node::tee::model_source::EncryptedModelSpec {
            model_id: m_b,
            policy_hash: H1,
            encrypted_path: "models/other.enc".to_string(),
        };
        let (prov_a, prov_b) = (good_provider(), good_provider());
        let spec_a = spec(M, H1);
        let (a, b) = tokio::join!(
            l.prepare_encrypted_model(&blobs, &kbs_a, &prov_a, &spec_a),
            l.prepare_encrypted_model(&blobs, &kbs_b, &prov_b, &spec_b),
        );
        let (a, b) = (a.expect("load A"), b.expect("load B"));
        assert_eq!(std::fs::read(&a).unwrap(), pt_a);
        assert_eq!(std::fs::read(&b).unwrap(), pt_b);
        assert_eq!(
            names(&d.containers).len(),
            2,
            "both keys' containers survive while both loads are in flight: {:?}",
            names(&d.containers)
        );
    })
    .await
    .expect("hung");
}
