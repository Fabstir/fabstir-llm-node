// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! SCI-C counters (D21): who completed sessions, what transcode runs ended as,
//! what result fetches got, and how many sockets the liveness rule closed.
//! Process-wide lock-free atomics (the producers sit in modules with no shared
//! handle), surfaced on `/metrics`. Logs for the same events carry the stable
//! `[SCI-C]` prefix.

use std::sync::atomic::{AtomicU64, Ordering};

/// A point-in-time view of the counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionCompletionCounts {
    /// D15: the job's last unit of work ended with no live connection.
    pub completed_last_work: u64,
    /// D15: the grace period after a billed transcode elapsed.
    pub completed_grace: u64,
    /// D15: the job's connection moved to another job and nothing carried it.
    pub completed_unowned: u64,
    /// D17: the expiry sweeper completed a stranded session.
    pub sweeper_completed: u64,
    /// D17: a sweeper completion failed (retried next tick).
    pub sweeper_failed: u64,
    /// D19: transcode runs stored `Done` (paid and released).
    pub transcode_done: u64,
    /// D19: transcode runs stored `Failed`.
    pub transcode_failed: u64,
    pub fetch_served: u64,
    pub fetch_pending: u64,
    pub fetch_not_found: u64,
    pub fetch_expired: u64,
    pub fetch_not_owner: u64,
    pub fetch_failed: u64,
    pub fetch_held: u64,
    /// D16: sockets closed after an unanswered or unsendable Ping.
    pub liveness_closed: u64,
}

macro_rules! counters {
    ($($name:ident),* $(,)?) => {
        /// The process-wide SCI-C counters.
        pub struct SessionCompletionMetrics {
            $($name: AtomicU64,)*
        }

        impl SessionCompletionMetrics {
            pub const fn new() -> Self {
                Self { $($name: AtomicU64::new(0),)* }
            }

            $(
                pub fn $name(&self) {
                    self.$name.fetch_add(1, Ordering::Relaxed);
                }
            )*

            pub fn snapshot(&self) -> SessionCompletionCounts {
                SessionCompletionCounts {
                    $($name: self.$name.load(Ordering::Relaxed),)*
                }
            }
        }
    };
}

counters!(
    completed_last_work,
    completed_grace,
    completed_unowned,
    sweeper_completed,
    sweeper_failed,
    transcode_done,
    transcode_failed,
    fetch_served,
    fetch_pending,
    fetch_not_found,
    fetch_expired,
    fetch_not_owner,
    fetch_failed,
    fetch_held,
    liveness_closed,
);

impl Default for SessionCompletionMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// The one instance.
pub static SCI_C: SessionCompletionMetrics = SessionCompletionMetrics::new();

/// Prometheus text for `/metrics`.
pub fn render(c: &SessionCompletionCounts) -> String {
    format!(
        "# HELP session_completions_total Sessions the node completed itself, by trigger\n\
         # TYPE session_completions_total counter\n\
         session_completions_total{{trigger=\"last_work\"}} {}\n\
         session_completions_total{{trigger=\"grace\"}} {}\n\
         session_completions_total{{trigger=\"unowned\"}} {}\n\
         session_completions_total{{trigger=\"sweeper\"}} {}\n\
         # HELP session_sweeper_failures_total Expiry-sweeper completions that failed\n\
         # TYPE session_sweeper_failures_total counter\n\
         session_sweeper_failures_total {}\n\
         # HELP transcode_results_total Transcode runs by stored outcome\n\
         # TYPE transcode_results_total counter\n\
         transcode_results_total{{outcome=\"done\"}} {}\n\
         transcode_results_total{{outcome=\"failed\"}} {}\n\
         # HELP transcode_result_fetches_total Result fetches by answer\n\
         # TYPE transcode_result_fetches_total counter\n\
         transcode_result_fetches_total{{answer=\"served\"}} {}\n\
         transcode_result_fetches_total{{answer=\"pending\"}} {}\n\
         transcode_result_fetches_total{{answer=\"not_found\"}} {}\n\
         transcode_result_fetches_total{{answer=\"expired\"}} {}\n\
         transcode_result_fetches_total{{answer=\"not_owner\"}} {}\n\
         transcode_result_fetches_total{{answer=\"failed\"}} {}\n\
         transcode_result_fetches_total{{answer=\"held\"}} {}\n\
         # HELP ws_liveness_closes_total Sockets closed after an unanswered Ping\n\
         # TYPE ws_liveness_closes_total counter\n\
         ws_liveness_closes_total {}\n",
        c.completed_last_work,
        c.completed_grace,
        c.completed_unowned,
        c.sweeper_completed,
        c.sweeper_failed,
        c.transcode_done,
        c.transcode_failed,
        c.fetch_served,
        c.fetch_pending,
        c.fetch_not_found,
        c.fetch_expired,
        c.fetch_not_owner,
        c.fetch_failed,
        c.fetch_held,
        c.liveness_closed,
    )
}
