// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Dataset staging (interface C.3 order, D.1/D.2 schemas): manifest fetch +
//! decrypt + sha256 verify, the wire↔manifest cross-checks, per-shard
//! plaintext-sha256 verification, and the write-then-rename into
//! `TRAINING_STAGING_ROOT/job-<jobId>/dataset.jsonl` the sidecar reads.
//!
//! Crypto/transport rides the LTX capability-CID primitives
//! (`ltx::input_image::fetch_image_hash` — download by `blob_download_cid`,
//! blake3 gate BEFORE decrypt, chunked XChaCha decrypt, blake3(plaintext)
//! check). This module adds the TRAINING-side claims on top: the manifest's
//! sha256 is the wire's `manifestSha256`, and every shard's DECLARED
//! plaintext sha256/size must match what actually decrypts —
//! `DATASET_INTEGRITY` on any mismatch (C.3).

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::training::types::TrainingJob;

/// D.1: one value, both repos — 24 MiB − 4 KiB, deliberately NOT a 256 KiB
/// multiple (the chunk scheme refuses exact-multiple plaintexts).
pub const SHARD_PLAINTEXT_MAX_BYTES: u64 = 25_161_728;

/// The AEAD chunk stride (the D.1 non-multiple rule's modulus).
pub const AEAD_CHUNK_BYTES: u64 = 262_144;

/// Ceiling on the ENCRYPTED manifest's declared plaintext size — a manifest
/// is a small JSON document; the T3 converge round proved an unbounded
/// fetch here let a dust-cost session OOM the node via a capability CID
/// claiming gigabytes (the LTX `validate_input_cids` hole, reopened).
pub const MANIFEST_MAX_BYTES: u64 = 1_048_576;

/// Ceiling on the shard COUNT (converge round: 1-byte shards made the count
/// effectively unbounded — a 100k-shard manifest is a sequential fetch
/// storm holding the GPU permit). M0's 120 MB byte ceiling needs ≤ 5 full
/// shards; 64 is generous.
pub const MAX_SHARDS: usize = 64;

/// Pre-fetch gate on a capability CID's SELF-DECLARED plaintext length —
/// the client controls that number, so it must be bounded BEFORE any bytes
/// move (the LTX rule). Also enforces D.1's non-multiple rule so a doomed
/// blob is a terminal client error, not an endlessly re-shopped
/// "host infra" failure.
fn gate_capability_len(cid: &str, max_bytes: u64, what: &str) -> Result<u64, StageError> {
    let envelope = crate::ltx::input_image::parse_capability_cid(cid)
        .map_err(|e| StageError::Validation(format!("{what} capability CID invalid: {e}")))?;
    let declared = envelope.plaintext_len as u64;
    if declared == 0 || declared > max_bytes {
        return Err(StageError::Validation(format!(
            "{what} declares {declared} plaintext bytes (bound 1..={max_bytes}) — refused before fetch"
        )));
    }
    if declared.is_multiple_of(AEAD_CHUNK_BYTES) {
        return Err(StageError::Validation(format!(
            "{what} plaintext is an exact {AEAD_CHUNK_BYTES}-byte multiple — the D.1 chunk \
             scheme can never stage it (client-side split bug, terminal)"
        )));
    }
    Ok(declared)
}

/// `dataset-manifest-v1` (interface D.2), decrypted.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DatasetManifest {
    pub schema: String,
    pub format: String,
    #[serde(rename = "countingRecipe")]
    pub counting_recipe: String,
    #[serde(rename = "tokenizerSha256")]
    pub tokenizer_sha256: String,
    pub samples: u64,
    #[serde(rename = "declaredTokens")]
    pub declared_tokens: u64,
    #[serde(rename = "totalBytes")]
    pub total_bytes: u64,
    pub shards: Vec<ManifestShard>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ManifestShard {
    pub cid: String,
    pub sha256: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: u64,
}

/// Staging failures, typed by the wire class they will map to in `core.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageError {
    /// A cryptographic/content claim failed → `DATASET_INTEGRITY` (C.3).
    Integrity(String),
    /// A schema/cross-check violation → `VALIDATION_FAILED`.
    Validation(String),
    /// Fetch/network death — infrastructure, never brands the dataset.
    Transport(String),
    /// Local filesystem failure on the staging volume.
    Io(String),
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("0x{}", hex::encode(Sha256::digest(data)))
}

/// Case-insensitive hex equality tolerating an optional `0x` prefix on
/// either side (the wire and the vectors both use `0x`-prefixed lowercase,
/// but a claim is a claim, not a formatting contest).
fn hex_eq(a: &str, b: &str) -> bool {
    let strip = |s: &str| {
        s.trim_start_matches("0x")
            .trim_start_matches("0X")
            .to_lowercase()
    };
    strip(a) == strip(b)
}

/// Fetch + decrypt the manifest and verify the wire's `manifestSha256` over
/// the exact decrypted (canonical, as-stored) bytes. Capability-internal
/// failures (portal 404, blake3 gate, decrypt auth) are Transport — the blob
/// SERVICE failed us; only the wire's own sha256 claim failing is Integrity.
pub async fn fetch_manifest(
    s5_base: &str,
    manifest_cid: &str,
    expected_sha256_hex: &str,
) -> Result<DatasetManifest, StageError> {
    gate_capability_len(manifest_cid, MANIFEST_MAX_BYTES, "manifest")?;
    let (_pt_hash, bytes) = crate::ltx::input_image::fetch_image_hash(s5_base, manifest_cid)
        .await
        .map_err(|e| StageError::Transport(format!("manifest fetch: {e}")))?;
    let actual = sha256_hex(&bytes);
    if !hex_eq(&actual, expected_sha256_hex) {
        // Round-8 F-R8-3: the dataset twin of the serve-back echo bounded in
        // F-R7-5. `expected_sha256_hex` is the wire's `dataset.manifestSha256`,
        // unbounded, and this is a whitelisted Integrity arm.
        return Err(StageError::Integrity(format!(
            "manifestSha256 mismatch: stored bytes hash {actual}, wire claims {}",
            crate::training::redact::echo(expected_sha256_hex)
        )));
    }
    let manifest: DatasetManifest = serde_json::from_slice(&bytes)
        .map_err(|e| StageError::Validation(format!("manifest parse: {e}")))?;
    if manifest.schema != "dataset-manifest-v1" {
        return Err(StageError::Validation(format!(
            "manifest schema {:?} is not dataset-manifest-v1",
            manifest.schema
        )));
    }
    if manifest.format != "jsonl-text-v1" {
        return Err(StageError::Validation(format!(
            "manifest format {:?} is not jsonl-text-v1",
            manifest.format
        )));
    }
    if manifest.counting_recipe != "count-v1" {
        return Err(StageError::Validation(format!(
            "manifest countingRecipe {:?} is not count-v1",
            manifest.counting_recipe
        )));
    }
    Ok(manifest)
}

/// The wire↔manifest↔template cross-checks (D.2): declaredTokens, samples,
/// tokenizer pin, shard bounds (D.1), byte totals.
pub fn cross_check_manifest(
    manifest: &DatasetManifest,
    job: &TrainingJob,
    template_tokenizer_sha256: &str,
) -> Result<(), StageError> {
    let fail = |detail: String| Err(StageError::Validation(detail));
    if manifest.declared_tokens != job.dataset.declared_tokens {
        return fail(format!(
            "declaredTokens diverge: wire {} vs manifest {}",
            job.dataset.declared_tokens, manifest.declared_tokens
        ));
    }
    if manifest.samples != job.dataset.samples {
        return fail(format!(
            "samples diverge: wire {} vs manifest {}",
            job.dataset.samples, manifest.samples
        ));
    }
    if !hex_eq(&manifest.tokenizer_sha256, template_tokenizer_sha256) {
        return fail(format!(
            "manifest tokenizer pin {} != template's {template_tokenizer_sha256}",
            manifest.tokenizer_sha256
        ));
    }
    if manifest.shards.is_empty() {
        return fail("manifest has no shards".to_string());
    }
    if manifest.shards.len() > MAX_SHARDS {
        return fail(format!(
            "manifest declares {} shards > the {MAX_SHARDS} cap (fetch-storm bound)",
            manifest.shards.len()
        ));
    }
    let mut sum: u64 = 0;
    for (index, shard) in manifest.shards.iter().enumerate() {
        if shard.size_bytes == 0 {
            return fail(format!("shard {index} declares zero bytes"));
        }
        if shard.size_bytes > SHARD_PLAINTEXT_MAX_BYTES {
            return fail(format!(
                "shard {index} declares {} bytes > the D.1 cap {SHARD_PLAINTEXT_MAX_BYTES}",
                shard.size_bytes
            ));
        }
        if shard.size_bytes.is_multiple_of(AEAD_CHUNK_BYTES) {
            return fail(format!(
                "shard {index} declares an exact {AEAD_CHUNK_BYTES}-byte multiple — D.1's \
                 splitter rule forbids it (the chunk scheme cannot stage it anywhere)"
            ));
        }
        sum = sum.saturating_add(shard.size_bytes);
    }
    if sum != manifest.total_bytes {
        return fail(format!(
            "totalBytes {} != shard sum {sum}",
            manifest.total_bytes
        ));
    }
    Ok(())
}

/// Fetch every shard in manifest order, verify each plaintext against its
/// declared sha256/size, concatenate into `job-<jobId>/dataset.jsonl` via
/// write-then-rename (the sidecar's §3.3 stat discipline sees either nothing
/// or the complete file). Returns the staged dataset path.
pub async fn stage_shards(
    s5_base: &str,
    staging_root: &Path,
    job_id: u64,
    manifest: &DatasetManifest,
) -> Result<PathBuf, StageError> {
    use tokio::io::AsyncWriteExt;

    let job_dir = staging_root.join(format!("job-{job_id}"));
    tokio::fs::create_dir_all(&job_dir)
        .await
        .map_err(|e| StageError::Io(format!("create {job_dir:?}: {e}")))?;
    let tmp_path = job_dir.join("dataset.jsonl.tmp");
    let final_path = job_dir.join("dataset.jsonl");

    let result = async {
        let mut file = tokio::fs::File::create(&tmp_path)
            .await
            .map_err(|e| StageError::Io(format!("create tmp: {e}")))?;
        for (index, shard) in manifest.shards.iter().enumerate() {
            // Pre-fetch: the capability CID's self-declared length must
            // EQUAL the manifest's claim — an oversized blob is refused
            // before a byte moves (converge round: the post-fetch length
            // check let a 100-byte claim buffer gigabytes first).
            let declared_len = gate_capability_len(&shard.cid, SHARD_PLAINTEXT_MAX_BYTES, "shard")
                .map_err(|e| match e {
                    StageError::Validation(d) => {
                        StageError::Integrity(format!("shard {index}: {d}"))
                    }
                    other => other,
                })?;
            if declared_len != shard.size_bytes {
                return Err(StageError::Integrity(format!(
                    "shard {index} capability declares {declared_len} bytes but the manifest \
                     sizeBytes claims {} — refused before fetch",
                    shard.size_bytes
                )));
            }
            let (_h, plaintext) = crate::ltx::input_image::fetch_image_hash(s5_base, &shard.cid)
                .await
                .map_err(|e| StageError::Transport(format!("shard {index} fetch: {e}")))?;
            if plaintext.len() as u64 != shard.size_bytes {
                return Err(StageError::Integrity(format!(
                    "shard {index} size claim {} vs actual {} bytes",
                    shard.size_bytes,
                    plaintext.len()
                )));
            }
            let actual = sha256_hex(&plaintext);
            if !hex_eq(&actual, &shard.sha256) {
                return Err(StageError::Integrity(format!(
                    "shard {index} plaintext sha256 mismatch: {actual} vs declared {}",
                    shard.sha256
                )));
            }
            file.write_all(&plaintext)
                .await
                .map_err(|e| StageError::Io(format!("write shard {index}: {e}")))?;
        }
        file.sync_all()
            .await
            .map_err(|e| StageError::Io(format!("sync: {e}")))?;
        drop(file);
        tokio::fs::rename(&tmp_path, &final_path)
            .await
            .map_err(|e| StageError::Io(format!("rename: {e}")))?;
        Ok(final_path.clone())
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&tmp_path).await; // no torn staging survives
    }
    result
}

/// TD15 boot sweep: at startup NOTHING is legitimately in flight, so every
/// `job-*` dir under the given root is an orphan of a crashed run — delete
/// them all. Returns how many were removed. Called for BOTH the staging root
/// and the work root.
pub fn sweep_orphan_job_dirs(root: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0; // absent root = nothing to sweep (first boot)
    };
    let mut swept = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("job-")
            && entry.path().is_dir()
            && std::fs::remove_dir_all(entry.path()).is_ok()
        {
            swept += 1;
        }
    }
    swept
}
