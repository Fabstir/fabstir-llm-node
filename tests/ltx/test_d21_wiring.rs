// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Batch D21 round-1 review: `LtxGenerateTask::run`'s keep-alive and
//! final-proof wiring lives in pure functions, tested here —
//! `keepalive_cfg_for` (the keep-alive's inputs from the D20 admission
//! snapshot), `relight_final_inputs` (the claim and the final's budget from the
//! admission and finalising reads) and `final_policy` (relight ⇒ the relight
//! send rules). `test_relight_source.rs` pins only that `run` calls them.

use std::time::Duration;

use ethers::types::{Address, U256};
use fabstir_llm_node::ltx::final_proof::{
    final_claim, final_policy, relight_final_inputs, RelightFinalInputs, SubmitPolicy,
};
use fabstir_llm_node::ltx::keepalive::{keepalive_cfg_for, KeepAliveIds, SystemClock};
use fabstir_llm_node::ltx::relight::AdmitCfg;
use fabstir_llm_node::training::accept::{SessionSnapshot, SessionStatus};
use tokio::time::Instant;

use super::final_support::Reads;

const FULL: &str = "cosmos-passes-full";
const TOKENS: u64 = 754_115; // Full 361
const NOW: u64 = 1_790_000_000;

fn snap(used: u64, w9: u64, interval: u64) -> SessionSnapshot {
    SessionSnapshot {
        depositor: Address::zero(),
        attempt_address: Address::zero(),
        host: Address::repeat_byte(0xB0),
        payment_token: Address::zero(),
        deposit: U256::from(10_000_000u64),
        price_per_token: U256::from(3700u64),
        tokens_used: U256::from(used),
        max_duration: U256::from(14_400u64),
        start_time: U256::from(NOW - 60),
        last_proof_time: U256::from(w9),
        proof_interval: U256::from(interval),
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

// ---- keepalive_cfg_for ----

#[tokio::test]
async fn the_keepalive_takes_its_first_claim_from_the_admission_snapshot() {
    let t0 = Instant::now();
    let c = keepalive_cfg_for(
        ids(),
        &snap(0, NOW - 60, 1234),
        &AdmitCfg::default(),
        FULL,
        361,
        TOKENS,
        t0,
        NOW,
    );
    assert_eq!(c.proof_interval, 1234, "w10 from the D20 snapshot");
    assert_eq!((c.job_id, c.job_tag.as_str()), (42, "42-p1"));
}

#[tokio::test]
async fn the_keepalive_stops_at_the_whole_job_deadline_not_the_watch_timeout() {
    let t0 = Instant::now();
    let cfg = AdmitCfg::default();
    let c = keepalive_cfg_for(
        ids(),
        &snap(0, NOW - 60, 1000),
        &cfg,
        FULL,
        361,
        TOKENS,
        t0,
        NOW,
    );
    let deadline = cfg.deadline_for(FULL, 361);
    assert_ne!(deadline, cfg.watch_timeout_for(FULL, 361));
    assert_eq!(c.deadline_at, t0 + Duration::from_secs(deadline));
    assert_eq!(c.interval, Duration::from_secs(cfg.keepalive_secs));
    assert_eq!(c.interim_cap, TOKENS / 2);
}

// ---- relight_final_inputs ----

fn inputs(
    admission_used: u64,
    final_used: u64,
    interim_sent: u64,
    in_flight: bool,
) -> (RelightFinalInputs, Instant) {
    let t0 = Instant::now();
    // w10 1234 at admission (the final read's 1000 must not be used)
    let admission = snap(admission_used, NOW - 60, 1234);
    let fin = snap(final_used, NOW - 10, 1000);
    let i = relight_final_inputs(
        TOKENS,
        Some(&admission),
        &fin,
        interim_sent,
        in_flight,
        &AdmitCfg::default(),
        FULL,
        361,
        t0,
    );
    (i, t0)
}

#[tokio::test]
async fn a_foreign_proof_since_admission_is_subtracted_from_the_claim() {
    // 500 before admission; then 1,100 keep-alive + 5,000 from another clip
    let (i, _) = inputs(500, 500 + 1100 + 5000, 1100, false);
    assert_eq!(i.claim, TOKENS - 6100);
}

#[tokio::test]
async fn an_interim_in_flight_at_the_final_read_is_subtracted_from_the_claim() {
    // 1,000 on chain, 1,100 handed to the queue
    let (i, _) = inputs(0, 1000, 1100, false);
    assert_eq!(i.claim, TOKENS - 1100);
}

#[tokio::test]
async fn the_final_budget_is_deadline_plus_120_plus_the_rate_wait() {
    let (i, t0) = inputs(0, 1000, 1100, false);
    let cfg = AdmitCfg::default();
    let budget = cfg.deadline_for(FULL, 361) + 120 + TOKENS.div_ceil(10_000);
    assert_eq!(budget, 8_168 + 120 + 76);
    assert_eq!(i.budget_end, t0 + Duration::from_secs(budget));
    assert_eq!(i.rate_tokens_per_sec, 10_000);
    assert_eq!(i.final_w9, NOW - 10, "w9 from the finalising read");
    assert_eq!(i.final_used, U256::from(1000u64));
    assert_eq!(i.first_attempt_extra, Duration::ZERO);
    // round 2: the first-proof floor travels with the inputs (no chain read)
    assert_eq!(i.proof_interval, 1234, "w10 from the ADMISSION snapshot");
}

#[tokio::test]
async fn an_interim_in_flight_at_stop_adds_one_receipt_wait_to_the_first_attempt() {
    let (i, _) = inputs(0, 1000, 1100, true);
    assert_eq!(i.first_attempt_extra, Duration::from_secs(60));
}

// ---- final_policy ----

#[tokio::test]
async fn a_relight_final_goes_through_the_relight_rules_and_ltx_through_its_own() {
    let (i, _) = inputs(0, 1000, 1100, true);
    let reads = Reads::default();
    match final_policy(Some(&i), &reads, &SystemClock) {
        SubmitPolicy::Relight(rf) => {
            assert_eq!(rf.final_w9, i.final_w9);
            assert_eq!(rf.final_used, i.final_used);
            assert_eq!(rf.budget_end, i.budget_end);
            assert_eq!(rf.rate_tokens_per_sec, i.rate_tokens_per_sec);
            assert_eq!(rf.first_attempt_extra, i.first_attempt_extra);
            assert_eq!(rf.proof_interval, i.proof_interval);
        }
        SubmitPolicy::Ltx => panic!("a relight final must not take the LTX rules"),
    }
    assert!(matches!(
        final_policy(None, &reads, &SystemClock),
        SubmitPolicy::Ltx
    ));
}

// ---- final_claim (round 3: defence in depth) ----

#[tokio::test]
async fn the_ltx_policy_claim_also_subtracts_interim_sent() {
    // interims only run for relight jobs, so interim_sent is 0 on the Ltx
    // policy today; if the start condition ever regressed, the full-tokens
    // claim would over-claim by the interims
    assert_eq!(final_claim(TOKENS, None, 0), TOKENS);
    assert_eq!(final_claim(TOKENS, None, 1100), TOKENS - 1100);
    assert_eq!(final_claim(100, None, 5000), 0, "saturating");
    let (i, _) = inputs(500, 500 + 1100 + 5000, 1100, false);
    assert_eq!(
        final_claim(TOKENS, Some(&i), 1100),
        i.claim,
        "relight: the remainder"
    );
}
