// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 2.4/2.5 — model-source orchestration: fetch → attest → DEK → decrypt to
//! a private tmpfs file, with a model-identity cache, refcounting, fail-closed
//! cleanup, and `secure_delete`.

use async_trait::async_trait;
use fabstir_llm_node::tee::container::{encrypt_model, AEAD_TAG_LEN, HEADER_LEN};
use fabstir_llm_node::tee::mock::{MockAttestationProvider, MockKeyBroker};
use fabstir_llm_node::tee::model_source::{
    is_tmpfs, secure_delete, BlobSource, EncryptedModelLoader, EncryptedModelSpec,
};
use fabstir_llm_node::tee::types::{Policy, TeeError, TeeResult};
use std::collections::HashMap;
use std::path::Path;

const SKU: &str = "H100";
const MEASUREMENT: [u8; 48] = [0x42u8; 48];

fn test_policy(model_id: [u8; 32]) -> Policy {
    Policy {
        policy_version: 1,
        allowed_skus: vec![SKU.to_string()],
        expected_measurement: MEASUREMENT,
        require_cc_on: true,
        require_production_tcb: true,
        max_tcb_age_days: 30,
        not_before: 0,
        expiry: u64::MAX - 1, // valid now (avoid the u64::MAX clock-error sentinel)
        model_id,
    }
}

/// In-memory blob store standing in for S5.
struct InMemoryBlobs {
    blobs: HashMap<String, Vec<u8>>,
}
#[async_trait]
impl BlobSource for InMemoryBlobs {
    async fn get_file(&self, path: &str) -> TeeResult<Vec<u8>> {
        self.blobs.get(path).cloned().ok_or_else(|| {
            TeeError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                path.to_string(),
            ))
        })
    }
}

fn loader() -> (EncryptedModelLoader, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let loader = EncryptedModelLoader::new(dir.path()).with_tee_enabled(true);
    (loader, dir)
}

fn good_provider() -> MockAttestationProvider {
    MockAttestationProvider::new(SKU, MEASUREMENT, true)
}

fn dir_is_empty(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().count() == 0)
        .unwrap_or(true)
}

#[tokio::test]
async fn prepare_decrypts_attested_model_to_tmpfs() {
    let (loader, _dir) = loader();
    let (model_id, dek, policy_hash) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    let plaintext: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
    let container = encrypt_model(&plaintext, &dek, model_id, policy_hash, 1024).unwrap();

    let s5 = InMemoryBlobs {
        blobs: HashMap::from([("models/m.enc".to_string(), container)]),
    };
    let kbs = MockKeyBroker::new(HashMap::from([(model_id, (dek, test_policy(model_id)))]));
    let spec = EncryptedModelSpec {
        model_id,
        policy_hash,
        encrypted_path: "models/m.enc".to_string(),
    };

    let path = loader
        .prepare_encrypted_model(&s5, &kbs, &good_provider(), &spec)
        .await
        .expect("prepare");
    let got = std::fs::read(&path).expect("read decrypted file");
    assert_eq!(
        got, plaintext,
        "decrypted file must equal the original plaintext"
    );
}

#[tokio::test]
async fn prepare_fails_closed_on_bad_attestation() {
    let (loader, dir) = loader();
    let (model_id, dek, policy_hash) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    let container = encrypt_model(b"weights", &dek, model_id, policy_hash, 16).unwrap();
    let s5 = InMemoryBlobs {
        blobs: HashMap::from([("m".to_string(), container)]),
    };
    let kbs = MockKeyBroker::new(HashMap::from([(model_id, (dek, test_policy(model_id)))]));
    let spec = EncryptedModelSpec {
        model_id,
        policy_hash,
        encrypted_path: "m".to_string(),
    };

    // Provider with a WRONG measurement → verifier rejects → broker withholds the DEK.
    let bad = MockAttestationProvider::new(SKU, [0u8; 48], true);
    let err = loader
        .prepare_encrypted_model(&s5, &kbs, &bad, &spec)
        .await
        .expect_err("bad attestation must fail closed");
    assert!(
        matches!(err, TeeError::VerificationFailed(_)),
        "got {err:?}"
    );
    assert!(dir_is_empty(dir.path()), "no decrypted file may be written");
}

#[tokio::test]
async fn prepare_fails_closed_on_wrong_key() {
    let (loader, dir) = loader();
    let (model_id, real_dek, wrong_dek, policy_hash) = ([1u8; 32], [2u8; 32], [9u8; 32], [3u8; 32]);
    let container = encrypt_model(b"secret weights", &real_dek, model_id, policy_hash, 8).unwrap();
    let s5 = InMemoryBlobs {
        blobs: HashMap::from([("m".to_string(), container)]),
    };
    // Attestation passes, but the broker returns the WRONG DEK → chunk 0 fails the
    // AEAD tag immediately (no bytes written). The empty file must still be removed.
    let kbs = MockKeyBroker::new(HashMap::from([(
        model_id,
        (wrong_dek, test_policy(model_id)),
    )]));
    let spec = EncryptedModelSpec {
        model_id,
        policy_hash,
        encrypted_path: "m".to_string(),
    };

    let err = loader
        .prepare_encrypted_model(&s5, &kbs, &good_provider(), &spec)
        .await
        .expect_err("wrong DEK must fail the AEAD and fail closed");
    assert!(matches!(err, TeeError::Crypto(_)), "got {err:?}");
    assert!(dir_is_empty(dir.path()), "no decrypted file may remain");
}

#[tokio::test]
async fn prepare_secure_deletes_nonempty_partial_on_late_chunk_failure() {
    // The headline fail-closed guarantee: chunk 0 decrypts + writes REAL bytes,
    // then a tampered chunk 1 fails → the NON-empty partial file must be deleted.
    let (loader, dir) = loader();
    let (model_id, dek, policy_hash) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    let plaintext = vec![0x5Au8; 8 * 2 + 4]; // 20 bytes at chunk_size 8 ⇒ 3 chunks
    let mut container = encrypt_model(&plaintext, &dek, model_id, policy_hash, 8).unwrap();
    // Corrupt a byte inside chunk 1's ciphertext (after the header + full chunk 0).
    let full_ct = 8 + AEAD_TAG_LEN;
    container[HEADER_LEN + full_ct + 2] ^= 0x01;

    let s5 = InMemoryBlobs {
        blobs: HashMap::from([("m".to_string(), container)]),
    };
    let kbs = MockKeyBroker::new(HashMap::from([(model_id, (dek, test_policy(model_id)))]));
    let spec = EncryptedModelSpec {
        model_id,
        policy_hash,
        encrypted_path: "m".to_string(),
    };

    let err = loader
        .prepare_encrypted_model(&s5, &kbs, &good_provider(), &spec)
        .await
        .expect_err("a tampered later chunk must fail after an earlier chunk was written");
    assert!(matches!(err, TeeError::Crypto(_)), "got {err:?}");
    assert!(
        dir_is_empty(dir.path()),
        "the non-empty partial decrypted file must be securely deleted"
    );
}

#[tokio::test]
async fn prepare_caches_by_model_identity_and_refcounts() {
    let (loader, _dir) = loader();
    let (model_id, dek, policy_hash) = ([7u8; 32], [8u8; 32], [9u8; 32]);
    let plaintext = vec![0xABu8; 2048];
    let container = encrypt_model(&plaintext, &dek, model_id, policy_hash, 512).unwrap();
    let s5 = InMemoryBlobs {
        blobs: HashMap::from([("m".to_string(), container)]),
    };
    let kbs = MockKeyBroker::new(HashMap::from([(model_id, (dek, test_policy(model_id)))]));
    let spec = EncryptedModelSpec {
        model_id,
        policy_hash,
        encrypted_path: "m".to_string(),
    };

    let p1 = loader
        .prepare_encrypted_model(&s5, &kbs, &good_provider(), &spec)
        .await
        .unwrap();
    let p2 = loader
        .prepare_encrypted_model(&s5, &kbs, &good_provider(), &spec)
        .await
        .unwrap();
    assert_eq!(
        p1, p2,
        "a second load of the same model returns the cached path"
    );

    // Two refs held; one release leaves the file in place under eviction.
    loader.release(&model_id, &policy_hash);
    loader.evict_unreferenced();
    assert!(
        p1.exists(),
        "still referenced (refcount 1) — must not be evicted"
    );

    // Final release → refcount 0 → eviction securely deletes it.
    loader.release(&model_id, &policy_hash);
    loader.evict_unreferenced();
    assert!(
        !p1.exists(),
        "unreferenced model must be evicted and deleted"
    );
}

#[tokio::test]
async fn prepare_re_decrypts_when_policy_hash_changes() {
    // Phase 4.3.1a — the decrypted-file cache keys on (model_id, policy_hash), so a
    // policy rotation (new policy_hash, same model) forces a fresh decrypt + re-attest
    // instead of silently serving the file decrypted under the OLD policy.
    let (loader, _dir) = loader();
    let (model_id, dek) = ([0x11u8; 32], [0x22u8; 32]);
    let plaintext = vec![0x5Cu8; 1024];
    let (hash_a, hash_b) = ([0xAAu8; 32], [0xBBu8; 32]);

    let container_a = encrypt_model(&plaintext, &dek, model_id, hash_a, 256).unwrap();
    let container_b = encrypt_model(&plaintext, &dek, model_id, hash_b, 256).unwrap();
    let s5 = InMemoryBlobs {
        blobs: HashMap::from([
            ("a".to_string(), container_a),
            ("b".to_string(), container_b),
        ]),
    };
    let kbs = MockKeyBroker::new(HashMap::from([(model_id, (dek, test_policy(model_id)))]));
    let spec_a = EncryptedModelSpec {
        model_id,
        policy_hash: hash_a,
        encrypted_path: "a".to_string(),
    };
    let spec_b = EncryptedModelSpec {
        model_id,
        policy_hash: hash_b,
        encrypted_path: "b".to_string(),
    };

    let p_a = loader
        .prepare_encrypted_model(&s5, &kbs, &good_provider(), &spec_a)
        .await
        .unwrap();
    let p_b = loader
        .prepare_encrypted_model(&s5, &kbs, &good_provider(), &spec_b)
        .await
        .unwrap();
    assert_ne!(
        p_a, p_b,
        "a changed policy_hash must force a separate decrypt, not reuse the old-policy file"
    );

    // The original policy still resolves to its own cached file (same key → hit).
    let p_a2 = loader
        .prepare_encrypted_model(&s5, &kbs, &good_provider(), &spec_a)
        .await
        .unwrap();
    assert_eq!(
        p_a, p_a2,
        "same (model_id, policy_hash) still hits the cache"
    );
}

/// A BlobSource that sleeps so two concurrent first-loads overlap (forcing the
/// `cache_publish` decrypt-twice-keep-one race, not just the cache fast-path).
struct SlowBlobs {
    blobs: HashMap<String, Vec<u8>>,
}
#[async_trait]
impl BlobSource for SlowBlobs {
    async fn get_file(&self, path: &str) -> TeeResult<Vec<u8>> {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        self.blobs.get(path).cloned().ok_or_else(|| {
            TeeError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                path.to_string(),
            ))
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prepare_dedups_concurrent_loads_of_same_model() {
    use std::sync::Arc;
    let (loader_raw, dir) = loader();
    let loader = Arc::new(loader_raw);
    let (model_id, dek, policy_hash) = ([3u8; 32], [4u8; 32], [5u8; 32]);
    let plaintext = vec![0x77u8; 1500];
    let container = encrypt_model(&plaintext, &dek, model_id, policy_hash, 256).unwrap();
    let s5 = Arc::new(SlowBlobs {
        blobs: HashMap::from([("m".to_string(), container)]),
    });
    let kbs = Arc::new(MockKeyBroker::new(HashMap::from([(
        model_id,
        (dek, test_policy(model_id)),
    )])));
    let spec = EncryptedModelSpec {
        model_id,
        policy_hash,
        encrypted_path: "m".to_string(),
    };

    // Two concurrent first-loads of the same model.
    let (l1, s1, k1, sp1) = (loader.clone(), s5.clone(), kbs.clone(), spec.clone());
    let (l2, s2, k2, sp2) = (loader.clone(), s5.clone(), kbs.clone(), spec.clone());
    let h1 = tokio::spawn(async move {
        let p = good_provider();
        l1.prepare_encrypted_model(s1.as_ref(), k1.as_ref(), &p, &sp1)
            .await
    });
    let h2 = tokio::spawn(async move {
        let p = good_provider();
        l2.prepare_encrypted_model(s2.as_ref(), k2.as_ref(), &p, &sp2)
            .await
    });
    let p1 = h1.await.unwrap().expect("load 1");
    let p2 = h2.await.unwrap().expect("load 2");

    assert_eq!(
        p1, p2,
        "concurrent loads of the same model must return the same cached path"
    );
    let files = std::fs::read_dir(dir.path()).unwrap().flatten().count();
    assert_eq!(
        files, 1,
        "the redundant concurrent decrypt must be purged; found {files} files"
    );

    // Both callers took a reference (refcount == 2): one release survives eviction, two frees it.
    loader.release(&model_id, &policy_hash);
    loader.evict_unreferenced();
    assert!(
        p1.exists(),
        "one reference still outstanding — must not be evicted"
    );
    loader.release(&model_id, &policy_hash);
    loader.evict_unreferenced();
    assert!(
        !p1.exists(),
        "after both releases the model is evicted and deleted"
    );
}

#[tokio::test]
async fn decrypted_file_is_private_0600() {
    use std::os::unix::fs::PermissionsExt;
    let (loader, _dir) = loader();
    let (model_id, dek, policy_hash) = ([5u8; 32], [6u8; 32], [7u8; 32]);
    let container = encrypt_model(b"weights", &dek, model_id, policy_hash, 16).unwrap();
    let s5 = InMemoryBlobs {
        blobs: HashMap::from([("m".to_string(), container)]),
    };
    let kbs = MockKeyBroker::new(HashMap::from([(model_id, (dek, test_policy(model_id)))]));
    let spec = EncryptedModelSpec {
        model_id,
        policy_hash,
        encrypted_path: "m".to_string(),
    };
    let path = loader
        .prepare_encrypted_model(&s5, &kbs, &good_provider(), &spec)
        .await
        .unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(
        mode & 0o077,
        0,
        "decrypted weights must not be group/world accessible (mode {mode:o})"
    );
    assert_eq!(mode & 0o600, 0o600, "owner must have read+write");
}

#[tokio::test]
async fn prepare_refuses_encrypted_model_when_tee_disabled() {
    // A non-TEE node (HOST_TEE_ENABLED=false) must refuse encrypted models, fail-closed,
    // before any S5 fetch — no download, no plaintext. (new() defaults tee-disabled.)
    let dir = tempfile::tempdir().unwrap();
    let loader = EncryptedModelLoader::new(dir.path());
    let (model_id, dek, policy_hash) = ([1u8; 32], [2u8; 32], [3u8; 32]);
    let container = encrypt_model(b"weights", &dek, model_id, policy_hash, 16).unwrap();
    let s5 = InMemoryBlobs {
        blobs: HashMap::from([("m".to_string(), container)]),
    };
    let kbs = MockKeyBroker::new(HashMap::from([(model_id, (dek, test_policy(model_id)))]));
    let spec = EncryptedModelSpec {
        model_id,
        policy_hash,
        encrypted_path: "m".to_string(),
    };
    let err = loader
        .prepare_encrypted_model(&s5, &kbs, &good_provider(), &spec)
        .await
        .expect_err("a non-TEE node must refuse encrypted models");
    assert!(
        matches!(err, TeeError::NonTeeNodeRefusesEncrypted),
        "got {err:?}"
    );
    assert!(
        dir_is_empty(dir.path()),
        "no plaintext on a non-TEE refusal"
    );
}

#[test]
fn secure_delete_overwrites_and_removes_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secret.bin");
    std::fs::write(&path, b"sensitive plaintext").unwrap();
    assert!(path.exists());
    secure_delete(&path).expect("secure_delete");
    assert!(!path.exists(), "file must be removed");
    secure_delete(&path).expect("secure_delete is idempotent on a missing file");
}

// ---- Sub-phase 2.5: tmpfs verification + decrypted-file lifecycle -----------

#[test]
fn is_tmpfs_classifies_known_mounts() {
    // /dev/shm is tmpfs on essentially every Linux; assert it when present.
    if Path::new("/dev/shm").exists() {
        assert!(
            is_tmpfs(Path::new("/dev/shm")),
            "/dev/shm must be detected as tmpfs"
        );
    }
    // A path that does not exist cannot be classified tmpfs — and must not panic.
    assert!(!is_tmpfs(Path::new("/nonexistent/tee/decrypt/dir")));
}

#[test]
fn verify_decrypt_dir_ok_when_writable() {
    let (loader, _dir) = loader();
    loader
        .verify_decrypt_dir()
        .expect("a writable decrypt dir must verify");
}

#[tokio::test]
async fn decrypt_file_exists_then_secure_delete_makes_read_fail() {
    let (loader, _dir) = loader();
    let (model_id, dek, policy_hash) = ([4u8; 32], [5u8; 32], [6u8; 32]);
    let plaintext = vec![0xCDu8; 300];
    let container = encrypt_model(&plaintext, &dek, model_id, policy_hash, 128).unwrap();
    let s5 = InMemoryBlobs {
        blobs: HashMap::from([("m".to_string(), container)]),
    };
    let kbs = MockKeyBroker::new(HashMap::from([(model_id, (dek, test_policy(model_id)))]));
    let spec = EncryptedModelSpec {
        model_id,
        policy_hash,
        encrypted_path: "m".to_string(),
    };

    let path = loader
        .prepare_encrypted_model(&s5, &kbs, &good_provider(), &spec)
        .await
        .unwrap();
    assert!(path.exists(), "decrypted file must exist after prepare");
    assert_eq!(std::fs::read(&path).unwrap(), plaintext);

    secure_delete(&path).unwrap();
    assert!(
        std::fs::read(&path).is_err(),
        "reading the model after secure_delete must fail"
    );
}

// ---- Sub-phase 3.3: end-to-end real-KBS wiring -----------------------------

#[tokio::test]
async fn e2e_real_attestation_kbs_roundtrip_then_measurement_flip_fails() {
    let (loader, dir) = loader();
    let (model_id, dek, policy_hash) = ([0xE2u8; 32], [0xDEu8; 32], [0xAAu8; 32]);
    let plaintext = vec![0x33u8; 4096];
    let container = encrypt_model(&plaintext, &dek, model_id, policy_hash, 1024).unwrap();
    let s5 = InMemoryBlobs {
        blobs: HashMap::from([("m".to_string(), container)]),
    };
    let kbs = MockKeyBroker::new(HashMap::from([(model_id, (dek, test_policy(model_id)))]));
    let spec = EncryptedModelSpec {
        model_id,
        policy_hash,
        encrypted_path: "m".to_string(),
    };

    // Happy path: provider → evidence → KBS verifies → wraps DEK → node unwraps → decrypts.
    let path = loader
        .prepare_encrypted_model(&s5, &kbs, &good_provider(), &spec)
        .await
        .expect("e2e real-KBS flow");
    assert_eq!(
        std::fs::read(&path).unwrap(),
        plaintext,
        "the full real-KBS flow recovers the plaintext weights"
    );
    loader.release(&model_id, &policy_hash);
    loader.evict_unreferenced();

    // Flip the attested measurement → KBS verification rejects → fail closed, no plaintext.
    let wrong_meas = MockAttestationProvider::new(SKU, [0xFFu8; 48], true);
    let err = loader
        .prepare_encrypted_model(&s5, &kbs, &wrong_meas, &spec)
        .await
        .expect_err("flipping the measurement must fail closed");
    assert!(
        matches!(err, TeeError::VerificationFailed(_)),
        "got {err:?}"
    );
    assert!(
        dir_is_empty(dir.path()),
        "no plaintext may remain after a rejected attestation"
    );
}
