// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Batch D21: admission at 0 tokens. On the D20 gate's snapshot a relight job
//! is refused `SESSION_TOO_SHORT` when the session cannot outlive its deadline
//! + 120 s + tokens ÷ rate, and `SESSION_TERMS` when its terms would let a
//! keep-alive claim most of the job or let the session time out mid-render.

use ethers::types::{Address, U256};
use fabstir_llm_node::ltx::relight::{relight_session_admission, AdmitCfg};
use fabstir_llm_node::ltx::submit::ltx_tokens;
use fabstir_llm_node::training::accept::{SessionSnapshot, SessionStatus};

const NOW: u64 = 1_790_000_000;
const FULL: &str = "cosmos-passes-full";

fn cfg() -> AdmitCfg {
    AdmitCfg::from_vars(&|_| None).unwrap().0
}

/// A session opened `age` seconds ago for `max_duration` seconds, no proof yet.
fn session(max_duration: u64, age: u64) -> SessionSnapshot {
    SessionSnapshot {
        depositor: Address::repeat_byte(0xD1),
        attempt_address: Address::repeat_byte(0xD1),
        host: Address::repeat_byte(0xB0),
        payment_token: Address::repeat_byte(0xEC),
        deposit: U256::from(10_000_000u64),
        price_per_token: U256::from(3700u64),
        tokens_used: U256::zero(),
        max_duration: U256::from(max_duration),
        start_time: U256::from(NOW - age),
        last_proof_time: U256::from(NOW - age),
        proof_interval: U256::from(1000u64),
        proof_timeout_window: U256::from(3600u64),
        status: SessionStatus::Active,
    }
}

fn admit(frames: u32, snap: &SessionSnapshot) -> Result<(), (&'static str, String)> {
    let tokens = ltx_tokens(frames, 1920, 1088);
    relight_session_admission(&cfg(), FULL, frames, tokens, snap, NOW)
}

#[test]
fn full_145_on_a_3600_s_session_opened_60_s_ago_is_admitted() {
    // 3200 + 120 + ceil(302,900 ÷ 10,000) = 3351 ≤ 3540 — today's helpers keep working
    assert_eq!(ltx_tokens(145, 1920, 1088), 302_900);
    assert_eq!(admit(145, &session(3600, 60)), Ok(()));
}

#[test]
fn full_361_on_a_3600_s_session_is_too_short() {
    let (code, msg) = admit(361, &session(3600, 60)).unwrap_err();
    assert_eq!(code, "SESSION_TOO_SHORT", "{msg}");
}

#[test]
fn full_361_on_a_14400_s_session_is_admitted() {
    assert_eq!(admit(361, &session(14_400, 60)), Ok(()));
}

#[test]
fn the_boundary_is_the_final_proof_budget() {
    let budget = cfg().final_budget_secs(FULL, 145, ltx_tokens(145, 1920, 1088));
    assert_eq!(budget, 3351);
    // exactly the budget left: admitted; one second less: refused
    assert_eq!(admit(145, &session(3600, 3600 - budget)), Ok(()));
    let (code, _) = admit(145, &session(3600, 3600 - budget + 1)).unwrap_err();
    assert_eq!(code, "SESSION_TOO_SHORT");
    // a session already past its lifetime never underflows into "plenty left"
    let (code, _) = admit(145, &session(3600, 7200)).unwrap_err();
    assert_eq!(code, "SESSION_TOO_SHORT");
}

#[test]
fn a_proof_interval_above_a_quarter_of_the_job_is_refused_terms() {
    let tokens = ltx_tokens(145, 1920, 1088);
    let mut s = session(3600, 60);
    s.proof_interval = U256::from(tokens / 4);
    assert_eq!(admit(145, &s), Ok(()), "exactly a quarter is allowed");
    s.proof_interval = U256::from(tokens / 4 + 1);
    let (code, msg) = admit(145, &s).unwrap_err();
    assert_eq!(code, "SESSION_TERMS", "{msg}");
}

#[test]
fn above_145_frames_a_300_s_window_is_refused_terms() {
    let mut s = session(14_400, 60);
    s.proof_timeout_window = U256::from(300u64);
    let (code, msg) = admit(361, &s).unwrap_err();
    assert_eq!(code, "SESSION_TERMS", "{msg}");
    // window = 2 × keep-alive is the floor
    s.proof_timeout_window = U256::from(1800u64);
    assert_eq!(admit(361, &s), Ok(()));
    s.proof_timeout_window = U256::from(1799u64);
    assert_eq!(admit(361, &s).unwrap_err().0, "SESSION_TERMS");
    // ≤ 145 frames on a 300 s window keeps today's exposure (accepted)
    let mut short = session(3600, 60);
    short.proof_timeout_window = U256::from(300u64);
    assert_eq!(admit(145, &short), Ok(()));
}

#[test]
fn above_145_frames_a_session_near_its_timeout_is_refused_terms() {
    // last proof 2,700 s ago on a 3,600 s window: 900 s left < 900 + 120
    let mut s = session(14_400, 6000);
    s.last_proof_time = U256::from(NOW - 2700);
    let (code, msg) = admit(361, &s).unwrap_err();
    assert_eq!(code, "SESSION_TERMS", "{msg}");
    // 1,020 s left: admitted
    s.last_proof_time = U256::from(NOW - 2580);
    assert_eq!(admit(361, &s), Ok(()));
    // no proof yet: measured from startTime (w9 may read 0)
    let mut fresh = session(14_400, 2700);
    fresh.last_proof_time = U256::zero();
    assert_eq!(admit(361, &fresh).unwrap_err().0, "SESSION_TERMS");
}
