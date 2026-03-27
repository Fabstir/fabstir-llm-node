// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Transcoding checkpoint submission — billing token conversion and checkpoint building.

use serde::{Deserialize, Serialize};

/// A checkpoint for on-chain submission after a batch of GOPs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscodeCheckpoint {
    pub gop_index: u32,
    pub billing_tokens: u64,
    pub proof_hash: [u8; 32],
    pub proof_cid: String,
}

/// Convert billing units to on-chain tokens (multiply by 1000, ceiling).
pub fn billing_units_to_tokens(billing_units: f64) -> u64 {
    (billing_units * 1000.0).ceil() as u64
}

/// Build a checkpoint from GOP index, billing units, proof hash, and CID.
pub fn build_checkpoint(
    gop_index: u32,
    billing_units: f64,
    proof_hash: [u8; 32],
    proof_cid: &str,
) -> TranscodeCheckpoint {
    TranscodeCheckpoint {
        gop_index,
        billing_tokens: billing_units_to_tokens(billing_units),
        proof_hash,
        proof_cid: proof_cid.to_string(),
    }
}
