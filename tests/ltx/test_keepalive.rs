// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Batch D21: the keep-alive scheduler (`src/ltx/keepalive.rs`). Every
//! interval it uploads a progress attestation and posts an interim proof; it
//! stops on the guard (every exit of `run`), on a closed progress channel and at
//! the deadline; interims count in `interim_sent` BEFORE they are handed to the
//! queue and never touch the tracker's pending count.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ethers::types::H256;
use fabstir_llm_node::ltx::billing::LtxTracker;
use fabstir_llm_node::ltx::keepalive::start_keepalive;
use fabstir_llm_node::ltx::submit::ProofSubmitError;
use fabstir_llm_node::storage::s5_client::{MockS5Backend, S5Storage};
use serde_json::Value;
use tokio::sync::mpsc;

use super::keepalive_support::*;

const LATCH: Duration = Duration::from_secs(120);

#[tokio::test(start_paused = true)]
async fn a_two_hour_render_posts_every_900_s_first_the_interval_then_100_each_hash_fresh() {
    let t0 = tokio::time::Instant::now();
    let r = start(vec![], Duration::from_secs(3 * 3600));
    at(7_250, t0).await;
    // 900, 1800, …, 7200: never a silence above the 3600 s window
    assert_eq!(
        r.sub.claims(),
        vec![1000, 100, 100, 100, 100, 100, 100, 100]
    );
    let calls = r.sub.calls.lock().unwrap().clone();
    let mut hashes: Vec<[u8; 32]> = calls.iter().map(|c| c.1).collect();
    hashes.sort();
    hashes.dedup();
    assert_eq!(hashes.len(), 8, "a fresh proofHash every tick");
    // the attestation lives at the keep-alive path and carries the progress fields
    let bytes =
        r.s5.get("home/ltx/job_42-p1_keepalive_1.json")
            .await
            .unwrap();
    let att: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(att["i"], 1);
    assert_eq!(att["session"], "0xabc");
    assert!(att["steps"]["value"].is_u64() && att["steps"]["max"].is_u64());
    assert_eq!(r.guard.shared().interim_sent(), 1000 + 7 * 100);
    drop(r.rx);
}

#[tokio::test(start_paused = true)]
async fn dropping_the_guard_on_an_error_exit_stops_every_post() {
    // a watch error at t = 2,000 s: run's single-exit cleanup drops the guard
    let t0 = tokio::time::Instant::now();
    let r = start(vec![], Duration::from_secs(3 * 3600));
    at(2_000, t0).await;
    let Run { sub, guard, rx, .. } = r;
    drop(guard);
    at(7_250, t0).await;
    assert_eq!(sub.claims(), vec![1000, 100], "no proof after the exit");
    drop(rx);
}

#[tokio::test(start_paused = true)]
async fn a_closed_progress_channel_stops_the_ticks() {
    let t0 = tokio::time::Instant::now();
    let r = start(vec![], Duration::from_secs(3 * 3600));
    at(1_000, t0).await;
    drop(r.rx); // the renter is gone
    at(7_250, t0).await;
    assert_eq!(r.sub.claims(), vec![1000]);
    assert!(r.guard.is_finished(), "the scheduler ended on its own");
}

#[tokio::test(start_paused = true)]
async fn nothing_is_posted_after_the_deadline() {
    let t0 = tokio::time::Instant::now();
    let r = start(vec![], Duration::from_secs(2_000));
    at(7_250, t0).await;
    assert_eq!(r.sub.claims(), vec![1000, 100]);
    assert!(r.guard.is_finished());
    drop(r.rx);
}

#[tokio::test(start_paused = true)]
async fn interims_never_resolve_the_pending_so_the_disconnect_path_still_defers() {
    let t0 = tokio::time::Instant::now();
    let r = start(
        vec![
            Ok(()),
            Err(ProofSubmitError::Unconfirmed(H256::repeat_byte(3))),
        ],
        Duration::from_secs(3 * 3600),
    );
    r.tracker.mark_proof_pending(42, LATCH).await;
    at(1_850, t0).await;
    assert_eq!(r.sub.claims().len(), 2);
    assert_eq!(r.tracker.proofs_submitted(42).await, 1, "one CONFIRMED");
    assert!(r.tracker.has_pending(42).await, "the clip's pending stands");
    assert!(
        r.tracker.defer_completion(42).await,
        "a disconnect still defers"
    );
    drop(r.rx);
}

#[tokio::test(start_paused = true)]
async fn an_interim_that_failed_without_mining_is_followed_by_one_claiming_the_interval_again() {
    // "first" = no interim CONFIRMED yet (tick order would claim 100 → "Low first")
    let t0 = tokio::time::Instant::now();
    let r = start(
        vec![Err(ProofSubmitError::Unconfirmed(H256::repeat_byte(4)))],
        Duration::from_secs(3 * 3600),
    );
    at(2_750, t0).await;
    assert_eq!(r.sub.claims(), vec![1000, 1000, 100]);
    drop(r.rx);
}

#[tokio::test(start_paused = true)]
async fn every_interim_handed_to_the_queue_counts_even_one_that_erred() {
    // an Err interim may still mine: it counts (a final claimed from Ok
    // interims only could over-claim by it)
    let t0 = tokio::time::Instant::now();
    let mut r = start(
        vec![Err(ProofSubmitError::Unconfirmed(H256::repeat_byte(5)))],
        Duration::from_secs(3 * 3600),
    );
    at(1_000, t0).await;
    assert_eq!(r.guard.stop().await, 1000);
    drop(r.rx);
}

#[tokio::test(start_paused = true)]
async fn an_upload_failure_posts_nothing_and_the_next_tick_retries() {
    let t0 = tokio::time::Instant::now();
    let r = start(vec![], Duration::from_secs(3 * 3600));
    r.s5.inject_error(
        fabstir_llm_node::storage::s5_client::StorageError::NetworkError("s5 down".into()),
    )
    .await;
    at(1_000, t0).await;
    assert!(r.sub.claims().is_empty());
    assert_eq!(r.guard.shared().interim_sent(), 0);
    at(1_850, t0).await;
    assert_eq!(r.sub.claims(), vec![1000], "the next tick retries");
    drop(r.rx);
}

// ---- the stop races a tick that is mid-poll (multi-thread, real clock) ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_awaits_a_tick_that_is_mid_poll_before_the_final_read() {
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let s5 = Arc::new(GatedS5::blocking(entered_tx, release_rx));
    let sub = Interims::scripted(vec![]);
    let (tx, _rx) = mpsc::channel(64);
    let mut guard = start_keepalive(
        cfg(Duration::from_millis(50), Duration::from_secs(60)),
        s5,
        sub.clone(),
        LtxTracker::new(),
        tx,
        None,
        Arc::new(TokioClock(tokio::time::Instant::now())),
    );
    // the tick is now blocked inside S5 put (mid-poll)
    tokio::task::spawn_blocking(move || entered_rx.recv().unwrap())
        .await
        .unwrap();
    // a timer thread releases it only AFTER the stop has been called
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        let _ = release_tx.send(());
    });
    let read = guard.stop().await; // abort, THEN await the handle
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        guard.shared().interim_sent(),
        read,
        "nothing may add to interim_sent after the finalising read"
    );
    assert_eq!(read, 1000, "the racing tick was counted before the read");
}
