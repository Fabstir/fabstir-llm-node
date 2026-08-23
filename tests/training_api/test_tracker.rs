// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! T4.b: the TrainTracker billing/race machine (TD8). The billing-law rows
//! pin B.2/C.1: forfeited slices BILL (artifacts delivered) but do not
//! settle; the completion-race rows pin the LTX-proven deferral/latch rules.

use std::time::Duration;

use fabstir_llm_node::training::tracker::TrainTracker;

const LATCH: Duration = Duration::from_secs(30);

#[tokio::test]
async fn billing_triple_with_a_forfeited_slice() {
    // 9-slice schedule: 8 × 1M + 1.6M; slice 3 forfeits. The wire bill must
    // equal the SCHEDULE TOTAL; on-chain settles total − forfeited delta.
    let tracker = TrainTracker::new();
    let deltas: Vec<u64> = vec![1_000_000; 8]
        .into_iter()
        .chain([1_600_000u64])
        .collect();
    for (index, delta) in deltas.iter().enumerate() {
        assert!(tracker.mark_slice_pending(9, LATCH).await, "slice {index}");
        if index == 3 {
            tracker.mark_slice_forfeited(9, *delta).await;
        } else {
            tracker.mark_slice_submitted(9, *delta).await;
        }
    }
    let info = tracker.info(9).await.expect("run tracked");
    assert_eq!(info.billed_tokens, 9_600_000, "wire bill = schedule total");
    assert_eq!(info.settled_tokens, 8_600_000, "on-chain = landed deltas");
    assert_eq!(info.slices_submitted, 8);
    assert_eq!(info.slices_forfeited, 1);
    assert_eq!(info.pending_count, 0, "every pending resolved");
}

#[tokio::test]
async fn pending_gate_refuses_inside_the_completing_latch() {
    let tracker = TrainTracker::new();
    assert!(
        tracker.mark_completing_if_idle(10).await,
        "idle → completing"
    );
    assert!(
        !tracker.mark_slice_pending(10, LATCH).await,
        "an accept inside the fresh latch must refuse (the settle-under race)"
    );
    // With a zero-width latch the same call passes (the latch self-expires).
    assert!(tracker.mark_slice_pending(10, Duration::ZERO).await);
}

#[tokio::test]
async fn deferral_hands_completion_to_the_run_exactly_once() {
    let tracker = TrainTracker::new();
    assert!(tracker.mark_slice_pending(11, LATCH).await);
    // Disconnect mid-proof: deferral engages.
    assert!(tracker.defer_completion(11).await);
    assert!(tracker.has_pending(11).await);
    // Not idle yet: nothing to take.
    assert!(!tracker.take_deferred_if_idle(11).await);
    tracker.mark_slice_submitted(11, 1_000_000).await;
    assert!(!tracker.has_pending(11).await);
    // Idle now: the run task takes it EXACTLY once (and the latch arms).
    assert!(tracker.take_deferred_if_idle(11).await);
    assert!(!tracker.take_deferred_if_idle(11).await, "exactly once");
    assert!(
        !tracker.mark_slice_pending(11, LATCH).await,
        "the handover armed the completing latch"
    );
}

#[tokio::test]
async fn defer_is_false_for_idle_or_unknown_sessions() {
    let tracker = TrainTracker::new();
    assert!(!tracker.defer_completion(12).await, "unknown session");
    assert!(tracker.mark_slice_pending(12, LATCH).await);
    tracker.mark_slice_submitted(12, 5).await;
    assert!(!tracker.defer_completion(12).await, "idle session");
}

#[tokio::test]
async fn completing_guard_refuses_while_a_proof_is_pending() {
    let tracker = TrainTracker::new();
    assert!(tracker.mark_slice_pending(13, LATCH).await);
    assert!(
        !tracker.mark_completing_if_idle(13).await,
        "a pending proof owns completion"
    );
    tracker.mark_slice_forfeited(13, 7).await;
    assert!(tracker.mark_completing_if_idle(13).await);
}

#[tokio::test]
async fn proof_wait_counts_from_the_last_confirmed_proof() {
    let tracker = TrainTracker::new();
    // No proof ever landed: nothing gates completion.
    assert_eq!(tracker.proof_wait_remaining(14, 30).await, Duration::ZERO);
    assert!(tracker.mark_slice_pending(14, LATCH).await);
    tracker.mark_slice_submitted(14, 5).await;
    let wait = tracker.proof_wait_remaining(14, 30).await;
    assert!(
        wait > Duration::from_secs(25) && wait <= Duration::from_secs(30),
        "{wait:?} must be ~the full window just after confirmation"
    );
    // Forfeits do NOT reset the dispute clock (no tx confirmed).
    tracker.mark_slice_pending(14, Duration::ZERO).await;
    tracker.mark_slice_forfeited(14, 5).await;
    let wait2 = tracker.proof_wait_remaining(14, 30).await;
    assert!(wait2 <= wait, "a forfeit must not extend the wait");
}
