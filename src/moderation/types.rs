// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Core moderation value types & errors.
//!
//! `Verdict`, `ModerationResult`, and `MatchResult` are added in Sub-phase 1.1;
//! this file (Sub-phase 0.2) defines the error type and the supporting enums.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Fail-closed result alias: every fallible moderation step yields a
/// [`ModerationError`] which the gate maps to a HOLD (§3.4 / 1.2).
pub type Result<T> = std::result::Result<T, ModerationError>;

/// Errors across the moderation pipeline. Every variant is a *hold* condition at
/// the gate — none ever maps to `Cleared` (fail-closed, §1.2 / §3.4).
#[derive(Debug, Error)]
pub enum ModerationError {
    /// Matcher returns this (never a fabricated match) and the gate HOLDs (§3.4).
    #[error("CSAM hash list unavailable")]
    ListUnavailable,
    #[error("decode failed: {0}")]
    DecodeFailed(String),
    #[error("store error: {0}")]
    StoreError(String),
    #[error("report failed: {0}")]
    ReportFailed(String),
    #[error("unauthorised: {0}")]
    Unauthorised(String),
    #[error("internal: {0}")]
    Internal(String),
}

/// Per-category policy decision. Maps onto a [`super::types::Verdict`] at the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Disposition {
    /// Withhold completion / publish (fail-closed default for unmapped categories).
    Block,
    /// Allow but mark for review.
    Flag,
    /// Release.
    Clear,
}

/// Content category a match/scan resolves to. Unit variants so it can key a
/// serde-serialisable `HashMap` (JSON string keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Category {
    /// Known child sexual abuse material (Track 1).
    Csam,
    /// Adult / not-safe-for-work imagery (Track 2, deferred).
    Nsfw,
    /// Illegal speech in subtitles / text (B8).
    IllegalSpeech,
    /// Unrecognised — fail-closed (treated as Block).
    Unknown,
}

/// The kind of asset being moderated (routes the intake logic in `asset.rs`, B8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetKind {
    /// Poster / backdrop / logo still image.
    Image,
    /// A `.vtt` subtitle track (text scan).
    Subtitle,
    /// A keyframe extracted from a transcode (perceptual image match).
    VideoKeyframe,
}

/// A 256-bit PDQ perceptual hash. Carried across the ingest boundary as an opaque
/// value (not "matched content"), so it may live outside the `csam` submodule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Pdq256(pub [u8; 32]);

/// The moderation verdict the node emits (B9). The host-reachable gate releases a
/// job **only** on `Cleared`; everything else holds (§1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    /// Safe to bill / prove / complete / publish.
    Cleared,
    /// Matched a block rule (e.g. CSAM) — withhold + quarantine.
    Blocked,
    /// Needs human review — withhold pending a reviewer decision.
    Flagged,
}

impl Verdict {
    /// The single fail-closed release predicate: ONLY `Cleared` releases a job to
    /// billing / proof / completion / publish. Every other verdict holds (§1.2).
    pub fn releases(&self) -> bool {
        matches!(self, Verdict::Cleared)
    }

    /// The wire string for this verdict (`"cleared"`/`"blocked"`/`"flagged"`). Shared
    /// by the asset + frames response DTOs so they cannot drift.
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::Cleared => "cleared",
            Verdict::Blocked => "blocked",
            Verdict::Flagged => "flagged",
        }
    }
}

/// The `reason` a genuine Track-1 content MATCH sets on a `Blocked` result (the match
/// sentinel). Used to distinguish a real match — which warrants preserving evidence —
/// from a fail-closed HOLD (list unavailable / undecodable / hash-compute failure /
/// empty), whose `Blocked` carries a different reason and must NOT preserve. A shared
/// named constant (not ad-hoc string parsing): the producer (`csam::entry`) and the
/// consumer (`ModerationResult::is_genuine_hit`) reference the same symbol (R8/R9).
pub const REASON_CSAM_MATCH: &str = "csam-match";

/// The result the node records per job/asset (B9). `reason` carries a category or
/// rule id only — **never raw matched content** (CSAM isolation, §2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModerationResult {
    pub verdict: Verdict,
    pub reason: Option<String>,
    pub report_id: Option<String>,
}

impl ModerationResult {
    pub fn cleared() -> Self {
        Self {
            verdict: Verdict::Cleared,
            reason: None,
            report_id: None,
        }
    }
    pub fn blocked(reason: impl Into<String>) -> Self {
        Self {
            verdict: Verdict::Blocked,
            reason: Some(reason.into()),
            report_id: None,
        }
    }
    pub fn flagged(reason: impl Into<String>) -> Self {
        Self {
            verdict: Verdict::Flagged,
            reason: Some(reason.into()),
            report_id: None,
        }
    }

    /// True iff this verdict is a genuine content MATCH/flag that warrants preserving
    /// evidence — as opposed to a fail-closed HOLD (list unavailable / undecodable /
    /// hash-compute failure / empty), which must NOT preserve and is retryable (the API
    /// surfaces it as `503`). A `Flagged` is always a real text flag; a `Blocked` is a
    /// genuine match ONLY when its reason is the match sentinel [`REASON_CSAM_MATCH`]
    /// (an own-hash or NCMEC exact/PDQ hit — including an own-hash hit while the NCMEC
    /// list is unavailable, and an undecodable exact-SHA hit), never a can't-scan reason.
    pub fn is_genuine_hit(&self) -> bool {
        match self.verdict {
            Verdict::Flagged => true,
            Verdict::Blocked => self.reason.as_deref() == Some(REASON_CSAM_MATCH),
            Verdict::Cleared => false,
        }
    }
}

/// The outcome of a Track-1 match. `distance` is the PDQ Hamming distance for a
/// near-match (`Some(0)` for an exact hit), `None` when no distance was computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchResult {
    pub is_match: bool,
    pub distance: Option<u32>,
}

impl MatchResult {
    pub fn no_match() -> Self {
        Self {
            is_match: false,
            distance: None,
        }
    }
    /// An exact (bit-identical / own-hash) hit.
    pub fn exact() -> Self {
        Self {
            is_match: true,
            distance: Some(0),
        }
    }
    /// A PDQ near-match at the given Hamming distance.
    pub fn near(distance: u32) -> Self {
        Self {
            is_match: true,
            distance: Some(distance),
        }
    }
}
