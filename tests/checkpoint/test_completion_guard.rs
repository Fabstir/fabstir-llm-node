// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! SCI-B (session completion integrity, D1-D5): the held per-job latch, the
//! sticky settled set, and the drift-canaried chain status read.

use fabstir_llm_node::contracts::completion_guard::{
    session_status_for_completion, CompletionGuard,
};
use fabstir_llm_node::training::accept::SessionStatus;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

const FIXTURE_JOB: u64 = 931;

fn fixture() -> Vec<u8> {
    let hex_text = include_str!("../training_api/fixtures/sessionjobs_931.hex");
    hex::decode(hex_text.trim()).expect("fixture is hex")
}

fn with_status_byte(status: u8) -> Vec<u8> {
    let mut raw = fixture();
    raw[12 * 32 + 31] = status;
    raw
}

// --- the latch ---

#[tokio::test]
async fn a_second_owner_waits_while_the_first_holds_the_latch() {
    let guard = Arc::new(CompletionGuard::new());
    let first = guard.acquire(7).await;

    let second = {
        let guard = guard.clone();
        tokio::spawn(async move { guard.acquire(7).await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !second.is_finished(),
        "second owner must wait for the first"
    );

    drop(first);
    timeout(Duration::from_secs(2), second)
        .await
        .expect("second owner proceeds once the first releases")
        .expect("task joins");
}

#[tokio::test]
async fn one_jobs_completion_never_blocks_another_job() {
    let guard = CompletionGuard::new();
    let _held = guard.acquire(1).await;
    timeout(Duration::from_millis(500), guard.acquire(2))
        .await
        .expect("a different job's latch is independent");
}

#[tokio::test]
async fn the_latch_stays_held_across_a_long_await_in_a_spawned_task() {
    // The dispute-retry loop moves the latch into a detached task; it must keep
    // excluding other owners until that task ends, not until the caller returns.
    let guard = Arc::new(CompletionGuard::new());
    let latch = guard.acquire(9).await;
    let retry = tokio::spawn(async move {
        let _latch = latch;
        tokio::time::sleep(Duration::from_millis(300)).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        timeout(Duration::from_millis(100), guard.acquire(9))
            .await
            .is_err(),
        "no owner may enter while the retry task holds the latch"
    );
    retry.await.expect("retry task joins");
    timeout(Duration::from_millis(500), guard.acquire(9))
        .await
        .expect("latch is free once the retry task ends");
}

// --- the sticky set ---

#[tokio::test]
async fn nothing_is_settled_until_it_is_marked() {
    let guard = CompletionGuard::new();
    assert!(!guard.is_settled(5).await);
    guard.mark_settled(5).await;
    assert!(guard.is_settled(5).await);
    assert!(
        !guard.is_settled(6).await,
        "marking one job settles no other"
    );
}

#[tokio::test]
async fn the_second_owner_sees_the_first_owners_settlement() {
    // The job-1160 shape: two owners race; the one that waits must find the job
    // settled and skip, not send a second completion.
    let guard = Arc::new(CompletionGuard::new());
    let first = guard.acquire(1160).await;
    let second = {
        let guard = guard.clone();
        tokio::spawn(async move {
            let _latch = guard.acquire(1160).await;
            guard.is_settled(1160).await
        })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    guard.mark_settled(1160).await; // the first owner's status-1 receipt
    drop(first);
    let skipped = timeout(Duration::from_secs(2), second)
        .await
        .expect("second owner finishes")
        .expect("task joins");
    assert!(skipped, "the second owner must see the job settled");
}

// --- the chain status read (D3, D4, D5) ---

#[test]
fn the_pinned_live_return_reads_completed_for_its_own_job() {
    assert_eq!(
        session_status_for_completion(&fixture(), FIXTURE_JOB),
        Ok(SessionStatus::Completed)
    );
}

#[test]
fn each_status_word_is_read() {
    assert_eq!(
        session_status_for_completion(&with_status_byte(0), FIXTURE_JOB),
        Ok(SessionStatus::Active)
    );
    assert_eq!(
        session_status_for_completion(&with_status_byte(2), FIXTURE_JOB),
        Ok(SessionStatus::TimedOut)
    );
}

#[test]
fn a_return_for_another_job_is_unreadable_not_settled() {
    // D4's canary: word 0 must be the job id asked about.
    assert!(session_status_for_completion(&fixture(), FIXTURE_JOB + 1).is_err());
}

#[test]
fn an_unknown_status_is_unreadable() {
    assert!(session_status_for_completion(&with_status_byte(7), FIXTURE_JOB).is_err());
}

#[test]
fn a_short_return_is_unreadable() {
    let raw = fixture();
    assert!(session_status_for_completion(&raw[..17 * 32], FIXTURE_JOB).is_err());
}
