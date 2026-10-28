// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! SCI-C D20: transcode billing in steps, inside every contract limit.

use fabstir_llm_node::training::accept::SessionStatus;
use fabstir_llm_node::transcoder::proof_steps::{
    final_step, interim_amount, interim_due, session_view, steps_can_carry, transcode_accept,
    AcceptLimits, AcceptRefusal, FinalStep, SessionView, MIN_PROVEN_TOKENS,
};

const RATE: u64 = 2000; // the contract's default tokens per second

/// Session 1433's terms: 0.50 USDC deposit at price 5000 → 100,000 tokens.
fn session() -> SessionView {
    SessionView {
        status: SessionStatus::Active,
        deposit: 500_000,
        price_per_token: 5_000,
        tokens_used: 0,
        max_duration: 3_600,
        start_time: 1_000_000,
        last_proof_time: 1_000_000,
        proof_interval: 1_000,
        proof_timeout_window: 300,
    }
}

fn limits() -> AcceptLimits {
    AcceptLimits {
        min_proof_window_secs: 300,
        min_lifetime_secs: 600,
    }
}

// --- decoding ---

#[test]
fn the_pinned_live_return_decodes_with_its_own_job_id_only() {
    let raw =
        hex::decode(include_str!("../training_api/fixtures/sessionjobs_931.hex").trim()).unwrap();
    let view = session_view(&raw, 931).expect("decodes");
    assert_eq!(view.proof_interval, 1_000);
    assert_eq!(view.proof_timeout_window, 300);
    assert!(session_view(&raw, 932).is_err(), "drift canary");
}

// --- the contract's caps ---

#[test]
fn the_deposit_and_rate_caps() {
    let view = session();
    assert_eq!(view.deposit_room(), 100_000);
    assert_eq!(view.claimable(1_000_010, RATE), 20_000, "rate: 10 s × 2000");
    assert_eq!(view.claimable(1_000_100, RATE), 100_000, "deposit caps it");
    assert_eq!(
        view.minimum_proof(),
        1_000,
        "first proof meets proofInterval"
    );
    let later = SessionView {
        tokens_used: 5_000,
        ..view
    };
    assert_eq!(later.minimum_proof(), MIN_PROVEN_TOKENS);
    assert_eq!(later.deposit_room(), 95_000);
}

// --- accept ---

#[test]
fn accept_refuses_what_cannot_be_paid_safely() {
    let now = 1_000_100;
    assert_eq!(transcode_accept(&session(), now, &limits()), Ok(()));
    let settled = SessionView {
        status: SessionStatus::Completed,
        ..session()
    };
    assert_eq!(
        transcode_accept(&settled, now, &limits()),
        Err(AcceptRefusal::NotActive)
    );
    let short_window = SessionView {
        proof_timeout_window: 60,
        ..session()
    };
    assert_eq!(
        transcode_accept(&short_window, now, &limits()),
        Err(AcceptRefusal::ProofWindowTooShort)
    );
    assert_eq!(
        transcode_accept(&session(), 1_000_000 + 3_600 - 300, &limits()),
        Err(AcceptRefusal::LifetimeTooShort)
    );
    let spent = SessionView {
        tokens_used: 99_950,
        ..session()
    };
    assert_eq!(
        transcode_accept(&spent, now, &limits()),
        Err(AcceptRefusal::DepositExhausted)
    );
}

#[test]
fn the_estimate_must_fit_the_deposit_and_the_rate_before_expiry() {
    let now = 1_000_100;
    assert!(steps_can_carry(&session(), now, 28_875, RATE));
    assert!(
        steps_can_carry(&session(), now, 100_001, RATE),
        "beyond the deposit: runs, and bills what the deposit pays"
    );
    let rich = SessionView {
        deposit: 500_000_000,
        ..session()
    };
    let near_expiry = 1_000_000 + 3_600 - 1;
    assert!(
        !steps_can_carry(&rich, near_expiry, 50_000_000, RATE),
        "the rate cap cannot carry it in the time left"
    );
}

// --- interim proofs ---

#[test]
fn an_interim_proof_is_due_every_quarter_or_half_window() {
    assert!(!interim_due(10, 0, 100, 0, 300));
    assert!(interim_due(25, 0, 100, 0, 300), "first quarter");
    assert!(!interim_due(40, 25, 100, 0, 300));
    assert!(interim_due(50, 25, 100, 0, 300), "second quarter");
    assert!(interim_due(30, 25, 150, 0, 300), "half the proof window");
    assert!(
        !interim_due(25, 25, 1_000, 0, 300),
        "no new progress, nothing to prove"
    );
}

#[test]
fn interim_amounts_respect_every_minimum_and_leave_room_for_the_final() {
    let view = session();
    let now = 1_000_100;
    // First proof: must reach proofInterval, else defer.
    assert_eq!(interim_amount(&view, 0, 500, 28_875, now, RATE), None);
    assert_eq!(
        interim_amount(&view, 0, 7_218, 28_875, now, RATE),
        Some(7_218)
    );
    // Never so much that under MIN_PROVEN_TOKENS is left for the final proof.
    assert_eq!(
        interim_amount(&view, 0, 28_875, 28_875, now, RATE),
        Some(28_875 - MIN_PROVEN_TOKENS)
    );
    // Rate cap: only 2 s since the last proof → 4,000.
    assert_eq!(
        interim_amount(&view, 0, 20_000, 28_875, 1_000_002, RATE),
        Some(4_000)
    );
    // Later proofs: 100 minimum.
    let later = SessionView {
        tokens_used: 7_218,
        last_proof_time: 1_000_050,
        ..view
    };
    assert_eq!(
        interim_amount(&later, 7_218, 7_290, 28_875, now, RATE),
        None
    );
    assert_eq!(
        interim_amount(&later, 7_218, 14_437, 28_875, now, RATE),
        Some(7_219)
    );
}

// --- the final proof ---

#[test]
fn the_final_proof_claims_the_rest_within_the_caps() {
    let now = 1_000_100;
    let later = SessionView {
        tokens_used: 21_656,
        last_proof_time: 1_000_090,
        ..session()
    };
    assert_eq!(
        final_step(&later, 21_656, 28_875, now, RATE),
        FinalStep::Claim(7_219)
    );
    assert_eq!(
        final_step(&later, 28_875, 28_875, now, RATE),
        FinalStep::Done
    );
    // Rate-limited: 0 s since the last proof → wait.
    let just_proved = SessionView {
        last_proof_time: now,
        ..later
    };
    assert_eq!(
        final_step(&just_proved, 21_656, 28_875, now, RATE),
        FinalStep::Wait
    );
}

#[test]
fn a_small_job_on_a_fresh_session_bills_the_first_proof_minimum() {
    // Same minimum billing the old single checkpoint applied.
    assert_eq!(
        final_step(&session(), 0, 300, 1_000_100, RATE),
        FinalStep::Claim(1_000)
    );
}

#[test]
fn a_deposit_capped_job_bills_what_the_deposit_pays_then_is_done() {
    let nearly_spent = SessionView {
        tokens_used: 99_000,
        last_proof_time: 1_000_050,
        ..session()
    };
    assert_eq!(
        final_step(&nearly_spent, 99_000, 150_000, 1_000_100, RATE),
        FinalStep::Claim(1_000)
    );
    let spent = SessionView {
        tokens_used: 100_000,
        ..nearly_spent
    };
    assert_eq!(
        final_step(&spent, 100_000, 150_000, 1_000_100, RATE),
        FinalStep::Done
    );
}

#[test]
fn a_remainder_under_the_contract_minimum_is_done_not_waited_on() {
    let later = SessionView {
        tokens_used: 28_800,
        last_proof_time: 1_000_050,
        ..session()
    };
    assert_eq!(
        final_step(&later, 28_800, 28_875, 1_000_100, RATE),
        FinalStep::Done
    );
}
