// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 3.1 — HashListSource adapter + mock + fail-closed availability. 🚨

use fabstir_llm_node::moderation::csam::hashlist::{HashListSource, ListState, NcmecHashStore};
use fabstir_llm_node::moderation::csam::mock_source::MockHashListSource;
use fabstir_llm_node::moderation::types::ModerationError;

#[test]
fn mock_source_loads_vectors() {
    let src = MockHashListSource::with_test_vectors();
    let snap = src.refresh().unwrap();
    assert_eq!(snap.state, ListState::Loaded);
    assert!(
        !snap.sha256.is_empty(),
        "the mock must load known-bad SHA-256 vectors"
    );
    assert!(
        !snap.pdq.is_empty(),
        "the mock must load known-bad PDQ vectors"
    );
}

#[test]
fn snapshot_versioned() {
    let src = MockHashListSource::with_test_vectors();
    assert_eq!(src.version(), src.refresh().unwrap().version);
    assert!(src.version() >= 1, "a loaded list has a non-zero version");
}

#[test]
fn first_boot_before_any_refresh_is_unavailable() {
    // A fresh store with no successful refresh yet must be Unavailable (fail-closed),
    // never an empty "clean" list.
    let store = NcmecHashStore::new(b"test-key-material".to_vec());
    assert_eq!(
        store.current_snapshot().state,
        ListState::Unavailable,
        "first boot must be Unavailable"
    );
}

#[test]
fn refresh_failure_sets_unavailable_not_empty() {
    // No real NCMEC endpoint is wired, so refresh cannot fetch ⇒ Unavailable, and it
    // must NEVER install an empty Loaded list (R7).
    let store = NcmecHashStore::new(b"k".to_vec());
    let snap = store.refresh().unwrap();
    assert_eq!(snap.state, ListState::Unavailable);
    assert_ne!(
        snap.state,
        ListState::Loaded,
        "a failed refresh must never install an empty Loaded list"
    );
}

#[test]
fn match_against_unavailable_list_holds() {
    // The fail-closed core (§3.4): an Unavailable snapshot must yield
    // Err(ListUnavailable) — which the matcher maps to a HOLD. It must NOT be
    // treated as a clean/empty list.
    let store = NcmecHashStore::new(b"k".to_vec());
    let snap = store.current_snapshot();
    assert!(matches!(
        snap.require_available(),
        Err(ModerationError::ListUnavailable)
    ));
}

#[test]
fn loaded_list_is_available() {
    // Positive control: a genuinely-loaded list is usable.
    let snap = MockHashListSource::with_test_vectors().refresh().unwrap();
    assert!(snap.require_available().is_ok(), "a Loaded list is usable");
}
