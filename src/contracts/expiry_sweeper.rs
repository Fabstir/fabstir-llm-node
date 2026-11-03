// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The expiry sweeper, the backstop (SCI-C D17, IMPLEMENTATION-SESSION-COMPLETION-INTEGRITY).
//!
//! Every owner of a session's completion can fail: a socket that never closes
//! (session 1433), a completion whose send failed, a node that restarted
//! mid-run. Every `SESSION_SWEEP_INTERVAL_SECS` (300) the sweeper walks the
//! jobs this node may still owe a completion (its token trackers, retained until
//! settled per D9, and the transcode result store's jobs, including runs a stop
//! interrupted), minus those known settled, one at a time. For each it reads
//! `sessionJobs` and completes the session through the held latch (D1) only
//! when the chain's own clock (the latest block's timestamp, never the node's)
//! is past BOTH the session's expiry plus `SESSION_SWEEP_MARGIN_SECS` (300) AND
//! the last proof plus the contract's dispute window, and no work is in flight
//! (a transcode's unit of work is waited on only up to its own timeout).
//!
//! It never calls `triggerSessionTimeout` (Open question 4). A chain read of
//! `Completed` or `TimedOut` drops the job; nothing else does.

use std::collections::{HashMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use ethers::types::Address;
use futures::FutureExt;
use tokio::time::Instant;

use crate::training::accept::SessionStatus;
use crate::transcoder::proof_steps::session_view;

pub const INTERVAL_DEFAULT_SECS: u64 = 300;
pub const MARGIN_DEFAULT_SECS: u64 = 300;
/// Bound on each chain read the sweeper makes.
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// A failed completion waits interval × 2^(failures-1) ticks, up to this many.
pub const MAX_BACKOFF_TICKS: u32 = 64;

/// What the sweeper does with one job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepDecision {
    /// Settled on chain (or known settled): forget it.
    Drop,
    /// Not yet; look again next tick.
    Skip(&'static str),
    /// Complete the session now.
    Complete,
}

/// One job's facts, as read this tick.
#[derive(Debug, Clone, Copy)]
pub struct SweepFacts {
    pub status: SessionStatus,
    /// Latest block timestamp.
    pub chain_now: u64,
    pub start_time: u64,
    pub max_duration: u64,
    pub last_proof_time: u64,
    pub dispute_window: u64,
    pub margin: u64,
    /// A unit of work (a transcode) is still running on the job.
    pub work_in_flight: bool,
    /// How long past expiry plus margin running work is waited on.
    pub work_cap: u64,
    /// An LTX clip or training slice has a proof in flight: its own task
    /// owns completion.
    pub owned_elsewhere: bool,
    /// The completion latch's settled set already holds the job.
    pub settled: bool,
    /// The session's host (word 2) is not this node: never ours to complete
    /// (another host's session, or a job id from another chain).
    pub foreign: bool,
}

/// D17(c), pure.
pub fn sweep_decision(f: &SweepFacts) -> SweepDecision {
    if f.settled || f.foreign {
        return SweepDecision::Drop;
    }
    match f.status {
        SessionStatus::Completed | SessionStatus::TimedOut => return SweepDecision::Drop,
        SessionStatus::Active => {}
    }
    let expiry = f.start_time.saturating_add(f.max_duration);
    let due = expiry
        .saturating_add(f.margin)
        .max(f.last_proof_time.saturating_add(f.dispute_window));
    if f.chain_now <= due {
        return SweepDecision::Skip("not yet past expiry, margin and dispute window");
    }
    if f.owned_elsewhere {
        return SweepDecision::Skip("a proof in flight owns completion");
    }
    if f.work_in_flight && f.chain_now <= expiry.saturating_add(f.margin).saturating_add(f.work_cap)
    {
        return SweepDecision::Skip("work in flight");
    }
    SweepDecision::Complete
}

/// What the sweeper needs from the node. The chain sits behind it so a tick is
/// testable without one.
#[async_trait::async_trait]
pub trait SweepTarget: Send + Sync {
    /// Jobs that may still be owed a completion (unsettled ones included).
    async fn registry(&self) -> Vec<u64>;
    /// Raw `sessionJobs(job_id)`.
    async fn session_raw(&self, job_id: u64) -> Result<Vec<u8>>;
    /// Latest block timestamp.
    async fn chain_now(&self) -> Result<u64>;
    /// The contract's dispute window, read now (never below the startup value).
    async fn dispute_window_secs(&self) -> u64;
    /// This node's host address; a session hosted by anyone else is never swept.
    fn host_address(&self) -> Address;
    async fn is_settled(&self, job_id: u64) -> bool;
    /// Units of work running on the job (a transcode's `WorkGuard`).
    fn work_in_flight(&self, job_id: u64) -> bool;
    /// An LTX or training proof is in flight on the job.
    async fn owned_elsewhere(&self, job_id: u64) -> bool;
    /// Arm the other owners' completing latches (LTX, training) so no new clip
    /// or slice is accepted under this completion. `false`: one was accepted
    /// meanwhile, and its own task owns completion.
    async fn claim(&self, _job_id: u64) -> bool {
        true
    }
    /// Complete through the held latch (`complete_session_job`).
    async fn complete(&self, job_id: u64) -> Result<()>;
    /// Housekeeping run once per tick (the result store's retention sweep).
    fn housekeeping(&self) {}
}

#[derive(Debug, Clone, Copy)]
pub struct SweepConfig {
    pub interval: Duration,
    pub margin: u64,
    pub work_cap: u64,
}

impl SweepConfig {
    /// `SESSION_SWEEP_INTERVAL_SECS` (300, at least 30), `SESSION_SWEEP_MARGIN_SECS`
    /// (300); the work cap is the transcode timeout (`TRANSCODE_TIMEOUT_SECONDS`, 1800).
    pub fn from_env() -> Self {
        let read = |name: &str, default: u64| {
            std::env::var(name)
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(default)
        };
        Self {
            interval: Duration::from_secs(
                read("SESSION_SWEEP_INTERVAL_SECS", INTERVAL_DEFAULT_SECS).max(30),
            ),
            margin: read("SESSION_SWEEP_MARGIN_SECS", MARGIN_DEFAULT_SECS),
            work_cap: read("TRANSCODE_TIMEOUT_SECONDS", 1800),
        }
    }
}

/// What one tick did, for the log and the tests.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TickReport {
    /// Completions started this tick (each runs as its own task).
    pub dispatched: Vec<u64>,
    pub dropped: Vec<u64>,
    pub skipped: usize,
    pub unreadable: Vec<u64>,
    /// A completion from an earlier tick is still running.
    pub in_flight: Vec<u64>,
    /// Waiting out a backoff after a failed completion.
    pub backing_off: Vec<u64>,
    /// The tick could not read chain time; nothing was decided.
    pub no_chain_time: bool,
}

/// Removes a job from the in-flight set on every exit of its completion task,
/// a panic included.
struct InFlight {
    set: Arc<Mutex<HashSet<u64>>>,
    job_id: u64,
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.set
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.job_id);
    }
}

async fn bounded<T>(what: &str, f: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    match tokio::time::timeout(READ_TIMEOUT, f).await {
        Ok(r) => r,
        Err(_) => Err(anyhow!(
            "{what} timed out after {}s",
            READ_TIMEOUT.as_secs()
        )),
    }
}

/// The session's host, word 2 of `sessionJobs`.
fn session_host(raw: &[u8]) -> Option<Address> {
    (raw.len() >= 96).then(|| Address::from_slice(&raw[64 + 12..96]))
}

pub struct ExpirySweeper {
    config: SweepConfig,
    /// Jobs read as `Completed` or `TimedOut`, or not this host's: never read again.
    finished: HashSet<u64>,
    in_flight: Arc<Mutex<HashSet<u64>>>,
    /// Per job: consecutive failed completions, and when the last one ended.
    failures: Arc<Mutex<HashMap<u64, (u32, Instant)>>>,
}

impl ExpirySweeper {
    pub fn new(config: SweepConfig) -> Self {
        Self {
            config,
            finished: HashSet::new(),
            in_flight: Arc::new(Mutex::new(HashSet::new())),
            failures: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn backing_off(&self, job_id: u64) -> bool {
        let failures = self.failures.lock().unwrap_or_else(|p| p.into_inner());
        match failures.get(&job_id) {
            Some((count, at)) => {
                let ticks = 2u32
                    .saturating_pow(count.saturating_sub(1))
                    .min(MAX_BACKOFF_TICKS);
                Instant::now() < *at + self.config.interval * ticks
            }
            None => false,
        }
    }

    /// One pass over the registry, one job at a time. Completions are spawned,
    /// so one long completion (a dispute-window wait inside it) never holds up
    /// the others; every chain read is bounded.
    pub async fn tick(&mut self, target: &Arc<dyn SweepTarget>) -> TickReport {
        target.housekeeping();
        let mut report = TickReport::default();
        let chain_now = match bounded("chain time", target.chain_now()).await {
            Ok(now) => now,
            Err(e) => {
                tracing::warn!(
                    "[SCI-C] sweeper: chain time unreadable ({}); tick skipped",
                    e
                );
                report.no_chain_time = true;
                return report;
            }
        };
        let dispute_window =
            match tokio::time::timeout(READ_TIMEOUT, target.dispute_window_secs()).await {
                Ok(w) => w,
                Err(_) => {
                    tracing::warn!("[SCI-C] sweeper: dispute window read timed out; tick skipped");
                    report.no_chain_time = true;
                    return report;
                }
            };
        let ours = target.host_address();
        let mut jobs = target.registry().await;
        jobs.sort_unstable();
        jobs.dedup();
        for job_id in jobs {
            if self.finished.contains(&job_id) {
                continue;
            }
            if self
                .in_flight
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .contains(&job_id)
            {
                report.in_flight.push(job_id);
                continue;
            }
            if self.backing_off(job_id) {
                report.backing_off.push(job_id);
                continue;
            }
            if target.is_settled(job_id).await {
                self.finished.insert(job_id);
                report.dropped.push(job_id);
                continue;
            }
            let raw = match bounded("sessionJobs", target.session_raw(job_id)).await {
                Ok(raw) => raw,
                Err(e) => {
                    tracing::warn!("[SCI-C] sweeper: job {} unreadable: {}", job_id, e);
                    report.unreadable.push(job_id);
                    continue;
                }
            };
            let view = match session_view(&raw, job_id) {
                Ok(view) => view,
                Err(e) => {
                    tracing::warn!("[SCI-C] sweeper: job {} unreadable: {}", job_id, e);
                    report.unreadable.push(job_id);
                    continue;
                }
            };
            let foreign = session_host(&raw) != Some(ours);
            let facts = SweepFacts {
                status: view.status,
                chain_now,
                start_time: view.start_time,
                max_duration: view.max_duration,
                last_proof_time: view.last_proof_time,
                dispute_window,
                margin: self.config.margin,
                work_in_flight: target.work_in_flight(job_id),
                work_cap: self.config.work_cap,
                owned_elsewhere: target.owned_elsewhere(job_id).await,
                settled: false,
                foreign,
            };
            match sweep_decision(&facts) {
                SweepDecision::Drop => {
                    if foreign && view.status == SessionStatus::Active {
                        tracing::warn!(
                            "[SCI-C] sweeper: job {} is hosted by {:?}, not this node; never swept",
                            job_id,
                            session_host(&raw)
                        );
                    }
                    self.finished.insert(job_id);
                    report.dropped.push(job_id);
                }
                SweepDecision::Skip(_) => report.skipped += 1,
                SweepDecision::Complete => {
                    if !target.claim(job_id).await {
                        report.skipped += 1;
                        continue;
                    }
                    tracing::warn!(
                        "[SCI-C] sweeper: job {} is past expiry (start {} + max {} + margin {}) and its dispute window on chain time {}; completing",
                        job_id,
                        view.start_time,
                        view.max_duration,
                        self.config.margin,
                        chain_now
                    );
                    self.in_flight
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .insert(job_id);
                    let guard = InFlight {
                        set: self.in_flight.clone(),
                        job_id,
                    };
                    let failures = self.failures.clone();
                    let target = target.clone();
                    tokio::spawn(async move {
                        let _guard = guard;
                        let metrics = &crate::monitoring::session_completion_metrics::SCI_C;
                        match target.complete(job_id).await {
                            Ok(()) => {
                                metrics.sweeper_completed();
                                failures
                                    .lock()
                                    .unwrap_or_else(|p| p.into_inner())
                                    .remove(&job_id);
                            }
                            Err(e) => {
                                metrics.sweeper_failed();
                                let mut failures =
                                    failures.lock().unwrap_or_else(|p| p.into_inner());
                                let entry = failures.entry(job_id).or_insert((0, Instant::now()));
                                entry.0 += 1;
                                entry.1 = Instant::now();
                                tracing::error!(
                                    "[SCI-C] sweeper: completing job {} failed ({} in a row): {}; backing off",
                                    job_id,
                                    entry.0,
                                    e
                                );
                            }
                        }
                    });
                    report.dispatched.push(job_id);
                }
            }
        }
        if !report.dispatched.is_empty() {
            tracing::info!("[SCI-C] sweeper tick: {:?}", report);
        }
        report
    }

    /// Run until `cancel` fires: one tick per interval. A panicking tick is
    /// logged and the next tick runs.
    pub async fn run(
        mut self,
        target: Arc<dyn SweepTarget>,
        cancel: tokio_util::sync::CancellationToken,
    ) {
        tracing::info!(
            "[SCI-C] expiry sweeper started (every {}s, margin {}s, work cap {}s)",
            self.config.interval.as_secs(),
            self.config.margin,
            self.config.work_cap
        );
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(self.config.interval) => {}
            }
            if AssertUnwindSafe(self.tick(&target))
                .catch_unwind()
                .await
                .is_err()
            {
                tracing::error!("[SCI-C] sweeper tick panicked; continuing with the next tick");
            }
        }
        tracing::info!("[SCI-C] expiry sweeper stopped");
    }
}
