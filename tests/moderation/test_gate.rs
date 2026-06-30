// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 1.2 — the fail-closed gate. 🚨 SECURITY-CRITICAL.
//!
//! The gate releases (`Cleared`) for EXACTLY ONE input: `Ok(Some(Cleared))`.
//! Every other input holds. Each test asserts the *specific* resulting verdict
//! (no false-green on the safety path) and that `releases()` is true only for
//! `Cleared`.

use fabstir_llm_node::moderation::gate::Gate;
use fabstir_llm_node::moderation::types::{ModerationError, ModerationResult, Verdict};

#[test]
fn cleared_releases() {
    let r = ModerationResult::cleared();
    let v = Gate::decide(Ok(Some(&r)));
    assert_eq!(v, Verdict::Cleared);
    assert!(v.releases(), "Cleared is the only verdict that releases");
}

#[test]
fn blocked_holds() {
    let r = ModerationResult::blocked("csam");
    let v = Gate::decide(Ok(Some(&r)));
    assert_eq!(v, Verdict::Blocked);
    assert!(!v.releases(), "Blocked must hold");
}

#[test]
fn flagged_holds() {
    let r = ModerationResult::flagged("needs review");
    let v = Gate::decide(Ok(Some(&r)));
    // A flag holds at the gate but stays distinguishable from a hard block
    // (the transcode path emits CONTENT_FLAGGED vs CONTENT_BLOCKED, §3.2).
    assert_eq!(v, Verdict::Flagged);
    assert!(!v.releases(), "Flagged must hold pending review");
}

#[test]
fn unknown_holds() {
    // No recorded result for the job ⇒ hold (fail-closed), not clear.
    let v = Gate::decide(Ok(None));
    assert_eq!(v, Verdict::Blocked, "absent verdict must hold as Blocked");
    assert!(!v.releases());
}

#[test]
fn error_holds() {
    // A component error (e.g. the hash list is unavailable) ⇒ hold, never panic,
    // never clear.
    let v = Gate::decide(Err(ModerationError::ListUnavailable));
    assert_eq!(v, Verdict::Blocked, "Err input must hold as Blocked");
    assert!(!v.releases());
}
