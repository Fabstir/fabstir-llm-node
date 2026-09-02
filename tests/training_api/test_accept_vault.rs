// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! FT1 D7: on a vault-paid session the C.6 attempt registry is keyed on the
//! backend-authorised CLIENT, not on the shared vault address. The pure key
//! function first, then the pipeline rows: two card customers on one vault
//! both accepted; the same customer twice is `addressBusy`; one customer's
//! consuming template-shape reject cools that customer and nobody else
//! (the key is set BEFORE the first consuming consult — the placement test).
//! Wallet sessions never carry a client, so their key stays the depositor
//! and the pre-existing rows are unchanged.

use fabstir_llm_node::training::accept::{attempt_address, AttemptClaim, AttemptRegistry};
use fabstir_llm_node::training::core::{accept_session_for_client, prepare_dataset, CAPACITY};
use fabstir_llm_node::training::types::TrainingJob;

use super::support::{
    addr, fixture, make_deps, model_id, passing_snapshot, CountBehaviour, MockSessions,
    ScanBehaviour, NOW,
};

// The registry's cooldown for the pure rows; the pipeline rows use the deps' value.
const COOLDOWN: u64 = 60;

fn vault_sessions() -> MockSessions {
    MockSessions {
        snapshot: Ok(passing_snapshot()),
        model: model_id(0xAA),
        dispute: 30,
    }
}

#[test]
fn key_is_the_depositor_without_a_client() {
    assert_eq!(attempt_address(addr(0xD1), None), addr(0xD1));
}

#[test]
fn key_is_the_client_when_the_init_verified_a_vault() {
    assert_eq!(attempt_address(addr(0xD1), Some(addr(0xC1))), addr(0xC1));
}

#[test]
fn the_vault_itself_connecting_keys_on_the_depositor() {
    // FC1.6 accepts client == depositor; the key is the same address either way.
    assert_eq!(attempt_address(addr(0xD1), Some(addr(0xD1))), addr(0xD1));
}

#[test]
fn registry_rows_are_per_client_under_one_vault() {
    // Pure registry view of the rule the pipeline rows below drive end to end.
    let reg = AttemptRegistry::new();
    let a = attempt_address(addr(0xD1), Some(addr(0xC1)));
    let b = attempt_address(addr(0xD1), Some(addr(0xC2)));
    assert_eq!(reg.try_begin(1, a, NOW, COOLDOWN), AttemptClaim::Ok);
    assert_eq!(reg.try_begin(2, b, NOW + 1, COOLDOWN), AttemptClaim::Ok);
    // Keyed on the vault, the second would have been AddressBusy:
    let vault_keyed = AttemptRegistry::new();
    assert_eq!(
        vault_keyed.try_begin(1, addr(0xD1), NOW, COOLDOWN),
        AttemptClaim::Ok
    );
    assert_eq!(
        vault_keyed.try_begin(2, addr(0xD1), NOW + 1, COOLDOWN),
        AttemptClaim::AddressBusy
    );
}

#[tokio::test]
async fn two_card_customers_on_one_vault_are_both_accepted() {
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        vault_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let a = accept_session_for_client(&h.deps, 90, &fx.job, NOW, Some(addr(0xC1)))
        .await
        .expect("customer A accepted");
    assert_eq!(a.snapshot.attempt_address, addr(0xC1));
    assert_eq!(
        a.snapshot.depositor,
        addr(0xD1),
        "the chain view is untouched"
    );
    // A DIFFERENT customer, same vault depositor, while A is Active: accepted,
    // not `addressBusy` (the vault-keyed behaviour this row exists to forbid).
    let b = accept_session_for_client(&h.deps, 91, &fx.job, NOW + 5, Some(addr(0xC2)))
        .await
        .expect("customer B accepted while A runs");
    assert_eq!(b.snapshot.attempt_address, addr(0xC2));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(h.completer.count(), 0, "nothing was consumed");
}

#[tokio::test]
async fn the_same_card_customer_twice_is_address_busy_and_consumed() {
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        vault_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    accept_session_for_client(&h.deps, 92, &fx.job, NOW, Some(addr(0xC1)))
        .await
        .expect("first run");
    let reject = accept_session_for_client(&h.deps, 93, &fx.job, NOW + 5, Some(addr(0xC1)))
        .await
        .unwrap_err();
    assert_eq!(reject.code, CAPACITY, "{reject:?}");
    assert_eq!(reject.reason, Some("addressBusy"), "{reject:?}");
    for _ in 0..100 {
        if h.completer.count() > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(h.completer.calls.lock().unwrap().as_slice(), &[93]);
}

#[tokio::test]
async fn one_customers_template_reject_does_not_cool_another() {
    // The placement test: the key must be set BEFORE the template consult, or
    // this consuming reject would arm the cooldown on the DEPOSITOR (the vault)
    // and lock every card customer out for TRAIN_ACCEPT_COOLDOWN_SECS.
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        vault_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let mut bad: TrainingJob = fx.job.clone();
    bad.hyper.rank = 7; // not in the template's allowed ranks → consuming template reject
    let reject = accept_session_for_client(&h.deps, 94, &bad, NOW, Some(addr(0xC1)))
        .await
        .unwrap_err();
    assert!(
        reject.reason != Some("addressBusy") && reject.reason != Some("cooldown"),
        "a template-shape reject, not a registry one: {reject:?}"
    );
    // Customer B, ten seconds later, inside A's cooldown window: accepted.
    let b = accept_session_for_client(&h.deps, 95, &fx.job, NOW + 10, Some(addr(0xC2)))
        .await
        .expect("B is not inside A's cooldown");
    assert_eq!(b.snapshot.attempt_address, addr(0xC2));
    // And A itself IS cooling (the rule still bites the right customer).
    let again = accept_session_for_client(&h.deps, 96, &fx.job, NOW + 10, Some(addr(0xC1)))
        .await
        .unwrap_err();
    assert_eq!(again.reason, Some("cooldown"), "{again:?}");
}

#[tokio::test]
async fn a_wallet_session_still_keys_on_the_depositor() {
    // No client: the pre-FT1 behaviour, byte for byte — the second session on
    // the same depositor is addressBusy while the first runs.
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        vault_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let a = accept_session_for_client(&h.deps, 97, &fx.job, NOW, None)
        .await
        .expect("wallet run");
    assert_eq!(a.snapshot.attempt_address, a.snapshot.depositor);
    let reject = accept_session_for_client(&h.deps, 98, &fx.job, NOW + 5, None)
        .await
        .unwrap_err();
    assert_eq!(reject.reason, Some("addressBusy"), "{reject:?}");
}

#[tokio::test]
async fn a_dataset_reject_after_accept_cools_the_client_not_the_vault() {
    // prepare_dataset's consuming rejects run terminal_reject_effects on the
    // ACCEPT-TIME snapshot, so the cooldown they arm must land on the client's
    // key. Mutation that turns this red: `terminal_reject_effects` (or the
    // struct update in accept) keying on `snapshot.depositor`.
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        vault_sessions(),
        ScanBehaviour::Flagged,
        CountBehaviour::Tokens(9),
    );
    let accepted = accept_session_for_client(&h.deps, 99, &fx.job, NOW, Some(addr(0xC1)))
        .await
        .expect("accepted before the scan");
    assert_eq!(accepted.snapshot.attempt_address, addr(0xC1));
    prepare_dataset(&h.deps, 99, &fx.job, &accepted, NOW, None)
        .await
        .expect_err("a flagged scan is a consuming reject");
    // Another card customer on the same vault is untouched by A's cooldown.
    accept_session_for_client(&h.deps, 100, &fx.job, NOW + 10, Some(addr(0xC2)))
        .await
        .expect("B is not inside A's cooldown");
    // And A is the one cooling.
    let again = accept_session_for_client(&h.deps, 101, &fx.job, NOW + 10, Some(addr(0xC1)))
        .await
        .unwrap_err();
    assert_eq!(again.reason, Some("cooldown"), "{again:?}");
}

#[tokio::test]
async fn a_completed_run_frees_the_client_key_not_the_vault() {
    // The handler's `finish` must release the SAME key `try_begin` took —
    // the client's. Mutation that turns this red: `finish` keyed on
    // `accepted.snapshot.depositor` (C1 would stay busy for the process
    // lifetime and every later session by that card customer would be
    // `addressBusy`, consumed and settled).
    use super::support::line;
    use super::test_execute::{finalise_line, run_execute_with_snapshot, slice_line};
    use std::time::Duration;

    let fx = fixture(None).await;
    // dispute 0: the completion path sleeps out the dispute window on the real
    // clock before `finish` (as every other execute row uses; see
    // test_execute::good_sessions), and 30 s here would only slow the gate.
    let h = make_deps(
        &fx,
        MockSessions {
            snapshot: Ok(passing_snapshot()),
            model: model_id(0xAA),
            dispute: 0,
        },
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let script = vec![
        slice_line(&h, 0, 10),
        slice_line(&h, 1, 10),
        finalise_line(&h, 10),
        line(10, r#"{"event":"done"}"#),
    ];
    // The accept-time claim the handler would have made, on the CLIENT key.
    let registry = h.deps.attempts.clone();
    assert_eq!(
        registry.try_begin(42, addr(0xC1), NOW, 60),
        AttemptClaim::Ok
    );
    let snapshot = fabstir_llm_node::training::accept::SessionSnapshot {
        attempt_address: addr(0xC1),
        ..passing_snapshot()
    };
    let out = run_execute_with_snapshot(
        h,
        fx.manifest_cid.clone(),
        fx.manifest_sha256.clone(),
        script,
        vec![5, 4],
        Duration::from_secs(60),
        None,
        false,
        snapshot,
        false,
    )
    .await;
    assert!(
        out.frames.iter().any(|f| f["type"] == "train_complete"),
        "the run completed: {:?}",
        out.frames
            .iter()
            .map(|f| f["type"].clone())
            .collect::<Vec<_>>()
    );
    // C1 is free again (a fresh session for the same card customer is accepted).
    assert_eq!(
        out.attempts.try_begin(43, addr(0xC1), NOW + 100, 60),
        AttemptClaim::Ok,
        "finish released the client key"
    );
}
