// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Session-completion ownership (SCI-B, IMPLEMENTATION-SESSION-COMPLETION-INTEGRITY D1-D5).
//!
//! `CheckpointManager::complete_session_job` has several owners (the WebSocket
//! disconnect path and its retry, LTX deferred completion, the training run's
//! end and its zero-settle). Job 1160 showed two of them sending
//! `completeSessionJob` 3.4 s apart, the second reverting. This module is the
//! mutual exclusion they all go through:
//!
//! - a per-job HELD latch (D1a): an `OwnedMutexGuard` that lives for the whole
//!   completion, however long the proof poll and dispute sleep take. Not a
//!   self-expiring timestamp, which would let a second owner in mid-call;
//! - a never-evicted sticky "settled" set (D1c, D1d), written ONLY on a confirmed
//!   status-1 completion receipt or a chain read of `Completed`, never on a
//!   function's `Ok` path (a reverted receipt also reaches `Ok` today);
//! - the drift-canaried chain read (D3, D4) that decides whether a send is
//!   still needed.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::{Mutex, OwnedMutexGuard, RwLock};

use crate::training::accept::{decode_session_snapshot, SessionStatus};

/// Both maps are never evicted: an entry is a few dozen bytes per job, and
/// evicting the settled set would let a later owner send a second completion.
#[derive(Default)]
pub struct CompletionGuard {
    locks: RwLock<HashMap<u64, Arc<Mutex<()>>>>,
    settled: RwLock<HashSet<u64>>,
}

impl CompletionGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take job `job_id`'s completion latch, waiting while another owner holds
    /// it. The map lock is held only to clone the per-job `Arc`, never across
    /// the wait, so one long completion never serialises other jobs.
    pub async fn acquire(&self, job_id: u64) -> OwnedMutexGuard<()> {
        let existing = self.locks.read().await.get(&job_id).cloned();
        let lock = match existing {
            Some(lock) => lock,
            None => self
                .locks
                .write()
                .await
                .entry(job_id)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone(),
        };
        lock.lock_owned().await
    }

    pub async fn is_settled(&self, job_id: u64) -> bool {
        self.settled.read().await.contains(&job_id)
    }

    /// Record that job `job_id` is settled. Call ONLY on a confirmed status-1
    /// completion receipt or a chain read of `Completed` (D1c). Never evicted (D1d).
    pub async fn mark_settled(&self, job_id: u64) {
        self.settled.write().await.insert(job_id);
    }
}

/// The session status a completion decision acts on, decoded from the raw
/// `sessionJobs(job_id)` return. `Err` means unreadable, which the caller
/// treats as "proceed with the send" (D5): a skipped completion strands a
/// deposit, a redundant one costs gas.
///
/// D4's drift canary: word 0 must equal `job_id`. A layout drift under the
/// upgradeable proxy would otherwise either disable the guard or, if the
/// drifted word read as 1, make every session look settled and strand every
/// deposit on the host.
pub fn session_status_for_completion(raw: &[u8], job_id: u64) -> Result<SessionStatus, String> {
    let snapshot = decode_session_snapshot(raw)?;
    let word0 = ethers::types::U256::from_big_endian(&raw[0..32]);
    if word0 != ethers::types::U256::from(job_id) {
        return Err(format!(
            "sessionJobs word 0 is {word0}, expected job id {job_id} (layout drift; fail open)"
        ));
    }
    Ok(snapshot.status)
}
