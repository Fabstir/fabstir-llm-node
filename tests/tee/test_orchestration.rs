// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 4.3.1 / 4.3.2 — the live-request model-load orchestration:
//! fetch+validate policy → attested decrypt to tmpfs → on-chain hash binding.
//! Every gate is fail-closed; a failure after decrypt securely deletes the plaintext.

use async_trait::async_trait;
use fabstir_llm_node::crypto::recover_client_address;
use fabstir_llm_node::tee::container::encrypt_model;
use fabstir_llm_node::tee::mock::{MockAttestationProvider, MockKeyBroker};
use fabstir_llm_node::tee::model_source::{BlobSource, EncryptedModelLoader, EncryptedModelSpec};
use fabstir_llm_node::tee::orchestration::prepare_attested_model;
use fabstir_llm_node::tee::policy::{
    canonical_policy_bytes, policy_signature_digest, SignedModelPolicy,
};
use fabstir_llm_node::tee::policy_source::{PolicySource, ProviderRegistry};
use fabstir_llm_node::tee::types::{CcMode,Policy, TeeError, TeeResult};
use k256::ecdsa::{RecoveryId, Signature, SigningKey};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

const SKU: &str = "H100";
const MEASUREMENT: [u8; 48] = [0x42u8; 48];
const BLOB_PATH: &str = "s5://models/proprietary.enc";

fn test_policy(model_id: [u8; 32]) -> Policy {
    Policy {
        policy_version: 1,
        allowed_skus: vec![SKU.to_string()],
        expected_measurement: MEASUREMENT,
        require_cc_mode: Some(CcMode::On),
        require_production_tcb: true,
        max_tcb_age_days: 30,
        not_before: 0,
        expiry: u64::MAX - 1, // valid now (avoid the u64::MAX clock-error sentinel)
        model_id,
    }
}

/// Sign `policy` as the provider; return the signed blob (carrying `encrypted_ref`)
/// + the provider's recovered `0x` address.
fn sign(policy: &Policy, encrypted_ref: &str, sk: &SigningKey) -> (SignedModelPolicy, String) {
    let canonical = canonical_policy_bytes(policy).unwrap();
    let digest = policy_signature_digest(&canonical);
    let (sig, recid): (Signature, RecoveryId) = sk.sign_prehash_recoverable(&digest).unwrap();
    let mut sig65 = vec![0u8; 65];
    sig65[..64].copy_from_slice(&sig.to_bytes());
    sig65[64] = recid.to_byte() + 27;
    let provider = recover_client_address(&sig65, &digest).unwrap();
    (
        SignedModelPolicy {
            policy: policy.clone(),
            encrypted_ref: encrypted_ref.to_string(),
            signer: provider.clone(),
            signature: sig65,
        },
        provider,
    )
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

/// Mock policy source: serves stored policies; an unknown model = fetch failure.
struct MockSource {
    policies: HashMap<[u8; 32], SignedModelPolicy>,
}
#[async_trait]
impl PolicySource for MockSource {
    async fn fetch_policy(&self, model_id: [u8; 32]) -> TeeResult<SignedModelPolicy> {
        self.policies.get(&model_id).cloned().ok_or_else(|| {
            TeeError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "policy fetch failed",
            ))
        })
    }
}

fn good_provider() -> MockAttestationProvider {
    MockAttestationProvider::new(SKU, MEASUREMENT, CcMode::On)
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Wire a complete fixture: signed policy + container + blob store + KBS + registry.
struct Fixture {
    loader: EncryptedModelLoader,
    _dir: tempfile::TempDir,
    source: MockSource,
    providers: ProviderRegistry,
    s5: InMemoryBlobs,
    kbs: MockKeyBroker,
    model_id: [u8; 32],
    plaintext: Vec<u8>,
}

fn fixture() -> Fixture {
    let model_id = [0xA1u8; 32];
    let dek = [0xDEu8; 32];
    let plaintext: Vec<u8> = (0..6000u32).map(|i| (i % 251) as u8).collect();

    let sk = SigningKey::random(&mut rand::rngs::OsRng);
    let policy = test_policy(model_id);
    let (signed, provider) = sign(&policy, BLOB_PATH, &sk);
    // The container is bound to the SAME policy_hash the orchestration derives.
    let policy_hash = signed.policy_hash().unwrap();
    let container = encrypt_model(&plaintext, &dek, model_id, policy_hash, 1024).unwrap();

    let dir = tempfile::tempdir().expect("tempdir");
    let loader = EncryptedModelLoader::new(dir.path()).with_tee_enabled(true);

    Fixture {
        loader,
        _dir: dir,
        source: MockSource {
            policies: HashMap::from([(model_id, signed)]),
        },
        providers: ProviderRegistry::new().with_provider(model_id, provider),
        s5: InMemoryBlobs {
            blobs: HashMap::from([(BLOB_PATH.to_string(), container)]),
        },
        kbs: MockKeyBroker::new(HashMap::from([(model_id, (dek, policy))])),
        model_id,
        plaintext,
    }
}

#[tokio::test]
async fn prepare_attested_model_happy_path_with_hash_binding() {
    let f = fixture();
    let expected = sha256_hex(&f.plaintext);

    let prepared = prepare_attested_model(
        &f.loader,
        &f.source,
        &f.providers,
        &f.s5,
        &f.kbs,
        &good_provider(),
        f.model_id,
        Some(&expected),
    )
    .await
    .expect("valid policy + good attestation + matching hash must load");

    assert_eq!(prepared.model_id, f.model_id);
    assert_eq!(prepared.policy.model_id, f.model_id);
    assert_eq!(
        std::fs::read(&prepared.path).unwrap(),
        f.plaintext,
        "the decrypted weights must equal the original plaintext"
    );
}

#[tokio::test]
async fn prepare_attested_model_none_hash_loads_with_warning() {
    let f = fixture();
    // No on-chain hash supplied (open-weight test model): still loads, but logs CRITICAL.
    let prepared = prepare_attested_model(
        &f.loader,
        &f.source,
        &f.providers,
        &f.s5,
        &f.kbs,
        &good_provider(),
        f.model_id,
        None,
    )
    .await
    .expect("None hash is an explicit opt-out, not a failure");
    assert_eq!(std::fs::read(&prepared.path).unwrap(), f.plaintext);
}

#[tokio::test]
async fn prepare_attested_model_fails_closed_on_hash_mismatch() {
    let f = fixture();
    let path_dir = f._dir.path().to_path_buf();
    let wrong = sha256_hex(b"a different model entirely");

    let err = prepare_attested_model(
        &f.loader,
        &f.source,
        &f.providers,
        &f.s5,
        &f.kbs,
        &good_provider(),
        f.model_id,
        Some(&wrong),
    )
    .await
    .expect_err("a decrypted file that doesn't match the on-chain hash must fail closed");

    assert!(
        matches!(err, TeeError::ModelHashMismatch { .. }),
        "got {err:?}"
    );
    let remaining = std::fs::read_dir(&path_dir)
        .map(|rd| rd.flatten().count())
        .unwrap_or(0);
    assert_eq!(
        remaining, 0,
        "the mismatched plaintext must be securely deleted (found {remaining} files)"
    );
}

#[tokio::test]
async fn prepare_attested_model_fails_closed_on_bad_attestation() {
    let f = fixture();
    let expected = sha256_hex(&f.plaintext);
    // Wrong measurement → verifier rejects → KBS withholds the DEK (no plaintext).
    let bad = MockAttestationProvider::new(SKU, [0u8; 48], CcMode::On);
    let err = prepare_attested_model(
        &f.loader,
        &f.source,
        &f.providers,
        &f.s5,
        &f.kbs,
        &bad,
        f.model_id,
        Some(&expected),
    )
    .await
    .expect_err("a failing attestation must fail closed");
    assert!(
        matches!(err, TeeError::VerificationFailed(_)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn prepare_attested_model_fails_closed_on_expired_policy() {
    let model_id = [0xB2u8; 32];
    let dek = [0xDEu8; 32];
    let plaintext = vec![0x7u8; 256];
    let sk = SigningKey::random(&mut rand::rngs::OsRng);
    let mut policy = test_policy(model_id);
    policy.expiry = 0; // revoked / expired
    let (signed, provider) = sign(&policy, BLOB_PATH, &sk);
    let policy_hash = signed.policy_hash().unwrap();
    let container = encrypt_model(&plaintext, &dek, model_id, policy_hash, 64).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let loader = EncryptedModelLoader::new(dir.path()).with_tee_enabled(true);
    let source = MockSource {
        policies: HashMap::from([(model_id, signed)]),
    };
    let providers = ProviderRegistry::new().with_provider(model_id, provider);
    let s5 = InMemoryBlobs {
        blobs: HashMap::from([(BLOB_PATH.to_string(), container)]),
    };
    let kbs = MockKeyBroker::new(HashMap::from([(model_id, (dek, policy))]));

    let err = prepare_attested_model(
        &loader,
        &source,
        &providers,
        &s5,
        &kbs,
        &good_provider(),
        model_id,
        Some(&sha256_hex(&plaintext)),
    )
    .await
    .expect_err("an expired policy must fail closed before any decrypt");
    assert!(
        matches!(err, TeeError::VerificationFailed(_)),
        "got {err:?}"
    );
    let empty = std::fs::read_dir(dir.path())
        .map(|rd| rd.flatten().count() == 0)
        .unwrap_or(true);
    assert!(empty, "no plaintext on a rejected policy");
}

#[tokio::test]
async fn prepare_attested_model_refuses_on_non_tee_node() {
    let model_id = [0xC3u8; 32];
    let dek = [0xDEu8; 32];
    let plaintext = vec![0x9u8; 128];
    let sk = SigningKey::random(&mut rand::rngs::OsRng);
    let policy = test_policy(model_id);
    let (signed, provider) = sign(&policy, BLOB_PATH, &sk);
    let policy_hash = signed.policy_hash().unwrap();
    let container = encrypt_model(&plaintext, &dek, model_id, policy_hash, 64).unwrap();

    let dir = tempfile::tempdir().unwrap();
    // Default loader is TEE-DISABLED → must refuse before any S5 fetch.
    let loader = EncryptedModelLoader::new(dir.path());
    let source = MockSource {
        policies: HashMap::from([(model_id, signed)]),
    };
    let providers = ProviderRegistry::new().with_provider(model_id, provider);
    let s5 = InMemoryBlobs {
        blobs: HashMap::from([(BLOB_PATH.to_string(), container)]),
    };
    let kbs = MockKeyBroker::new(HashMap::from([(model_id, (dek, policy))]));

    let err = prepare_attested_model(
        &loader,
        &source,
        &providers,
        &s5,
        &kbs,
        &good_provider(),
        model_id,
        Some(&sha256_hex(&plaintext)),
    )
    .await
    .expect_err("a non-TEE node must refuse an encrypted model");
    assert!(
        matches!(err, TeeError::NonTeeNodeRefusesEncrypted),
        "got {err:?}"
    );
}
