// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Completion keyed to the JOB, not the connection (SCI-C D15,
//! IMPLEMENTATION-SESSION-COMPLETION-INTEGRITY).
//!
//! Before this, a session was completed only when the per-connection read loop
//! exited, so a client that never hung up (session 1433) was never settled, and
//! closing one socket completed the session under work running on another.
//! `JobWork` records, per job, the live connections carrying it and the units
//! of work in flight. A `WorkGuard` holds one unit for as long as it runs and
//! releases it on every exit, panics included. When the last unit ends:
//!
//! - no live connection: the job is completed now (billed or not);
//! - live connections, every unit transcode, the last one billed: completion is
//!   scheduled after a grace period, and only runs if nothing new was accepted
//!   meanwhile. A job that carried any other work (chat, LTX, training) is never
//!   completed under its live connection.
//!
//! The completion itself goes through `CheckpointManager::complete_session_job`,
//! whose held latch makes every owner safe to call it.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Starts settlement of a job; it must not block (it spawns).
pub type Completer = Arc<dyn Fn(u64) + Send + Sync>;

/// Why a new unit of work was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkRefusal {
    /// The job's completion has been decided; it takes no new work.
    Closing,
}

#[derive(Default)]
struct JobWorkEntry {
    count: u32,
    connections: HashSet<String>,
    other_work: bool,
    /// Some unit of work on the job has had its proof confirmed.
    billed: bool,
    epoch: u64,
    closing: bool,
}

/// What the disconnect path needs to know about a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobState {
    pub work_in_flight: u32,
    pub other_connections: usize,
}

pub struct JobWork {
    jobs: Mutex<HashMap<u64, JobWorkEntry>>,
    completer: Completer,
    grace: Duration,
}

impl JobWork {
    pub fn new(completer: Completer, grace: Duration) -> Arc<Self> {
        Arc::new(Self {
            jobs: Mutex::new(HashMap::new()),
            completer,
            grace,
        })
    }

    /// The grace period from `TRANSCODE_COMPLETE_GRACE_SECS` (default 60 s).
    pub fn grace_from_env() -> Duration {
        Duration::from_secs(
            std::env::var("TRANSCODE_COMPLETE_GRACE_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
        )
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, JobWorkEntry>> {
        self.jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Connection `conn_id` now carries job `job_id`.
    pub fn connection_opened(&self, job_id: u64, conn_id: &str) {
        self.lock()
            .entry(job_id)
            .or_default()
            .connections
            .insert(conn_id.to_string());
    }

    /// Connection `conn_id` no longer carries job `job_id` (closed, or re-inited
    /// onto another job). Returns what is left on the job.
    pub fn connection_closed(&self, job_id: u64, conn_id: &str) -> JobState {
        let mut jobs = self.lock();
        let entry = jobs.entry(job_id).or_default();
        entry.connections.remove(conn_id);
        JobState {
            work_in_flight: entry.count,
            other_connections: entry.connections.len(),
        }
    }

    /// Job `job_id` carried work that is not a transcode, so it is never
    /// completed by the transcode grace timer.
    pub fn note_other_work(&self, job_id: u64) {
        let mut jobs = self.lock();
        let entry = jobs.entry(job_id).or_default();
        entry.other_work = true;
        // A pending grace timer must not complete under this work.
        entry.epoch += 1;
    }

    /// A unit of work on job `job_id` has had its proof confirmed.
    pub fn mark_billed(&self, job_id: u64) {
        self.lock().entry(job_id).or_default().billed = true;
    }

    pub fn is_closing(&self, job_id: u64) -> bool {
        self.lock().get(&job_id).map(|e| e.closing).unwrap_or(false)
    }

    /// Begin one unit of transcode work on job `job_id`, refused once the job's
    /// completion has been decided.
    pub fn begin(self: &Arc<Self>, job_id: u64) -> Result<WorkGuard, WorkRefusal> {
        let mut jobs = self.lock();
        let entry = jobs.entry(job_id).or_default();
        if entry.closing {
            return Err(WorkRefusal::Closing);
        }
        entry.count += 1;
        entry.epoch += 1;
        Ok(WorkGuard {
            work: self.clone(),
            job_id,
        })
    }

    fn finish(self: &Arc<Self>, job_id: u64) {
        let action = {
            let mut jobs = self.lock();
            let entry = jobs.entry(job_id).or_default();
            entry.count = entry.count.saturating_sub(1);
            let action = finish_action(
                entry.count,
                entry.connections.len(),
                entry.other_work,
                entry.billed,
            );
            if action == Some(FinishAction::CompleteNow) {
                entry.closing = true;
            }
            action.map(|action| (action, entry.epoch))
        };
        match action {
            Some((FinishAction::CompleteNow, _)) => {
                tracing::info!(
                    "[SCI] job {}: last unit of work ended with no live connection; completing",
                    job_id
                );
                (self.completer)(job_id);
            }
            Some((FinishAction::CompleteAfterGrace, epoch)) => {
                tracing::info!(
                    "[SCI] job {}: transcode finished and billed; completing in {}s unless new work arrives",
                    job_id,
                    self.grace.as_secs()
                );
                let work = self.clone();
                let grace = self.grace;
                match tokio::runtime::Handle::try_current() {
                    Ok(runtime) => {
                        runtime.spawn(async move {
                            tokio::time::sleep(grace).await;
                            if work.close_if_idle(job_id, epoch) {
                                tracing::info!("[SCI] job {}: grace elapsed, completing", job_id);
                                (work.completer)(job_id);
                            }
                        });
                    }
                    Err(_) => tracing::warn!(
                        "[SCI] job {}: no runtime to arm the grace completion; the disconnect path stays the owner",
                        job_id
                    ),
                }
            }
            None => {}
        }
    }

    /// Complete job `job_id` now if nothing carries it any more (no work in
    /// flight, no live connection): the connection that carried it moved to
    /// another job. Returns whether completion started.
    pub fn complete_if_unowned(&self, job_id: u64) -> bool {
        {
            let mut jobs = self.lock();
            let entry = jobs.entry(job_id).or_default();
            if entry.count > 0 || !entry.connections.is_empty() || entry.closing {
                return false;
            }
            entry.closing = true;
        }
        tracing::info!(
            "[SCI] job {}: its connection moved to another job and nothing else carries it; completing",
            job_id
        );
        (self.completer)(job_id);
        true
    }

    /// The grace timer's atomic decision (D15d): close the job iff nothing is
    /// in flight and nothing was accepted since the timer was armed. Once
    /// closing, `begin` refuses new work.
    pub fn close_if_idle(&self, job_id: u64, armed_epoch: u64) -> bool {
        let mut jobs = self.lock();
        match jobs.get_mut(&job_id) {
            Some(entry)
                if entry.count == 0
                    && entry.epoch == armed_epoch
                    && !entry.closing
                    && !entry.other_work =>
            {
                entry.closing = true;
                true
            }
            _ => false,
        }
    }
}

/// What happens when a unit of work ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishAction {
    CompleteNow,
    CompleteAfterGrace,
}

/// D15(c): the pure rule for the end of a unit of work.
pub fn finish_action(
    remaining: u32,
    connections: usize,
    other_work: bool,
    billed: bool,
) -> Option<FinishAction> {
    if remaining > 0 {
        return None;
    }
    if connections == 0 {
        return Some(FinishAction::CompleteNow);
    }
    if billed && !other_work {
        return Some(FinishAction::CompleteAfterGrace);
    }
    None
}

/// One unit of work in flight on a job. Dropping it, on any path, ends it.
pub struct WorkGuard {
    work: Arc<JobWork>,
    job_id: u64,
}

impl WorkGuard {
    /// The unit's proof has been confirmed. Recorded on the job, not the unit,
    /// so a later unit that fails cannot erase it: once the job's last unit
    /// ends under a live connection, it is completed after the grace period.
    pub fn mark_billed(&self) {
        self.work.mark_billed(self.job_id);
    }

    pub fn job_id(&self) -> u64 {
        self.job_id
    }
}

impl Drop for WorkGuard {
    fn drop(&mut self) {
        self.work.finish(self.job_id);
    }
}

/// A connection's registration on the job it carries. Dropping it, on any
/// path (a panic or an aborted handler included), deregisters the connection
/// so a dead connection never holds a job open.
pub struct ConnectionJob {
    work: Arc<JobWork>,
    conn_id: String,
    job_id: Option<u64>,
}

impl ConnectionJob {
    pub fn new(work: Arc<JobWork>, conn_id: String) -> Self {
        Self {
            work,
            conn_id,
            job_id: None,
        }
    }

    /// Point the registration at `job_id`. Returns the state left on the job
    /// the connection stopped carrying, if it changed.
    pub fn set(&mut self, job_id: Option<u64>) -> Option<(u64, JobState)> {
        if self.job_id == job_id {
            return None;
        }
        let left = self
            .job_id
            .take()
            .map(|old| (old, self.work.connection_closed(old, &self.conn_id)));
        if let Some(new) = job_id {
            self.work.connection_opened(new, &self.conn_id);
        }
        self.job_id = job_id;
        left
    }

    /// Deregister now (the connection is closing). Returns what is left on the job.
    pub fn close(&mut self) -> Option<(u64, JobState)> {
        self.job_id
            .take()
            .map(|old| (old, self.work.connection_closed(old, &self.conn_id)))
    }
}

impl Drop for ConnectionJob {
    fn drop(&mut self) {
        self.close();
    }
}
