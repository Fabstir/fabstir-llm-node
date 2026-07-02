// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! M1a input-image path: turn an ordered S5 **capability CID** (the key-bearing
//! `0xae` envelope produced by [`crate::ltx::exr::capability_cid`]) into the
//! `keccak256(plaintext)` `imageHash` the commitment binds.
//!
//! Four steps, each independently testable:
//!   1. [`parse_capability_cid`] — inverse of `capability_cid`: recover
//!      `ct_hash`, `key`, `padding`, `plaintext_len`, `pt_hash` from the envelope.
//!   2. [`blob_download_cid`] — the S5 blob-download CID for the ciphertext
//!      (`z` + base58btc of `[0x5b,0x82,0x1f] ++ ct_hash ++ 0x00`).
//!   3. [`download_blob`] — a **portal-direct** `GET {portal}/s5/blob/{cid}` (no
//!      auth), gated by `blake3(body) == ct_hash` BEFORE any decrypt.
//!   4. [`fetch_image_hash`] — compose the above, decrypt, verify
//!      `blake3(plaintext) == pt_hash`, and return `(imageHash, plaintext)`.
//!
//! The capability CID is transport ONLY; it is never hashed into the commitment
//! (see [`crate::ltx::attestation::commitment_for`]). The bytes fetched here are
//! the exact ciphertext the node itself uploaded, so the same crypto that built
//! the envelope inverts it.

use std::time::Duration;

use anyhow::{anyhow, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ethers::utils::keccak256;

use crate::ltx::exr::decrypt_frame;

/// Envelope bytes (must match [`crate::ltx::exr`] `capability_cid`).
const CID_TYPE_ENCRYPTED_STATIC: u8 = 0xae;
const ENC_ALG_XCHACHA20POLY1305: u8 = 0xa6;
const BLAKE3_MARKER: u8 = 0x1f;
const LEGACY_CID_PREFIX: u8 = 0x26;
/// The S5 blob CID's leading bytes (the ciphertext BlobIdentifier form the portal
/// keys on): `raw-blob` type `0x5b`, size-class `0x82`, then the blake3 marker.
const BLOB_CID_PREFIX: [u8; 3] = [0x5b, 0x82, BLAKE3_MARKER];

const HTTP_TIMEOUT_SECS: u64 = 120;

/// Fields recovered from a parsed `0xae` capability envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapEnvelope {
    /// `blake3(ciphertext blob)` — the portal download key AND the pre-decrypt
    /// integrity check.
    pub ct_hash: [u8; 32],
    /// The XChaCha20-Poly1305 frame key.
    pub key: [u8; 32],
    /// Zero-padding applied to the final chunk (informational; `decrypt_frame`
    /// truncates to `plaintext_len`).
    pub padding: u32,
    /// Original plaintext length (drives chunk maths + the `imageMaxBytes` gate).
    pub plaintext_len: usize,
    /// `blake3(plaintext)` — the post-decrypt integrity check.
    pub pt_hash: [u8; 32],
}

/// Parse a `u`-prefixed capability CID (base64url-nopad of the `0xae` envelope)
/// into its [`CapEnvelope`] fields. Inverse of
/// [`crate::ltx::exr::capability_cid`]; validates the fixed marker bytes so a
/// non-capability CID is rejected rather than silently mis-read.
pub fn parse_capability_cid(cid: &str) -> Result<CapEnvelope> {
    let b64 = cid
        .strip_prefix('u')
        .ok_or_else(|| anyhow!("capability CID must be 'u'-multibase: {cid:?}"))?;
    let env = URL_SAFE_NO_PAD
        .decode(b64)
        .map_err(|e| anyhow!("capability CID is not base64url: {e}"))?;

    // 4-byte header + ct_hash(32) + key(32) + padding(4) + plaintextCID(>=34).
    if env.len() < 4 + 32 + 32 + 4 + 2 + 32 + 1 {
        return Err(anyhow!(
            "capability envelope too short: {} bytes",
            env.len()
        ));
    }
    if env[0] != CID_TYPE_ENCRYPTED_STATIC
        || env[1] != ENC_ALG_XCHACHA20POLY1305
        || env[3] != BLAKE3_MARKER
    {
        return Err(anyhow!("not a 0xae/XChaCha20 capability envelope"));
    }

    let mut ct_hash = [0u8; 32];
    ct_hash.copy_from_slice(&env[4..36]);
    let mut key = [0u8; 32];
    key.copy_from_slice(&env[36..68]);
    let padding = u32::from_le_bytes([env[68], env[69], env[70], env[71]]);

    // plaintextCID = [0x26, 0x1f] ++ pt_hash(32) ++ size_le_trimmed.
    let pcid = &env[72..];
    if pcid[0] != LEGACY_CID_PREFIX || pcid[1] != BLAKE3_MARKER {
        return Err(anyhow!("malformed plaintextCID prefix in envelope"));
    }
    let mut pt_hash = [0u8; 32];
    pt_hash.copy_from_slice(&pcid[2..34]);

    // size_le_trimmed: 1..=8 LE bytes (trailing zeros trimmed); re-pad to u64.
    let size_le = &pcid[34..];
    if size_le.is_empty() || size_le.len() > 8 {
        return Err(anyhow!("invalid size field ({} bytes)", size_le.len()));
    }
    let mut buf = [0u8; 8];
    buf[..size_le.len()].copy_from_slice(size_le);
    let plaintext_len = u64::from_le_bytes(buf) as usize;

    Ok(CapEnvelope {
        ct_hash,
        key,
        padding,
        plaintext_len,
        pt_hash,
    })
}

/// The S5 blob-download CID for a ciphertext blob keyed by `ct_hash`:
/// `"z" + base58btc([0x5b, 0x82, 0x1f] ++ ct_hash(32) ++ 0x00)`. The portal
/// resolves purely on the embedded 32-byte blake3 hash (the type/size/pad bytes
/// are not load-bearing for the GET), matching `@julesl23/s5js`.
pub fn blob_download_cid(ct_hash: &[u8; 32]) -> String {
    let mut raw = Vec::with_capacity(3 + 32 + 1);
    raw.extend_from_slice(&BLOB_CID_PREFIX);
    raw.extend_from_slice(ct_hash);
    raw.push(0x00);
    format!("z{}", bs58::encode(raw).into_string())
}

/// `GET {portal_url}/s5/blob/{cid}` (portal-direct, no auth) and return the raw
/// ciphertext, hard-failing if `blake3(body) != ct_hash`. The integrity gate runs
/// BEFORE the bytes are handed to any decrypt, so a portal that serves the wrong
/// or tampered blob can never reach the AEAD. `portal_url` is a parameter so the
/// same code path is exercised against a mock server in tests.
pub async fn download_blob(portal_url: &str, ct_hash: &[u8; 32]) -> Result<Vec<u8>> {
    let cid = blob_download_cid(ct_hash);
    let url = format!("{}/s5/blob/{}", portal_url.trim_end_matches('/'), cid);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()?;
    let response = client.get(&url).send().await?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "portal /s5/blob returned {} for {cid}",
            response.status()
        ));
    }
    let body = response.bytes().await?.to_vec();
    if blake3::hash(&body).as_bytes() != ct_hash {
        return Err(anyhow!(
            "blob integrity check failed: blake3(body) != ct_hash for {cid}"
        ));
    }
    Ok(body)
}

/// Full input-image resolution: parse the capability CID, fetch + integrity-check
/// the ciphertext from the portal, decrypt, verify `blake3(plaintext) == pt_hash`,
/// and return `(imageHash, plaintext)` where `imageHash = keccak256(plaintext)`.
/// The plaintext is returned so the caller can upload the exact bytes to ComfyUI
/// without re-fetching.
pub async fn fetch_image_hash(portal_url: &str, cid: &str) -> Result<([u8; 32], Vec<u8>)> {
    let env = parse_capability_cid(cid)?;
    let ciphertext = download_blob(portal_url, &env.ct_hash).await?;
    let plaintext = decrypt_frame(&ciphertext, &env.key, env.plaintext_len)?;
    if blake3::hash(&plaintext).as_bytes() != &env.pt_hash {
        return Err(anyhow!(
            "plaintext integrity check failed: blake3(plaintext) != pt_hash"
        ));
    }
    Ok((keccak256(&plaintext), plaintext))
}
