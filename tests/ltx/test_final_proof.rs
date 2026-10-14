// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Batch D21: the relight final proof. Its claim is the remainder
//! `tokens − max(on-chain delta, interim_sent)`; it is first sent at ≥ the
//! on-chain w9 + remainder ÷ rate; `RateLimited` waits and resends, `Reverted`
//! is judged by a read AT its receipt's block, `Unconfirmed` is polled (never
//! resent) to the budget; at most 3 sends, a re-read before each resend, and
//! no attempt that would end past the budget. Fake clock (paused tokio time).

use std::time::Duration;

use ethers::types::{Address, H256, U256};
use fabstir_llm_node::ltx::final_proof::final_remainder;
use fabstir_llm_node::ltx::submit::ProofSubmitError;
use fabstir_llm_node::ltx::template::check_session_terms;
use tokio::time::Instant;

use super::final_support::*;

// ---- the remainder ----

#[test]
fn the_remainder_subtracts_the_larger_of_the_on_chain_delta_and_interim_sent() {
    let t = TOKENS;
    let z = U256::zero();
    // an interim in flight when finalising begins (sent 1,100, 1,000 on chain)
    assert_eq!(final_remainder(t, z, U256::from(1000u64), 1100), t - 1100);
    // a re-read lagging before the last confirmed interim (0 on chain)
    assert_eq!(final_remainder(t, z, z, 1200), t - 1200);
    // a FOREIGN proof between the admission read and the re-read
    assert_eq!(
        final_remainder(
            t,
            U256::from(500u64),
            U256::from(500u64 + 1100 + 5000),
            1100
        ),
        t - 6100
    );
    // saturating
    assert_eq!(final_remainder(100, z, U256::from(5000u64), 0), 0);
    assert_eq!(
        final_remainder(100, U256::from(9u64), z, 0),
        100,
        "a delta never underflows"
    );
}

#[test]
fn the_finalising_gate_passes_an_exact_deposit_on_the_remainder() {
    // deposit exactly covers the job; 1,100 keep-alive tokens are on chain
    let s = snap(USED, W9);
    let host = Address::repeat_byte(0xB0);
    assert!(check_session_terms(&s, host, REMAINDER, 0).is_ok());
    assert!(
        check_session_terms(&s, host, TOKENS, 0).is_err(),
        "gated on the full tokens it would be SESSION_CLOSED"
    );
}

// ---- timing ----

#[tokio::test(start_paused = true)]
async fn the_first_final_send_waits_for_on_chain_w9_plus_remainder_over_rate() {
    // w9 = 10 s ago (an interim that returned Err mined late and moved it)
    let t0 = Instant::now();
    let sub = Final::new(t0, vec![]);
    let reads = Reads::default();
    let clock = Clock(t0);
    assert!(send(&sub, &rf(&reads, &clock, 10_000)).await);
    let calls = sub.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, REMAINDER);
    assert!(
        calls[0].1 >= Duration::from_secs(RW - 10),
        "sent at {:?}, before w9 + remainder ÷ rate",
        calls[0].1
    );
}

// ---- Reverted ----

#[tokio::test(start_paused = true)]
async fn a_status_zero_final_after_a_late_mined_interim_is_resent_from_the_new_w9_and_lands() {
    let t0 = Instant::now();
    let sub = Final::new(
        t0,
        vec![Err(ProofSubmitError::Reverted { block: 100 }), Ok(())],
    );
    let new_w9 = BASE_UNIX + 70; // the late interim mined at ≈ 70 s
    let reads = Reads::default()
        .with(Some(100), snap(USED + 100, new_w9))
        .with(None, snap(USED, W9)); // `latest` one block behind
    let clock = Clock(t0);
    assert!(send(&sub, &rf(&reads, &clock, 10_000)).await);
    let calls = sub.calls();
    assert_eq!(calls.len(), 2);
    assert!(calls[1].1 >= Duration::from_secs(70 + RW), "{calls:?}");
    // the check read AT the receipt's block, never a lagging `latest`
    assert_eq!(reads.asked.lock().unwrap().clone(), vec![Some(100)]);
}

#[tokio::test(start_paused = true)]
async fn a_reverted_final_whose_w9_did_not_move_is_forfeited() {
    let t0 = Instant::now();
    let sub = Final::new(t0, vec![Err(ProofSubmitError::Reverted { block: 100 })]);
    let reads = Reads::default().with(Some(100), snap(USED, W9));
    let clock = Clock(t0);
    assert!(!send(&sub, &rf(&reads, &clock, 10_000)).await);
    assert_eq!(sub.calls().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn after_a_reverted_attempt_a_read_showing_the_remainder_landed_stops_further_sends() {
    let t0 = Instant::now();
    let sub = Final::new(
        t0,
        vec![Err(ProofSubmitError::Reverted { block: 100 }), Ok(())],
    );
    let reads = Reads::default().with(Some(100), snap(USED + REMAINDER, BASE_UNIX + 70));
    let clock = Clock(t0);
    assert!(send(&sub, &rf(&reads, &clock, 10_000)).await, "it landed");
    assert_eq!(sub.calls().len(), 1, "never a second remainder");
}

// ---- RateLimited ----

#[tokio::test(start_paused = true)]
async fn after_a_rate_limited_first_attempt_a_latest_read_showing_the_remainder_landed_stops() {
    let t0 = Instant::now();
    let sub = Final::new(t0, vec![Err(ProofSubmitError::RateLimited), Ok(())]);
    let reads = Reads::default().with(None, snap(USED + REMAINDER, BASE_UNIX + 70));
    let clock = Clock(t0);
    assert!(send(&sub, &rf(&reads, &clock, 10_000)).await);
    assert_eq!(sub.calls().len(), 1);
    assert_eq!(
        reads.asked.lock().unwrap().clone(),
        vec![None],
        "read at latest"
    );
}

#[tokio::test(start_paused = true)]
async fn a_rate_limited_final_waits_remainder_over_rate_plus_5_and_resends() {
    let t0 = Instant::now();
    let sub = Final::new(t0, vec![Err(ProofSubmitError::RateLimited), Ok(())]);
    let reads = Reads::default().with(None, snap(USED, W9));
    let clock = Clock(t0);
    assert!(send(&sub, &rf(&reads, &clock, 10_000)).await);
    let calls = sub.calls();
    assert_eq!(calls.len(), 2);
    assert!(
        calls[1].1 - calls[0].1 >= Duration::from_secs(RW + 5),
        "{calls:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn a_retry_that_would_end_past_the_budget_is_not_started() {
    // first attempt: waits ≈ 66 s for w9 + 76, ends by 0 + 81 + 60 = 141 ≤ 150;
    // the retry would end at ≈ 66 + 81 + 60 = 207 > 150 → not started
    let t0 = Instant::now();
    let sub = Final::new(t0, vec![Err(ProofSubmitError::RateLimited), Ok(())]);
    let reads = Reads::default().with(None, snap(USED, W9));
    let clock = Clock(t0);
    assert!(!send(&sub, &rf(&reads, &clock, 150)).await, "forfeited");
    assert_eq!(sub.calls().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn at_most_three_sends() {
    let t0 = Instant::now();
    let sub = Final::new(
        t0,
        vec![
            Err(ProofSubmitError::RateLimited),
            Err(ProofSubmitError::RateLimited),
            Err(ProofSubmitError::RateLimited),
            Ok(()),
        ],
    );
    let reads = Reads::default().with(None, snap(USED, W9));
    let clock = Clock(t0);
    assert!(!send(&sub, &rf(&reads, &clock, 100_000)).await);
    assert_eq!(sub.calls().len(), 3);
}

#[tokio::test(start_paused = true)]
async fn other_errors_forfeit_at_once() {
    let t0 = Instant::now();
    let sub = Final::new(
        t0,
        vec![Err(ProofSubmitError::Other("nonce".into())), Ok(())],
    );
    let reads = Reads::default().with(None, snap(USED, W9));
    let clock = Clock(t0);
    assert!(!send(&sub, &rf(&reads, &clock, 100_000)).await);
    assert_eq!(sub.calls().len(), 1);
}

// ---- Unconfirmed ----

#[tokio::test(start_paused = true)]
async fn an_unconfirmed_final_that_mines_later_lands_exactly_once() {
    // the session reads stay unchanged until well after a second send would
    // have happened; only the SAME hash's receipt is polled
    let t0 = Instant::now();
    let h = H256::repeat_byte(0xA1);
    let sub = Final::new(t0, vec![Err(ProofSubmitError::Unconfirmed(h)), Ok(())])
        .receipt(h, 600, true, 300);
    let reads = Reads::default()
        .with(None, snap(USED, W9))
        .with(Some(300), snap(USED, W9));
    let clock = Clock(t0);
    assert!(send(&sub, &rf(&reads, &clock, 10_000)).await);
    assert_eq!(sub.calls().len(), 1, "exactly one submit_ltx_proof call");
}

#[tokio::test(start_paused = true)]
async fn an_unconfirmed_final_that_turns_status_zero_at_90_s_is_resent_once_and_lands() {
    // an Unconfirmed interim mined late (moving w9); the Unconfirmed final then
    // mines status 0 at 90 s → the Reverted rule → one resend from the new w9
    let t0 = Instant::now();
    let h = H256::repeat_byte(0xA2);
    let sub = Final::new(t0, vec![Err(ProofSubmitError::Unconfirmed(h)), Ok(())]).receipt(
        h,
        90 + RW,
        false,
        200,
    );
    let reads = Reads::default().with(Some(200), snap(USED + 100, BASE_UNIX + 80));
    let clock = Clock(t0);
    assert!(send(&sub, &rf(&reads, &clock, 10_000)).await);
    assert_eq!(sub.calls().len(), 2);
}

#[tokio::test(start_paused = true)]
async fn an_unconfirmed_final_whose_receipt_never_appears_is_forfeited_at_the_budget() {
    let t0 = Instant::now();
    let h = H256::repeat_byte(0xA3);
    let budget = 600;
    // a poll every 5 s to the budget, never more: polling past the cap FAILS
    // here (the mock asserts) instead of hanging
    let mut sub = Final::new(t0, vec![Err(ProofSubmitError::Unconfirmed(h)), Ok(())]);
    sub.max_polls = (budget / 5 + 5) as u32;
    let reads = Reads::default().with(None, snap(USED, W9));
    let clock = Clock(t0);
    let landed = tokio::time::timeout(
        Duration::from_secs(budget + 60),
        send(&sub, &rf(&reads, &clock, budget)),
    )
    .await
    .expect("returns by the budget, so ltx_complete goes before the helper's timeout");
    assert!(!landed);
    assert_eq!(sub.calls().len(), 1);
    assert!(t0.elapsed() <= Duration::from_secs(budget + 5));
    assert!(
        sub.polls.load(std::sync::atomic::Ordering::SeqCst) >= 2,
        "it did poll"
    );
}
