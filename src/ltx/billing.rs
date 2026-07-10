// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Megapixel-frame billing for the LTX sidecar: cost estimation and per-job
//! token/cost tracking. Mirrors `transcoder::billing::TranscodingTracker`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ethers::types::U256;
use tokio::sync::RwLock;

use crate::ltx::submit::ltx_tokens;
use crate::ltx::types::LtxJob;

/// Minimum billable tokens floor (mirrors `checkpoint_manager::MIN_PROVEN_TOKENS`).
/// Any real clip clears this by orders of magnitude (121×1280×720 = 111,514).
pub const MIN_PROVEN_TOKENS: u64 = 100;

/// How long the accept path treats a dispatched `completeSessionJob` as in
/// flight (`is_completing`). Long enough to cover queue + confirmation + the
/// once-retry; short enough that a false latch (completion reported Ok but the
/// tx actually reverted) self-heals instead of wedging the session's LTX use.
pub const COMPLETING_LATCH_SECS: u64 = 120;

/// Estimate the on-chain cost of a job: `tokens × price_per_token`.
///
/// Straight multiply with no PRICE_PRECISION scaling — `price_per_token` already
/// carries the precision (see `pricing_constants`), exactly as the on-chain
/// `host_payment = (tokens × price_per_token) / PRICE_PRECISION` formula treats it.
/// `tokens` is the pinned megapixel-frame count from `submit::ltx_tokens`.
pub fn estimate_cost(job: &LtxJob, price_per_token: U256) -> U256 {
    let tokens = ltx_tokens(job.frames, job.resolution.w, job.resolution.h);
    U256::from(tokens) * price_per_token
}

/// Per-job LTX billing info (mirrors `transcoder::billing::TranscodingJobInfo`).
///
/// M1 economics adds per-job proof state. `pending_count` is a COUNT, not an
/// enum: `MAX_CONCURRENT_GENERATIONS` may exceed 1, so two clips of one session
/// can overlap — clip A's landed proof must not mask clip B's in-flight one
/// (settling early would revert B's proof).
#[derive(Debug, Clone)]
pub struct LtxJobInfo {
    pub job_id: u64,
    pub session_id: Option<String>,
    pub total_tokens: u64,
    pub total_cost: U256,
    pub generation_count: u32,
    pub last_update: Instant,
    /// Clips accepted whose proof outcome (submitted/forfeited) is unresolved.
    pub pending_count: u32,
    /// When the most recent proof tx CONFIRMED (drives the dispute-window wait).
    pub last_confirmed_at: Option<Instant>,
    /// A disconnect arrived while a proof was pending; the finishing generation
    /// task owns `completeSessionJob` once the count drains to 0.
    pub completion_deferred: bool,
    /// Landed proofs on this session (index 0 is gated by `proofInterval`).
    pub proofs_submitted: u32,
    /// When a `completeSessionJob` was dispatched for this session. The accept
    /// path rejects new clips while this is fresher than
    /// [`COMPLETING_LATCH_SECS`] — a clip accepted mid-completion would be
    /// settled under (its proof reverts, the clip delivers free).
    pub completing_since: Option<Instant>,
}

impl LtxJobInfo {
    fn new(job_id: u64, session_id: Option<String>) -> Self {
        Self {
            job_id,
            session_id,
            total_tokens: 0,
            total_cost: U256::zero(),
            generation_count: 0,
            last_update: Instant::now(),
            pending_count: 0,
            last_confirmed_at: None,
            completion_deferred: false,
            proofs_submitted: 0,
            completing_since: None,
        }
    }
}

/// Tracks LTX generation tokens/cost per job for billing.
pub struct LtxTracker {
    jobs: Arc<RwLock<HashMap<u64, LtxJobInfo>>>,
}

impl LtxTracker {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Record tokens and cost for a job, accumulating across generations.
    pub async fn track(&self, job_id: u64, session_id: Option<String>, tokens: u64, cost: U256) {
        let mut jobs = self.jobs.write().await;
        let entry = jobs
            .entry(job_id)
            .or_insert_with(|| LtxJobInfo::new(job_id, session_id.clone()));
        // Backfill: `mark_proof_pending` (at accept) creates the entry with no
        // session_id; the first `track` (at clip end) knows it.
        if entry.session_id.is_none() {
            entry.session_id = session_id;
        }
        entry.total_tokens += tokens;
        entry.total_cost += cost;
        entry.generation_count += 1;
        entry.last_update = Instant::now();
    }

    /// Get tracking info for a job.
    pub async fn get_job_info(&self, job_id: u64) -> Option<LtxJobInfo> {
        self.jobs.read().await.get(&job_id).cloned()
    }

    // -------------------------------------------------------------------------
    // M1 economics — proof-state race machine (see LtxJobInfo docs). Pending is
    // marked at ACCEPT (in the handler, before the spawn detaches) and resolved
    // on every path by `finalize_clip`/the spawn's single-exit cleanup.
    // -------------------------------------------------------------------------

    /// A clip was accepted: its proof outcome is now pending. Creates the entry
    /// if this is the session's first clip. Also CLEARS a stale deferral — a new
    /// clip on a reconnected session cancels it; this clip's own lifecycle now
    /// owns completion.
    ///
    /// ATOMIC ACCEPT GATE: returns `false` (and marks NOTHING) when a
    /// `completeSessionJob` was dispatched less than `completing_latch` ago —
    /// a clip accepted mid-completion would be settled under (its proof
    /// reverts, the clip delivers free, the session dies mid-use). Check and
    /// mark share one lock, so a completion cannot slip between them.
    pub async fn mark_proof_pending(&self, job_id: u64, completing_latch: Duration) -> bool {
        let mut jobs = self.jobs.write().await;
        let entry = jobs
            .entry(job_id)
            .or_insert_with(|| LtxJobInfo::new(job_id, None));
        if let Some(at) = entry.completing_since {
            if at.elapsed() < completing_latch {
                return false;
            }
        }
        entry.pending_count += 1;
        entry.completion_deferred = false;
        true
    }

    /// The clip's proof tx CONFIRMED (receipt status 1).
    pub async fn mark_proof_submitted(&self, job_id: u64) {
        let mut jobs = self.jobs.write().await;
        if let Some(entry) = jobs.get_mut(&job_id) {
            entry.pending_count = entry.pending_count.saturating_sub(1);
            entry.last_confirmed_at = Some(Instant::now());
            entry.proofs_submitted += 1;
        }
    }

    /// The clip's revenue is forfeited (error path, skipped submit, failed tx).
    /// Saturating and a NO-OP on a missing entry — must never underflow when a
    /// pending was never marked.
    pub async fn mark_proof_forfeited(&self, job_id: u64) {
        let mut jobs = self.jobs.write().await;
        if let Some(entry) = jobs.get_mut(&job_id) {
            entry.pending_count = entry.pending_count.saturating_sub(1);
        }
    }

    /// Disconnect gate: `true` (and the deferral flag set) iff a proof is in
    /// flight — the caller must then SKIP `completeSessionJob`; the finishing
    /// generation task completes instead. `false` for LLM-only sessions (no
    /// entry) and for idle LTX sessions.
    pub async fn defer_completion(&self, job_id: u64) -> bool {
        let mut jobs = self.jobs.write().await;
        match jobs.get_mut(&job_id) {
            Some(entry) if entry.pending_count > 0 => {
                entry.completion_deferred = true;
                true
            }
            _ => false,
        }
    }

    /// Read-only: a proof outcome is currently unresolved for this job. Used
    /// by the completion paths to re-check at wake after a dispute-window
    /// sleep — a clip accepted mid-sleep must not get settled under.
    pub async fn has_pending(&self, job_id: u64) -> bool {
        self.jobs
            .read()
            .await
            .get(&job_id)
            .map(|e| e.pending_count > 0)
            .unwrap_or(false)
    }

    /// Read-only peek of `take_deferred_if_idle` (does NOT clear the flag):
    /// completion was deferred and nothing is pending. The cleanup peeks
    /// before its dispute-window sleep and only TAKES at wake, so a clip
    /// accepted mid-sleep (which clears the flag at accept) transfers
    /// completion ownership to its own lifecycle.
    pub async fn deferred_idle(&self, job_id: u64) -> bool {
        self.jobs
            .read()
            .await
            .get(&job_id)
            .map(|e| e.completion_deferred && e.pending_count == 0)
            .unwrap_or(false)
    }

    /// `true` iff completion was deferred AND no proof is pending any more;
    /// clears the flag so the deferred completion runs exactly once — and SETS
    /// the completing latch in the same lock (taking ownership IS the start of
    /// completing; the accept gate then rejects new clips for the latch
    /// window). The latch is never cleared: it self-expires (a completion that
    /// failed silently cannot wedge the session forever).
    pub async fn take_deferred_if_idle(&self, job_id: u64) -> bool {
        let mut jobs = self.jobs.write().await;
        match jobs.get_mut(&job_id) {
            Some(entry) if entry.completion_deferred && entry.pending_count == 0 => {
                entry.completion_deferred = false;
                entry.completing_since = Some(Instant::now());
                true
            }
            _ => false,
        }
    }

    /// Atomic guard for the disconnect path's `completeSessionJob` dispatch:
    /// if no proof is pending, set the completing latch and return `true`;
    /// else return `false` — a clip is in flight and ITS lifecycle owns
    /// completion. Called immediately before each completion attempt (covers
    /// the zero-wait branch, the post-sleep wake and the retry).
    pub async fn mark_completing_if_idle(&self, job_id: u64) -> bool {
        let mut jobs = self.jobs.write().await;
        let entry = jobs
            .entry(job_id)
            .or_insert_with(|| LtxJobInfo::new(job_id, None));
        if entry.pending_count > 0 {
            return false;
        }
        entry.completing_since = Some(Instant::now());
        true
    }

    /// Read-only: a completion was dispatched less than `within` ago (the
    /// accept-gate condition inside `mark_proof_pending`, exposed for tests
    /// and observability).
    pub async fn is_completing(&self, job_id: u64, within: Duration) -> bool {
        self.jobs
            .read()
            .await
            .get(&job_id)
            .and_then(|e| e.completing_since)
            .map(|at| at.elapsed() < within)
            .unwrap_or(false)
    }

    /// Landed proofs on this session (the first-proof `proofInterval` gate).
    pub async fn proofs_submitted(&self, job_id: u64) -> u32 {
        self.jobs
            .read()
            .await
            .get(&job_id)
            .map(|e| e.proofs_submitted)
            .unwrap_or(0)
    }

    /// Time still to wait (of `window_secs` since the last CONFIRMED proof)
    /// before the host may call `completeSessionJob` without a "Dispute wait"
    /// revert. Zero when no proof ever landed (nothing gates completion).
    pub async fn proof_wait_remaining(&self, job_id: u64, window_secs: u64) -> Duration {
        let jobs = self.jobs.read().await;
        match jobs.get(&job_id).and_then(|e| e.last_confirmed_at) {
            Some(at) => Duration::from_secs(window_secs).saturating_sub(at.elapsed()),
            None => Duration::ZERO,
        }
    }
}

impl Default for LtxTracker {
    fn default() -> Self {
        Self::new()
    }
}
