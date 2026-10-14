// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Batch D21: keep-alive proofs for long relight renders. The contract's proof
//! window is frozen at 3600 s, so a render longer than the window is kept alive
//! by the node proving DURING it: every `RELIGHT_KEEPALIVE_SECS` (≤ 900, four
//! chances per window) a small progress attestation is uploaded to S5 and posted
//! through [`ProofSubmit::submit_ltx_proof`] as an interim proof.
//!
//! Rules (IMPLEMENTATION-BATCH-MODE D21):
//! - started in `LtxGenerateTask::run` after `client.submit`, owned by a
//!   [`KeepAliveGuard`] declared OUTSIDE the core and stopped by `run`'s
//!   single-exit cleanup (after `catch_unwind`) — so it ends on every exit; its
//!   `Drop` aborts too;
//! - on the success path it is stopped before the finalising re-read by
//!   `abort()` FOLLOWED BY awaiting the `JoinHandle` ([`KeepAliveGuard::stop`]):
//!   a tick running on another worker could otherwise still add to
//!   `interim_sent` after the final claim read it. Nothing waits for an
//!   in-flight transaction (the final claim is safe whatever it does);
//! - never posts after the job's deadline; each tick first checks the progress
//!   channel (the renter gone ⇒ stop and interrupt the sidecar);
//! - each claim is added to `interim_sent` BEFORE it is handed to the tx queue;
//!   a failed interim never ends the job (the next tick retries);
//! - "first" = no interim CONFIRMED yet (the tracker's count): the first claims
//!   `max(100, proofInterval)`, later ones `MIN_PROVEN_TOKENS`;
//! - a confirmed interim uses [`LtxTracker::mark_interim_confirmed`], never
//!   `mark_proof_submitted` (whose pending decrement would let the disconnect
//!   path complete the session mid-render).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use ethers::types::U256;

use crate::ltx::billing::{LtxTracker, MIN_PROVEN_TOKENS};
use crate::ltx::relight::AdmitCfg;
use crate::ltx::submit::{ProofSubmit, ProofSubmitError};
use crate::ltx::ComfyClient;
use crate::storage::s5_client::S5Storage;
use crate::training::accept::SessionSnapshot;

/// The wall clock in unix seconds (chain time is compared against it).
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Unix time as the keep-alive and the final proof see it (tests drive it from
/// tokio's paused clock).
pub trait UnixClock: Send + Sync {
    fn now_unix(&self) -> u64;
}

/// The real wall clock.
pub struct SystemClock;

impl UnixClock for SystemClock {
    fn now_unix(&self) -> u64 {
        unix_now()
    }
}

/// State the scheduler shares with `run`: the tokens of every interim handed to
/// the queue, and the render's step progress (updated by the watch loop).
#[derive(Debug, Default)]
pub struct KeepAliveShared {
    interim_sent: AtomicU64,
    in_flight: AtomicBool,
    step_value: AtomicU64,
    step_max: AtomicU64,
}

impl KeepAliveShared {
    /// Every interim ever handed to the queue (mined, pending, evicted or
    /// unknown) — the final claim subtracts it.
    pub fn interim_sent(&self) -> u64 {
        self.interim_sent.load(Ordering::SeqCst)
    }

    /// An interim was handed to the queue and no result came back yet (set before
    /// the submit, cleared only when it RETURNS — a timed-out or aborted one may
    /// still sit in the serial tx queue, ahead of the final proof).
    pub fn in_flight(&self) -> bool {
        self.in_flight.load(Ordering::SeqCst)
    }

    /// The watch loop's latest `{value, max}` sampler step.
    pub fn set_progress(&self, value: u64, max: u64) {
        self.step_value.store(value, Ordering::SeqCst);
        self.step_max.store(max, Ordering::SeqCst);
    }
}

/// One job's keep-alive parameters.
#[derive(Debug, Clone)]
pub struct KeepAliveCfg {
    pub job_id: u64,
    /// The per-clip `{jobId}-{promptId}` tag (the attestation path).
    pub job_tag: String,
    pub session_id: String,
    pub host: String,
    pub model_id: String,
    /// `RELIGHT_KEEPALIVE_SECS` (300–900).
    pub interval: Duration,
    /// When the first tick fires after the start ([`keepalive_cfg_for`]: before
    /// the admitted session's proof window runs out); later ticks every `interval`.
    pub first_tick: Duration,
    /// `started + deadline_for(preset, frames)`: nothing is posted after it.
    pub deadline_at: tokio::time::Instant,
    /// The session's `proofInterval` (w10 from the D20 snapshot): the first claim.
    pub proof_interval: u64,
    /// The interims never claim more than this in total (the job's tokens ÷ 2).
    pub interim_cap: u64,
}

/// The first tick fires at least this long after the start.
pub const FIRST_TICK_FLOOR_SECS: u64 = 30;
/// … and this long before the admitted session's proof window runs out.
pub const FIRST_TICK_MARGIN_SECS: u64 = 300;

/// The keep-alive's identity fields (what its attestation names).
#[derive(Debug, Clone)]
pub struct KeepAliveIds {
    pub job_id: u64,
    pub job_tag: String,
    pub session_id: String,
    pub host: String,
    pub model_id: String,
}

/// Batch D21: the keep-alive's inputs, from the D20 admission snapshot and the
/// relight config (`run` calls this; tested on its own). `started_at` is the
/// job's start on tokio's clock; `now_unix` the wall clock at the keep-alive's
/// start. The first tick is `min(interval, until the proof window − 300 s)`,
/// at least 30 s; the deadline is the WHOLE-job deadline; the first claim is
/// the session's w10; the interims are capped at half the job.
#[allow(clippy::too_many_arguments)]
pub fn keepalive_cfg_for(
    ids: KeepAliveIds,
    admission: &SessionSnapshot,
    cfg: &AdmitCfg,
    template_id: &str,
    frames: u32,
    tokens: u64,
    started_at: tokio::time::Instant,
    now_unix: u64,
) -> KeepAliveCfg {
    let as_u64 = |v: U256| v.min(U256::from(u64::MAX)).as_u64();
    let since = as_u64(admission.last_proof_time.max(admission.start_time));
    let window_ends = since.saturating_add(as_u64(admission.proof_timeout_window));
    let until_margin = window_ends
        .saturating_sub(FIRST_TICK_MARGIN_SECS)
        .saturating_sub(now_unix);
    let interval = Duration::from_secs(cfg.keepalive_secs);
    let first_tick = interval
        .min(Duration::from_secs(until_margin))
        .max(Duration::from_secs(FIRST_TICK_FLOOR_SECS));
    KeepAliveCfg {
        job_id: ids.job_id,
        job_tag: ids.job_tag,
        session_id: ids.session_id,
        host: ids.host,
        model_id: ids.model_id,
        interval,
        first_tick,
        deadline_at: started_at + Duration::from_secs(cfg.deadline_for(template_id, frames)),
        proof_interval: as_u64(admission.proof_interval),
        interim_cap: tokens / 2,
    }
}

/// Owns the scheduler task: [`KeepAliveGuard::stop`] on the success path, and
/// `Drop` (abort) on every other exit.
pub struct KeepAliveGuard {
    handle: Option<JoinHandle<()>>,
    shared: Arc<KeepAliveShared>,
}

impl KeepAliveGuard {
    pub fn shared(&self) -> Arc<KeepAliveShared> {
        self.shared.clone()
    }

    /// The task has ended (stopped, deadline, renter gone).
    pub fn is_finished(&self) -> bool {
        self.handle.as_ref().is_none_or(|h| h.is_finished())
    }

    /// Stop: `abort()` THEN await the handle (a tick mid-poll on another worker
    /// finishes its poll first, so nothing adds to `interim_sent` after this
    /// returns), then read `interim_sent`. Idempotent.
    pub async fn stop(&mut self) -> u64 {
        if let Some(h) = self.handle.take() {
            h.abort();
            let _ = h.await;
        }
        self.shared.interim_sent()
    }
}

impl Drop for KeepAliveGuard {
    fn drop(&mut self) {
        if let Some(h) = self.handle.take() {
            h.abort();
        }
    }
}

/// Start the scheduler (relight jobs with a session only). The first tick is
/// `first_tick` after the start, then one every `interval`.
pub fn start_keepalive(
    cfg: KeepAliveCfg,
    s5: Arc<dyn S5Storage>,
    submitter: Arc<dyn ProofSubmit>,
    tracker: LtxTracker,
    progress_tx: mpsc::Sender<Value>,
    interrupt: Option<Arc<ComfyClient>>,
    clock: Arc<dyn UnixClock>,
) -> KeepAliveGuard {
    let shared = Arc::new(KeepAliveShared::default());
    let task_shared = shared.clone();
    let handle = tokio::spawn(async move {
        run_ticks(
            cfg,
            s5,
            submitter,
            tracker,
            progress_tx,
            interrupt,
            clock,
            task_shared,
        )
        .await
    });
    KeepAliveGuard {
        handle: Some(handle),
        shared,
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_ticks(
    cfg: KeepAliveCfg,
    s5: Arc<dyn S5Storage>,
    submitter: Arc<dyn ProofSubmit>,
    tracker: LtxTracker,
    progress_tx: mpsc::Sender<Value>,
    interrupt: Option<Arc<ComfyClient>>,
    clock: Arc<dyn UnixClock>,
    shared: Arc<KeepAliveShared>,
) {
    let jid = cfg.job_id;
    let start = tokio::time::Instant::now();
    // one stalled call must not push the next tick past its interval
    let put_bound = cfg.interval / 3;
    let submit_bound = cfg.interval / 2;
    let mut i: u32 = 0;
    loop {
        i += 1;
        let at = start + cfg.first_tick + cfg.interval * (i - 1);
        if at > cfg.deadline_at {
            info!("LTX job {jid}: keep-alive stops at the job's deadline");
            return;
        }
        tokio::time::sleep_until(at).await;
        if progress_tx.is_closed() {
            warn!("LTX job {jid}: renter gone — keep-alive stops and interrupts the render");
            if let Some(c) = interrupt.as_ref() {
                let _ = tokio::time::timeout(Duration::from_secs(10), c.interrupt()).await;
            }
            return;
        }
        let att = json!({
            "type": "ltx_keepalive",
            "jobId": jid,
            "session": cfg.session_id,
            "host": cfg.host,
            "modelId": cfg.model_id,
            "steps": {
                "value": shared.step_value.load(Ordering::SeqCst),
                "max": shared.step_max.load(Ordering::SeqCst),
            },
            "time": clock.now_unix(),
            "i": i,
        });
        let bytes = serde_json::to_vec(&att).expect("json serialises");
        let proof_hash: [u8; 32] = Sha256::digest(&bytes).into();
        let path = format!("home/ltx/job_{}_keepalive_{i}.json", cfg.job_tag);
        let proof_cid = match tokio::time::timeout(put_bound, s5.put(&path, bytes)).await {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                warn!("LTX job {jid}: keep-alive {i} upload failed ({e}); the next tick retries");
                continue;
            }
            Err(_) => {
                warn!("LTX job {jid}: keep-alive {i} upload timed out; the next tick retries");
                continue;
            }
        };
        // the upload took time: still never post past the deadline
        if tokio::time::Instant::now() > cfg.deadline_at {
            return;
        }
        let first = tracker.proofs_submitted(jid).await == 0;
        let claim = if first {
            MIN_PROVEN_TOKENS.max(cfg.proof_interval)
        } else {
            MIN_PROVEN_TOKENS
        };
        // the interims never claim more than half the job (each one that erred
        // but may mine still counts)
        if shared.interim_sent().saturating_add(claim) > cfg.interim_cap {
            warn!(
                "LTX job {jid}: keep-alive {i} skipped — {} + {claim} tokens would pass the \
                 interim cap {}",
                shared.interim_sent(),
                cfg.interim_cap
            );
            continue;
        }
        // counted BEFORE it is handed to the queue (it may mine whatever we hear)
        shared.interim_sent.fetch_add(claim, Ordering::SeqCst);
        shared.in_flight.store(true, Ordering::SeqCst);
        let sent = tokio::time::timeout(
            submit_bound,
            submitter.submit_ltx_proof(jid, claim, proof_hash, proof_cid),
        )
        .await;
        match sent {
            Ok(Ok(tx)) => {
                shared.in_flight.store(false, Ordering::SeqCst);
                info!("LTX job {jid}: keep-alive {i} landed ({claim} tokens, tx {tx:?})");
                tracker.mark_interim_confirmed(jid).await;
            }
            Ok(Err(ProofSubmitError::RateLimited)) => {
                // provably NOT broadcast (the gas estimate failed): it can never
                // mine, so it does not count toward interim_sent or the cap
                shared.in_flight.store(false, Ordering::SeqCst);
                shared.interim_sent.fetch_sub(claim, Ordering::SeqCst);
                warn!(
                    "LTX job {jid}: keep-alive {i} ({claim} tokens) rate-limited, nothing \
                     broadcast; the next tick retries"
                );
            }
            Ok(Err(e)) => {
                shared.in_flight.store(false, Ordering::SeqCst);
                warn!(
                    "LTX job {jid}: keep-alive {i} ({claim} tokens) did not land: {e}; the next \
                     tick retries"
                )
            }
            // still counted and still flagged: it may sit in the tx queue
            Err(_) => warn!(
                "LTX job {jid}: keep-alive {i} ({claim} tokens) got no answer within \
                 {submit_bound:?}; the next tick retries"
            ),
        }
    }
}
