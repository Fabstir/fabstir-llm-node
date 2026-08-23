// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Wire types for the `train` action (interface Contract A; FROZEN — the
//! interface's Status line is the version authority).
//!
//! A.1's numeric wire rule is enforced STRUCTURALLY: every numeric field is a
//! non-Option primitive, so a JSON `null` (the NaN → `JSON.stringify` shape) or
//! a missing member FAILS deserialisation instead of silently defaulting — the
//! LTX advisory fields' `Option<f64> + serde(default)` pattern is deliberately
//! NOT used here. `lr` and `seed` are decimal STRINGS, committed byte-for-byte.

use ethers::types::U256;
use serde::{Deserialize, Serialize};

/// The `train` wire job (interface A.1). Unknown keys are tolerated (the node
/// ignores extra members, per protocol); known members are strict.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainingJob {
    pub template_id: String,
    /// "0x" + keccak256 of the canonical template JSON.
    pub template_hash: String,
    pub dataset: TrainingDataset,
    /// 1..=bounds.maxEpochs.
    pub epochs: u32,
    pub hyper: TrainingHyper,
    /// Fixed in M0: "adapter-v1".
    pub output: String,
}

/// The dataset reference (interface A.1 / D.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainingDataset {
    /// Capability CID of the encrypted `dataset-manifest-v1`.
    #[serde(rename = "manifestCID")]
    pub manifest_cid: String,
    /// SHA256 of the exact stored (canonical) manifest bytes.
    pub manifest_sha256: String,
    /// count-v1 total over all samples; the billing basis with `epochs`.
    pub declared_tokens: u64,
    /// JSONL line count; cross-checked against the manifest.
    pub samples: u64,
}

/// Hyper-parameters (interface A.1); every value validated against the
/// template's pinned lists/ranges at accept (T3), not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainingHyper {
    pub rank: u32,
    pub alpha: u32,
    /// DECIMAL STRING, e.g. "0.000200" — committed byte-for-byte as sent (no
    /// normalisation, no exponent form). See [`TrainingHyper::lr_is_canonical`].
    pub lr: String,
    /// DECIMAL STRING (uint256 in the commitment — the LTX seed rule).
    pub seed: String,
    pub seq_len: u32,
}

impl TrainingHyper {
    /// Parse the decimal-string seed into uint256 (mirrors `LtxJob::seed_u256`).
    pub fn seed_u256(&self) -> Result<U256, String> {
        U256::from_dec_str(&self.seed).map_err(|e| format!("invalid seed {:?}: {e}", self.seed))
    }

    /// A.1's `lr` regex `^[0-9]+(\.[0-9]+)?$`, checked without a regex dep:
    /// ASCII digits with at most one interior dot, digits on both sides. The
    /// exponent form and empty parts are rejected — the committed bytes must be
    /// exactly what a canonical decimal renders.
    pub fn lr_is_canonical(&self) -> bool {
        let s = self.lr.as_bytes();
        if s.is_empty() {
            return false;
        }
        let mut parts = self.lr.splitn(2, '.');
        let int_part = parts.next().unwrap_or("");
        let frac = parts.next();
        let all_digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
        all_digits(int_part) && frac.is_none_or(all_digits)
    }
}
