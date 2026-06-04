// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! `ModerationConfig` — data-driven thresholds & policy with fail-closed defaults.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::moderation::types::{Category, Disposition, ModerationError, Result};

/// Legal floor for quarantine retention (B6).
pub const MIN_RETENTION_DAYS: u32 = 90;
/// A PDQ hash is 256 bits, so the max meaningful Hamming distance is 256.
pub const MAX_PDQ_DISTANCE: u32 = 256;

/// Tunable moderation parameters. Defaults are fail-closed: retention ≥ 90 days,
/// and any category not in the disposition map resolves to `Block` (§0.2 / B6).
///
/// Deserialization is validated ([`Self::validate`]): a config loaded from JSON
/// with `retention_days < 90` or `pdq_max_distance > 256` is **rejected**, not
/// silently accepted (fail-closed at the untrusted config-file boundary). Code
/// that mutates the public fields directly should re-run `validate()`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawModerationConfig")]
pub struct ModerationConfig {
    /// PDQ near-match Hamming-distance threshold (0..=256). Default ~31/256 (Meta guidance).
    pub pdq_max_distance: u32,
    /// Quarantine retention floor in days (legal minimum 90).
    pub retention_days: u32,
    /// NCMEC hash-list refresh cadence, seconds.
    pub refresh_interval_secs: u64,
    /// Per-category policy. Any category absent here ⇒ fail-closed `Block`
    /// (see [`Self::disposition_for`]).
    pub disposition: HashMap<Category, Disposition>,
}

impl Default for ModerationConfig {
    fn default() -> Self {
        let mut disposition = HashMap::new();
        disposition.insert(Category::Csam, Disposition::Block);
        disposition.insert(Category::IllegalSpeech, Disposition::Block);
        disposition.insert(Category::Nsfw, Disposition::Flag);
        // Category::Unknown is intentionally NOT inserted — `disposition_for`
        // returns the fail-closed default (Block) for any unmapped category.
        Self {
            pdq_max_distance: 31,
            retention_days: MIN_RETENTION_DAYS,
            refresh_interval_secs: 3600,
            disposition,
        }
    }
}

impl ModerationConfig {
    /// Disposition for a category, fail-closed: any unmapped category ⇒ `Block`.
    pub fn disposition_for(&self, category: Category) -> Disposition {
        self.disposition
            .get(&category)
            .copied()
            .unwrap_or(Disposition::Block)
    }

    /// Enforce the fail-closed invariants. Returns `Err` if retention is below the
    /// legal floor or the PDQ distance is not a valid Hamming distance.
    pub fn validate(&self) -> Result<()> {
        if self.retention_days < MIN_RETENTION_DAYS {
            return Err(ModerationError::Internal(format!(
                "retention_days {} below legal floor {}",
                self.retention_days, MIN_RETENTION_DAYS
            )));
        }
        if self.pdq_max_distance > MAX_PDQ_DISTANCE {
            return Err(ModerationError::Internal(format!(
                "pdq_max_distance {} exceeds max {}",
                self.pdq_max_distance, MAX_PDQ_DISTANCE
            )));
        }
        Ok(())
    }
}

/// Unvalidated wire mirror used only to run [`ModerationConfig::validate`] during
/// deserialization (so a bad config file is rejected, not silently accepted).
#[derive(Deserialize)]
struct RawModerationConfig {
    pdq_max_distance: u32,
    retention_days: u32,
    refresh_interval_secs: u64,
    disposition: HashMap<Category, Disposition>,
}

impl TryFrom<RawModerationConfig> for ModerationConfig {
    type Error = String;

    fn try_from(raw: RawModerationConfig) -> std::result::Result<Self, String> {
        let cfg = ModerationConfig {
            pdq_max_distance: raw.pdq_max_distance,
            retention_days: raw.retention_days,
            refresh_interval_secs: raw.refresh_interval_secs,
            disposition: raw.disposition,
        };
        cfg.validate().map_err(|e| e.to_string())?;
        Ok(cfg)
    }
}
