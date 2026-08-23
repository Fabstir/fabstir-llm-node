// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Checkpoint/adapter artifact handling (interface B.2 steps 2–3, D.1, D.3):
//! the D.1 splitter, per-artifact fresh-key encryption over the LTX
//! capability-CID primitives, sharded upload through the `S5Storage` seam,
//! and the canonical `artifact-manifest-v1` build + upload. "Publish before
//! prove" is the CALLER's ordering duty (core's slice loop).

use crate::storage::s5_client::S5Storage;
use crate::training::staging::{AEAD_CHUNK_BYTES, SHARD_PLAINTEXT_MAX_BYTES};

/// One uploaded shard (D.2/D.3 shard shape).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShardRef {
    pub cid: String,
    pub sha256: String,
    pub size_bytes: u64,
}

/// One artifact file's manifest entry (D.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub shards: Vec<ShardRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactManifestRef {
    pub manifest_cid: String,
    pub manifest_sha256: String,
    /// Σ file sizeBytes — the protocol's `checkpoint.sizeBytes`.
    pub total_bytes: u64,
}

/// The D.1 splitter (v0.3.2 disambiguation, pinned by the `shiftedRemainder`
/// vector): cut at EXACTLY `SHARD_PLAINTEXT_MAX_BYTES` (itself a non-multiple
/// by construction); the remainder becomes the final shard — EXCEPT an
/// exact-262,144-multiple remainder, which splits into `(remainder − 1)` + a
/// trailing 1-byte shard. No shard is ever an exact chunk multiple.
pub fn shard_sizes(total_bytes: u64) -> Result<Vec<u64>, String> {
    if total_bytes == 0 {
        return Err("cannot shard zero bytes".to_string());
    }
    let mut sizes = Vec::new();
    let mut remaining = total_bytes;
    while remaining > SHARD_PLAINTEXT_MAX_BYTES {
        sizes.push(SHARD_PLAINTEXT_MAX_BYTES);
        remaining -= SHARD_PLAINTEXT_MAX_BYTES;
    }
    if remaining.is_multiple_of(AEAD_CHUNK_BYTES) {
        sizes.push(remaining - 1);
        sizes.push(1);
    } else {
        sizes.push(remaining);
    }
    Ok(sizes)
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("0x{}", hex::encode(Sha256::digest(data)))
}

/// Encrypt one plaintext with a FRESH random key (B.2) and upload the
/// ciphertext; returns the capability CID.
async fn upload_encrypted(
    s5: &dyn S5Storage,
    s5_path: &str,
    plaintext: &[u8],
) -> Result<String, String> {
    use rand::RngCore;
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    // Round-7 F-R7-3: `s5_path` is this node's storage layout and `e` is the
    // S5 client's error, whose reqwest Display carries ENHANCED_S5_URL. Both
    // reach the client through `RunEnd::Failed.detail`.
    let ciphertext = crate::ltx::exr::encrypt_frame(plaintext, &key)
        .map_err(|e| crate::training::redact::opaque("artifact encrypt failed", e))?;
    let capability = crate::ltx::exr::capability_cid(
        plaintext,
        &ciphertext,
        &key,
        crate::ltx::exr::padding_for(plaintext.len()) as u32,
    );
    s5.put(s5_path, ciphertext)
        .await
        .map_err(|e| crate::training::redact::opaque("artifact upload failed", e))?;
    Ok(capability)
}

/// Split, encrypt (fresh random key per shard) and upload one artifact file;
/// returns its D.3 entry. `s5_prefix` is the per-job artifact path prefix.
pub async fn upload_file_sharded(
    s5: &dyn S5Storage,
    s5_prefix: &str,
    name: &str,
    bytes: &[u8],
) -> Result<FileEntry, String> {
    let sizes = shard_sizes(bytes.len() as u64)?;
    let mut shards = Vec::with_capacity(sizes.len());
    let mut offset = 0usize;
    for (index, size) in sizes.iter().enumerate() {
        let end = offset + *size as usize;
        let plaintext = &bytes[offset..end];
        let cid =
            upload_encrypted(s5, &format!("{s5_prefix}/{name}.shard{index}"), plaintext).await?;
        shards.push(ShardRef {
            cid,
            sha256: sha256_hex(plaintext),
            size_bytes: *size,
        });
        offset = end;
    }
    Ok(FileEntry {
        name: name.to_string(),
        sha256: sha256_hex(bytes),
        size_bytes: bytes.len() as u64,
        shards,
    })
}

/// Build the canonical `artifact-manifest-v1`, encrypt + upload it, return
/// `(manifestCID, manifestSha256)` — the sha over the EXACT canonical
/// plaintext bytes (the wire/attestation claim the client verifies).
pub async fn upload_artifact_manifest(
    s5: &dyn S5Storage,
    s5_prefix: &str,
    kind: &str,
    slice_index: Option<u64>,
    files: &[FileEntry],
) -> Result<ArtifactManifestRef, String> {
    let mut manifest = serde_json::json!({
        "schema": "artifact-manifest-v1",
        "kind": kind,
        "files": files
            .iter()
            .map(|f| {
                serde_json::json!({
                    "name": f.name,
                    "sha256": f.sha256,
                    "sizeBytes": f.size_bytes,
                    "shards": f
                        .shards
                        .iter()
                        .map(|s| serde_json::json!({
                            "cid": s.cid,
                            "sha256": s.sha256,
                            "sizeBytes": s.size_bytes,
                        }))
                        .collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>(),
    });
    if let Some(index) = slice_index {
        manifest["sliceIndex"] = serde_json::json!(index);
    }
    let canonical = crate::training::attestation::canonical_manifest_bytes(&manifest);
    let canonical_bytes = canonical.into_bytes();
    let manifest_sha256 = sha256_hex(&canonical_bytes);
    let path_suffix = match slice_index {
        Some(index) => format!("manifest.{kind}.{index}"),
        None => format!("manifest.{kind}"),
    };
    let manifest_cid =
        upload_encrypted(s5, &format!("{s5_prefix}/{path_suffix}"), &canonical_bytes).await?;
    Ok(ArtifactManifestRef {
        manifest_cid,
        manifest_sha256,
        total_bytes: files.iter().map(|f| f.size_bytes).sum(),
    })
}
