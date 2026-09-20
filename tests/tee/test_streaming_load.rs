// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P5.5 (design §7, `test_streaming_load.rs`) — the ciphertext cache:
//! reuse, the header check BEFORE the challenge (stale, undecodable, a fresh
//! file's wrong header) and the space check's wiring. Every `get_file_to` and
//! `challenge` lands on ONE ordered log. The once-retry, the prune and the
//! digest are in `test_streaming_retry.rs` (the 400-line cap).

use super::streaming_fixture::*;
use fabstir_llm_node::tee::container::HEADER_LEN;
use fabstir_llm_node::tee::container_cache::{ContainerOutcome, FsSpace};
use fabstir_llm_node::tee::types::TeeError;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const M: [u8; 32] = [0x11u8; 32];
const H1: [u8; 32] = [0x21u8; 32];
const H2: [u8; 32] = [0x22u8; 32];
const DEK1: [u8; 32] = [0x31u8; 32];
const WRAP: Duration = Duration::from_secs(20);

fn fresh_log() -> Log {
    Arc::new(Mutex::new(Vec::new()))
}

#[tokio::test]
async fn a_cached_container_is_reused_after_release_and_evict() {
    tokio::time::timeout(WRAP, async {
        let d = dirs();
        let log = fresh_log();
        let pt = plaintext(5000, 1);
        let blobs = SwappableBlobs::new(Arc::clone(&log), REF, seal(&pt, &DEK1, M, H1));
        let kbs = LoggingBroker::new(Arc::clone(&log), M, DEK1);
        let l = loader(&d);
        let p1 = l
            .prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H1))
            .await
            .unwrap();
        assert_eq!(log_of(&log), vec!["get_file_to", "challenge"]);
        assert_eq!(l.last_container_outcome(), Some(ContainerOutcome::Miss));
        assert!(cache_file(&d).exists(), "cache file named <key>.enc");
        l.release(&M, &H1);
        l.evict_unreferenced();
        assert!(!p1.exists());
        let p2 = l
            .prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H1))
            .await
            .unwrap();
        // One challenge per load (there is no DEK cache); NO second download.
        assert_eq!(log_of(&log), vec!["get_file_to", "challenge", "challenge"]);
        assert_eq!(blobs.get_file_to_calls.load(Ordering::SeqCst), 1);
        assert_eq!(kbs.challenges.load(Ordering::SeqCst), 2);
        assert_eq!(l.last_container_outcome(), Some(ContainerOutcome::Hit));
        assert_eq!(std::fs::read(&p2).unwrap(), pt, "plaintext byte-exact");
    })
    .await
    .expect("hung");
}

#[tokio::test]
async fn a_stale_cache_is_a_miss_before_the_challenge_on_policy_rotation() {
    tokio::time::timeout(WRAP, async {
        let d = dirs();
        let log = fresh_log();
        // A sealed for (M, H1) sits in the cache; the spec now says (M, H2)
        // and the SAME ref serves B sealed for (M, H2).
        plant(&d, &seal(&plaintext(5000, 1), &DEK1, M, H1));
        let pt_b = plaintext(5000, 2);
        let b = seal(&pt_b, &DEK1, M, H2);
        let blobs = SwappableBlobs::new(Arc::clone(&log), REF, b.clone());
        let kbs = LoggingBroker::new(Arc::clone(&log), M, DEK1);
        let l = loader(&d);
        let p = l
            .prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H2))
            .await
            .unwrap();
        assert_eq!(
            log_of(&log),
            vec!["get_file_to", "challenge"],
            "the header check runs BEFORE the challenge"
        );
        assert_eq!(l.last_container_outcome(), Some(ContainerOutcome::Stale));
        assert_eq!(std::fs::read(cache_file(&d)).unwrap(), b, "cache = B");
        assert_eq!(std::fs::read(&p).unwrap(), pt_b);
    })
    .await
    .expect("hung");
}

#[tokio::test]
async fn an_undecodable_cached_header_is_a_miss_not_a_refusal() {
    tokio::time::timeout(WRAP, async {
        let d = dirs();
        let log = fresh_log();
        let pt = plaintext(5000, 3);
        let blobs = SwappableBlobs::new(Arc::clone(&log), REF, seal(&pt, &DEK1, M, H1));
        let kbs = LoggingBroker::new(Arc::clone(&log), M, DEK1);
        let l = loader(&d);
        // 10 bytes.
        plant(&d, b"0123456789");
        l.prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H1))
            .await
            .expect("a garbage cache file is a miss, never a refusal");
        assert_eq!(l.last_container_outcome(), Some(ContainerOutcome::Stale));
        assert_eq!(log_of(&log), vec!["get_file_to", "challenge"]);
        l.release(&M, &H1);
        l.evict_unreferenced();
        // 98 bytes of the wrong magic.
        let mut wrong = vec![0u8; HEADER_LEN];
        wrong[..8].copy_from_slice(b"NOTMAGIC");
        plant(&d, &wrong);
        l.prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H1))
            .await
            .expect("a wrong-magic cache file is a miss");
        assert_eq!(l.last_container_outcome(), Some(ContainerOutcome::Stale));
        assert_eq!(
            log_of(&log),
            vec!["get_file_to", "challenge", "get_file_to", "challenge"]
        );
        // A third load with nothing planted and no release: an in-process
        // plaintext hit, which never reaches the container step.
        l.prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H1))
            .await
            .unwrap();
        assert_eq!(l.last_container_outcome(), None);
    })
    .await
    .expect("hung");
}

#[tokio::test]
async fn a_fresh_container_with_the_wrong_header_refuses_before_the_challenge() {
    tokio::time::timeout(WRAP, async {
        // No cache; the blob is sealed for (M, H1) but this boot pinned H2.
        let d = dirs();
        let log = fresh_log();
        let blobs = SwappableBlobs::new(
            Arc::clone(&log),
            REF,
            seal(&plaintext(5000, 1), &DEK1, M, H1),
        );
        let kbs = LoggingBroker::new(Arc::clone(&log), M, DEK1);
        let l = loader(&d);
        let err = l
            .prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H2))
            .await
            .expect_err("a fresh container bound to another policy is refused");
        match &err {
            TeeError::VerificationFailed(m) => assert!(m.contains("sealed for"), "{m}"),
            other => panic!("expected VerificationFailed, got {other:?}"),
        }
        assert_eq!(log_of(&log), vec!["get_file_to"], "no challenge spent");
        assert!(!cache_file(&d).exists(), "the mismatched file is deleted");
        assert!(regular_files(&d.decrypt).is_empty());

        // The same on the retry path, on a fresh fixture: A planted
        // corrupt-but-decodable for (M, H2); the blob serves B sealed for (M, H1).
        let d = dirs();
        let log = fresh_log();
        let mut a = seal(&plaintext(5000, 1), &DEK1, M, H2);
        let last = a.len() - 1;
        a[last] ^= 0xFF;
        plant(&d, &a);
        let blobs = SwappableBlobs::new(
            Arc::clone(&log),
            REF,
            seal(&plaintext(5000, 1), &DEK1, M, H1),
        );
        let kbs = LoggingBroker::new(Arc::clone(&log), M, DEK1);
        let l = loader(&d);
        let err = l
            .prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H2))
            .await
            .expect_err("the retry's fresh download has the wrong header");
        assert!(matches!(err, TeeError::VerificationFailed(_)), "{err:?}");
        assert_eq!(log_of(&log), vec!["challenge", "get_file_to"]);
        assert!(!cache_file(&d).exists());
    })
    .await
    .expect("hung");
}

#[tokio::test]
async fn the_loader_wires_the_space_check_on_miss_and_on_hit() {
    tokio::time::timeout(WRAP, async {
        // MISS: one filesystem, 1.5 × the length available → refused before
        // any byte, no challenge.
        let d = dirs();
        let log = fresh_log();
        let sealed = seal(&plaintext(5000, 1), &DEK1, M, H1);
        let len = sealed.len() as u64;
        let blobs = SwappableBlobs::new(Arc::clone(&log), REF, sealed.clone());
        let kbs = LoggingBroker::new(Arc::clone(&log), M, DEK1);
        let avail = len + len / 2;
        let l = loader(&d).with_space_probe(Box::new(move |_| Ok(FsSpace { dev: 1, avail })));
        let err = l
            .prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H1))
            .await
            .expect_err("1.5× on one filesystem is refused");
        assert!(err.to_string().contains("needs"), "{err}");
        assert_eq!(
            log_of(&log),
            vec!["get_file_to"],
            "on_length ran, no challenge"
        );
        assert!(!cache_file(&d).exists());
        assert!(names(&d.containers).is_empty(), "no .part either");

        // HIT (fresh fixture): a valid A planted; the decrypt dir is short.
        let d = dirs();
        let log = fresh_log();
        plant(&d, &sealed);
        let blobs = SwappableBlobs::new(Arc::clone(&log), REF, sealed.clone());
        let kbs = LoggingBroker::new(Arc::clone(&log), M, DEK1);
        let l = loader(&d).with_space_probe(Box::new(move |_| {
            Ok(FsSpace {
                dev: 2,
                avail: len - 1,
            })
        }));
        let err = l
            .prepare_encrypted_model(&blobs, &kbs, &good_provider(), &spec(M, H1))
            .await
            .expect_err("a hit still checks the decrypt dir");
        assert!(err.to_string().contains("needs"), "{err}");
        assert!(log_of(&log).is_empty(), "no download, no challenge");
        assert!(cache_file(&d).exists(), "cache kept");
    })
    .await
    .expect("hung");
}
