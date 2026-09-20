// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P5.5 (code-review rounds 7 and 11) — what a hash-bind mismatch
//! does to the ciphertext cache: a CACHED container that decrypted fine but
//! whose plaintext is not the on-chain model is evicted (a corrected re-seal
//! at the same ref must be re-downloaded at the next boot, whenever the fix
//! lands); a fresh download's mismatch keeps the cache (re-downloading the
//! same bytes cannot help). Round 9's once-per-hash record is retracted: it
//! wedged the corrected re-seal once the automatic restarts had spent it.

use super::streaming_fixture::*;
use fabstir_llm_node::tee::container_cache::ContainerOutcome;
use fabstir_llm_node::tee::orchestration::prepare_attested_model;
use fabstir_llm_node::tee::types::TeeError;
use sha2::{Digest, Sha256};
use std::time::Duration;

const WRAP: Duration = Duration::from_secs(20);

#[tokio::test]
async fn a_hash_bind_mismatch_evicts_a_cached_container_and_keeps_a_fresh_one() {
    tokio::time::timeout(WRAP, async {
        let f = super::test_orchestration::fixture();
        let right = format!("{:x}", Sha256::digest(&f.plaintext));
        let wrong = format!("{:x}", Sha256::digest(b"the corrected model"));
        let containers = f._dir.path().join("decrypt.containers");
        let prov = super::test_orchestration::good_provider();
        let encs = |dir: &std::path::Path| {
            names(dir)
                .into_iter()
                .filter(|n| n.ends_with(".enc"))
                .count()
        };

        // Boot 1: a FRESH download that mismatches keeps the cache.
        let err = prepare_attested_model(
            &f.loader,
            &f.source,
            &f.providers,
            &f.s5,
            &f.kbs,
            &prov,
            f.model_id,
            Some(&wrong),
        )
        .await
        .expect_err("mismatch");
        assert!(matches!(err, TeeError::ModelHashMismatch { .. }), "{err:?}");
        assert_eq!(
            f.loader.last_container_outcome(),
            Some(ContainerOutcome::Miss)
        );
        assert_eq!(encs(&containers), 1, "a fresh mismatch keeps the cache");

        // Boot 2: the same container is a HIT; the mismatch evicts it, so the
        // next boot re-downloads, which is how a corrected re-seal uploaded at
        // the same ref (same DEK, same policy) is ever picked up.
        let err = prepare_attested_model(
            &f.loader,
            &f.source,
            &f.providers,
            &f.s5,
            &f.kbs,
            &prov,
            f.model_id,
            Some(&wrong),
        )
        .await
        .expect_err("mismatch on a hit");
        assert!(matches!(err, TeeError::ModelHashMismatch { .. }), "{err:?}");
        assert_eq!(
            f.loader.last_container_outcome(),
            Some(ContainerOutcome::Hit)
        );
        assert_eq!(encs(&containers), 0, "evicted: {:?}", names(&containers));

        // Boot 3: a Miss with the RIGHT hash binds and the cache stays.
        let p = prepare_attested_model(
            &f.loader,
            &f.source,
            &f.providers,
            &f.s5,
            &f.kbs,
            &prov,
            f.model_id,
            Some(&right),
        )
        .await
        .expect("bound");
        assert_eq!(
            f.loader.last_container_outcome(),
            Some(ContainerOutcome::Miss)
        );
        f.loader.release(&p.model_id, &p.policy_hash);
        f.loader.evict_unreferenced();
        assert_eq!(encs(&containers), 1);

        // Boot 4: a Hit with the right hash binds; nothing evicted.
        let p = prepare_attested_model(
            &f.loader,
            &f.source,
            &f.providers,
            &f.s5,
            &f.kbs,
            &prov,
            f.model_id,
            Some(&right),
        )
        .await
        .expect("bound from the cache");
        assert_eq!(
            f.loader.last_container_outcome(),
            Some(ContainerOutcome::Hit)
        );
        f.loader.release(&p.model_id, &p.policy_hash);
        f.loader.evict_unreferenced();
        assert_eq!(encs(&containers), 1);
    })
    .await
    .expect("hung");
}
