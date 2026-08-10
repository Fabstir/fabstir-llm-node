// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 of the LTX 2.3 sidecar: EXR frame collection, per-frame encryption,
//! and the keyless commitment chain.
//!
//! This module is **byte-compatible** with the vendored `@julesl23/s5js`
//! encryptor (`dist/src/fs/fs5.js` `uploadBlobEncrypted`). Each EXR frame is
//! XChaCha20-Poly1305 encrypted in 256 KiB chunks; we then derive:
//!   * a **keyless** `frameHash = keccak256(ciphertext blob)` (public, no key),
//!   * a **key-bearing** capability CID (the `0xae` envelope; carries the key),
//!   * a **keyless** Merkle manifest over the raw 32-byte frame hashes.
//!
//! The capability CIDs ride only the encrypted `ltx_complete`; the manifest is
//! public and never holds a key or a `u`-prefixed capability CID.
//!
//! Crypto is factored away from S5 so the chunk/pad/envelope maths is testable
//! without a storage backend.

use anyhow::{anyhow, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ethers::utils::keccak256;
use std::path::{Path, PathBuf};

use crate::crypto::{decrypt_with_aead, encrypt_with_aead};
use crate::ltx::types::{FrameManifest, LtxJob};
use crate::storage::s5_client::S5Storage;
use crate::transcoder::merkle::MerkleTree;

/// 2^18 = 262144 bytes (256 KiB). `maxChunkSize` in fs5.js.
pub const CHUNK: usize = 262_144;
/// log2(CHUNK) — the `maxChunkSizeAsPowerOf2` envelope byte.
const MAX_CHUNK_SIZE_AS_POW2: u8 = 18;
/// Poly1305 tag length appended by XChaCha20-Poly1305.
const TAG: usize = 16;
/// `cidTypeEncryptedStatic` (fs5.js).
const CID_TYPE_ENCRYPTED_STATIC: u8 = 0xae;
/// `ENCRYPTION_ALGORITHM_XCHACHA20POLY1305` (fs5.js).
const ENC_ALG_XCHACHA20POLY1305: u8 = 0xa6;
/// blake3 hash-type marker used in S5 CIDs (the post-`subarray(1)` prefix).
const BLAKE3_MARKER: u8 = 0x1f;
/// Legacy S5 CID type prefix (`plaintextCID[0]`).
const LEGACY_CID_PREFIX: u8 = 0x26;
/// Exact colour-encoding label the manifest must carry (M0).
const COLOUR_ENCODING: &str = "linear-HDR-from-LogC3";

// ---------------------------------------------------------------------------
// Frame collection
// ---------------------------------------------------------------------------

/// Collect `*.exr` files from `dir`, ordered by the numeric run in each stem
/// (so `f_2_`, `f_10_`, `f_1_` sort to 1, 2, 10 — numerically, not lexically).
/// Non-`.exr` entries are ignored. Ties break on the full path for determinism.
pub fn collect(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut frames: Vec<(u64, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("exr") {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            frames.push((last_numeric_run(stem), path));
        }
    }
    frames.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    Ok(frames.into_iter().map(|(_, p)| p).collect())
}

/// The last contiguous run of ASCII digits in `stem`, parsed as `u64`
/// (`ltx_00001_` -> 1, `f_10_` -> 10). No digits -> 0.
fn last_numeric_run(stem: &str) -> u64 {
    let mut runs: Vec<String> = Vec::new();
    let mut cur = String::new();
    for c in stem.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else if !cur.is_empty() {
            runs.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    runs.last().and_then(|r| r.parse::<u64>().ok()).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Little-endian + padding (exact ports of the s5js utilities)
// ---------------------------------------------------------------------------

/// Port of `encodeLittleEndian(value, length)` from `util/little_endian.js`:
/// write the low `length` bytes of `value`, little-endian (truncating).
fn le(value: u64, length: usize) -> Vec<u8> {
    let mut buf = vec![0u8; length];
    let mut v = value;
    for b in buf.iter_mut() {
        *b = (v & 0xff) as u8;
        v >>= 8;
    }
    buf
}

/// Exact port of `padFileSize(initialSize)` from `encryption/padding.js`.
/// `u128` internally so the `(1<<n)*80*kib` thresholds never overflow (the JS
/// uses float64 and cannot overflow for in-range sizes).
fn pad_file_size(size: usize) -> usize {
    let kib: u128 = 1 << 10;
    let size = size as u128;
    for n in 0u32..53 {
        if size <= (1u128 << n) * 80 * kib {
            let padding_block = (1u128 << n) * 4 * kib;
            let mut final_size = size;
            if final_size % padding_block != 0 {
                final_size = size - (size % padding_block) + padding_block;
            }
            return final_size as usize;
        }
    }
    panic!("Could not pad file size, overflow detected.");
}

/// The number of trailing zero bytes the LAST chunk's plaintext is padded with,
/// per fs5.js `uploadBlobEncrypted`. Public so the chunk maths is testable
/// without S5.
pub fn padding_for(size: usize) -> usize {
    let chunk_count = chunk_count(size);
    let total_with_overhead = size + chunk_count * TAG;
    let mut padding = pad_file_size(total_with_overhead) - total_with_overhead;
    let last_chunk_size = size % CHUNK;
    if padding + last_chunk_size >= CHUNK {
        padding = CHUNK - last_chunk_size;
    }
    padding
}

/// `ceil(size / CHUNK)`.
fn chunk_count(size: usize) -> usize {
    size.div_ceil(CHUNK)
}

// ---------------------------------------------------------------------------
// Per-frame encryption / decryption (XChaCha20-Poly1305, 256 KiB chunks)
// ---------------------------------------------------------------------------

/// Encrypt `plaintext` into the S5 ciphertext blob: each full chunk `i` is
/// `encrypt(plaintext[i*CHUNK..(i+1)*CHUNK], nonce=le(i,24))`; the final chunk
/// is `plaintext[(n-1)*CHUNK..] ++ padding*0x00`, encrypted with nonce
/// `le(n-1,24)`. AAD is empty. Output = all encrypted chunks concatenated.
pub fn encrypt_frame(plaintext: &[u8], key: &[u8; 32]) -> Result<Vec<u8>> {
    let size = plaintext.len();
    if size == 0 {
        return Ok(Vec::new());
    }
    if size % CHUNK == 0 {
        // The s5js scheme pads the FINAL chunk; an exact CHUNK multiple makes that
        // chunk CHUNK+padding, which the fixed-stride decryptor (ours and s5js's)
        // cannot read — fail fast rather than emit an undecryptable blob. EXR frames
        // are never an exact 256 KiB multiple in practice.
        return Err(anyhow!(
            "frame size {size} is an exact {CHUNK}-byte multiple (unsupported)"
        ));
    }
    let n = chunk_count(size);
    let padding = padding_for(size);
    let mut blob = Vec::new();
    // Full chunks 0..n-1.
    for i in 0..(n - 1) {
        let start = i * CHUNK;
        let end = start + CHUNK;
        let enc = encrypt_with_aead(&plaintext[start..end], &le(i as u64, 24), b"", key)?;
        blob.extend_from_slice(&enc);
    }
    // Final (n-1) chunk: remaining bytes + zero padding.
    let last_start = (n - 1) * CHUNK;
    let mut last = plaintext[last_start..].to_vec();
    last.resize(last.len() + padding, 0u8);
    let enc = encrypt_with_aead(&last, &le((n - 1) as u64, 24), b"", key)?;
    blob.extend_from_slice(&enc);
    Ok(blob)
}

/// Inverse of [`encrypt_frame`]: read `(CHUNK + 16)`-sized ciphertext chunks
/// (last one short), decrypt each with nonce `le(i,24)`, concatenate, and
/// truncate to the original `size` (drops padding). Mirrors fs5.js
/// `downloadAndDecryptBlob`.
pub fn decrypt_frame(ciphertext: &[u8], key: &[u8; 32], size: usize) -> Result<Vec<u8>> {
    if size == 0 {
        return Ok(Vec::new());
    }
    let n = chunk_count(size);
    let mut out = Vec::new();
    for i in 0..n {
        let start = i * (CHUNK + TAG);
        let end = std::cmp::min((i + 1) * (CHUNK + TAG), ciphertext.len());
        if start >= end {
            return Err(anyhow!("ciphertext too short for chunk {i}"));
        }
        let dec = decrypt_with_aead(&ciphertext[start..end], &le(i as u64, 24), b"", key)?;
        out.extend_from_slice(&dec);
    }
    out.truncate(size);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Keyless frame hash + key-bearing capability CID
// ---------------------------------------------------------------------------

/// Keyless commitment to one encrypted frame: `0x` + hex of
/// `keccak256(ciphertext blob)`. Depends ONLY on the ciphertext (no key).
pub fn frame_hash(ciphertext: &[u8]) -> String {
    format!("0x{}", hex::encode(keccak256(ciphertext)))
}

/// Build the `0xae` capability CID string (`"u" + base64url_nopad(envelope)`),
/// byte-for-byte matching fs5.js `uploadBlobEncrypted`. The envelope embeds the
/// key, so it differs per key even for identical plaintext.
///
/// `padding` is the zero-padding applied to the final chunk (see [`padding_for`]).
pub fn capability_cid(plaintext: &[u8], ciphertext: &[u8], key: &[u8; 32], padding: u32) -> String {
    let pt_hash = blake3::hash(plaintext); // blake3 of PLAINTEXT (32)
    let ct_hash = blake3::hash(ciphertext); // blake3 of CIPHERTEXT blob (32)

    // size_le_trimmed = u64 LE of the ORIGINAL frame length, trailing zero
    // bytes removed, minimum 1 byte (matches BlobIdentifier.toBytes()).
    let mut size_le = le(plaintext.len() as u64, 8);
    while size_le.len() > 1 && *size_le.last().unwrap() == 0 {
        size_le.pop();
    }

    // plaintextCID = [0x26, 0x1f] ++ pt_hash(32) ++ size_le_trimmed
    let mut plaintext_cid = Vec::with_capacity(2 + 32 + size_le.len());
    plaintext_cid.push(LEGACY_CID_PREFIX); // 0x26
    plaintext_cid.push(BLAKE3_MARKER); // 0x1f (overwrites the 0x1e multihash)
    plaintext_cid.extend_from_slice(pt_hash.as_bytes());
    plaintext_cid.extend_from_slice(&size_le);

    // encryptedCIDBytes
    let mut env = Vec::with_capacity(4 + 32 + 32 + 4 + plaintext_cid.len());
    env.push(CID_TYPE_ENCRYPTED_STATIC); // 0xae
    env.push(ENC_ALG_XCHACHA20POLY1305); // 0xa6
    env.push(MAX_CHUNK_SIZE_AS_POW2); // 18
    env.push(BLAKE3_MARKER); // 0x1f
    env.extend_from_slice(ct_hash.as_bytes()); // ct_hash(32)
    env.extend_from_slice(key); // key(32)
    env.extend_from_slice(&le(padding as u64, 4)); // padding LE(4)
    env.extend_from_slice(&plaintext_cid); // plaintextCID

    format!("u{}", URL_SAFE_NO_PAD.encode(&env))
}

// ---------------------------------------------------------------------------
// Keyless Merkle manifest
// ---------------------------------------------------------------------------

/// Build the PUBLIC, KEYLESS frame manifest. `frameHashes` are the ordered
/// `0x`-hex `keccak256(ciphertext)` strings; the Merkle root is computed over
/// their RAW 32-byte decoded leaves (keccak node = `keccak256(left||right)`,
/// odd layer duplicates the last leaf). NO capability CIDs / keys are included.
/// Deterministic delivery order (A2). Legacy jobs pass through untouched
/// (byte-identical path). For `exr-frames`: exactly ONE non-EXR ref (the
/// preview mp4) comes FIRST, then the EXR frames sorted by filename
/// (frame_%05d), and the EXR count must equal the billed frames — fail
/// closed: a paid EXR request must never silently deliver fewer frames.
pub fn order_refs(
    job: &LtxJob,
    refs: Vec<crate::ltx::client::ExrRef>,
) -> Result<Vec<crate::ltx::client::ExrRef>> {
    use crate::ltx::types::OutputKind;
    if job.output != OutputKind::ExrFrames {
        return Ok(refs);
    }
    let (mut exr, other): (Vec<_>, Vec<_>) = refs
        .into_iter()
        .partition(|r| r.filename.to_ascii_lowercase().ends_with(".exr"));
    if other.len() != 1 {
        return Err(anyhow!(
            "exr-frames delivery expects exactly one preview artefact, got {}",
            other.len()
        ));
    }
    if exr.len() as u32 != job.frames {
        return Err(anyhow!(
            "exr-frames delivery produced {} frames but {} were billed",
            exr.len(),
            job.frames
        ));
    }
    exr.sort_by(|a, b| a.filename.cmp(&b.filename));
    let mut out = other;
    out.extend(exr);
    Ok(out)
}

/// The manifest's colour_encoding for this job. Legacy single-artefact jobs
/// keep the historical constant byte-for-byte; A2 EXR masters are linearised
/// display content (RadianceSaveEXR converts to Linear (sRGB) = Rec.709
/// primaries on write).
pub fn colour_encoding_for(job: &LtxJob) -> &'static str {
    use crate::ltx::types::OutputKind;
    match job.output {
        OutputKind::ExrFrames => "linear-rec709",
        OutputKind::ExrSequence => COLOUR_ENCODING,
    }
}

pub fn build_manifest(frame_hashes: &[String], job: &LtxJob) -> Result<FrameManifest> {
    let mut tree = MerkleTree::new();
    for h in frame_hashes {
        // Strict decode: a malformed leaf must error, never silently coerce — the
        // merkleRoot is the public integrity commitment.
        let raw = hex::decode(h.strip_prefix("0x").unwrap_or(h))
            .map_err(|e| anyhow!("invalid frameHash {h:?}: {e}"))?;
        if raw.len() != 32 {
            return Err(anyhow!(
                "frameHash {h:?} must be 32 bytes, got {}",
                raw.len()
            ));
        }
        let mut leaf = [0u8; 32];
        leaf.copy_from_slice(&raw);
        tree.add_leaf(leaf);
    }
    Ok(FrameManifest {
        frame_count: frame_hashes.len() as u32,
        fps: job.fps,
        resolution: job.resolution,
        colour_encoding: colour_encoding_for(job).to_string(),
        frame_hashes: frame_hashes.to_vec(),
        merkle_root: format!("0x{}", hex::encode(tree.root())),
    })
}

// ---------------------------------------------------------------------------
// S5-touching orchestration (thin: read file, crypt, upload)
// ---------------------------------------------------------------------------

/// Generate a fresh 32-byte per-frame key from the OS CSPRNG.
pub fn generate_frame_key() -> [u8; 32] {
    use rand::{rngs::OsRng, RngCore};
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    key
}

/// Read one EXR `frame`, encrypt it, upload the ciphertext blob to `s5` at
/// `dest`, and return `(capabilityCid, frameHash)`. The capability CID carries
/// the key; the frame hash is keyless.
pub async fn encrypt_and_upload(
    frame: &Path,
    s5: &dyn S5Storage,
    dest: &str,
) -> Result<(String, String)> {
    encrypt_bytes_and_upload(std::fs::read(frame)?, s5, dest).await
}

/// Encrypt an in-memory frame/clip and upload its ciphertext. Same crypto as
/// [`encrypt_and_upload`] but sourced from bytes (e.g. fetched over ComfyUI's
/// `/view`) instead of a local file, so no shared output volume is required.
pub async fn encrypt_bytes_and_upload(
    plaintext: Vec<u8>,
    s5: &dyn S5Storage,
    dest: &str,
) -> Result<(String, String)> {
    // Generate the key INSIDE so a fresh per-frame key can't be skipped by the
    // caller (key reuse across frames = catastrophic XChaCha20 nonce reuse).
    let key = generate_frame_key();
    let ciphertext = encrypt_frame(&plaintext, &key)?;
    let padding = padding_for(plaintext.len()) as u32;
    let cap = capability_cid(&plaintext, &ciphertext, &key, padding);
    let fhash = frame_hash(&ciphertext);
    s5.put(dest, ciphertext)
        .await
        .map_err(|e| anyhow!("s5 put (ciphertext) failed: {e}"))?;
    Ok((cap, fhash))
}

/// Serialise the keyless manifest as JSON and upload it to `s5` at `dest`,
/// returning the storage CID (the public `outputCID`).
pub async fn upload_manifest(m: &FrameManifest, s5: &dyn S5Storage, dest: &str) -> Result<String> {
    let bytes = serde_json::to_vec(m)?;
    s5.put(dest, bytes)
        .await
        .map_err(|e| anyhow!("s5 put (manifest) failed: {e}"))
}
