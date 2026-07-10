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
//!      (`z` + base58btc of `[0x5b,0x82,0x1e] ++ ct_hash ++ 0x00`).
//!   3. [`download_blob`] — `GET {base}/s5/blob/{cid}` against the LOCAL S5 bridge
//!      (`downloadByCID`, P2P — the working S5 transport, NOT a raw portal GET),
//!      gated by `blake3(body) == ct_hash` BEFORE any decrypt.
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
use futures::StreamExt;

use crate::ltx::exr::{decrypt_frame, padding_for, CHUNK};

/// Envelope bytes (must match [`crate::ltx::exr`] `capability_cid`).
const CID_TYPE_ENCRYPTED_STATIC: u8 = 0xae;
const ENC_ALG_XCHACHA20POLY1305: u8 = 0xa6;
/// `maxChunkSizeAsPowerOf2` — fixed at 18 (256 KiB) in both s5.js and `exr`; a
/// different value cannot be decrypted by our fixed-stride `decrypt_frame`.
const MAX_CHUNK_SIZE_AS_POW2: u8 = 18;
/// Marker byte inside the `0xae` capability envelope's plaintextCID: a MODIFIED
/// blake3 tag (`0x1f`), NOT the code used in a standalone BlobIdentifier.
const BLAKE3_MARKER: u8 = 0x1f;
/// The real blake3 multihash code (`MULTIHASH_BLAKE3` in s5.js), used in a
/// BlobIdentifier's 33-byte hash. This is `0x1e`, NOT the envelope's `0x1f`: the
/// portal blob-download CID MUST use `0x1e` or the portal 404s.
const MULTIHASH_BLAKE3: u8 = 0x1e;
const LEGACY_CID_PREFIX: u8 = 0x26;
/// Poly1305 tag bytes appended per chunk (matches `exr`); used to size the fetch cap.
const TAG: usize = 16;
/// The S5 BlobIdentifier prefix the portal keys on: `blobIdentifierPrefixBytes`
/// `[0x5b, 0x82]` then the blake3 multihash `0x1e`. Mirrors s5.js
/// `new BlobIdentifier(hash, 0).toBase58()` (the portal-download fallback in
/// `@julesl23/s5js` `identity/api.js`), where `hash = [0x1e] ++ blake3(32)`.
const BLOB_CID_PREFIX: [u8; 3] = [0x5b, 0x82, MULTIHASH_BLAKE3];

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
        || env[2] != MAX_CHUNK_SIZE_AS_POW2
        || env[3] != BLAKE3_MARKER
    {
        return Err(anyhow!("not a 0xae/XChaCha20/256KiB capability envelope"));
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
/// `"z" + base58btc([0x5b, 0x82, 0x1e] ++ ct_hash(32) ++ 0x00)`. Byte-for-byte
/// `new BlobIdentifier([0x1e] ++ ct_hash, 0).toBase58()` from `@julesl23/s5js`
/// (the raw-portal download fallback in `identity/api.js`): base58btc, the size
/// field set to 0 (the portal resolves on the 32-byte blake3 hash), and the blake3
/// multihash `0x1e` — NOT the capability envelope's `0x1f`.
pub fn blob_download_cid(ct_hash: &[u8; 32]) -> String {
    let mut raw = Vec::with_capacity(3 + 32 + 1);
    raw.extend_from_slice(&BLOB_CID_PREFIX);
    raw.extend_from_slice(ct_hash);
    raw.push(0x00);
    format!("z{}", bs58::encode(raw).into_string())
}

/// The exact ciphertext-blob length `encrypt_frame` produces for `plaintext_len`:
/// `plaintext_len + padding_for(plaintext_len) + ceil(len/CHUNK)*TAG`. Used to cap
/// the portal fetch tightly — a valid blob is EXACTLY this size. Saturating so a
/// bogus (huge) claimed length can never overflow-panic in a debug build.
fn expected_ciphertext_len(plaintext_len: usize) -> usize {
    if plaintext_len == 0 {
        return 0;
    }
    let chunks = plaintext_len.div_ceil(CHUNK);
    plaintext_len
        .saturating_add(padding_for(plaintext_len))
        .saturating_add(chunks.saturating_mul(TAG))
}

/// `GET {base_url}/s5/blob/{cid}` and return the raw ciphertext, hard-failing if
/// `blake3(body) != ct_hash`. `base_url` is the LOCAL S5 bridge (`ENHANCED_S5_URL`),
/// whose `/s5/blob/{cid}` route resolves the blob over the S5 protocol
/// (`downloadByCID`, P2P); a raw portal HTTP GET is not a supported transport. The
/// body is streamed and bounded to `max_bytes` (reject as soon as it is exceeded)
/// so a capability CID that claims a small size but commits to a huge blob cannot
/// force an unbounded download — the integrity gate would still catch the mismatch,
/// but only after the bytes were already in memory. The gate runs BEFORE the bytes
/// reach any decrypt. `base_url` is a parameter so the same path is tested.
pub async fn download_blob(
    base_url: &str,
    ct_hash: &[u8; 32],
    max_bytes: usize,
) -> Result<Vec<u8>> {
    let cid = blob_download_cid(ct_hash);
    let url = format!("{}/s5/blob/{}", base_url.trim_end_matches('/'), cid);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()?;
    let response = client.get(&url).send().await?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "s5 /s5/blob returned {} for {cid}",
            response.status()
        ));
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len() + chunk.len() > max_bytes {
            return Err(anyhow!(
                "blob {cid} exceeds the expected {max_bytes} bytes (claimed-size / blob-size mismatch)"
            ));
        }
        body.extend_from_slice(&chunk);
    }
    if blake3::hash(&body).as_bytes() != ct_hash {
        return Err(anyhow!(
            "blob integrity check failed: blake3(body) != ct_hash for {cid}"
        ));
    }
    Ok(body)
}

/// Full input-image resolution: parse the capability CID, fetch + integrity-check
/// the ciphertext from the portal (bounded to the exact size the claimed
/// `plaintext_len` implies), decrypt, verify `blake3(plaintext) == pt_hash`, and
/// return `(imageHash, plaintext)` where `imageHash = keccak256(plaintext)`. The
/// plaintext is returned so the caller can upload the exact bytes to ComfyUI
/// without re-fetching.
pub async fn fetch_image_hash(base_url: &str, cid: &str) -> Result<([u8; 32], Vec<u8>)> {
    let env = parse_capability_cid(cid)?;
    // The chunk scheme pads the FINAL chunk, so an exact CHUNK-multiple plaintext
    // yields a blob the fixed-stride `decrypt_frame` cannot read (`encrypt_frame`
    // rejects it on the write side for the same reason). Fail fast with an
    // actionable message instead of a cryptic downstream AEAD error. Rare
    // (~1/CHUNK for arbitrary sizes); the client can re-save to shift the size.
    if env.plaintext_len != 0 && env.plaintext_len % CHUNK == 0 {
        return Err(anyhow!(
            "input image is an exact {CHUNK}-byte multiple ({} bytes); the chunked \
             encryption cannot represent it — re-save to change its size by a byte",
            env.plaintext_len
        ));
    }
    let max_bytes = expected_ciphertext_len(env.plaintext_len);
    let ciphertext = download_blob(base_url, &env.ct_hash, max_bytes).await?;
    let plaintext = decrypt_frame(&ciphertext, &env.key, env.plaintext_len)?;
    if blake3::hash(&plaintext).as_bytes() != &env.pt_hash {
        return Err(anyhow!(
            "plaintext integrity check failed: blake3(plaintext) != pt_hash"
        ));
    }
    Ok((keccak256(&plaintext), plaintext))
}
