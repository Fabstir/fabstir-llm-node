// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 1.3 — the verdict store (fail-closed lookup).

use std::sync::Arc;
use std::thread;

use fabstir_llm_node::moderation::types::{ModerationResult, Verdict};
use fabstir_llm_node::moderation::verdict_store::VerdictStore;

#[test]
fn absent_job_is_hold() {
    let store = VerdictStore::new();
    assert!(
        store.get(42).is_none(),
        "an absent job_id must return None so the gate HOLDs"
    );
}

#[test]
fn set_then_get_cleared() {
    let store = VerdictStore::new();
    store.set(7, ModerationResult::cleared());
    let r = store.get(7).expect("set then get");
    assert_eq!(r.verdict, Verdict::Cleared);
}

#[test]
fn overwrite_to_blocked() {
    let store = VerdictStore::new();
    store.set(7, ModerationResult::cleared());
    store.set(7, ModerationResult::blocked("csam"));
    let r = store.get(7).expect("overwrite");
    assert_eq!(
        r.verdict,
        Verdict::Blocked,
        "a later block must overwrite an earlier clear"
    );
}

#[test]
fn cleared_over_blocked_rejected() {
    // C4: a Cleared verdict must NEVER downgrade an existing non-Cleared (Blocked)
    // verdict — a later benign /frames POST cannot release a job already blocked.
    let store = VerdictStore::new();
    store.set(7, ModerationResult::blocked("csam"));
    store.set_if_not_downgrade(7, ModerationResult::cleared());
    let r = store.get(7).expect("entry");
    assert_eq!(
        r.verdict,
        Verdict::Blocked,
        "Cleared must not overwrite an existing Blocked"
    );
}

#[test]
fn blocked_over_cleared_writes() {
    // The reverse IS allowed: a block always overwrites a prior clear (escalation).
    let store = VerdictStore::new();
    store.set(7, ModerationResult::cleared());
    store.set_if_not_downgrade(7, ModerationResult::blocked("csam"));
    assert_eq!(store.get(7).unwrap().verdict, Verdict::Blocked);
}

#[test]
fn blocked_over_absent_writes() {
    // First write of any kind (including the common Cleared) lands when absent.
    let store = VerdictStore::new();
    store.set_if_not_downgrade(7, ModerationResult::cleared());
    assert_eq!(store.get(7).unwrap().verdict, Verdict::Cleared);
    let store2 = VerdictStore::new();
    store2.set_if_not_downgrade(9, ModerationResult::blocked("csam"));
    assert_eq!(store2.get(9).unwrap().verdict, Verdict::Blocked);
}

#[test]
fn concurrent_absent_lookups_all_hold() {
    // N concurrent readers of an absent job_id must all see None (hold). No race
    // may produce a spurious clear.
    let store = Arc::new(VerdictStore::new());
    let handles: Vec<_> = (0..16u64)
        .map(|i| {
            let s = Arc::clone(&store);
            thread::spawn(move || s.get(1000 + i).is_none())
        })
        .collect();
    for h in handles {
        assert!(
            h.join().unwrap(),
            "every concurrent absent lookup must hold (None)"
        );
    }
}
