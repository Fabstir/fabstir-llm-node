// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The fail-closed publish gate. 🚨 SECURITY-CRITICAL.
//!
//! [`Gate::decide`] is the one place that turns a (possibly-missing, possibly-
//! errored) moderation result into a release/hold decision. It is **default-hold**:
//! the ONLY input that releases is `Ok(Some(Cleared))`. An absent result
//! (`Ok(None)`) and any component error (`Err`) both HOLD — they map to `Blocked`,
//! never panic, never clear. A `Flagged` result holds too, but stays distinct so
//! the transcode path can emit `CONTENT_FLAGGED` vs `CONTENT_BLOCKED` (§3.2).

use crate::moderation::types::{ModerationResult, Result, Verdict};
use crate::moderation::verdict_store::VerdictStore;

/// Stateless decision point for the host-reachable publish gate (seam #2 slice).
pub struct Gate;

/// Outcome of the host-reachable transcode gate. `Release` falls through to
/// billing/proof/`transcode_complete`; `Hold` makes the caller send the error and
/// `break` — skipping billing, the S5 proof upload, and the completion message.
pub enum GateOutcome {
    Release,
    Hold { code: &'static str, message: String },
}

impl Gate {
    /// Fail-closed verdict. Releases (`Cleared`) for EXACTLY ONE input —
    /// `Ok(Some(result))` whose verdict is `Cleared`. Everything else holds:
    /// a `Blocked`/`Flagged` result is passed through (so the caller knows which),
    /// while an absent result or a component error both collapse to `Blocked`.
    ///
    /// Callers gate on [`Verdict::releases`], so any future non-`Cleared` verdict
    /// is held by default.
    pub fn decide(result: Result<Option<&ModerationResult>>) -> Verdict {
        match result {
            Ok(Some(r)) => r.verdict,
            Ok(None) => Verdict::Blocked, // absent verdict ⇒ hold (fail-closed)
            Err(_) => Verdict::Blocked,   // component error ⇒ hold (fail-closed)
        }
    }

    /// Decide whether a transcode job may complete (host-reachable seam-#2 slice).
    /// The release decision routes through [`Gate::decide`] (single fail-closed
    /// authority): only a recorded `Cleared` releases. A `Blocked`/`Flagged`
    /// verdict holds with the matching protocol code; an absent verdict OR a
    /// missing `job_id` (cannot be moderated) holds as `MODERATION_UNAVAILABLE`.
    pub fn transcode_decision(store: &VerdictStore, job_id: Option<u64>) -> GateOutcome {
        let lookup = job_id.and_then(|jid| store.get(jid));
        if Gate::decide(Ok(lookup.as_ref())).releases() {
            return GateOutcome::Release;
        }
        match lookup {
            Some(r) if r.verdict == Verdict::Flagged => GateOutcome::Hold {
                code: "CONTENT_FLAGGED",
                message: r.reason.unwrap_or_else(|| "flagged by moderation".into()),
            },
            Some(r) => GateOutcome::Hold {
                code: "CONTENT_BLOCKED",
                message: r.reason.unwrap_or_else(|| "blocked by moderation".into()),
            },
            None => GateOutcome::Hold {
                code: "MODERATION_UNAVAILABLE",
                message: "Held: moderation verdict unavailable".into(),
            },
        }
    }
}
