// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 2 — encrypted-model container format (header + chunked AEAD body).
//!
//! On-disk / S5 layout: `[ContainerHeader (HEADER_LEN bytes)] [chunk_0] [chunk_1] …`
//! where each chunk is an XChaCha20-Poly1305 ciphertext (Phase 2.2). The header
//! is a fixed-size, explicitly-encoded binary record (not serde) so the wire
//! format is stable and self-describing via its magic + version, and so a
//! malformed prefix fails closed at parse time rather than mis-decrypting.

use crate::crypto::{decrypt_with_aead, encrypt_with_aead};
use crate::tee::types::{TeeError, TeeResult};
use rand::{rngs::OsRng, RngCore};
use std::io::Write;

/// Container magic — identifies a Fabstir TEE encrypted-model container.
pub const CONTAINER_MAGIC: [u8; 8] = *b"FABS-TEE";
/// Current container format version (`decode` rejects anything else — fail-closed).
pub const CONTAINER_VERSION: u16 = 1;
/// Fixed encoded length of a [`ContainerHeader`], in bytes:
/// 8 (magic) + 2 (version) + 32 (model_id) + 4 (chunk_size) + 4 (num_chunks)
/// + 16 (nonce_base) + 32 (policy_hash) = 98.
pub const HEADER_LEN: usize = 8 + 2 + 32 + 4 + 4 + 16 + 32;
/// XChaCha20-Poly1305 authentication-tag length appended to every sealed chunk.
pub const AEAD_TAG_LEN: usize = 16;

/// Fixed-size header prefixing an encrypted-model container.
///
/// `nonce_base` + `model_id` + `policy_hash` are the inputs the chunked AEAD
/// (Phase 2.2/2.3) binds each chunk to: per-chunk nonce =
/// `nonce_base ‖ chunk_idx_u32_be ‖ 0x00×4` (24 B), AAD =
/// `model_id ‖ policy_hash ‖ chunk_idx_u32_be`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerHeader {
    /// Format magic; must equal [`CONTAINER_MAGIC`].
    pub magic: [u8; 8],
    /// Format version; must equal [`CONTAINER_VERSION`].
    pub version: u16,
    /// The model these weights belong to (bound into per-chunk AAD).
    pub model_id: [u8; 32],
    /// Plaintext bytes per chunk before AEAD (the final chunk may be shorter).
    pub chunk_size: u32,
    /// Number of AEAD chunks in the body.
    pub num_chunks: u32,
    /// CSPRNG-random 16-byte nonce base for the per-chunk nonce construction.
    pub nonce_base: [u8; 16],
    /// SHA-256 of the canonical signed policy (bound into per-chunk AAD).
    pub policy_hash: [u8; 32],
}

impl ContainerHeader {
    /// Serialize to the fixed [`HEADER_LEN`]-byte wire layout (big-endian integers).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN);
        out.extend_from_slice(&self.magic);
        out.extend_from_slice(&self.version.to_be_bytes());
        out.extend_from_slice(&self.model_id);
        out.extend_from_slice(&self.chunk_size.to_be_bytes());
        out.extend_from_slice(&self.num_chunks.to_be_bytes());
        out.extend_from_slice(&self.nonce_base);
        out.extend_from_slice(&self.policy_hash);
        debug_assert_eq!(out.len(), HEADER_LEN);
        out
    }

    /// Parse the first [`HEADER_LEN`] bytes of `buf`. Fail-closed (`TeeError::Crypto`)
    /// on a short buffer, wrong magic, or unsupported version — never a panic.
    pub fn decode(buf: &[u8]) -> TeeResult<Self> {
        if buf.len() < HEADER_LEN {
            return Err(TeeError::Crypto(format!(
                "container header truncated: have {} bytes, need {HEADER_LEN}",
                buf.len()
            )));
        }
        // Every fixed-range slice below is in-bounds given the length guard
        // above (HEADER_LEN == 98), so each `try_into` is infallible — `expect`
        // documents that invariant and can never fire.
        let magic: [u8; 8] = buf[0..8].try_into().expect("8-byte slice");
        if magic != CONTAINER_MAGIC {
            return Err(TeeError::Crypto("bad container magic".into()));
        }
        let version = u16::from_be_bytes(buf[8..10].try_into().expect("2-byte slice"));
        if version != CONTAINER_VERSION {
            return Err(TeeError::Crypto(format!(
                "unsupported container version {version} (expected {CONTAINER_VERSION})"
            )));
        }
        let model_id: [u8; 32] = buf[10..42].try_into().expect("32-byte slice");
        let chunk_size = u32::from_be_bytes(buf[42..46].try_into().expect("4-byte slice"));
        let num_chunks = u32::from_be_bytes(buf[46..50].try_into().expect("4-byte slice"));
        let nonce_base: [u8; 16] = buf[50..66].try_into().expect("16-byte slice");
        let policy_hash: [u8; 32] = buf[66..98].try_into().expect("32-byte slice");
        Ok(Self {
            magic,
            version,
            model_id,
            chunk_size,
            num_chunks,
            nonce_base,
            policy_hash,
        })
    }
}

/// Number of AEAD chunks needed for `plaintext_len` bytes at `chunk_size`.
///
/// Fail-closed bounds: `chunk_size` must be non-zero, and the chunk count must
/// fit in a `u32` — ≥ 2^32 chunks ⇒ [`TeeError::ContainerTooLarge`]. That bound
/// keeps the 4-byte per-chunk nonce counter from ever overflowing; silent nonce
/// reuse under one DEK would break XChaCha20-Poly1305 confidentiality.
pub fn chunk_count(plaintext_len: u64, chunk_size: u32) -> TeeResult<u32> {
    if chunk_size == 0 {
        return Err(TeeError::Crypto("chunk_size must be non-zero".into()));
    }
    let n = plaintext_len.div_ceil(chunk_size as u64);
    if n > u32::MAX as u64 {
        return Err(TeeError::ContainerTooLarge);
    }
    Ok(n as u32)
}

/// Per-chunk 24-byte XChaCha20-Poly1305 nonce: `nonce_base ‖ chunk_idx_u32_be ‖ 0x00×4`.
///
/// SECURITY-CRITICAL: the encrypt and decrypt paths MUST build this identically
/// (and the AAD via [`chunk_aad`]) or every chunk fails authentication.
fn chunk_nonce(nonce_base: &[u8; 16], chunk_idx: u32) -> [u8; 24] {
    let mut nonce = [0u8; 24];
    nonce[..16].copy_from_slice(nonce_base);
    nonce[16..20].copy_from_slice(&chunk_idx.to_be_bytes());
    // nonce[20..24] stays 0x00 (the 4-byte padding to 24 bytes).
    nonce
}

/// Per-chunk AEAD AAD: the full encoded header ‖ `chunk_idx_u32_be`.
///
/// Binding the **entire** header (model_id, policy_hash, num_chunks, chunk_size,
/// nonce_base, version, magic) into every chunk's AAD makes the header
/// tamper-evident: dropping or duplicating chunks (which requires rewriting
/// `num_chunks`) or editing any structural field changes the AAD and breaks
/// every chunk's authentication tag — closing the silent-truncation vector that
/// a bare `model_id‖policy_hash‖chunk_idx` AAD leaves open. `chunk_idx`
/// additionally pins each chunk's position (no reorder/replay). Whole-model
/// integrity vs. the on-chain hash is an *additional* layer at Phase 4.3.2.
///
/// `header_bytes` must be the exact encoded header (`ContainerHeader::encode`),
/// so encrypt and decrypt MUST derive it identically (encrypt: the freshly
/// encoded header; decrypt: `container[..HEADER_LEN]`, the authenticated bytes).
fn chunk_aad(header_bytes: &[u8], chunk_idx: u32) -> Vec<u8> {
    let mut aad = Vec::with_capacity(header_bytes.len() + 4);
    aad.extend_from_slice(header_bytes);
    aad.extend_from_slice(&chunk_idx.to_be_bytes());
    aad
}

/// Encrypt `plaintext` into a self-describing container (header + chunked AEAD).
///
/// Provider-side **offline** tool (NOT node production code). Generates a fresh
/// CSPRNG `nonce_base`, then seals each `chunk_size`-byte slice under `dek` with
/// XChaCha20-Poly1305, binding the chunk index into the nonce ([`chunk_nonce`])
/// and the full header + chunk index into the AAD ([`chunk_aad`]). The container
/// is returned in memory; nothing is written to disk here.
pub fn encrypt_model(
    plaintext: &[u8],
    dek: &[u8; 32],
    model_id: [u8; 32],
    policy_hash: [u8; 32],
    chunk_size: u32,
) -> TeeResult<Vec<u8>> {
    let num_chunks = chunk_count(plaintext.len() as u64, chunk_size)?;

    let mut nonce_base = [0u8; 16];
    OsRng.fill_bytes(&mut nonce_base);

    let header = ContainerHeader {
        magic: CONTAINER_MAGIC,
        version: CONTAINER_VERSION,
        model_id,
        chunk_size,
        num_chunks,
        nonce_base,
        policy_hash,
    };
    let header_bytes = header.encode();

    // Pre-size to the exact final length so a multi-GB model needs no reallocation.
    let mut out =
        Vec::with_capacity(HEADER_LEN + plaintext.len() + num_chunks as usize * AEAD_TAG_LEN);
    out.extend_from_slice(&header_bytes);

    let cs = chunk_size as usize;
    for chunk_idx in 0..num_chunks {
        let start = chunk_idx as usize * cs;
        let end = (start + cs).min(plaintext.len());
        let nonce = chunk_nonce(&nonce_base, chunk_idx);
        let aad = chunk_aad(&header_bytes, chunk_idx);
        let ct = encrypt_with_aead(&plaintext[start..end], &nonce, &aad, dek)
            .map_err(|e| TeeError::Crypto(format!("chunk {chunk_idx} encryption failed: {e}")))?;
        out.extend_from_slice(&ct);
    }
    Ok(out)
}

/// Decrypt a container produced by [`encrypt_model`], streaming the recovered
/// plaintext to `out`.
///
/// Verifies the header binds to the caller-supplied `expect_model_id` /
/// `expect_policy_hash` (fail-closed *before* any decryption), then authenticates
/// and decrypts each chunk with the matching [`chunk_nonce`] + [`chunk_aad`].
///
/// **Fail-closed (caller's half):** a chunk is written to `out` only after its
/// AEAD tag verifies, so no *inauthentic* bytes are ever emitted — but because
/// decryption streams, a failure on chunk *k* may leave chunks `0..k` already
/// written. The caller (Phase 2.4 `prepare_encrypted_model`) MUST discard /
/// `secure_delete` the partial output on `Err` to complete the guarantee.
pub fn decrypt_model<W: Write>(
    container: &[u8],
    dek: &[u8; 32],
    expect_model_id: &[u8; 32],
    expect_policy_hash: &[u8; 32],
    out: &mut W,
) -> TeeResult<()> {
    let header = ContainerHeader::decode(container)?;
    if &header.model_id != expect_model_id {
        return Err(TeeError::VerificationFailed(
            "container model_id does not match the expected model".into(),
        ));
    }
    if &header.policy_hash != expect_policy_hash {
        return Err(TeeError::VerificationFailed(
            "container policy_hash does not match the expected policy".into(),
        ));
    }

    // The exact on-wire header bytes — what was authenticated at encryption.
    let header_bytes = &container[..HEADER_LEN];
    let body = &container[HEADER_LEN..];
    let full_ct_len = header.chunk_size as usize + AEAD_TAG_LEN;
    let mut offset = 0usize;
    for chunk_idx in 0..header.num_chunks {
        // The header stores no per-chunk length: every chunk but the last is a
        // full `chunk_size + tag`; the last consumes the remaining body bytes.
        let ct = if chunk_idx + 1 == header.num_chunks {
            body.get(offset..)
        } else {
            body.get(offset..offset + full_ct_len)
        }
        .ok_or_else(|| {
            TeeError::Crypto(format!("container body truncated at chunk {chunk_idx}"))
        })?;
        let nonce = chunk_nonce(&header.nonce_base, chunk_idx);
        let aad = chunk_aad(header_bytes, chunk_idx);
        let pt = decrypt_with_aead(ct, &nonce, &aad, dek)
            .map_err(|e| TeeError::Crypto(format!("chunk {chunk_idx} decryption failed: {e}")))?;
        out.write_all(&pt)?;
        offset += full_ct_len;
    }
    Ok(())
}
