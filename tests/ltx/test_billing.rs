// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 7 billing tests for the LTX sidecar: the megapixel-frame token vector,
//! `estimate_cost`, the `MIN_PROVEN_TOKENS` floor, and `LtxTracker` accumulation.

use ethers::types::U256;
use fabstir_llm_node::ltx::billing::{estimate_cost, LtxTracker, MIN_PROVEN_TOKENS};
use fabstir_llm_node::ltx::submit::ltx_tokens;
use fabstir_llm_node::ltx::{LtxJob, OutputKind, Resolution};

fn sample_job() -> LtxJob {
    LtxJob {
        template_id: "ltx-t2v-hdr".to_string(),
        template_hash: "0x9f2c".to_string(),
        prompt: "a derelict spaceship corridor".to_string(),
        seed: "4815162342".to_string(),
        frames: 121,
        fps: 24,
        resolution: Resolution { w: 1280, h: 720 },
        lora: "ltx-iclora-hdr@v1".to_string(),
        output: OutputKind::ExrSequence,
        images: None,
        videos: None,
        strength: None,
        azimuth: None,
        elevation: None,
        distance: None,
    }
}

#[test]
fn test_token_count_vector() {
    // Worked example from the interface seam: 121 × 1280 × 720 → 111,514.
    assert_eq!(ltx_tokens(121, 1280, 720), 111_514);
}

#[test]
fn test_estimate_cost_is_tokens_times_price() {
    let price = U256::from(5_000u64); // pricePerToken (with PRICE_PRECISION)
    let cost = estimate_cost(&sample_job(), price);
    assert_eq!(cost, U256::from(111_514u64) * price);
}

#[test]
fn test_real_clip_clears_floor() {
    assert_eq!(MIN_PROVEN_TOKENS, 100);
    let tokens = ltx_tokens(121, 1280, 720);
    assert!(
        tokens >= MIN_PROVEN_TOKENS,
        "a real clip ({tokens}) must clear the floor"
    );
}

// -----------------------------------------------------------------------------
// M1 economics — proof-state race machine (a pending COUNT, not an enum:
// MAX_CONCURRENT_GENERATIONS may exceed 1, so two clips of one session overlap)
// -----------------------------------------------------------------------------

/// The accept-gate latch window used across these tests (mirrors
/// `COMPLETING_LATCH_SECS`); no test sets the latch unless it says so.
const LATCH: std::time::Duration = std::time::Duration::from_secs(120);

#[tokio::test]
async fn test_pending_defer_complete_after_confirm() {
    let t = LtxTracker::new();
    t.mark_proof_pending(1, LATCH).await;
    assert!(
        t.defer_completion(1).await,
        "proof in flight ⇒ disconnect must defer"
    );
    assert!(
        !t.take_deferred_if_idle(1).await,
        "still pending ⇒ completion stays deferred"
    );
    t.mark_proof_submitted(1).await;
    assert_eq!(t.proofs_submitted(1).await, 1);
    assert!(
        t.take_deferred_if_idle(1).await,
        "proof landed + deferred ⇒ the finishing task completes"
    );
    assert!(
        !t.take_deferred_if_idle(1).await,
        "take clears the flag — completion runs once"
    );
}

#[tokio::test]
async fn test_error_forfeit_still_releases_deferred_completion() {
    // error ⇒ no proof ⇒ deferred completion still runs (settles at 0 — correct:
    // no work was delivered).
    let t = LtxTracker::new();
    t.mark_proof_pending(1, LATCH).await;
    assert!(t.defer_completion(1).await);
    t.mark_proof_forfeited(1).await;
    assert_eq!(t.proofs_submitted(1).await, 0);
    assert!(
        t.take_deferred_if_idle(1).await,
        "forfeit drops the count to 0 ⇒ deferred completion released"
    );
}

#[tokio::test]
async fn test_overlapping_clips_first_submit_does_not_release_deferral() {
    let t = LtxTracker::new();
    t.mark_proof_pending(1, LATCH).await;
    t.mark_proof_pending(1, LATCH).await; // clip B of the same session
    assert!(t.defer_completion(1).await);
    t.mark_proof_submitted(1).await; // clip A lands
    assert!(
        !t.take_deferred_if_idle(1).await,
        "clip B still pending — settling now would revert B's proof"
    );
    t.mark_proof_submitted(1).await; // clip B lands
    assert!(t.take_deferred_if_idle(1).await);
    assert_eq!(t.proofs_submitted(1).await, 2);
}

#[tokio::test]
async fn test_new_pending_clears_stale_deferral() {
    // A reconnected session starting a new clip cancels the stale deferral: the
    // new clip's own lifecycle now owns completion.
    let t = LtxTracker::new();
    t.mark_proof_pending(1, LATCH).await;
    assert!(t.defer_completion(1).await);
    t.mark_proof_submitted(1).await; // count 0, flag still set
    t.mark_proof_pending(1, LATCH).await; // new clip on reconnect
    t.mark_proof_submitted(1).await; // count back to 0
    assert!(
        !t.take_deferred_if_idle(1).await,
        "new pending must have cleared the stale deferral"
    );
}

#[tokio::test]
async fn test_no_entry_defer_is_false_and_wait_is_zero() {
    let t = LtxTracker::new();
    assert!(
        !t.defer_completion(9).await,
        "LLM-only session (no LTX entry) must not defer"
    );
    assert!(!t.take_deferred_if_idle(9).await);
    assert_eq!(t.proofs_submitted(9).await, 0);
    assert_eq!(
        t.proof_wait_remaining(9, 35).await,
        std::time::Duration::ZERO
    );
}

#[tokio::test]
async fn test_forfeit_is_saturating_and_noop_on_missing() {
    let t = LtxTracker::new();
    t.mark_proof_forfeited(9).await; // missing entry: no-op, no underflow
    assert!(!t.defer_completion(9).await);
    t.mark_proof_pending(1, LATCH).await;
    t.mark_proof_forfeited(1).await;
    t.mark_proof_forfeited(1).await; // double-forfeit: saturates at 0
    assert!(!t.defer_completion(1).await, "count must be 0, not wrapped");
}

#[tokio::test]
async fn test_has_pending_and_deferred_idle_are_read_only() {
    let t = LtxTracker::new();
    assert!(!t.has_pending(1).await, "no entry ⇒ no pending");
    assert!(!t.deferred_idle(1).await);
    t.mark_proof_pending(1, LATCH).await;
    assert!(t.has_pending(1).await);
    assert!(!t.deferred_idle(1).await, "pending but no deferral");
    assert!(t.defer_completion(1).await);
    assert!(
        !t.deferred_idle(1).await,
        "deferred but still pending ⇒ not idle"
    );
    t.mark_proof_submitted(1).await;
    assert!(!t.has_pending(1).await);
    assert!(t.deferred_idle(1).await, "deferred + count 0 ⇒ idle");
    // Read-only: peeking must NOT clear the flag (take does).
    assert!(t.deferred_idle(1).await);
    assert!(t.take_deferred_if_idle(1).await);
    assert!(!t.deferred_idle(1).await, "take cleared the flag");
}

#[tokio::test]
async fn test_completing_latch_gates_accepts_within_window() {
    // Once a completion tx is dispatched, the accept path must reject new
    // clips for the latch window (a clip accepted mid-completion would be
    // settled under: proof reverts, clip delivered free, session dead).
    let t = LtxTracker::new();
    assert!(
        !t.is_completing(1, LATCH).await,
        "no entry ⇒ not completing"
    );
    assert!(
        t.mark_completing_if_idle(1).await,
        "idle session ⇒ latch set"
    );
    assert!(t.is_completing(1, LATCH).await);
    assert!(
        !t.mark_proof_pending(1, LATCH).await,
        "the accept gate atomically rejects while the latch is fresh"
    );
    assert!(
        !t.has_pending(1).await,
        "a rejected accept must not have marked anything"
    );
    assert!(
        t.mark_proof_pending(1, std::time::Duration::ZERO).await,
        "an expired latch self-heals (a false Ok must not wedge the session)"
    );
    assert!(!t.is_completing(2, LATCH).await, "other jobs unaffected");
}

#[tokio::test]
async fn test_mark_completing_if_idle_yields_to_inflight_clip() {
    // The disconnect path's pre-dispatch guard: a clip in flight owns
    // completion; the guard must neither latch nor allow the dispatch.
    let t = LtxTracker::new();
    t.mark_proof_pending(1, LATCH).await;
    assert!(!t.mark_completing_if_idle(1).await);
    assert!(
        !t.is_completing(1, LATCH).await,
        "yielding must not set the latch"
    );
    t.mark_proof_submitted(1).await;
    assert!(t.mark_completing_if_idle(1).await, "idle again ⇒ latch");
}

#[tokio::test]
async fn test_take_deferred_if_idle_sets_the_latch() {
    // Taking deferred-completion ownership IS the start of completing: the
    // accept gate must reject from that same atomic moment.
    let t = LtxTracker::new();
    t.mark_proof_pending(1, LATCH).await;
    assert!(t.defer_completion(1).await);
    t.mark_proof_submitted(1).await;
    assert!(t.take_deferred_if_idle(1).await);
    assert!(
        !t.mark_proof_pending(1, LATCH).await,
        "accepts rejected while the taken completion is in flight"
    );
}

#[tokio::test]
async fn test_track_backfills_session_id_on_pending_created_entry() {
    // mark_proof_pending (at accept) creates the entry before track() (at clip
    // end) — the session_id must not stay None forever.
    let t = LtxTracker::new();
    t.mark_proof_pending(1, LATCH).await;
    t.track(1, Some("session-xyz".into()), 100, U256::from(10u64))
        .await;
    let info = t.get_job_info(1).await.unwrap();
    assert_eq!(info.session_id.as_deref(), Some("session-xyz"));
}

#[tokio::test]
async fn test_proof_wait_remaining_is_full_window_after_proof() {
    let t = LtxTracker::new();
    t.mark_proof_pending(1, LATCH).await;
    // Before any landed proof: nothing to wait on.
    assert_eq!(
        t.proof_wait_remaining(1, 35).await,
        std::time::Duration::ZERO
    );
    t.mark_proof_submitted(1).await;
    let remaining = t.proof_wait_remaining(1, 35).await;
    assert!(
        remaining > std::time::Duration::from_secs(34)
            && remaining <= std::time::Duration::from_secs(35),
        "immediately after a landed proof the full window (≈35s) remains, got {remaining:?}"
    );
}

#[tokio::test]
async fn test_tracker_accumulates_across_records() {
    let tracker = LtxTracker::new();
    let price = U256::from(10u64);
    tracker
        .track(
            1,
            Some("session-1".into()),
            111_514,
            U256::from(111_514u64) * price,
        )
        .await;
    tracker
        .track(
            1,
            Some("session-1".into()),
            50_000,
            U256::from(50_000u64) * price,
        )
        .await;
    let info = tracker.get_job_info(1).await.unwrap();
    assert_eq!(info.job_id, 1);
    assert_eq!(info.total_tokens, 161_514);
    assert_eq!(info.total_cost, U256::from(161_514u64) * price);
    assert_eq!(info.generation_count, 2);
}
