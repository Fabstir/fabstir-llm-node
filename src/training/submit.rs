// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Settlement seams for training (IMPL TD1/T3.5).
//!
//! Proof submission reuses the LTX [`crate::ltx::submit::ProofSubmit`] trait
//! (per-slice direct submit — training is conversation-free, so the LLM
//! checkpoint manager's delta machinery is deliberately NOT used). This
//! module adds the missing half: session COMPLETION as a trait, so the
//! zero-proof settle (interface C.3) and the end-of-run settle are mockable —
//! `completeSessionJob` previously lived only on the concrete
//! `CheckpointManager`.

/// Completes a session on-chain (`completeSessionJob`). The production impl
/// wraps `CheckpointManager::complete_session_job`; tests record calls.
///
/// TIMING RULE (interface C.3): the contract's `"Dispute wait"` gates a host
/// completion relative to `lastProofTime`, initialised at session CREATION —
/// so every completion, including a fast terminal reject's zero-token settle,
/// must be scheduled **no earlier than sessionCreation + disputeWindow +
/// buffer**. Callers own that scheduling; implementations just submit.
#[async_trait::async_trait]
pub trait SessionComplete: Send + Sync {
    async fn complete_session(&self, job_id: u64) -> Result<(), String>;
}
