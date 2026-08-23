// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Wave B, part 1 (interface C.3 pipeline): the happy path, template rejects
//! (no consumption, no settle), A.3 rejects (consumption + zero-settle),
//! chain-read failure (retryable, nothing consumed), registry refusals, and
//! the PRECISE zero-settle timing under a paused tokio clock.
//!
//! Zero-`ProofSubmit` note: T3's pipeline has no proof path at all — the
//! structural zero. T4 adds the mock-`ProofSubmit` zero-call assert to these
//! rows when the slice loop exists.

use ethers::types::U256;
use fabstir_llm_node::training::core::{accept_and_prepare, CAPACITY, VALIDATION_FAILED};
use fabstir_llm_node::training::types::TrainingJob;

use super::support::{
    fixture, make_deps, model_id, passing_snapshot, CountBehaviour, Harness, MockSessions,
    ScanBehaviour, NOW, PRICE,
};

fn good_sessions() -> MockSessions {
    MockSessions {
        snapshot: Ok(passing_snapshot()),
        model: model_id(0xAA),
        dispute: 30,
    }
}

async fn settle_calls_after_wait(h: &Harness) -> usize {
    // The fixture session started 100 s ago with dispute 30 + buffer 45 —
    // the due time is already past, so a scheduled settle fires immediately.
    for _ in 0..100 {
        if h.completer.count() > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    h.completer.count()
}

#[tokio::test]
async fn happy_path_prepares_with_verified_price_and_schedule() {
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let prepared = accept_and_prepare(&h.deps, 42, &fx.job, NOW)
        .await
        .expect("the positive control must pass");
    assert_eq!(prepared.training_tokens, 9);
    assert_eq!(prepared.schedule, vec![9]); // B.1: total < sliceTokens → one slice
    assert_eq!(prepared.price_per_token, U256::from(PRICE)); // the ON-CHAIN price
    assert_eq!(prepared.verdict, "cleared");
    assert!(prepared.staged_dataset.ends_with("job-42/dataset.jsonl"));
    assert!(prepared.staged_dataset.exists());
    // No settle for a successful acceptance…
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(h.completer.count(), 0, "success must not schedule a settle");
    // …and the attempt is ACTIVE: a duplicate `train` mid-run rejects.
    let dup = accept_and_prepare(&h.deps, 42, &fx.job, NOW)
        .await
        .unwrap_err();
    assert_eq!(
        (dup.code, dup.reason),
        (VALIDATION_FAILED, Some("trainActive"))
    );
}

#[tokio::test]
async fn template_rejects_consume_and_settle_per_the_universal_rule() {
    // Realigned in the converge round: C.3's zero-settle rule is UNIVERSAL —
    // a funded session terminally rejected on template shape (the frozen A.4
    // allowlist-drift scenario) is consumed and settled, never left locked.
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );

    let mutations: Vec<(Box<dyn Fn(&mut TrainingJob)>, &str)> = vec![
        (
            Box::new(|j: &mut TrainingJob| j.template_id = "unknown-template".to_string()),
            "template",
        ),
        (
            Box::new(|j: &mut TrainingJob| j.template_hash = "0xwrong".to_string()),
            "hash",
        ),
        (Box::new(|j: &mut TrainingJob| j.hyper.rank = 7), "rank"),
        (
            Box::new(|j: &mut TrainingJob| j.hyper.seq_len = 999),
            "seqLen",
        ),
        (Box::new(|j: &mut TrainingJob| j.epochs = 99), "epochs"),
        (
            Box::new(|j: &mut TrainingJob| j.hyper.lr = "2e-4".to_string()),
            "lr",
        ),
        (
            Box::new(|j: &mut TrainingJob| j.output = "adapter-v2".to_string()),
            "output",
        ),
    ];
    let count = mutations.len();
    for (offset, (mutate, needle)) in mutations.into_iter().enumerate() {
        let job_id = 50 + offset as u64;
        let mut job = fx.job.clone();
        mutate(&mut job);
        let reject = accept_and_prepare(&h.deps, job_id, &job, NOW)
            .await
            .unwrap_err();
        assert_eq!(reject.code, VALIDATION_FAILED, "{needle}: {reject:?}");
        assert!(
            reject
                .detail
                .to_lowercase()
                .contains(&needle.to_lowercase()),
            "{needle} not named in {:?}",
            reject.detail
        );
        // Consumed: the SAME session now refuses with sessionReused.
        let again = accept_and_prepare(&h.deps, job_id, &fx.job, NOW + 3_600)
            .await
            .unwrap_err();
        assert_eq!(
            (again.code, again.reason),
            (VALIDATION_FAILED, Some("sessionReused")),
            "{needle} must consume the session"
        );
    }
    // Every template reject scheduled its zero-settle (due already past).
    for _ in 0..100 {
        if h.completer.count() >= count {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        h.completer.count(),
        count,
        "one settle per consumed session"
    );
}

#[tokio::test]
async fn total_tokens_over_template_ceiling_rejects() {
    let fx = fixture(None).await;
    let mut h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    h.deps.template.max_total_tokens = 8; // declared 9 × epochs 1 = 9 > 8
    let reject = accept_and_prepare(&h.deps, 51, &fx.job, NOW)
        .await
        .unwrap_err();
    assert_eq!(reject.code, VALIDATION_FAILED);
    assert!(
        reject.detail.contains("maxTotalTokens"),
        "{:?}",
        reject.detail
    );
}

#[tokio::test]
async fn a3_reject_consumes_settles_and_deletes_staging() {
    let fx = fixture(None).await;
    let mut sessions = good_sessions();
    if let Ok(snap) = &mut sessions.snapshot {
        snap.deposit = U256::zero(); // headroom gate fails
    }
    let h = make_deps(
        &fx,
        sessions,
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let reject = accept_and_prepare(&h.deps, 60, &fx.job, NOW)
        .await
        .unwrap_err();
    assert_eq!(
        (reject.code, reject.reason),
        (VALIDATION_FAILED, Some("sessionParams"))
    );
    assert!(reject.detail.contains("headroom"), "{:?}", reject.detail);
    // C.3: the zero-token settle fires (due time already past for the fixture).
    assert_eq!(
        settle_calls_after_wait(&h).await,
        1,
        "exactly one zero-settle"
    );
    assert_eq!(h.completer.calls.lock().unwrap()[0], 60);
    // TD15: no staging leftovers.
    assert!(!h.staging_dir.path().join("job-60").exists());
    // A.3 one-train-EVER: the session is consumed although still Active on-chain.
    let again = accept_and_prepare(&h.deps, 60, &fx.job, NOW + 3_600)
        .await
        .unwrap_err();
    assert_eq!(
        (again.code, again.reason),
        (VALIDATION_FAILED, Some("sessionReused"))
    );
}

#[tokio::test]
async fn chain_read_failure_is_retryable_capacity_nothing_consumed() {
    let fx = fixture(None).await;
    let sessions = MockSessions {
        snapshot: Err("rpc timeout".to_string()),
        model: model_id(0xAA),
        dispute: 30,
    };
    let h = make_deps(
        &fx,
        sessions,
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let reject = accept_and_prepare(&h.deps, 70, &fx.job, NOW)
        .await
        .unwrap_err();
    assert_eq!(
        reject.code, CAPACITY,
        "fail closed but RETRYABLE: {reject:?}"
    );
    // Round-8 F-R8-6: the SDK BRANCHES on this. It treats an unknown reason as
    // "session consumed, re-shop", against our written commitment that
    // `chainUnavailable` is the only reason that ever means nothing was
    // consumed. Inverting the vocabulary left the whole suite green, so an
    // inversion here would strand a funded deposit until timeout with nothing
    // failing. Assert the string, not just the code.
    assert_eq!(
        reject.reason,
        Some("chainUnavailable"),
        "a chain-read failure consumes and settles NOTHING: {reject:?}"
    );
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(
        h.completer.count(),
        0,
        "an unreadable session must never be settled"
    );
    // Not consumed: the same session proceeds once the chain answers. Build a
    // fresh harness (healthy reader) sharing the SAME registry.
    let h2_fx_job = fx.job.clone();
    let mut h2 = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    h2.deps.attempts = h.deps.attempts.clone();
    accept_and_prepare(&h2.deps, 70, &h2_fx_job, NOW)
        .await
        .expect("a read-failure reject must not consume the session");
}

#[tokio::test]
async fn same_address_second_session_is_capacity_while_first_runs() {
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    accept_and_prepare(&h.deps, 80, &fx.job, NOW)
        .await
        .expect("first run");
    // A DIFFERENT session, same depositor, while the first is Active (C.6):
    // CAPACITY — and per the universal rule the SECOND session is consumed
    // and settled (it is funded and terminally rejected).
    let reject = accept_and_prepare(&h.deps, 81, &fx.job, NOW)
        .await
        .unwrap_err();
    assert_eq!(reject.code, CAPACITY, "{reject:?}");
    // F-R8-6: this one IS funded and IS zero-completed, so it must NOT read as
    // chainUnavailable. The two demand opposite client behaviour.
    assert_eq!(reject.reason, Some("addressBusy"), "{reject:?}");
    for _ in 0..100 {
        if h.completer.count() > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(h.completer.calls.lock().unwrap().as_slice(), &[81]);
    let again = accept_and_prepare(&h.deps, 81, &fx.job, NOW + 3_600)
        .await
        .unwrap_err();
    assert_eq!(again.reason, Some("sessionReused"), "session 81 consumed");
    // The FIRST run is untouched by the refusal.
    assert!(h
        .staging_dir
        .path()
        .join("job-80")
        .join("dataset.jsonl")
        .exists());
}

#[tokio::test(start_paused = true)]
async fn zero_settle_waits_for_creation_plus_dispute_plus_buffer() {
    // A.3 reject path touches no network before the settle is scheduled, so
    // the paused clock is safe. Session created NOW−100; dispute 30 + buffer
    // 45… make the due time FUTURE: dispute 500 → due = NOW − 100 + 545 =
    // NOW + 445.
    let fx = fixture(None).await;
    let mut sessions = good_sessions();
    sessions.dispute = 500;
    if let Ok(snap) = &mut sessions.snapshot {
        snap.deposit = U256::zero();
    }
    let h = make_deps(
        &fx,
        sessions,
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let reject = accept_and_prepare(&h.deps, 90, &fx.job, NOW)
        .await
        .unwrap_err();
    assert_eq!(reject.code, VALIDATION_FAILED);
    // Let the spawned settle task poll once so its sleep registers at t0.
    for _ in 0..5 {
        tokio::task::yield_now().await;
    }
    // Well before the due time: nothing may fire.
    tokio::time::advance(std::time::Duration::from_secs(400)).await;
    tokio::task::yield_now().await;
    assert_eq!(
        h.completer.count(),
        0,
        "settle fired before creation+dispute+buffer"
    );
    // Past it: exactly one completion.
    tokio::time::advance(std::time::Duration::from_secs(60)).await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        h.completer.count(),
        1,
        "settle must fire once the wait elapses"
    );
}

// --- accept-time sidecar consult (B.6 + the capacity clause; round-1 F3) ---

#[tokio::test]
async fn sidecar_pin_skew_is_sidecar_unavailable_consumed_and_settled() {
    use super::support::{make_deps_with_sidecar, SidecarHealth};
    let fx = fixture(None).await;
    let h = make_deps_with_sidecar(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
        SidecarHealth::PinSkew,
    );
    let reject = accept_and_prepare(&h.deps, 120, &fx.job, NOW)
        .await
        .unwrap_err();
    assert_eq!(reject.code, "SIDECAR_UNAVAILABLE", "{reject:?}");
    assert!(reject.detail.contains("pin skew"), "{:?}", reject.detail);
    assert_eq!(settle_calls_after_wait(&h).await, 1);
    let again = accept_and_prepare(&h.deps, 120, &fx.job, NOW + 60)
        .await
        .unwrap_err();
    assert_eq!(again.reason, Some("sessionReused"));
}

#[tokio::test]
async fn sidecar_slot_busy_at_accept_is_capacity_consumed_and_settled() {
    use super::support::{make_deps_with_sidecar, SidecarHealth};
    let fx = fixture(None).await;
    let h = make_deps_with_sidecar(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
        SidecarHealth::SlotBusy,
    );
    let reject = accept_and_prepare(&h.deps, 121, &fx.job, NOW)
        .await
        .unwrap_err();
    assert_eq!(reject.code, CAPACITY, "{reject:?}");
    assert_eq!(reject.reason, Some("slotBusy"), "{reject:?}");
    assert!(reject.detail.contains("slot busy"), "{:?}", reject.detail);
    assert_eq!(settle_calls_after_wait(&h).await, 1);

    // Round-3 pin: a capacity back-off must NOT arm the cooldown. The
    // pipeline path re-hits the consult before the registry, so probe the
    // REGISTRY directly: a fresh same-depositor claim 30 s later must be Ok
    // (an armed clock would answer Cooldown).
    use fabstir_llm_node::training::accept::AttemptClaim;
    assert_eq!(
        h.deps
            .attempts
            .try_begin(122, super::support::addr(0xD1), NOW + 30, 60),
        AttemptClaim::Ok,
        "slot-busy must not have armed the depositor's cooldown"
    );
}

// --- settle retry on completer error (round-1 F8) ---

#[tokio::test(start_paused = true)]
async fn zero_settle_retries_after_completer_error() {
    let fx = fixture(None).await;
    let mut sessions = good_sessions();
    if let Ok(snap) = &mut sessions.snapshot {
        snap.deposit = U256::zero(); // A.3 reject → settle scheduled
    }
    let h = make_deps(
        &fx,
        sessions,
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    h.completer
        .fail_times
        .store(1, std::sync::atomic::Ordering::SeqCst);
    let reject = accept_and_prepare(&h.deps, 130, &fx.job, NOW)
        .await
        .unwrap_err();
    assert_eq!(reject.code, VALIDATION_FAILED);
    // Due time is past → first attempt fires promptly and FAILS…
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(
        h.completer.count(),
        0,
        "first attempt failed, none recorded"
    );
    // …then the 60 s backoff elapses and the retry lands.
    tokio::time::advance(std::time::Duration::from_secs(61)).await;
    for _ in 0..10 {
        tokio::task::yield_now().await;
    }
    assert_eq!(h.completer.count(), 1, "the retry must land the settle");
}

#[tokio::test]
async fn terminal_effects_twice_settle_exactly_once() {
    // Round-2 catch: concurrent consult failures ran the effects twice and
    // double-scheduled the settle. The registry transition is the key: the
    // second invocation must schedule NOTHING.
    use fabstir_llm_node::training::core::terminal_reject_effects;
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let snapshot = super::support::passing_snapshot();
    terminal_reject_effects(&h.deps, 140, &snapshot, NOW, true).await;
    terminal_reject_effects(&h.deps, 140, &snapshot, NOW, true).await;
    for _ in 0..100 {
        if h.completer.count() > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    // Settle-due is already past; give a duplicate every chance to fire too.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(h.completer.count(), 1, "exactly one settle per session");
}

/// T5.3 round-9 F-R9-2: the FIFTH echo in `validate_against_template`, five
/// lines below one that round 8 bounded, on the SAME field.
///
/// `lr_is_canonical` imposes no length bound (ASCII digits with at most one
/// interior dot), so a 200,000-digit `lr` is canonical, passes the earlier
/// gate, and lands in the pinned-list arm. This runs at accept step 3 — after
/// the chain reads but BEFORE the attempt claim and before the A.3 gates — so
/// no funding has been verified when it echoes.
///
/// Only reachable when a template pins `method.lrs`, which the sidecar supports
/// and enforces, and which the default harness leaves as None. That is why
/// four rounds of sweeping for this class walked past it.
#[tokio::test]
async fn a_long_lr_is_not_echoed_whole_when_the_template_pins_a_list() {
    let fx = fixture(None).await;
    let mut h = make_deps(
        &fx,
        MockSessions {
            snapshot: Ok(passing_snapshot()),
            model: model_id(0xAA),
            dispute: 30,
        },
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    // A template that pins its learning rates, which the default fixture does
    // not. Without this the arm is unreachable and the row proves nothing.
    h.deps.template.lrs = Some(vec!["0.0002".to_string()]);

    let mut job = fx.job.clone();
    job.hyper.lr = "0".repeat(200_000);

    let reject = accept_and_prepare(&h.deps, 205, &job, NOW)
        .await
        .unwrap_err();
    assert_eq!(reject.code, VALIDATION_FAILED, "{}", reject.code);
    assert!(
        reject.detail.len() < 1024,
        "the reject echoed {} bytes back before any funding was verified",
        reject.detail.len()
    );
    assert!(
        reject.detail.contains("pinned list"),
        "the bound must not cost the reason: {}",
        reject.detail
    );
}
