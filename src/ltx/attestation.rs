// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Fixed-field attestation: `inputCommitment`, `envHash`, `sigDigest` (all
//! `keccak256(abi.encode(...))` — reproduced byte-identically by an ABI encoder
//! on either side, never canonical JSON), the EIP-191 provenance signature, and
//! `proofHash = SHA256(stored bytes)` (NOT keccak; matches the dispute check).

use anyhow::{anyhow, Result};
use ethers::abi::{encode, Token};
use ethers::types::{Address, U256};
use ethers::utils::keccak256;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::str::FromStr;

use crate::ltx::types::{Attestation, FrameManifest, LtxJob};

/// Reproduction environment hashed into `envHash` (spec §9).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvMeta {
    pub weights_hash: String,
    pub lora_hash: String,
    pub comfy_commit: String,
    pub node_commit: String,
    pub cuda_version: String,
    pub gpu_class: String,
}

fn keccak_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(keccak256(bytes)))
}

/// Decode a `0x`-prefixed bytes32 string, validating the exact length.
/// CRITICAL: `ethers::abi::encode` does NOT reject a mis-sized `FixedBytes` — it
/// silently mis-pads — so the length check is what keeps the bytes reproducible.
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

/// Parse a `0x`-prefixed hex `sessionId` into uint256. The `0x` prefix is
/// REQUIRED: a bare even-length hex string would be read as hex here but as
/// decimal by an SDK doing `BigInt(sessionId)`, a silent divergence — so we
/// reject the ambiguous form rather than guess.
fn session_u256(s: &str) -> Result<U256> {
    let raw = s
        .strip_prefix("0x")
        .ok_or_else(|| anyhow!("sessionId must be 0x-prefixed hex: {s:?}"))?;
    let bytes = hex::decode(raw).map_err(|e| anyhow!("invalid sessionId {s:?}: {e}"))?;
    if bytes.len() > 32 {
        return Err(anyhow!("sessionId exceeds 32 bytes: {s:?}"));
    }
    let mut buf = [0u8; 32];
    buf[32 - bytes.len()..].copy_from_slice(&bytes);
    Ok(U256::from_big_endian(&buf))
}

/// `inputCommitment = keccak256(abi.encode(string prompt, uint256 seed,
/// uint32 frames, uint32 fps, uint32 w, uint32 h, string lora))`. seed parsed
/// from the decimal-string wire value to uint256.
pub fn input_commitment(job: &LtxJob) -> Result<String> {
    let seed = job.seed_u256().map_err(|e| anyhow!(e))?;
    let tokens = [
        Token::String(job.prompt.clone()),
        Token::Uint(seed),
        Token::Uint(U256::from(job.frames)),
        Token::Uint(U256::from(job.fps)),
        Token::Uint(U256::from(job.resolution.w)),
        Token::Uint(U256::from(job.resolution.h)),
        Token::String(job.lora.clone()),
    ];
    Ok(keccak_hex(&encode(&tokens)))
}

/// `inputCommitment` **v2** (image-conditioned templates, M1a): the M0 seven
/// fields plus a trailing `bytes32[] imageHashes` in job order, where
/// `imageHashes[i] = keccak256(plaintext bytes of images[i])`. Used ONLY for
/// templates whose bundle entry has `imageInputs > 0` (i2v/flf2v/style_transition).
///
/// CRITICAL: appending a dynamic field shifts the ABI head offsets of the earlier
/// dynamic fields (`prompt`, `lora`), so this is NOT byte-equal to
/// [`input_commitment`] even when `image_hashes` is empty. t2v MUST keep calling
/// the seven-field [`input_commitment`]; never route it through here with an empty
/// slice (see [`commitment_for`], which enforces the split).
pub fn input_commitment_v2(job: &LtxJob, image_hashes: &[[u8; 32]]) -> Result<String> {
    let seed = job.seed_u256().map_err(|e| anyhow!(e))?;
    let hashes = Token::Array(
        image_hashes
            .iter()
            .map(|h| Token::FixedBytes(h.to_vec()))
            .collect(),
    );
    let tokens = [
        Token::String(job.prompt.clone()),
        Token::Uint(seed),
        Token::Uint(U256::from(job.frames)),
        Token::Uint(U256::from(job.fps)),
        Token::Uint(U256::from(job.resolution.w)),
        Token::Uint(U256::from(job.resolution.h)),
        Token::String(job.lora.clone()),
        hashes,
    ];
    Ok(keccak_hex(&encode(&tokens)))
}

/// `inputCommitment` **v3** (video-conditioned templates, BL3): the v2 eight
/// fields plus a trailing `bytes32[] videoHashes` in job order, where
/// `videoHashes[i] = keccak256(plaintext bytes of videos[i])`. Used ONLY for
/// templates whose bundle entry has `videoInputs > 0` (iclora).
///
/// CRITICAL: the same byte-inequality rule as the v1/v2 split — appending the
/// dynamic `videoHashes` shifts the earlier dynamic heads, so v3 with an empty
/// `video_hashes` is NOT byte-equal to v2. Templates without video inputs MUST
/// keep their v1/v2 form; never route an empty `video_hashes` through here (see
/// [`commitment_for`], which enforces the split).
pub fn input_commitment_v3(
    job: &LtxJob,
    image_hashes: &[[u8; 32]],
    video_hashes: &[[u8; 32]],
) -> Result<String> {
    let seed = job.seed_u256().map_err(|e| anyhow!(e))?;
    let as_array = |hashes: &[[u8; 32]]| {
        Token::Array(
            hashes
                .iter()
                .map(|h| Token::FixedBytes(h.to_vec()))
                .collect(),
        )
    };
    let tokens = [
        Token::String(job.prompt.clone()),
        Token::Uint(seed),
        Token::Uint(U256::from(job.frames)),
        Token::Uint(U256::from(job.fps)),
        Token::Uint(U256::from(job.resolution.w)),
        Token::Uint(U256::from(job.resolution.h)),
        Token::String(job.lora.clone()),
        as_array(image_hashes),
        as_array(video_hashes),
    ];
    Ok(keccak_hex(&encode(&tokens)))
}

/// Format-selected commitment, keyed by the template's input counts. No videos
/// and no images (t2v) → the byte-identical M0 seven-field [`input_commitment`];
/// images only (i2v/flf2v/style_transition) → [`input_commitment_v2`]; any video
/// (iclora) → [`input_commitment_v3`]. The emptiness tests are exactly the
/// bundle's `videoInputs`/`imageInputs` selectors once the upstream
/// `videos.len() == videoInputs` / `images.len() == imageInputs` checks have
/// run, and they are what keep deployed M0/M1a attestations byte-for-byte
/// unchanged.
pub fn commitment_for(
    job: &LtxJob,
    image_hashes: &[[u8; 32]],
    video_hashes: &[[u8; 32]],
) -> Result<String> {
    if !video_hashes.is_empty() {
        input_commitment_v3(job, image_hashes, video_hashes)
    } else if !image_hashes.is_empty() {
        input_commitment_v2(job, image_hashes)
    } else {
        input_commitment(job)
    }
}

/// `outputCommitment = keccak256(utf8 bytes of the outputCID STRING)` — the CID
/// string incl. its multibase prefix, NOT the decoded multibase payload.
pub fn output_commitment(output_cid: &str) -> [u8; 32] {
    keccak256(output_cid.as_bytes())
}

/// `envHash` over the six reproduction fields. abi.encode is length-prefixed, so
/// no two field-boundary splittings can collide (any single change moves it).
pub fn env_hash(meta: &EnvMeta) -> String {
    let tokens = [
        Token::String(meta.weights_hash.clone()),
        Token::String(meta.lora_hash.clone()),
        Token::String(meta.comfy_commit.clone()),
        Token::String(meta.node_commit.clone()),
        Token::String(meta.cuda_version.clone()),
        Token::String(meta.gpu_class.clone()),
    ];
    keccak_hex(&encode(&tokens))
}

/// `sigDigest = keccak256(abi.encode(bytes32 modelId, bytes32 templateHash,
/// bytes32 envHash, bytes32 inputCommitment, bytes32 outputCommitment,
/// uint256 sessionId, address host, uint256 timestamp))`.
pub fn sig_digest(att: &Attestation) -> Result<[u8; 32]> {
    let output_commit = output_commitment(&att.output_cid);
    let session = session_u256(&att.session_id)?;
    let host =
        Address::from_str(&att.host).map_err(|e| anyhow!("invalid host {:?}: {e}", att.host))?;
    let tokens = [
        Token::FixedBytes(bytes32(&att.model_id)?),
        Token::FixedBytes(bytes32(&att.template_hash)?),
        Token::FixedBytes(bytes32(&att.env_hash)?),
        Token::FixedBytes(bytes32(&att.input_commitment)?),
        Token::FixedBytes(output_commit.to_vec()),
        Token::Uint(session),
        Token::Address(host),
        Token::Uint(U256::from(att.timestamp)),
    ];
    Ok(keccak256(encode(&tokens)))
}

/// Assemble the attestation: compute `inputCommitment` (format-selected by
/// `image_hashes`/`video_hashes` via [`commitment_for`] — both empty ⇒ the
/// byte-identical M0 seven-field form, images only ⇒ v2, any video ⇒ v3), and
/// if a node key is provided, EIP-191-sign `sigDigest`. The signature is set
/// BEFORE the bytes are taken, because `proofHash` is SHA256 over the stored
/// bytes INCLUDING it.
#[allow(clippy::too_many_arguments)]
pub fn assemble(
    model_id: String,
    template_hash: String,
    env_hash: String,
    job: &LtxJob,
    image_hashes: &[[u8; 32]],
    video_hashes: &[[u8; 32]],
    output_cid: String,
    manifest: FrameManifest,
    session_id: String,
    host: String,
    timestamp: u64,
    node_key: Option<[u8; 32]>,
) -> Result<Attestation> {
    let mut att = Attestation {
        model_id,
        template_hash,
        env_hash,
        input_commitment: commitment_for(job, image_hashes, video_hashes)?,
        output_cid,
        manifest,
        session_id,
        host,
        timestamp,
        signature: None,
    };
    if let Some(key) = node_key {
        let digest = sig_digest(&att)?;
        let sig = crate::crypto::proof_signer::sign_eip191_digest(&key, digest)?;
        att.signature = Some(format!("0x{}", hex::encode(sig)));
    }
    Ok(att)
}

/// `proofHash = SHA256(the exact stored attestation bytes)` — NOT keccak. The
/// dispute check is `SHA256(fetched bytes) == on-chain hash`.
pub fn proof_hash(att: &Attestation) -> [u8; 32] {
    Sha256::digest(att.stored_bytes()).into()
}
