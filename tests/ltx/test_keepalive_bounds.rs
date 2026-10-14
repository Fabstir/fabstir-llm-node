// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Batch D21 round-1 review: the keep-alive's liveness and bounds. A stalled
//! S5 put or interim submit is bounded (the next tick still posts); an interim
//! still in flight at stop is flagged; the first tick follows the session's
//! remaining proof window; the interims never claim more than half the job.

use std::sync::Arc;
use std::time::Duration;

use ethers::types::{Address, H256, U256};
use fabstir_llm_node::ltx::billing::LtxTracker;
use fabstir_llm_node::ltx::keepalive::{keepalive_cfg_for, start_keepalive, KeepAliveIds};
use fabstir_llm_node::ltx::relight::AdmitCfg;
use fabstir_llm_node::ltx::submit::ProofSubmitError;
use fabstir_llm_node::training::accept::{SessionSnapshot, SessionStatus};
use tokio::sync::mpsc;

use super::keepalive_support::*;

const BASE_UNIX: u64 = 1_790_000_000;

#[tokio::test(start_paused = true)]
async fn an_s5_put_that_never_resolves_does_not_stop_the_next_tick() {
    let t0 = tokio::time::Instant::now();
    let sub = Interims::scripted(vec![]);
    let (tx, rx) = mpsc::channel(64);
    let _guard = start_keepalive(
        cfg(Duration::from_secs(900), Duration::from_secs(3 * 3600)),
        Arc::new(GatedS5::hanging()),
        sub.clone(),
        LtxTracker::new(),
        tx,
        None,
        Arc::new(TokioClock(t0)),
    );
    at(1_850, t0).await;
    assert_eq!(
        sub.claims(),
        vec![1000],
        "tick 2 posts though tick 1's put hung"
    );
    drop(rx);
}

#[tokio::test(start_paused = true)]
async fn an_interim_submit_that_never_resolves_is_counted_flagged_and_the_next_tick_posts() {
    let t0 = tokio::time::Instant::now();
    let r = start(vec![], Duration::from_secs(3 * 3600));
    r.sub
        .hang_first
        .store(true, std::sync::atomic::Ordering::SeqCst);
    at(1_000, t0).await;
    assert!(
        r.guard.shared().in_flight(),
        "handed to the queue, no result yet"
    );
    at(1_850, t0).await;
    // never confirmed ⇒ the next one claims the interval again
    assert_eq!(r.sub.claims(), vec![1000, 1000]);
    assert_eq!(
        r.guard.shared().interim_sent(),
        2000,
        "the stalled one counts"
    );
    assert!(!r.guard.shared().in_flight(), "tick 2 returned a result");
    drop(r.rx);
}

#[tokio::test(start_paused = true)]
async fn the_in_flight_flag_survives_a_stop_mid_submit() {
    let t0 = tokio::time::Instant::now();
    let mut r = start(vec![], Duration::from_secs(3 * 3600));
    r.sub
        .hang_first
        .store(true, std::sync::atomic::Ordering::SeqCst);
    at(1_000, t0).await;
    assert_eq!(r.guard.stop().await, 1000);
    assert!(r.guard.shared().in_flight());
    drop(r.rx);
}

#[tokio::test(start_paused = true)]
async fn the_interims_never_claim_more_than_half_the_job() {
    // every interim errs and mines late (never confirmed) ⇒ each claims the
    // interval again; the cap (tokens ÷ 2 = 2,500) stops the third
    let t0 = tokio::time::Instant::now();
    let mut c = cfg(Duration::from_secs(900), Duration::from_secs(3 * 3600));
    c.interim_cap = 5_000 / 2;
    let unconfirmed = |b| Err(ProofSubmitError::Unconfirmed(H256::repeat_byte(b)));
    let r = start_cfg((1..=12).map(unconfirmed).collect(), c);
    at(3 * 3600, t0).await;
    assert_eq!(r.sub.claims(), vec![1000, 1000]);
    assert!(r.guard.shared().interim_sent() <= 2_500);
    drop(r.rx);
}

fn admitted(window_left_at_admission: u64, admission_unix: u64) -> SessionSnapshot {
    SessionSnapshot {
        depositor: Address::zero(),
        attempt_address: Address::zero(),
        host: Address::repeat_byte(0xB0),
        payment_token: Address::zero(),
        deposit: U256::from(10_000_000u64),
        price_per_token: U256::from(3700u64),
        tokens_used: U256::zero(),
        max_duration: U256::from(14_400u64),
        start_time: U256::from(admission_unix - 5000),
        // the window (3600) runs out `window_left_at_admission` after admission
        last_proof_time: U256::from(admission_unix + window_left_at_admission - 3600),
        proof_interval: U256::from(1000u64),
        proof_timeout_window: U256::from(3600u64),
        status: SessionStatus::Active,
    }
}

fn ids() -> KeepAliveIds {
    KeepAliveIds {
        job_id: 42,
        job_tag: "42-p1".into(),
        session_id: "0xabc".into(),
        host: "0xb0".into(),
        model_id: "0x01".into(),
    }
}

#[tokio::test(start_paused = true)]
async fn a_session_admitted_with_1020_s_left_posts_its_first_interim_inside_that_window() {
    // admitted with 1,020 s of window left; staging took 200 s ⇒ 820 s left at
    // the keep-alive's start: a flat 900 s first tick would be too late
    let t0 = tokio::time::Instant::now();
    let snap = admitted(1020, BASE_UNIX - 200);
    let c = keepalive_cfg_for(
        ids(),
        &snap,
        &AdmitCfg::default(),
        "cosmos-passes-full",
        361,
        754_115,
        t0,
        BASE_UNIX,
    );
    assert_eq!(c.first_tick, Duration::from_secs(820 - 300));
    let r = start_cfg(vec![], c);
    at(820 - 300 + 5, t0).await;
    assert_eq!(
        r.sub.claims(),
        vec![1000],
        "posted with 300 s of the window to spare"
    );
    at(820 - 300 + 900 + 5, t0).await;
    assert_eq!(r.sub.claims().len(), 2, "then every interval");
    drop(r.rx);
}

#[test]
fn the_first_tick_is_the_interval_with_room_and_never_below_30_s() {
    let t0 = tokio::time::Instant::now();
    let cfg = AdmitCfg::default();
    let first = |left: u64| {
        keepalive_cfg_for(
            ids(),
            &admitted(left, BASE_UNIX),
            &cfg,
            "cosmos-passes-full",
            361,
            754_115,
            t0,
            BASE_UNIX,
        )
        .first_tick
    };
    assert_eq!(first(3600), Duration::from_secs(900));
    assert_eq!(first(1000), Duration::from_secs(700));
    assert_eq!(first(310), Duration::from_secs(30));
    assert_eq!(first(0), Duration::from_secs(30));
}

#[tokio::test(start_paused = true)]
async fn rate_limited_interims_broadcast_nothing_so_they_do_not_eat_the_cap() {
    // round 2: P = tokens ÷ 4 at the admission bound; two RateLimited firsts
    // are subtracted back (nothing was broadcast), so the third tick posts
    let t0 = tokio::time::Instant::now();
    let tokens: u64 = 4_000;
    let mut c = cfg(Duration::from_secs(900), Duration::from_secs(3 * 3600));
    c.proof_interval = tokens / 4;
    c.interim_cap = tokens / 2;
    let r = start_cfg(
        vec![
            Err(ProofSubmitError::RateLimited),
            Err(ProofSubmitError::RateLimited),
        ],
        c,
    );
    at(2_750, t0).await;
    assert_eq!(
        r.sub.claims(),
        vec![1000, 1000, 1000],
        "the third tick posts"
    );
    assert_eq!(
        r.guard.shared().interim_sent(),
        1000,
        "only the one that may mine"
    );
    drop(r.rx);
}

#[tokio::test(start_paused = true)]
async fn only_rate_limited_is_subtracted_other_failures_stay_counted() {
    // Unconfirmed and Other may have been broadcast: they keep counting
    let t0 = tokio::time::Instant::now();
    let r = start(
        vec![
            Err(ProofSubmitError::Other("nonce".into())),
            Err(ProofSubmitError::Unconfirmed(H256::repeat_byte(9))),
        ],
        Duration::from_secs(3 * 3600),
    );
    at(1_850, t0).await;
    assert_eq!(r.sub.claims(), vec![1000, 1000]);
    assert_eq!(r.guard.shared().interim_sent(), 2000);
    drop(r.rx);
}
