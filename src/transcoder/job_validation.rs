// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Job validation — format spec hashing and modelId generation for contract reuse.

use tiny_keccak::{Hasher, Keccak};

use super::types::VideoFormat;

/// Produce a canonical JSON string from formats (sorted by `id`, sorted keys).
pub fn canonical_format_spec(formats: &[VideoFormat]) -> String {
    let mut sorted: Vec<&VideoFormat> = formats.iter().collect();
    sorted.sort_by_key(|f| f.id);
    serde_json::to_string(&sorted).unwrap_or_default()
}

/// Compute the transcoding `modelId` as keccak256 of the canonical format spec.
/// This is used with `createSessionFromDepositForModel` on the contract.
pub fn compute_transcode_model_id(formats: &[VideoFormat]) -> [u8; 32] {
    let spec = canonical_format_spec(formats);
    let mut hasher = Keccak::v256();
    let mut out = [0u8; 32];
    hasher.update(spec.as_bytes());
    hasher.finalize(&mut out);
    out
}
