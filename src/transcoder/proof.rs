// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! GOP proof building — hash computation, proof assembly, S5 serialization.

use anyhow::Result;
use tiny_keccak::{Hasher, Keccak};

use super::types::{GOPProof, QualityMetrics, VideoFormat};
use crate::crypto::ezkl::witness::WitnessBuilder;

fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    let mut out = [0u8; 32];
    hasher.update(data);
    hasher.finalize(&mut out);
    out
}

/// Compute a deterministic hash of codec/format parameters.
/// Formats are sorted by `id` before hashing for order independence.
pub fn compute_codec_params_hash(formats: &[VideoFormat]) -> [u8; 32] {
    let mut sorted: Vec<&VideoFormat> = formats.iter().collect();
    sorted.sort_by_key(|f| f.id);
    let canonical = serde_json::to_string(&sorted).unwrap_or_default();
    keccak256(canonical.as_bytes())
}

/// Build a GOPProof from inputs and quality metrics.
pub fn build_gop_proof(
    gop_index: u32,
    input_hash: [u8; 32],
    output_hash: [u8; 32],
    metrics: &QualityMetrics,
) -> GOPProof {
    GOPProof {
        gop_index,
        input_gop_hash: hex::encode(input_hash),
        output_gop_hash: hex::encode(output_hash),
        psnr_db: metrics.psnr_db,
        ssim: metrics.ssim,
        actual_bitrate: metrics.actual_bitrate,
        stark_proof_hash: String::new(),
    }
}

/// Serialize a GOPProof + STARK proof bytes for S5 upload.
/// Format: 4-byte JSON length (big-endian) + JSON bytes + raw STARK proof bytes.
pub fn serialize_proof_for_s5(gop_proof: &GOPProof, stark_proof_bytes: &[u8]) -> Vec<u8> {
    let json_bytes = serde_json::to_vec(gop_proof).unwrap_or_default();
    let json_len = (json_bytes.len() as u32).to_be_bytes();
    let mut out = Vec::with_capacity(4 + json_bytes.len() + stark_proof_bytes.len());
    out.extend_from_slice(&json_len);
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(stark_proof_bytes);
    out
}

/// Compute keccak256 hash of proof data bytes.
pub fn compute_proof_hash(proof_data: &[u8]) -> [u8; 32] {
    keccak256(proof_data)
}

/// Generate a STARK proof for a GOP using the existing Risc0 4-hash witness.
///
/// Reuses the LLM inference prover with transcoding semantics:
/// - `job_id` → transcode job ID (zero-padded to 32 bytes)
/// - `model_hash` → codec params hash
/// - `input_hash` → source GOP hash
/// - `output_hash` → transcoded GOP hash
pub fn generate_gop_stark_proof(
    job_id: u64,
    codec_params_hash: [u8; 32],
    input_gop_hash: [u8; 32],
    output_gop_hash: [u8; 32],
) -> Result<Vec<u8>> {
    let mut job_id_bytes = [0u8; 32];
    job_id_bytes[24..].copy_from_slice(&job_id.to_be_bytes());

    let witness = WitnessBuilder::new()
        .with_job_id(job_id_bytes)
        .with_model_hash(codec_params_hash)
        .with_input_hash(input_gop_hash)
        .with_output_hash(output_gop_hash)
        .build()?;

    let proof_data = crate::crypto::ezkl::prover::generate_proof(&witness, None)?;
    Ok(proof_data.proof_bytes)
}
