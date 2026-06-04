// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 2.2 — host-reachable seam-#2 transcode gate decision. 🚨 SECURITY-CRITICAL.
//!
//! Tests the extracted, pure `Gate::transcode_decision` against a real
//! `VerdictStore`. It releases ONLY on a recorded `Cleared`; every other state
//! (blocked / flagged / absent / no-job_id) HOLDs with the right protocol code.

use fabstir_llm_node::moderation::gate::{Gate, GateOutcome};
use fabstir_llm_node::moderation::types::ModerationResult;
use fabstir_llm_node::moderation::verdict_store::VerdictStore;

#[test]
fn cleared_falls_through_to_complete() {
    let store = VerdictStore::new();
    store.set(1, ModerationResult::cleared());
    assert!(
        matches!(
            Gate::transcode_decision(&store, Some(1)),
            GateOutcome::Release
        ),
        "a recorded Cleared verdict must release to completion"
    );
}

#[test]
fn blocked_sends_error_not_complete() {
    let store = VerdictStore::new();
    store.set(2, ModerationResult::blocked("csam"));
    match Gate::transcode_decision(&store, Some(2)) {
        GateOutcome::Hold { code, .. } => assert_eq!(code, "CONTENT_BLOCKED"),
        GateOutcome::Release => panic!("a blocked job must NOT complete"),
    }
}

#[test]
fn flagged_holds_as_flagged() {
    let store = VerdictStore::new();
    store.set(3, ModerationResult::flagged("needs review"));
    match Gate::transcode_decision(&store, Some(3)) {
        GateOutcome::Hold { code, .. } => assert_eq!(code, "CONTENT_FLAGGED"),
        GateOutcome::Release => panic!("a flagged job must hold"),
    }
}

#[test]
fn absent_verdict_holds() {
    // No verdict recorded for the job ⇒ MODERATION_UNAVAILABLE (fail-closed).
    let store = VerdictStore::new();
    match Gate::transcode_decision(&store, Some(99)) {
        GateOutcome::Hold { code, .. } => assert_eq!(code, "MODERATION_UNAVAILABLE"),
        GateOutcome::Release => panic!("an absent verdict must hold"),
    }
}

#[test]
fn missing_job_id_holds() {
    // A transcode with no job_id cannot be moderated ⇒ hold (fail-closed).
    let store = VerdictStore::new();
    match Gate::transcode_decision(&store, None) {
        GateOutcome::Hold { code, .. } => assert_eq!(code, "MODERATION_UNAVAILABLE"),
        GateOutcome::Release => panic!("no job_id ⇒ cannot moderate ⇒ hold"),
    }
}

#[test]
fn blocked_skips_billing_and_proof() {
    // A Hold short-circuits the transcode handler BEFORE the billing/proof block
    // (the gate is inserted before :123), so a blocked job writes no S5 proof
    // artifact (R6). This test verifies the decision is Hold (the necessary
    // condition); placement-before-billing is verified by code review — driving
    // the spawned async handler against a live S5 is out of unit-test scope.
    let store = VerdictStore::new();
    store.set(7, ModerationResult::blocked("csam"));
    assert!(
        !matches!(
            Gate::transcode_decision(&store, Some(7)),
            GateOutcome::Release
        ),
        "a blocked job must hold (skipping billing + proof)"
    );
}

#[test]
fn apiserver_moderation_store_wired_and_enforce_defaults_off() {
    // The gate reads its store + activation from ApiServer. Verify the wiring:
    // the store is present and empty (⇒ any lookup holds), and enforcement defaults
    // OFF (dark-launch) so merging the gate does not brick transcoding before
    // seam-#1 ingest is wired. Flip MODERATION_ENFORCE=true at go-live.
    use fabstir_llm_node::api::server::ApiServer;
    let server = ApiServer::new_for_test();
    assert!(
        server.moderation_store().get(123).is_none(),
        "an unpopulated store must hold (None) for every job"
    );
    assert!(
        !server.moderation_enforce(),
        "enforcement must default OFF (dark-launch); enabled via MODERATION_ENFORCE"
    );
}
