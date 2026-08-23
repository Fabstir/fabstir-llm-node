// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Fixed-field encoders for training slices (interface B.4/B.5; FROZEN — the
//! interface's Status line is the version authority):
//! `inputCommitment`, the per-slice `sigDigest`, and the canonical-manifest
//! SHA256. All `keccak256(abi.encode(...))` / SHA256-over-exact-bytes — never
//! canonical JSON inside anything signed, mirroring `ltx::attestation`.

use anyhow::{anyhow, Result};
use ethers::abi::{encode, Token};
use ethers::types::{Address, U256};
use ethers::utils::keccak256;
use sha2::{Digest, Sha256};
use std::str::FromStr;

use crate::training::types::TrainingJob;

fn keccak_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(keccak256(bytes)))
}

/// Decode a `0x`-prefixed bytes32 string, validating the exact length — the
/// same guard as `ltx::attestation::bytes32` (`ethers::abi::encode` silently
/// mis-pads a wrong-sized `FixedBytes`, so the length check IS the
/// reproducibility guarantee).
fn bytes32(s: &str) -> Result<Vec<u8>> {
    let raw = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(raw).map_err(|e| anyhow!("invalid bytes32 {s:?}: {e}"))?;
    if bytes.len() != 32 {
        return Err(anyhow!(
            "expected 32-byte value, got {} bytes from {s:?}",
            bytes.len()
        ));
    }
    Ok(bytes)
}

/// The B.4 commitment token list, in the FROZEN order — the single
/// transcription of the field list on the node side (the SDK reproduces it from
/// the same interface section; `tests/training/vectors/input-commitment.json`
/// pins the bytes both must produce).
fn commitment_tokens(job: &TrainingJob) -> Result<Vec<Token>> {
    let seed = job.hyper.seed_u256().map_err(|e| anyhow!(e))?;
    Ok(vec![
        Token::FixedBytes(bytes32(&job.template_hash)?),
        Token::FixedBytes(bytes32(&job.dataset.manifest_sha256)?),
        Token::Uint(U256::from(job.dataset.declared_tokens)),
        Token::Uint(U256::from(job.epochs)),
        Token::Uint(U256::from(job.hyper.rank)),
        Token::Uint(U256::from(job.hyper.alpha)),
        Token::String(job.hyper.lr.clone()),
        Token::Uint(seed),
        Token::Uint(U256::from(job.hyper.seq_len)),
    ])
}

/// The exact `abi.encode` bytes of B.4 (exposed for the vector test's
/// byte-level assertion, not just the hash).
pub fn input_commitment_bytes(job: &TrainingJob) -> Result<Vec<u8>> {
    Ok(encode(&commitment_tokens(job)?))
}

/// `inputCommitment = keccak256(abi.encode(bytes32 templateHash, bytes32
/// datasetManifestSha256, uint256 declaredTokens, uint32 epochs, uint32 rank,
/// uint32 alpha, string lr, uint256 seed, uint32 seqLen))` (interface B.4).
pub fn input_commitment(job: &TrainingJob) -> Result<String> {
    Ok(keccak_hex(&input_commitment_bytes(job)?))
}

/// Inputs to the per-slice B.5 digest. `checkpoint_manifest_sha256` is the
/// artifact-manifest hash of THIS slice's checkpoint (the final slice's is the
/// adapter manifest).
#[derive(Debug, Clone)]
pub struct SliceSigFields {
    pub model_id: String,
    pub template_hash: String,
    pub env_hash: String,
    pub input_commitment: String,
    pub checkpoint_manifest_sha256: String,
    pub slice_index: u64,
    pub tokens_delta: u64,
    /// u64 because jobIds are u64 on this chain; the B.3 attestation renders it
    /// as a 0x-hex STRING (wire ground rule) — widen via the LTX
    /// `session_u256` pattern if a wider id ever appears.
    pub session_id: u64,
    pub host: String,
    pub timestamp: u64,
}

/// `sigDigest = keccak256(abi.encode(bytes32 modelId, bytes32 templateHash,
/// bytes32 envHash, bytes32 inputCommitment, bytes32 checkpointManifestSha256,
/// uint256 sliceIndex, uint256 tokensDelta, uint256 sessionId, address host,
/// uint256 timestamp))` (interface B.5). Sign with
/// `crypto::proof_signer::sign_eip191_digest`; recover with
/// `verifyMessage(getBytes(digest), sig)` on the SDK side.
pub fn sig_digest(f: &SliceSigFields) -> Result<[u8; 32]> {
    let host = Address::from_str(&f.host).map_err(|e| anyhow!("invalid host {:?}: {e}", f.host))?;
    let tokens = [
        Token::FixedBytes(bytes32(&f.model_id)?),
        Token::FixedBytes(bytes32(&f.template_hash)?),
        Token::FixedBytes(bytes32(&f.env_hash)?),
        Token::FixedBytes(bytes32(&f.input_commitment)?),
        Token::FixedBytes(bytes32(&f.checkpoint_manifest_sha256)?),
        Token::Uint(U256::from(f.slice_index)),
        Token::Uint(U256::from(f.tokens_delta)),
        Token::Uint(U256::from(f.session_id)),
        Token::Address(host),
        Token::Uint(U256::from(f.timestamp)),
    ];
    Ok(keccak256(encode(&tokens)))
}

/// The D-section canonicalisation: recursively key-sorted, compact, UTF-8 —
/// `checkpoint::delta::sort_json_keys` + serde_json's compact writer, the same
/// pair behind `templateHash`. Returns `"0x" + SHA256(canonical bytes)`. The
/// STORED manifest bytes must equal this canonical form (D.2: "the stored bytes
/// ARE the canonical form"), so the hash doubles as the storage-shape check.
///
/// WRITE-PATH ONLY. Never verify a FETCHED manifest by parsing and calling
/// this — hash the raw stored bytes (D.2: "no re-canonicalisation on read,
/// ever"). Re-canonicalising a peer's bytes reintroduces exactly the
/// cross-serialiser divergence the stored-bytes rule exists to contain.
pub fn canonical_manifest_sha256(manifest: &serde_json::Value) -> String {
    let canonical = crate::checkpoint::delta::sort_json_keys(manifest).to_string();
    format!("0x{}", hex::encode(Sha256::digest(canonical.as_bytes())))
}

/// The canonical bytes themselves (for writing a manifest to storage and for
/// the vector test's byte-level assertion).
pub fn canonical_manifest_bytes(manifest: &serde_json::Value) -> String {
    crate::checkpoint::delta::sort_json_keys(manifest).to_string()
}

// ---------------------------------------------------------------------------
// T4.d — the B.3 slice attestation: build + sign + canonical store bytes.
// ---------------------------------------------------------------------------

/// Everything a slice attestation carries (interface B.3). `adapter_manifest_
/// sha256` + `moderation` ride the FINAL slice only.
#[derive(Debug, Clone)]
pub struct SliceAttestationInputs<'a> {
    pub job: &'a TrainingJob,
    pub model_id: String,
    pub template_hash: String,
    pub env_hash: String,
    pub slice_index: u64,
    pub step_from: u64,
    pub step_to: u64,
    pub tokens_delta: u64,
    pub cumulative_tokens: u64,
    pub checkpoint_manifest_sha256: String,
    pub adapter_manifest_sha256: Option<String>,
    /// (status, policyVersion) — final slice only (CK-4).
    pub moderation: Option<(String, String)>,
    pub session_id: u64,
    pub host: String,
    pub timestamp: u64,
}

/// Build the B.3 attestation, sign its B.5 digest with the node key, and
/// return `(json, stored_bytes)` where `stored_bytes` is the CANONICAL
/// serialisation — `proofHash = SHA256(stored_bytes)` and the uploaded bytes
/// MUST be exactly these (the dispute-time check hashes raw fetched bytes).
pub fn build_slice_attestation(
    inputs: &SliceAttestationInputs<'_>,
    node_private_key: &[u8; 32],
) -> Result<(serde_json::Value, Vec<u8>)> {
    let input_commitment_hex = input_commitment(inputs.job)?;
    let digest = sig_digest(&SliceSigFields {
        model_id: inputs.model_id.clone(),
        template_hash: inputs.template_hash.clone(),
        env_hash: inputs.env_hash.clone(),
        input_commitment: input_commitment_hex.clone(),
        checkpoint_manifest_sha256: inputs.checkpoint_manifest_sha256.clone(),
        slice_index: inputs.slice_index,
        tokens_delta: inputs.tokens_delta,
        session_id: inputs.session_id,
        host: inputs.host.clone(),
        timestamp: inputs.timestamp,
    })?;
    let signature = crate::crypto::proof_signer::sign_eip191_digest(node_private_key, digest)?;

    let mut attestation = serde_json::json!({
        "modelId": inputs.model_id,
        "templateHash": inputs.template_hash,
        "envHash": inputs.env_hash,
        "inputCommitment": input_commitment_hex,
        "sliceIndex": inputs.slice_index,
        "stepFrom": inputs.step_from,
        "stepTo": inputs.step_to,
        "tokensDelta": inputs.tokens_delta,
        "cumulativeTokens": inputs.cumulative_tokens,
        "checkpointManifestSha256": inputs.checkpoint_manifest_sha256,
        "sessionId": format!("0x{:x}", inputs.session_id),
        "host": inputs.host,
        "timestamp": inputs.timestamp,
        "signature": format!("0x{}", hex::encode(signature)),
    });
    if let Some(adapter_sha) = &inputs.adapter_manifest_sha256 {
        attestation["adapterManifestSha256"] = serde_json::json!(adapter_sha);
    }
    if let Some((status, policy_version)) = &inputs.moderation {
        attestation["moderation"] = serde_json::json!({
            "status": status,
            "policyVersion": policy_version,
        });
    }
    let stored = canonical_manifest_bytes(&attestation).into_bytes();
    Ok((attestation, stored))
}

/// Upload the attestation plaintext; returns `(proofCID, proofHash)` with
/// the hash over the EXACT uploaded bytes (the LTX `upload_attestation`
/// convention).
pub async fn upload_slice_attestation(
    s5: &dyn crate::storage::s5_client::S5Storage,
    s5_path: &str,
    stored_bytes: Vec<u8>,
) -> Result<(String, [u8; 32])> {
    use sha2::Digest;
    let proof_hash: [u8; 32] = sha2::Sha256::digest(&stored_bytes).into();
    let proof_cid = s5
        .put(s5_path, stored_bytes)
        .await
        .map_err(|e| anyhow!("attestation upload failed: {e}"))?;
    Ok((proof_cid, proof_hash))
}
