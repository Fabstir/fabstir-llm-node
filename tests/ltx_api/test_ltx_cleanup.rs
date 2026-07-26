// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! A.1(ii) — unit tests over `finish_ltx_task`, the single-exit cleanup that
//! A.0 extracted out of the LTX generation task.
//!
//! The panic path that reaches this cleanup through the real wiring is covered
//! in `test_ltx_panic.rs`; these pin the cleanup's own decisions.

use super::ltx_task_support::{mark_pending, pending_count};
use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::api::websocket::handlers::ltx::finish_ltx_task;
use std::sync::Arc;

/// The cleanup must NOT double-resolve a clip that `finalize_clip` already
/// resolved.
///
/// Falsifiability note: with a SINGLE in-flight clip this cannot fail, because
/// `mark_proof_forfeited` is `pending_count.saturating_sub(1)` on a count that
/// is already 0 — it passes with or without the `!pending_resolved` guard. Two
/// clips make a spurious decrement observable (2 → 1).
#[tokio::test]
async fn cleanup_does_not_double_resolve_an_already_resolved_clip() {
    let server = Arc::new(ApiServer::new_for_test());
    let jid = 4444u64;
    mark_pending(&server, jid).await;
    mark_pending(&server, jid).await;
    assert_eq!(pending_count(&server, jid).await, 2, "two clips in flight");

    // Clip A: finalize_clip already resolved this clip's proof.
    finish_ltx_task(&server, Some(jid), true, true).await;
    assert_eq!(
        pending_count(&server, jid).await,
        2,
        "an already-resolved clip must not decrement again — doing so would consume the OTHER \
         in-flight clip's pending mark and let the disconnect path settle under its proof"
    );

    // Clip B: exited before finalize_clip — exactly one forfeit.
    finish_ltx_task(&server, Some(jid), true, false).await;
    assert_eq!(
        pending_count(&server, jid).await,
        1,
        "an unresolved clip forfeits exactly one pending"
    );
}

/// A task that never marked pending must not forfeit — otherwise it would
/// consume a CONCURRENT clip's pending mark on the same session.
///
/// (The `job_id == None` arm is deliberately not asserted: forfeiting a job
/// with no tracker entry is a documented no-op, so no mutation of the cleanup
/// could make such an assertion fail. It would be decorative.)
#[tokio::test]
async fn cleanup_is_a_noop_without_a_pending_mark() {
    let server = Arc::new(ApiServer::new_for_test());
    let jid = 4545u64;
    mark_pending(&server, jid).await;

    finish_ltx_task(&server, Some(jid), false, false).await;
    assert_eq!(
        pending_count(&server, jid).await,
        1,
        "pending_marked=false must not forfeit another clip's pending"
    );
}

/// The cleanup's branch (b) — deferred completion — with no checkpoint manager
/// available, which is the only reachable shape of that branch here
/// (`ApiServer::new_for_test` leaves `checkpoint_manager` as `None`, and
/// `CheckpointManager::new` needs a live `Web3Client`).
///
/// The deferral must SURVIVE. Taking it without being able to complete would
/// clear `completion_deferred` and set the completing latch while nobody ever
/// calls `completeSessionJob` — stranding the escrow permanently, which is the
/// same failure this slice exists to prevent.
#[tokio::test]
async fn deferred_completion_survives_when_no_checkpoint_manager_exists() {
    let server = Arc::new(ApiServer::new_for_test());
    let jid = 4848u64;
    mark_pending(&server, jid).await;
    assert!(
        server.ltx_tracker().defer_completion(jid).await,
        "a disconnect while a proof is pending defers completion"
    );

    finish_ltx_task(&server, Some(jid), true, false).await;

    assert_eq!(
        pending_count(&server, jid).await,
        0,
        "the clip is still forfeited"
    );
    assert!(
        server.ltx_tracker().deferred_idle(jid).await,
        "with no checkpoint manager the deferral must not be consumed"
    );
}
