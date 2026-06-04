// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! `VerdictStore` — job_id → ModerationResult, with a fail-closed lookup.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::moderation::types::ModerationResult;

/// Maps `job_id → ModerationResult`. Fail-closed: `get` of an unknown `job_id`
/// returns `None`, which the gate treats as a HOLD (§1.3 / §3.2).
///
/// In-memory by design for launch: a node restart loses pending verdicts, so
/// subsequent lookups are absent ⇒ the gate HOLDs (correct fail-closed, never
/// fail-open). Durable persistence + TTL are deferred to Phase 7.
///
/// A poisoned lock is recovered rather than `unwrap()`-panicked: a panic on the
/// safety path could be mishandled upstream, and the recovered map is still a
/// valid `HashMap` whose worst case is an absent entry ⇒ a HOLD.
#[derive(Default)]
pub struct VerdictStore {
    inner: RwLock<HashMap<u64, ModerationResult>>,
}

impl VerdictStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record (or overwrite) the verdict for a job.
    pub fn set(&self, job_id: u64, result: ModerationResult) {
        let mut map = self.inner.write().unwrap_or_else(|e| e.into_inner());
        map.insert(job_id, result);
    }

    /// Look up the verdict for a job. Absent ⇒ `None` ⇒ the gate HOLDs.
    pub fn get(&self, job_id: u64) -> Option<ModerationResult> {
        let map = self.inner.read().unwrap_or_else(|e| e.into_inner());
        map.get(&job_id).cloned()
    }
}
