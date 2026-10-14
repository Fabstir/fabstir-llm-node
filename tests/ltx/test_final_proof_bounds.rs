// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Batch D21 round-1 review: every await in the relight final proof is bounded
//! by the budget; chain data (an absurd w9) can never panic it; the read at a
//! revert's block is retried while a resend could still fit; an interim still
//! in the queue at stop costs one more receipt wait of budget.

use std::time::Duration;

use ethers::types::H256;
use fabstir_llm_node::ltx::submit::ProofSubmitError;
use tokio::time::Instant;

use super::final_support::*;

/// Runs `send` under an outer timeout of `budget + 60` (a missing bound FAILS
/// the test instead of hanging it: the paused clock auto-advances to it).
async fn bounded(sub: &Final, reads: &Reads, clock: &Clock, budget: u64) -> bool {
    tokio::time::timeout(
        Duration::from_secs(budget + 60),
        send(sub, &rf(reads, clock, budget)),
    )
    .await
    .expect("the final proof returns by its budget")
}

#[tokio::test(start_paused = true)]
async fn a_final_submit_that_never_resolves_is_forfeited_at_the_budget_and_never_resent() {
    let t0 = Instant::now();
    let mut sub = Final::new(t0, vec![]);
    sub.hang_submit = true;
    let reads = Reads::default().with(None, snap(USED, W9));
    let clock = Clock(t0);
    assert!(!bounded(&sub, &reads, &clock, 600).await);
    assert_eq!(
        sub.calls().len(),
        1,
        "it may have been broadcast: no resend"
    );
    assert!(t0.elapsed() <= Duration::from_secs(600));
}

#[tokio::test(start_paused = true)]
async fn a_receipt_read_that_never_resolves_counts_as_none_and_forfeits_at_the_budget() {
    let t0 = Instant::now();
    let h = H256::repeat_byte(0xB1);
    let mut sub = Final::new(t0, vec![Err(ProofSubmitError::Unconfirmed(h))]);
    sub.hang_receipts = true;
    let reads = Reads::default().with(None, snap(USED, W9));
    let clock = Clock(t0);
    assert!(!bounded(&sub, &reads, &clock, 600).await);
    assert_eq!(sub.calls().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn an_absurd_w9_forfeits_without_panicking() {
    let t0 = Instant::now();
    let sub = Final::new(t0, vec![]);
    let reads = Reads::default();
    let clock = Clock(t0);
    let mut f = rf(&reads, &clock, 10_000);
    f.final_w9 = u64::MAX - 5;
    assert!(!send(&sub, &f).await);
    assert!(sub.calls().is_empty(), "a wait past the budget never sends");
}

#[tokio::test(start_paused = true)]
async fn an_absurd_w9_read_at_a_revert_forfeits_without_panicking() {
    let t0 = Instant::now();
    let sub = Final::new(
        t0,
        vec![Err(ProofSubmitError::Reverted { block: 100 }), Ok(())],
    );
    let reads = Reads::default().with(Some(100), snap(USED, u64::MAX - 5));
    let clock = Clock(t0);
    assert!(!bounded(&sub, &reads, &clock, 10_000).await);
    assert_eq!(sub.calls().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn the_read_at_the_reverts_block_is_retried_until_it_answers_then_the_resend_lands() {
    // the revert comes back at ≈ 66 s; the read at its block fails for 20 s
    let t0 = Instant::now();
    let sub = Final::new(
        t0,
        vec![Err(ProofSubmitError::Reverted { block: 100 }), Ok(())],
    );
    let reads = Reads::default()
        .with(Some(100), snap(USED + 100, BASE_UNIX + 70))
        .failing_until(Some(100), t0, 66 + 20);
    let clock = Clock(t0);
    assert!(bounded(&sub, &reads, &clock, 10_000).await);
    assert_eq!(sub.calls().len(), 2);
    let asked = reads.asked.lock().unwrap().clone();
    assert!(
        asked.len() > 1 && asked.iter().all(|t| *t == Some(100)),
        "{asked:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn the_read_at_the_reverts_block_gives_up_once_a_resend_could_no_longer_fit() {
    let t0 = Instant::now();
    let sub = Final::new(
        t0,
        vec![Err(ProofSubmitError::Reverted { block: 100 }), Ok(())],
    );
    let reads = Reads::default()
        .with(Some(100), snap(USED + 100, BASE_UNIX + 70))
        .failing_until(Some(100), t0, 100_000);
    let clock = Clock(t0);
    assert!(!bounded(&sub, &reads, &clock, 600).await);
    assert_eq!(sub.calls().len(), 1, "nothing is sent while reading");
    // stopped when the last resend that could fit (76 + 5 + 60 s) had to start
    assert!(t0.elapsed() <= Duration::from_secs(600 - (RW + 5 + 60) + 10));
}

#[tokio::test(start_paused = true)]
async fn an_interim_in_flight_at_stop_needs_one_more_receipt_wait_before_the_first_send() {
    // budget 150: the first attempt ends by 0 + 81 + 60 = 141 — but behind an
    // interim still in the queue it can end 60 s later (201 > 150)
    let t0 = Instant::now();
    let sub = Final::new(t0, vec![]);
    let reads = Reads::default();
    let clock = Clock(t0);
    let mut f = rf(&reads, &clock, 150);
    f.first_attempt_extra = Duration::from_secs(60);
    assert!(!send(&sub, &f).await);
    assert!(sub.calls().is_empty());
    // without the interim in flight it starts
    let sub = Final::new(Instant::now(), vec![]);
    let clock = Clock(Instant::now());
    assert!(send(&sub, &rf(&reads, &clock, 150)).await);
}

fn attestation() -> fabstir_llm_node::ltx::types::Attestation {
    use fabstir_llm_node::ltx::types::{Attestation, FrameManifest, Resolution};
    Attestation {
        model_id: "0x01".into(),
        template_hash: "0x02".into(),
        env_hash: "0x03".into(),
        input_commitment: "0x04".into(),
        output_cid: "uOut".into(),
        manifest: FrameManifest {
            frame_count: 1,
            fps: 25,
            resolution: Resolution { w: 1920, h: 1088 },
            colour_encoding: "vfx-passes-v1".into(),
            frame_hashes: vec!["0xaa".into()],
            merkle_root: "0xbb".into(),
        },
        session_id: "0x05".into(),
        host: "0xb0".into(),
        timestamp: 1_790_000_000,
        signature: None,
    }
}

#[tokio::test(start_paused = true)]
async fn a_relight_final_with_no_confirmed_interim_never_reads_the_interval_from_chain() {
    // round 2: the first-proof floor comes from the admission snapshot's w10
    // (carried in RelightFinal), never an unbounded `session_proof_interval` read
    use fabstir_llm_node::ltx::billing::LtxTracker;
    use fabstir_llm_node::ltx::submit::{finalize_clip, SubmitPolicy};
    use fabstir_llm_node::storage::s5_client::MockS5Backend;
    let t0 = Instant::now();
    let mut sub = Final::new(t0, vec![]);
    sub.hang_interval = true;
    let reads = Reads::default();
    let clock = Clock(t0);
    let tracker = LtxTracker::new();
    tracker
        .mark_proof_pending(JID, Duration::from_secs(120))
        .await;
    assert_eq!(
        tracker.proofs_submitted(JID).await,
        0,
        "no interim confirmed"
    );
    let s5 = MockS5Backend::new();
    let att = attestation();
    let out = tokio::time::timeout(
        Duration::from_secs(10_000 + 60),
        finalize_clip(
            &s5,
            Some(&sub),
            &tracker,
            Some(JID),
            true,
            "42-p1",
            &att,
            REMAINDER,
            SubmitPolicy::Relight(rf(&reads, &clock, 10_000)),
        ),
    )
    .await
    .expect("the relight final never waits on the interval read")
    .unwrap();
    assert!(out.1, "landed");
    assert_eq!(sub.calls().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_relight_final_below_the_snapshot_interval_is_skipped_as_a_first_proof() {
    // the carried w10 is the floor: a 999-token first final would revert
    // "Low first" — skipped without a send
    use fabstir_llm_node::ltx::billing::LtxTracker;
    use fabstir_llm_node::ltx::submit::{finalize_clip, SubmitPolicy};
    use fabstir_llm_node::storage::s5_client::MockS5Backend;
    let t0 = Instant::now();
    let sub = Final::new(t0, vec![]);
    let reads = Reads::default();
    let clock = Clock(t0);
    let tracker = LtxTracker::new();
    tracker
        .mark_proof_pending(JID, Duration::from_secs(120))
        .await;
    let mut f = rf(&reads, &clock, 10_000);
    f.proof_interval = 5_000;
    let (_, landed) = finalize_clip(
        &MockS5Backend::new(),
        Some(&sub),
        &tracker,
        Some(JID),
        true,
        "42-p1",
        &attestation(),
        4_999,
        SubmitPolicy::Relight(f),
    )
    .await
    .unwrap();
    assert!(!landed);
    assert!(sub.calls().is_empty());
}
