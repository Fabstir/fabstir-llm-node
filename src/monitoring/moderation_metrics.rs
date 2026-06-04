// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Moderation observability counters (§8 #7): verdicts (cleared/blocked/flagged),
//! fail-closed holds, Track-1 matches, and NCMEC reports filed. Lock-free atomics;
//! `snapshot()` feeds `/metrics`.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::moderation::types::Verdict;

/// A point-in-time view of the moderation counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModerationCounts {
    pub cleared: u64,
    pub blocked: u64,
    pub flagged: u64,
    pub held: u64,
    pub matches: u64,
    pub reports_filed: u64,
}

/// Lock-free moderation counters, safe to share via `Arc` across the API.
#[derive(Default)]
pub struct ModerationMetrics {
    cleared: AtomicU64,
    blocked: AtomicU64,
    flagged: AtomicU64,
    held: AtomicU64,
    matches: AtomicU64,
    reports_filed: AtomicU64,
}

impl ModerationMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Count a verdict outcome.
    pub fn record_verdict(&self, verdict: Verdict) {
        match verdict {
            Verdict::Cleared => &self.cleared,
            Verdict::Blocked => &self.blocked,
            Verdict::Flagged => &self.flagged,
        }
        .fetch_add(1, Ordering::Relaxed);
    }

    /// Count a fail-closed HOLD (absent/errored verdict held at the gate).
    pub fn record_held(&self) {
        self.held.fetch_add(1, Ordering::Relaxed);
    }

    /// Count a Track-1 match.
    pub fn record_match(&self) {
        self.matches.fetch_add(1, Ordering::Relaxed);
    }

    /// Count an NCMEC report filed.
    pub fn record_report_filed(&self) {
        self.reports_filed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> ModerationCounts {
        ModerationCounts {
            cleared: self.cleared.load(Ordering::Relaxed),
            blocked: self.blocked.load(Ordering::Relaxed),
            flagged: self.flagged.load(Ordering::Relaxed),
            held: self.held.load(Ordering::Relaxed),
            matches: self.matches.load(Ordering::Relaxed),
            reports_filed: self.reports_filed.load(Ordering::Relaxed),
        }
    }
}
