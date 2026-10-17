// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Policy schema 3 (V3.1), the node half of "no model bytes change for a host
//! change" (acceptance 1): the container is bound to the policy, never to a
//! host profile, so a profile change is a container-cache HIT while the host
//! decision stays the broker's at every load (D19).

use super::streaming_fixture::*;
use fabstir_llm_node::tee::container_cache::ContainerOutcome;
use fabstir_llm_node::tee::host_profile::HostProfile;
use fabstir_llm_node::tee::mock::MockKeyBroker;
use fabstir_llm_node::tee::policy::SignedModelPolicy;
use fabstir_llm_node::tee::types::TeeError;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const M: [u8; 32] = [0x41u8; 32];
const DEK: [u8; 32] = [0x51u8; 32];

fn broker(log: &Log, profile: HostProfile) -> LoggingBroker {
    LoggingBroker {
        inner: MockKeyBroker::new(HashMap::from([(M, (DEK, test_policy(M), profile))])),
        log: Arc::clone(log),
        challenges: AtomicUsize::new(0),
    }
}

fn profile_named(id: &str, compose: &str) -> HostProfile {
    let mut p = test_profile();
    p.id = id.to_string();
    p.cvm.compose_hashes = vec![compose.repeat(32)];
    p.evidence = format!("approved as {id}");
    p
}

#[tokio::test]
async fn prepare_is_a_cache_hit_when_only_the_mock_host_profile_changes() {
    // Mutation: evict the container-cache entry after every successful release
    // in `model_source` → the second load downloads again → red.
    tokio::time::timeout(Duration::from_secs(20), async {
        // The real binding: the container is sealed under the policy's hash.
        let policy_hash = SignedModelPolicy {
            policy: test_policy(M),
            encrypted_ref: REF.to_string(),
            signer: String::new(),
            signature: vec![],
        }
        .policy_hash()
        .unwrap();
        let d = dirs();
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let pt = plaintext(5000, 9);
        let blobs = SwappableBlobs::new(Arc::clone(&log), REF, seal(&pt, &DEK, M, policy_hash));
        let l = loader(&d);

        // Load 1: the broker approves this host under profile "host-a".
        let a = broker(&log, profile_named("host-a", "aa"));
        l.prepare_encrypted_model(&blobs, &a, &good_provider(), &spec(M, policy_hash))
            .await
            .expect("load under host-a");
        assert_eq!(l.last_container_outcome(), Some(ContainerOutcome::Miss));
        l.release(&M, &policy_hash);
        l.evict_unreferenced();

        // Load 2: a NEW epoch approves the same host under a different
        // profile; the policy, hence policy_hash, is unchanged.
        let b = broker(&log, profile_named("host-b", "bb"));
        let p2 = l
            .prepare_encrypted_model(&blobs, &b, &good_provider(), &spec(M, policy_hash))
            .await
            .expect("load under host-b");
        assert_eq!(l.last_container_outcome(), Some(ContainerOutcome::Hit));
        assert_eq!(
            blobs.get_file_to_calls.load(Ordering::SeqCst),
            1,
            "no second download"
        );
        assert_eq!(
            b.challenges.load(Ordering::SeqCst),
            1,
            "the broker still decided"
        );
        assert_eq!(std::fs::read(&p2).unwrap(), pt, "plaintext byte-exact");
        l.release(&M, &policy_hash);
        l.evict_unreferenced();

        // Load 3: an epoch that no longer approves this host refuses, though
        // the container is still cached: acceptance is per boot, not per file.
        let mut gone = profile_named("host-c", "cc");
        gone.cvm.mrtd = "99".repeat(48);
        let c = broker(&log, gone);
        match l
            .prepare_encrypted_model(&blobs, &c, &good_provider(), &spec(M, policy_hash))
            .await
        {
            Err(TeeError::VerificationFailed(m)) => assert!(m.contains("mrtd: "), "{m}"),
            other => panic!("want the broker's refusal, got {other:?}"),
        }
        assert_eq!(blobs.get_file_to_calls.load(Ordering::SeqCst), 1);
    })
    .await
    .expect("hung");
}
