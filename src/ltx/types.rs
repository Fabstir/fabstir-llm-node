// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Shared types for the LTX 2.3 generation sidecar: the job contract (A), the
//! keyless frame manifest, and the attestation (B). Wire keys are camelCase to
//! match the authoritative seam (`docs/sdk-reference/LTX-SIDECAR-M0-INTERFACE.md`).

use ethers::types::U256;
use serde::{Deserialize, Serialize};

/// Output resolution in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    pub w: u32,
    pub h: u32,
}

/// The single output kind supported in M0 (HDR EXR image sequence).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputKind {
    ExrSequence,
}

/// Job contract A (M0, prompt-only). `seed` is a decimal STRING on the wire
/// (a JSON float64 corrupts values above 2^53), parsed to `U256` inside
/// `inputCommitment`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LtxJob {
    pub template_id: String,
    pub template_hash: String,
    pub prompt: String,
    pub seed: String,
    pub frames: u32,
    pub fps: u32,
    pub resolution: Resolution,
    pub lora: String,
    pub output: OutputKind,
}

impl LtxJob {
    /// Parse the decimal-string `seed` into the `U256` used inside
    /// `inputCommitment`. Rejects any non-decimal value (e.g. a `0x` hex form).
    pub fn seed_u256(&self) -> Result<U256, String> {
        U256::from_dec_str(&self.seed).map_err(|e| format!("invalid seed {:?}: {e}", self.seed))
    }
}

/// PUBLIC, KEYLESS frame manifest. Commits to ciphertext frame hashes and a
/// Merkle root; carries NO capability CIDs and NO keys (those ride the
/// encrypted `ltx_complete`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameManifest {
    pub frame_count: u32,
    pub fps: u32,
    pub resolution: Resolution,
    pub colour_encoding: String,
    /// Keyless: each entry is `keccak256(ciphertext bytes)` of one frame.
    pub frame_hashes: Vec<String>,
    /// keccak Merkle root over `frame_hashes`.
    pub merkle_root: String,
}

/// Proof/attestation B. Stored PLAINTEXT on S5; its CID is `proofCID` and its
/// exact bytes are SHA256-hashed for the on-chain `proofHash`. Keyless: the
/// key-bearing capability CIDs are delivered only in the encrypted
/// `ltx_complete`, never in this public object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attestation {
    pub model_id: String,
    pub template_hash: String,
    pub env_hash: String,
    pub input_commitment: String,
    /// Public, keyless manifest CID. `camelCase` would wrongly give `outputCid`.
    #[serde(rename = "outputCID")]
    pub output_cid: String,
    pub manifest: FrameManifest,
    pub session_id: String,
    pub host: String,
    pub timestamp: u64,
    /// Off-chain EIP-191 provenance over the fixed-field `sigDigest` (Phase 6),
    /// NOT over this JSON. `None` when the node has no signing key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl Attestation {
    /// The exact bytes uploaded to `proofCID` and SHA256-hashed for the on-chain
    /// `proofHash`. Deterministic: serde serialises struct fields in declaration
    /// order, so repeated calls are byte-identical.
    pub fn stored_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("attestation serialises to JSON")
    }
}
