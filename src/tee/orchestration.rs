// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 4.3.1 / 4.3.2 — the live-request model-load orchestration.
//!
//! [`prepare_attested_model`] is the single fail-closed entry the inference load
//! path calls when a model carries a TEE policy. It composes the already-tested
//! Phase-1–4 pieces into the end-to-end "client asks for model X → attest →
//! decrypt → bind" flow:
//!
//!   1. **Policy** (4.3.1): fetch the model's [`SignedModelPolicy`], confirm it
//!      was signed by the model's bound provider (Q2), and confirm its validity
//!      window — via [`fetch_validated_policy`]. Any failure → `Err`, nothing
//!      decrypted.
//!   2. **Attested decrypt**: fetch the ciphertext from the policy's
//!      `encrypted_ref` and decrypt it to a private tmpfs file *only* under a
//!      passing attestation — via [`EncryptedModelLoader::prepare_encrypted_model`].
//!      `encrypted_ref` lives outside `Policy`, so pointer substitution is caught
//!      not by a field-compare but downstream by the container header's
//!      `model_id`/`policy_hash` binding inside `decrypt_model` (fail-closed) plus
//!      the model-scoped DEK from the KBS.
//!   3. **On-chain binding** (4.3.2): bind the decrypted weights to the
//!      on-chain-approved model by SHA-256 (`expected_model_hash`, the hex of
//!      `ModelInfo.sha256_hash`). **verify-then-load**: a host can swap the tmpfs
//!      file between this check and llama.cpp opening it (TOCTOU) — Phase 4 logs
//!      the window (here + in `load_model`); Phase 5 closes it. On mismatch we
//!      drop our cache reference and securely delete the plaintext.
//!
//! On any failure after a successful decrypt, the cache reference is released and
//! the decrypted plaintext is securely deleted before returning `Err` — no
//! weights are ever handed to the engine on a failed gate.

use crate::tee::key_broker::KeyBrokerClient;
use crate::tee::model_source::{BlobSource, EncryptedModelLoader, EncryptedModelSpec};
use crate::tee::policy_source::{fetch_validated_policy, PolicySource, ProviderRegistry};
use crate::tee::provider::AttestationProvider;
use crate::tee::types::{Policy, TeeError, TeeResult};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

/// A decrypted, attested, hash-verified model ready to hand to the engine.
///
/// The caller builds a `ModelConfig { encrypted: true, model_path: path, .. }`
/// from this and calls `LlmEngine::load_model`; on unload it must call
/// [`EncryptedModelLoader::release`] with `model_id` so the tmpfs plaintext can
/// be evicted and securely deleted once unreferenced.
#[derive(Debug)]
pub struct PreparedModel {
    /// Path to the decrypted weights on tmpfs (private, `0600`).
    pub path: PathBuf,
    /// The model identity. With [`Self::policy_hash`], the cache key for
    /// [`EncryptedModelLoader::release`].
    pub model_id: [u8; 32],
    /// SHA-256 of the canonical policy that governed this load — the second half of
    /// the cache key; the caller passes both to `release` on unload.
    pub policy_hash: [u8; 32],
    /// The validated policy that governed this load.
    pub policy: Policy,
}

/// Fetch-validate-decrypt-bind a TEE-protected model, fail-closed at every step.
///
/// `expected_model_hash` is the hex of the model's on-chain `ModelInfo.sha256_hash`
/// (4.3.2). `Some(_)` binds the decrypted weights to the approved model and fails
/// closed on mismatch. `None` is an explicit opt-out for models with no on-chain
/// hash (e.g. open-weight test models) and logs a CRITICAL warning — the binding
/// is never skipped silently.
#[allow(clippy::too_many_arguments)]
pub async fn prepare_attested_model(
    loader: &EncryptedModelLoader,
    policy_src: &dyn PolicySource,
    providers: &ProviderRegistry,
    s5: &dyn BlobSource,
    kbs: &dyn KeyBrokerClient,
    attestation: &dyn AttestationProvider,
    model_id: [u8; 32],
    expected_model_hash: Option<&str>,
) -> TeeResult<PreparedModel> {
    // 1. Policy: fetch → signer == bound provider → validity window (fail-closed).
    let signed = fetch_validated_policy(policy_src, providers, model_id).await?;
    let policy_hash = signed.policy_hash()?;

    // 2. Decrypt the provider's ciphertext to a private tmpfs file under a passing
    //    attestation. The header's model_id/policy_hash binding + the model-scoped
    //    DEK defeat pointer substitution and cross-model replay (fail-closed).
    let spec = EncryptedModelSpec {
        model_id,
        policy_hash,
        encrypted_path: signed.encrypted_ref.clone(),
    };
    let path = loader
        .prepare_encrypted_model(s5, kbs, attestation, &spec)
        .await?;
    // From here until the caller owns the result, the plaintext is guarded: an
    // error OR a cancellation (a caller dropping this future at the hash step's
    // awaits) purges it. Disarmed only on success. In the node binary a stop
    // does not cancel this future (the watchdog unlinks from its own thread and
    // exits); the guard is what makes the function safe for any caller.
    let mut guard = PlaintextGuard {
        loader,
        model_id,
        policy_hash,
        armed: true,
    };

    // 3. (4.3.2) Bind decrypted weights to the on-chain-approved model by SHA-256.
    match expected_model_hash {
        Some(expected) => match sha256_file_hex(&path).await {
            Ok(got) if got.eq_ignore_ascii_case(expected) => {
                tracing::warn!(
                    target: "tee",
                    "verify-then-load: model {} hash bound to on-chain approval; \
                     TOCTOU window open until llama.cpp opens the file (Phase-4 risk, closed in Phase 5)",
                    hex::encode(model_id)
                );
            }
            Ok(got) => {
                return Err(TeeError::ModelHashMismatch {
                    expected: expected.to_string(),
                    got,
                })
            }
            Err(e) => return Err(e),
        },
        None => tracing::warn!(
            target: "tee",
            "CRITICAL: model {} loaded WITHOUT an on-chain hash binding (4.3.2 skipped — no expected hash supplied)",
            hex::encode(model_id)
        ),
    }

    guard.armed = false;
    Ok(PreparedModel {
        path,
        model_id,
        policy_hash,
        policy: signed.policy,
    })
}

/// Holds the loader's cache reference for a freshly decrypted plaintext while
/// `prepare_attested_model` is still working on it. While armed, dropping it
/// (an `Err` return, or a caller cancelling the future mid-await) drops the
/// reference and securely deletes the file; `release` + `evict_unreferenced`
/// only deletes when no *other* in-flight load still references it, so this is
/// safe under a concurrent load of the same model. Nothing has mapped the file
/// at this point, so the in-place overwrite is safe here.
struct PlaintextGuard<'a> {
    loader: &'a EncryptedModelLoader,
    model_id: [u8; 32],
    policy_hash: [u8; 32],
    armed: bool,
}

impl Drop for PlaintextGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.loader.release(&self.model_id, &self.policy_hash);
            self.loader.evict_unreferenced();
        }
    }
}

/// Stream-hash a file to lowercase-hex SHA-256.
///
/// Mirrors `ModelRegistryClient::verify_model_hash` (model_registry.rs) but lives
/// here so the orchestration stays decoupled from the contracts client and fully
/// mock-testable; the on-chain `sha256_hash` is fed in by the caller.
async fn sha256_file_hex(path: &Path) -> TeeResult<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
