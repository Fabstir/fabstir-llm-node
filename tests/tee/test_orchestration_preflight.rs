// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P4.5 — where the broker pre-check sits in the load: after the
//! plaintext cache check, BEFORE the container fetch (design §3). Uses the
//! orchestration fixture with a counting blob source and a scripted
//! `KeyBrokerClient` wrapper around the mock broker.

use super::test_orchestration::{fixture, good_provider, Fixture};
use async_trait::async_trait;
use fabstir_llm_node::tee::key_broker::KeyBrokerClient;
use fabstir_llm_node::tee::model_source::BlobSource;
use fabstir_llm_node::tee::orchestration::prepare_attested_model;
use fabstir_llm_node::tee::types::{Evidence, TeeError, TeeResult, WrappedKey};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Counts fetches; delegates to the fixture's in-memory store.
struct CountingBlobs<'a> {
    inner: &'a dyn BlobSource,
    fetches: AtomicUsize,
}

#[async_trait]
impl BlobSource for CountingBlobs<'_> {
    async fn get_file(&self, path: &str) -> TeeResult<Vec<u8>> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        self.inner.get_file(path).await
    }
}

/// Delegates the release flow to the mock; `preflight` is scripted and counted.
struct ScriptedBroker<'a> {
    inner: &'a dyn KeyBrokerClient,
    refuse: AtomicBool,
    preflights: AtomicUsize,
}

#[async_trait]
impl KeyBrokerClient for ScriptedBroker<'_> {
    async fn challenge(&self, model_id: [u8; 32], pk_att: &[u8]) -> TeeResult<[u8; 32]> {
        self.inner.challenge(model_id, pk_att).await
    }
    async fn request_key(&self, model_id: [u8; 32], ev: &Evidence) -> TeeResult<WrappedKey> {
        self.inner.request_key(model_id, ev).await
    }
    async fn preflight(&self, _model_id: [u8; 32]) -> TeeResult<()> {
        self.preflights.fetch_add(1, Ordering::SeqCst);
        if self.refuse.load(Ordering::SeqCst) {
            Err(TeeError::Kbs("scripted preflight refusal".into()))
        } else {
            Ok(())
        }
    }
}

fn wrap<'a>(f: &'a Fixture, refuse: bool) -> (CountingBlobs<'a>, ScriptedBroker<'a>) {
    (
        CountingBlobs {
            inner: &f.s5,
            fetches: AtomicUsize::new(0),
        },
        ScriptedBroker {
            inner: &f.kbs,
            refuse: AtomicBool::new(refuse),
            preflights: AtomicUsize::new(0),
        },
    )
}

#[tokio::test]
async fn a_failed_preflight_stops_the_load_before_the_container_fetch() {
    let f = fixture();
    let (s5, kbs) = wrap(&f, true);
    let r = prepare_attested_model(
        &f.loader,
        &f.source,
        &f.providers,
        &s5,
        &kbs,
        &good_provider(),
        f.model_id,
        None,
    )
    .await;
    match r {
        Err(TeeError::Kbs(m)) => assert_eq!(m, "scripted preflight refusal"),
        other => panic!("the scripted refusal must be the error, got {other:?}"),
    }
    assert_eq!(
        s5.fetches.load(Ordering::SeqCst),
        0,
        "nothing was downloaded (mutation: preflight after the fetch → 1)"
    );
    assert_eq!(kbs.preflights.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read_dir(f._dir.path()).unwrap().count(),
        0,
        "nothing on disk under the decrypt dir"
    );
}

#[tokio::test]
async fn a_cached_plaintext_skips_the_preflight() {
    let f = fixture();
    let (s5, kbs) = wrap(&f, false);
    let first = prepare_attested_model(
        &f.loader,
        &f.source,
        &f.providers,
        &s5,
        &kbs,
        &good_provider(),
        f.model_id,
        None,
    )
    .await
    .expect("first load");
    assert_eq!(kbs.preflights.load(Ordering::SeqCst), 1);
    assert_eq!(s5.fetches.load(Ordering::SeqCst), 1);
    // The broker now refuses; the same (model_id, policy_hash) is served from the
    // cache without asking it (mutation: preflight before cache_acquire → Err).
    kbs.refuse.store(true, Ordering::SeqCst);
    let second = prepare_attested_model(
        &f.loader,
        &f.source,
        &f.providers,
        &s5,
        &kbs,
        &good_provider(),
        f.model_id,
        None,
    )
    .await
    .expect("cached load needs no key and no preflight");
    assert_eq!(second.path, first.path);
    assert_eq!(kbs.preflights.load(Ordering::SeqCst), 1, "still one");
    assert_eq!(s5.fetches.load(Ordering::SeqCst), 1, "still one");
}
