// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Split of `test_execute.rs` (the 400-line integration-file rule): the
//! failure/end-state rows; helpers live in the sibling module.

use std::time::Duration;

use fabstir_llm_node::training::accept::AttemptClaim;

use super::support::{fixture, line, make_deps, CountBehaviour, ScanBehaviour};
use super::support::{passing_snapshot, snapshot_started_secs_ago};
use super::test_execute::{
    finalise_line, good_sessions, run_execute, run_execute_with_snapshot, slice_line,
};

#[tokio::test]
async fn cancel_mid_run_reports_cancelled_with_settled_detail_and_completes() {
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let script = vec![
        slice_line(&h, 0, 10),
        slice_line(&h, 1, 600), // NON-final (3-slice schedule); post-cancel
        slice_line(&h, 2, 10),
        finalise_line(&h, 10),
        line(10, r#"{"event":"done"}"#),
    ];
    let out = run_execute(
        h,
        fx.manifest_cid.clone(),
        fx.manifest_sha256.clone(),
        script,
        vec![3, 3, 3],
        Duration::from_secs(60),
        Some(250),
        false,
    )
    .await;
    let error = out
        .frames
        .iter()
        .find(|f| f["type"] == "train_error")
        .expect("CANCELLED frame");
    assert_eq!(error["error"]["code"], "CANCELLED");
    assert_eq!(error["error"]["detail"]["settledSlices"], 1);
    assert_eq!(error["error"]["detail"]["billedTokens"], 3);
    assert!(error["error"]["detail"]["lastCheckpoint"]["manifestCID"].is_string());
    // A cancelled run still completes the session (its slices settled).
    assert_eq!(out.completer.count(), 1);
}

#[tokio::test]
async fn zero_slice_death_reports_sidecar_unavailable_and_settles_exactly_once() {
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    // The train stream dies immediately (no lines at all).
    let out = run_execute(
        h,
        fx.manifest_cid.clone(),
        fx.manifest_sha256.clone(),
        Vec::new(),
        vec![5, 4],
        Duration::from_secs(60),
        None,
        true,
    )
    .await;
    let error = out
        .frames
        .iter()
        .find(|f| f["type"] == "train_error")
        .expect("error frame");
    assert_eq!(error["error"]["code"], "SIDECAR_UNAVAILABLE");
    assert_eq!(error["error"]["detail"]["settledSlices"], 0);
    // The zero-settle ran; the k = 0 path must NOT also run the end-of-run
    // completion (exactly once overall).
    for _ in 0..100 {
        if out.completer.count() > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(out.completer.count(), 1, "exactly one completion for k = 0");
    assert_eq!(out.attempts.peek(42), AttemptClaim::SessionReused);
}

#[tokio::test]
async fn train_failed_run_still_completes_the_session() {
    // T4 round falsifiability gap 2: a k≥1 stream death must COMPLETE the
    // session (its landed revenue settles; the deposit remainder refunds) —
    // previously nothing pinned that, and an inverted settled_any stranded
    // escrow until max_duration.
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let script = vec![slice_line(&h, 0, 10)]; // dies after one settled slice
    let out = run_execute(
        h,
        fx.manifest_cid.clone(),
        fx.manifest_sha256.clone(),
        script,
        vec![5, 4],
        Duration::from_secs(60),
        None,
        true,
    )
    .await;
    let error = out
        .frames
        .iter()
        .find(|f| f["type"] == "train_error")
        .expect("error frame");
    assert_eq!(error["error"]["code"], "TRAIN_FAILED");
    assert_eq!(error["error"]["detail"]["settledSlices"], 1);
    assert_eq!(
        out.completer.count(),
        1,
        "a k>=1 failure must complete the session"
    );
    assert_eq!(out.attempts.peek(42), AttemptClaim::SessionReused);
}

#[tokio::test]
async fn no_proof_completion_waits_for_the_creation_floor() {
    // Round-1 F2: a k=0 CANCEL computed wait 0 (no proof ever landed), so
    // the completion reverted "Dispute wait" and, un-retried, stranded the
    // deposit. The CREATION floor (start + dispute + buffer) must gate it.
    //
    // Round-2 F1: this row's first version was VACUOUS — it used the
    // constant `NOW` fixture (~a year behind the container's real clock),
    // so `due.saturating_sub(real_now)` was 0 and a floor-less mutant was
    // indistinguishable. Round-3 F1: the REPLACEMENT was flaky (~2 runs in
    // 5) — BOTH sides truncate to whole seconds, so a +1 s margin makes the
    // honest wait bimodal {0 s, 1 s} depending on where the row starts
    // inside a second. The margin is now +4: start = real−100, dispute =
    // 104, buffer 0 → due = floor(start)+4 → the honest wait is 3–4 s for
    // any sub-second pre-floor work, phase-independent, while a floor-less
    // mutant returns in ~0.1–0.2 s. The generous assert floor (2.5 s) also
    // NARROWS the load-masking hole a bare lower bound leaves: only an
    // upper bound or a clock seam could close it.
    // No paused clock: the floor reads SystemTime::now().
    let fx = fixture(None).await;
    let mut sessions = good_sessions();
    sessions.dispute = 104;
    let h = make_deps(
        &fx,
        sessions,
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let script = vec![line(30_000, r#"{"event":"done"}"#)]; // idles; the cancel ends it
    let started = std::time::Instant::now();
    let out = run_execute_with_snapshot(
        h,
        fx.manifest_cid.clone(),
        fx.manifest_sha256.clone(),
        script,
        vec![5, 4],
        Duration::from_secs(60),
        // Round-4 R1: arm the cancel at 0, not 50 ms — `prepare_dataset`
        // never inspects the flag, so this is semantically identical but
        // removes a ~1.4x timing margin (if the pre-check prefix ever ran
        // under 50 ms the row would block on the silence timeout instead).
        Some(0), // k = 0: no proof ever lands
        true,
        snapshot_started_secs_ago(100),
        false,
    )
    .await;
    let elapsed = started.elapsed();
    let error = out
        .frames
        .iter()
        .find(|f| f["type"] == "train_error")
        .expect("CANCELLED frame");
    assert_eq!(error["error"]["code"], "CANCELLED");
    assert_eq!(out.completer.count(), 1, "the completion must still happen");
    assert!(
        elapsed >= Duration::from_millis(2500),
        "the completion must wait for creation+dispute+buffer; returned in {elapsed:?}"
    );
}

#[tokio::test]
async fn forfeit_then_death_is_k0_class_with_executed_detail() {
    // The C.1-vs-§3.7 corner: a slice EXECUTES but its proof forfeits, then
    // the stream dies. §3.7 keys on LANDED (0 → SIDECAR_UNAVAILABLE class +
    // zero-settle, full refund); C.1's detail keys on EXECUTED (1) — the
    // round-1 landed-only detail broke the frozen formula here.
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    h.proof
        .script
        .lock()
        .unwrap()
        .push_back(Err("unconfirmed".to_string()));
    let script = vec![slice_line(&h, 0, 10)]; // executes, forfeits, then dies
    let out = run_execute(
        h,
        fx.manifest_cid.clone(),
        fx.manifest_sha256.clone(),
        script,
        vec![5, 4],
        Duration::from_secs(60),
        None,
        true,
    )
    .await;
    let error = out
        .frames
        .iter()
        .find(|f| f["type"] == "train_error")
        .expect("error frame");
    assert_eq!(
        error["error"]["code"], "SIDECAR_UNAVAILABLE",
        "no money moved"
    );
    assert_eq!(
        error["error"]["detail"]["settledSlices"], 1,
        "k = EXECUTED (C.1)"
    );
    assert_eq!(error["error"]["detail"]["billedTokens"], 5);
    for _ in 0..100 {
        if out.completer.count() > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(out.completer.count(), 1, "the zero-settle, exactly once");
}

#[tokio::test]
async fn train_open_failure_consumes_settles_and_sends_the_terminal_last() {
    // Round-3 F2 root cause: NEITHER early-return path had any coverage —
    // the harness could not drive one. This drives the train-open failure
    // (sidecar answers 409 SLOT_BUSY after the dataset legs succeed) and
    // pins: the §3.7 CAPACITY mapping, the C.3 consuming settle, session
    // consumption, and TERMINAL-FRAME-LAST (the ordering round 2 fixed
    // half-way and round 3 completed).
    let fx = fixture(None).await;
    let h = make_deps(
        &fx,
        good_sessions(),
        ScanBehaviour::Cleared,
        CountBehaviour::Tokens(9),
    );
    let out = run_execute_with_snapshot(
        h,
        fx.manifest_cid.clone(),
        fx.manifest_sha256.clone(),
        Vec::new(),
        vec![5, 4],
        Duration::from_millis(20), // fast heartbeat: frames DO flow beforehand
        None,
        true,
        passing_snapshot(),
        true, // /v1/train → 409 SLOT_BUSY
    )
    .await;
    let terminal = out.frames.last().expect("at least the terminal frame");
    assert_eq!(
        terminal["type"], "train_error",
        "terminal frame must be LAST"
    );
    assert_eq!(
        terminal["error"]["code"], "CAPACITY",
        "SLOT_BUSY → CAPACITY (§3.7)"
    );
    // Round-9 F-R9-3, as in the GPU-busy row: this is consumed and
    // zero-completed, so it must not read as retry-safe to the SDK.
    assert_eq!(terminal["error"]["detail"]["reason"], "slotBusy");
    // Progress frames DID flow before it (so "last" is a real ordering claim).
    assert!(
        out.frames.iter().any(|f| f["type"] == "train_progress"),
        "the dataset legs must have emitted progress first"
    );
    // C.3: consumed + zero-settled even though it is a capacity back-off.
    for _ in 0..100 {
        if out.completer.count() > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(out.completer.count(), 1);
    assert_eq!(out.attempts.peek(42), AttemptClaim::SessionReused);
    // TD15: no plaintext survives.
    assert!(!out.staging_root.join("job-42").exists());
}
