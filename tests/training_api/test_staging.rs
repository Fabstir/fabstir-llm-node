// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Staging matrix (interface C.3/D.1/D.2): manifest fetch/verify, the
//! wire↔manifest↔template cross-checks, per-shard integrity, and the
//! write-then-rename — against a REAL mock S5 blob server serving REAL
//! capability-CID-encrypted blobs (`ltx::exr::encrypt_frame` +
//! `capability_cid`), so every decrypt/hash path is the production one.

use std::collections::HashMap;

use fabstir_llm_node::training::staging::{
    cross_check_manifest, fetch_manifest, stage_shards, DatasetManifest, StageError,
    SHARD_PLAINTEXT_MAX_BYTES,
};

use fabstir_llm_node::training::attestation::{
    canonical_manifest_bytes, canonical_manifest_sha256,
};

use super::support::{encrypt_blob, fixture, sha256_hex, spawn_s5, Fixture, TOK_SHA};

// --- manifest fetch ---

#[tokio::test]
async fn manifest_fetch_happy_parses_and_verifies() {
    let fx = fixture(None).await;
    let manifest = fetch_manifest(&fx.base_url, &fx.manifest_cid, &fx.manifest_sha256)
        .await
        .expect("manifest fetches");
    assert_eq!(manifest, fx.manifest);
}

#[tokio::test]
async fn manifest_sha_mismatch_is_integrity() {
    // The wire claims a DIFFERENT manifestSha256 than the stored bytes hash
    // to — the C.3 "wrong manifestSha256" row.
    let fx = fixture(None).await;
    let wrong = sha256_hex(b"not the manifest");
    match fetch_manifest(&fx.base_url, &fx.manifest_cid, &wrong)
        .await
        .unwrap_err()
    {
        StageError::Integrity(detail) => assert!(detail.contains("manifestSha256"), "{detail}"),
        other => panic!("expected Integrity, got {other:?}"),
    }
}

#[tokio::test]
async fn manifest_bad_schema_literal_is_validation() {
    // A manifest whose schema/format literals are wrong is a VALIDATION
    // failure even though its bytes hash correctly.
    let value = serde_json::json!({
        "schema": "dataset-manifest-v2",
        "format": "jsonl-text-v1",
        "countingRecipe": "count-v1",
        "tokenizerSha256": TOK_SHA,
        "samples": 1u64, "declaredTokens": 1u64, "totalBytes": 1u64,
        "shards": [ { "cid": "u", "sha256": "0x00", "sizeBytes": 1u64 } ]
    });
    let stored = canonical_manifest_bytes(&value).into_bytes();
    let sha = canonical_manifest_sha256(&value);
    let (cap, dl, ct) = encrypt_blob(&stored);
    let base = spawn_s5(HashMap::from([(dl, ct)])).await;
    match fetch_manifest(&base, &cap, &sha).await.unwrap_err() {
        StageError::Validation(detail) => assert!(detail.contains("schema"), "{detail}"),
        other => panic!("expected Validation, got {other:?}"),
    }
}

#[tokio::test]
async fn manifest_fetch_transport_failure_is_transport() {
    let base = spawn_s5(HashMap::new()).await; // empty store → 404
    let fx_cid = encrypt_blob(b"whatever").0;
    match fetch_manifest(&base, &fx_cid, "0x00").await.unwrap_err() {
        StageError::Transport(_) => {}
        other => panic!("expected Transport, got {other:?}"),
    }
}

// --- cross-checks (pure) ---

fn check(fx: &Fixture, mutate: impl FnOnce(&mut DatasetManifest)) -> Result<(), StageError> {
    let mut manifest = fx.manifest.clone();
    mutate(&mut manifest);
    cross_check_manifest(&manifest, &fx.job, TOK_SHA)
}

#[tokio::test]
async fn cross_check_happy_passes() {
    let fx = fixture(None).await;
    assert_eq!(check(&fx, |_| {}), Ok(()));
}

#[tokio::test]
async fn cross_check_rejects_each_divergence() {
    let fx = fixture(None).await;
    let declared = check(&fx, |m| m.declared_tokens += 1);
    assert!(matches!(declared, Err(StageError::Validation(ref d)) if d.contains("declaredTokens")));
    let samples = check(&fx, |m| m.samples += 1);
    assert!(matches!(samples, Err(StageError::Validation(ref d)) if d.contains("samples")));
    let tok = check(&fx, |m| m.tokenizer_sha256 = "0xbb".to_string());
    assert!(matches!(tok, Err(StageError::Validation(ref d)) if d.contains("tokenizer")));
    let oversize = check(&fx, |m| {
        m.shards[0].size_bytes = SHARD_PLAINTEXT_MAX_BYTES + 1
    });
    assert!(matches!(oversize, Err(StageError::Validation(ref d)) if d.contains("shard")));
    let sum = check(&fx, |m| m.total_bytes += 5);
    assert!(matches!(sum, Err(StageError::Validation(ref d)) if d.contains("totalBytes")));
    let empty = check(&fx, |m| m.shards.clear());
    assert!(matches!(empty, Err(StageError::Validation(_))));
}

// --- shard staging ---

#[tokio::test]
async fn stage_shards_happy_concatenates_in_order_no_tmp_left() {
    let fx = fixture(None).await;
    let dir = tempfile::tempdir().unwrap();
    let path = stage_shards(&fx.base_url, dir.path(), 42, &fx.manifest)
        .await
        .expect("stages");
    assert_eq!(path, dir.path().join("job-42").join("dataset.jsonl"));
    let expected: Vec<u8> = fx.shard_plaintexts.concat();
    assert_eq!(std::fs::read(&path).unwrap(), expected);
    let leftovers: Vec<_> = std::fs::read_dir(dir.path().join("job-42"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains("tmp"))
        .collect();
    assert!(leftovers.is_empty(), "write-then-rename must leave no tmp");
}

#[tokio::test]
async fn tampered_shard_is_integrity_naming_the_shard() {
    // The blob decrypts fine (its capability hashes are self-consistent);
    // the MANIFEST's sha256 claim for shard 0 is the lie → DATASET_INTEGRITY.
    let fx = fixture(Some("shard-sha")).await;
    let dir = tempfile::tempdir().unwrap();
    match stage_shards(&fx.base_url, dir.path(), 43, &fx.manifest)
        .await
        .unwrap_err()
    {
        StageError::Integrity(detail) => assert!(detail.contains("shard 0"), "{detail}"),
        other => panic!("expected Integrity, got {other:?}"),
    }
    let staged = dir.path().join("job-43").join("dataset.jsonl");
    assert!(!staged.exists(), "no final file on integrity failure");
}

#[tokio::test]
async fn shard_size_claim_mismatch_is_integrity() {
    let fx = fixture(None).await;
    let mut manifest = fx.manifest.clone();
    manifest.shards[1].size_bytes -= 1;
    manifest.total_bytes -= 1; // keep the sum consistent so ONLY the size lies
    let dir = tempfile::tempdir().unwrap();
    match stage_shards(&fx.base_url, dir.path(), 44, &manifest)
        .await
        .unwrap_err()
    {
        StageError::Integrity(detail) => assert!(detail.contains("size"), "{detail}"),
        other => panic!("expected Integrity, got {other:?}"),
    }
}

// --- the round-1 fetch gates (unbounded-fetch DoS + D.1 + shard-count) ---

#[tokio::test]
async fn oversized_manifest_capability_is_refused_before_any_fetch() {
    // A capability CID claiming megabytes of manifest must die pre-fetch —
    // the round-1 review proved a dust session could OOM the node without
    // this (the LTX validate_input_cids hole, reopened and now closed).
    // 2 MiB + 5 (a plain 2 MiB is an exact chunk multiple the encryptor
    // itself refuses — the test must exercise OUR gate, not that one).
    let big = vec![0x41u8; 2 * 1024 * 1024 + 5];
    let (cap, _dl, _ct) = encrypt_blob(&big); // never hosted: must not matter
    let base = spawn_s5(HashMap::new()).await;
    match fetch_manifest(&base, &cap, "0x00").await.unwrap_err() {
        StageError::Validation(detail) => {
            assert!(detail.contains("refused before fetch"), "{detail}")
        }
        other => panic!("expected pre-fetch Validation, got {other:?}"),
    }
}

#[tokio::test]
async fn shard_count_over_the_cap_rejects() {
    let fx = fixture(None).await;
    let mut manifest = fx.manifest.clone();
    let template_shard = manifest.shards[0].clone();
    manifest.shards = (0..65).map(|_| template_shard.clone()).collect();
    manifest.total_bytes = 65 * template_shard.size_bytes;
    match cross_check_manifest(&manifest, &fx.job, TOK_SHA).unwrap_err() {
        StageError::Validation(detail) => assert!(detail.contains("shards"), "{detail}"),
        other => panic!("expected Validation, got {other:?}"),
    }
}

#[tokio::test]
async fn exact_chunk_multiple_shard_size_is_a_terminal_validation() {
    // D.1: the chunk scheme can NEVER stage an exact 262,144-byte multiple —
    // it must be a terminal client error, not an endlessly re-shopped
    // "host infra" failure.
    let fx = fixture(None).await;
    let mut manifest = fx.manifest.clone();
    manifest.shards[0].size_bytes = 262_144;
    manifest.total_bytes = 262_144 + manifest.shards[1].size_bytes;
    match cross_check_manifest(&manifest, &fx.job, TOK_SHA).unwrap_err() {
        StageError::Validation(detail) => assert!(detail.contains("multiple"), "{detail}"),
        other => panic!("expected Validation, got {other:?}"),
    }
}

#[tokio::test]
async fn shard_capability_length_must_equal_the_manifest_claim_pre_fetch() {
    // The capability envelope self-declares its plaintext length; a mismatch
    // with the manifest's sizeBytes is refused BEFORE any bytes move.
    let fx = fixture(None).await;
    let mut manifest = fx.manifest.clone();
    // Lie coherently at the manifest level (sum stays consistent) but the
    // capability CID still declares the TRUE length.
    manifest.shards[0].size_bytes += 7;
    manifest.total_bytes += 7;
    let dir = tempfile::tempdir().unwrap();
    match stage_shards(&fx.base_url, dir.path(), 45, &manifest)
        .await
        .unwrap_err()
    {
        StageError::Integrity(detail) => {
            assert!(detail.contains("refused before fetch"), "{detail}")
        }
        other => panic!("expected Integrity, got {other:?}"),
    }
}
