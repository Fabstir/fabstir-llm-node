// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The training run's billing/race machine (TD8 — the `LtxTracker` mirror,
//! simplified by training's shape: ONE run per session, slices sequential,
//! so `pending_count` is 0/1 and the slice loop is the only proof submitter).
//!
//! Billing law (interface B.2/C.1): the WIRE bill counts EXECUTED slices —
//! a forfeited slice's tokens still bill (its artifacts delivered; only its
//! on-chain revenue is lost). `settled_tokens` = Σ landed deltas (on-chain
//! truth); `billed_tokens` = Σ executed deltas (the `train_complete` /
//! `train_error` billing.tokens). The §B triple equality rides these fields.
//!
//! Completion race rules copied from the proven LTX machine: deferral (a
//! disconnect while a proof is in flight hands `completeSessionJob` to the
//! run task), the completing latch (an accept during a fresh completion is
//! refused), and the dispute-window wait from the last CONFIRMED proof.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct TrainRunInfo {
    pub job_id: u64,
    pub settled_tokens: u64,
    pub billed_tokens: u64,
    pub slices_submitted: u32,
    pub slices_forfeited: u32,
    pub pending_count: u32,
    pub last_confirmed_at: Option<Instant>,
    pub completion_deferred: bool,
    pub completing_since: Option<Instant>,
}

impl TrainRunInfo {
    fn new(job_id: u64) -> Self {
        TrainRunInfo {
            job_id,
            settled_tokens: 0,
            billed_tokens: 0,
            slices_submitted: 0,
            slices_forfeited: 0,
            pending_count: 0,
            last_confirmed_at: None,
            completion_deferred: false,
            completing_since: None,
        }
    }
}

#[derive(Default)]
pub struct TrainTracker {
    runs: Arc<RwLock<HashMap<u64, TrainRunInfo>>>,
}

impl TrainTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn info(&self, job_id: u64) -> Option<TrainRunInfo> {
        self.runs.read().await.get(&job_id).cloned()
    }

    /// A slice's proof outcome is now pending. Returns `false` (marking
    /// nothing) inside the completing latch — the LTX atomic accept gate:
    /// a slice accepted mid-completion would be settled under.
    pub async fn mark_slice_pending(&self, job_id: u64, latch: Duration) -> bool {
        let mut runs = self.runs.write().await;
        let entry = runs
            .entry(job_id)
            .or_insert_with(|| TrainRunInfo::new(job_id));
        if let Some(at) = entry.completing_since {
            if at.elapsed() < latch {
                return false;
            }
        }
        entry.pending_count += 1;
        entry.completion_deferred = false;
        true
    }

    /// The slice's proof tx CONFIRMED (receipt status 1): resolves the
    /// pending, lands the delta on-chain AND on the wire bill.
    pub async fn mark_slice_submitted(&self, job_id: u64, delta: u64) {
        let mut runs = self.runs.write().await;
        if let Some(entry) = runs.get_mut(&job_id) {
            entry.pending_count = entry.pending_count.saturating_sub(1);
            entry.settled_tokens += delta;
            entry.billed_tokens += delta;
            entry.slices_submitted += 1;
            entry.last_confirmed_at = Some(Instant::now());
        }
    }

    /// The slice's revenue is forfeited; its tokens still BILL (B.2 — the
    /// artifacts delivered) and the dispute clock is NOT touched (no tx).
    pub async fn mark_slice_forfeited(&self, job_id: u64, delta: u64) {
        let mut runs = self.runs.write().await;
        if let Some(entry) = runs.get_mut(&job_id) {
            entry.pending_count = entry.pending_count.saturating_sub(1);
            entry.billed_tokens += delta;
            entry.slices_forfeited += 1;
        }
    }

    /// Disconnect gate: `true` (deferral set) iff a proof is in flight —
    /// the run task then owns `completeSessionJob`.
    pub async fn defer_completion(&self, job_id: u64) -> bool {
        let mut runs = self.runs.write().await;
        match runs.get_mut(&job_id) {
            Some(entry) if entry.pending_count > 0 => {
                entry.completion_deferred = true;
                true
            }
            _ => false,
        }
    }

    pub async fn has_pending(&self, job_id: u64) -> bool {
        self.runs
            .read()
            .await
            .get(&job_id)
            .map(|e| e.pending_count > 0)
            .unwrap_or(false)
    }

    /// Deferred AND idle → clear the flag + arm the completing latch in the
    /// same lock (taking ownership IS the start of completing; the latch is
    /// never cleared — it self-expires).
    pub async fn take_deferred_if_idle(&self, job_id: u64) -> bool {
        let mut runs = self.runs.write().await;
        match runs.get_mut(&job_id) {
            Some(entry) if entry.completion_deferred && entry.pending_count == 0 => {
                entry.completion_deferred = false;
                entry.completing_since = Some(Instant::now());
                true
            }
            _ => false,
        }
    }

    /// Atomic pre-dispatch guard for a completion attempt: latch and go iff
    /// nothing is pending.
    pub async fn mark_completing_if_idle(&self, job_id: u64) -> bool {
        let mut runs = self.runs.write().await;
        let entry = runs
            .entry(job_id)
            .or_insert_with(|| TrainRunInfo::new(job_id));
        if entry.pending_count > 0 {
            return false;
        }
        entry.completing_since = Some(Instant::now());
        true
    }

    /// Dispute-window wait remaining since the last CONFIRMED proof; zero
    /// when none ever landed.
    pub async fn proof_wait_remaining(&self, job_id: u64, window_secs: u64) -> Duration {
        let runs = self.runs.read().await;
        match runs.get(&job_id).and_then(|e| e.last_confirmed_at) {
            Some(at) => Duration::from_secs(window_secs).saturating_sub(at.elapsed()),
            None => Duration::ZERO,
        }
    }
}
