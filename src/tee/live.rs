// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 (P3.3) — the live call site: how the node binary decides, at boot,
//! whether it serves an attested encrypted model, and assembles everything
//! `prepare_attested_model` needs from the container environment.
//!
//! The contract, in one place (the composes and the README follow it):
//!
//! | `HOST_TEE_ENABLED` | `TEE_MODEL_ID` | result |
//! |---|---|---|
//! | unset / false | unset | plain node, `MODEL_PATH` as before |
//! | unset / false | set | **refused**: a TEE model configured on a node that will not honour it |
//! | true | unset | **refused**: the node would advertise `tee-attested` with nothing attested |
//! | true | set | attested load; `MODEL_PATH` and `DISABLE_LLM` must be absent |
//!
//! "Refused" means the process exits non-zero before the API starts. A node
//! that advertises `tee-attested` (`protocol.rs`, `registration.rs`, both via
//! `tee::advertises_tee_attested`) therefore always has an attested model
//! behind it, or does not run; and a test-keyring release (CPU gate rounds,
//! `TEE_ACCEPT_TEST_RELEASE=1`) runs WITHOUT the advert. This is the guarantee
//! the composes' `HOST_TEE_ENABLED: "true"` flip (gate A-19) relies on.
//!
//! With the flag on, the required variables are `TEE_MODEL_ID` (hex, 32 bytes),
//! `TEE_MODEL_PROVIDER` (the `0x` address whose signature the policy must
//! carry), `TEE_KBS_URL`, `TEE_POLICY_URL`, `TEE_BLOB_URL`, plus the image's
//! `TEE_KBS_CA_FILE` / `TEE_GPU_EVIDENCE_SCRIPT`; optional
//! `TEE_EXPECTED_MODEL_SHA256` (the on-chain hash to bind the plaintext to;
//! absent = the existing CRITICAL warning, never silent) and `TEE_DECRYPT_DIR`.
//!
//! `REQUIRE_MODEL_VALIDATION=true` on the attested path means the plaintext
//! MUST be bound to `TEE_EXPECTED_MODEL_SHA256` (the operator copies the hash
//! from the ModelRegistry entry); with the variable absent the configuration is
//! refused rather than the request silently becoming a log line. The plain
//! path's filename-keyed registry validator does not apply to a tmpfs decrypt.

use crate::tee::dstack_provider::DstackAttestationProvider;
use crate::tee::http_sources::{HttpBlobSource, HttpPolicySource};
use crate::tee::kbs_http::HttpKeyBrokerClient;
use crate::tee::model_source::{host_tee_enabled, EncryptedModelLoader};
use crate::tee::orchestration::{prepare_attested_model, PreparedModel};
use crate::tee::policy_source::ProviderRegistry;
use crate::tee::types::{TeeError, TeeResult};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

pub const MODEL_ID_ENV: &str = "TEE_MODEL_ID";
pub const PROVIDER_ENV: &str = "TEE_MODEL_PROVIDER";
pub const EXPECTED_SHA256_ENV: &str = "TEE_EXPECTED_MODEL_SHA256";
pub const REQUIRE_VALIDATION_ENV: &str = "REQUIRE_MODEL_VALIDATION";

/// What the boot-time decision resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveConfig {
    pub model_id: [u8; 32],
    pub provider: String,
    pub expected_sha256: Option<String>,
}

impl LiveConfig {
    /// The decision table above, over an explicit environment map so it is
    /// testable without touching the process environment. `Ok(None)` is the plain
    /// node; `Ok(Some(_))` is the attested path; `Err` is a refused configuration.
    pub fn resolve(env: &HashMap<String, String>, tee_enabled: bool) -> TeeResult<Option<Self>> {
        let get = |k: &str| {
            env.get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let model_id = get(MODEL_ID_ENV);
        match (tee_enabled, model_id) {
            (false, None) => Ok(None),
            (false, Some(_)) => Err(TeeError::VerificationFailed(format!(
                "{MODEL_ID_ENV} is set but HOST_TEE_ENABLED is not true: this node would load \
                 a TEE model without honouring the TEE contract; refusing to start"
            ))),
            (true, None) => Err(TeeError::VerificationFailed(format!(
                "HOST_TEE_ENABLED=true requires {MODEL_ID_ENV}: a node must never advertise \
                 tee-attested with nothing attested behind it; refusing to start"
            ))),
            (true, Some(id_hex)) => {
                if get("DISABLE_LLM").is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")) {
                    return Err(TeeError::VerificationFailed(
                        "DISABLE_LLM with HOST_TEE_ENABLED=true would advertise tee-attested with no model; refusing".into(),
                    ));
                }
                if get("MODEL_PATH").is_some() {
                    return Err(TeeError::VerificationFailed(
                        "MODEL_PATH must not be set on the attested path (the model comes from \
                         the attested decrypt, never a plain file); refusing"
                            .into(),
                    ));
                }
                let raw = hex::decode(id_hex.trim_start_matches("0x")).map_err(|e| {
                    TeeError::VerificationFailed(format!("{MODEL_ID_ENV} is not hex: {e}"))
                })?;
                let model_id: [u8; 32] = raw.try_into().map_err(|v: Vec<u8>| {
                    TeeError::VerificationFailed(format!(
                        "{MODEL_ID_ENV} must be 32 bytes, got {}",
                        v.len()
                    ))
                })?;
                let provider = get(PROVIDER_ENV).ok_or_else(|| {
                    TeeError::VerificationFailed(format!(
                        "{PROVIDER_ENV} (the policy signer's 0x address) is required on the attested path"
                    ))
                })?;
                if !(provider.starts_with("0x")
                    && provider.len() == 42
                    && hex::decode(&provider[2..]).is_ok())
                {
                    return Err(TeeError::VerificationFailed(format!(
                        "{PROVIDER_ENV} must be a 0x-prefixed 20-byte address, got {provider}"
                    )));
                }
                let expected_sha256 = match get(EXPECTED_SHA256_ENV) {
                    None => None,
                    Some(h) => {
                        let h = h.trim_start_matches("0x").to_ascii_lowercase();
                        if h.len() != 64 || hex::decode(&h).is_err() {
                            return Err(TeeError::VerificationFailed(format!(
                                "{EXPECTED_SHA256_ENV} must be 64 hex chars"
                            )));
                        }
                        Some(h)
                    }
                };
                let validation_required = get(REQUIRE_VALIDATION_ENV)
                    .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
                if validation_required && expected_sha256.is_none() {
                    return Err(TeeError::VerificationFailed(format!(
                        "{REQUIRE_VALIDATION_ENV}=true on the attested path requires {EXPECTED_SHA256_ENV} \
                         (the ModelRegistry sha256 the decrypted plaintext is bound to); \
                         refusing rather than loading unvalidated"
                    )));
                }
                Ok(Some(Self {
                    model_id,
                    provider,
                    expected_sha256,
                }))
            }
        }
    }
}

/// An attested, decrypted, hash-bound model the engine may load, plus the
/// loader reference that keeps its tmpfs file alive.
pub struct AttestedLoad {
    pub path: PathBuf,
    pub prepared: PreparedModel,
    pub loader: Arc<EncryptedModelLoader>,
    /// The broker labelled the release as a test-keyring release.
    pub test_release: bool,
}

impl AttestedLoad {
    /// Resolve the configuration from the process environment and, on the
    /// attested path, run the whole flow. Every failure is an `Err`; the caller
    /// exits the process on it (fail-closed: no API, no advert).
    pub async fn from_env() -> TeeResult<Option<Self>> {
        Self::from_env_with_loader_hook(|_| {}).await
    }

    /// As [`from_env`](Self::from_env); `on_loader` receives the loader as soon
    /// as it exists, before anything is fetched or decrypted, so the caller's
    /// emergency exit can reach every plaintext the load creates
    /// (`EncryptedModelLoader::unlink_live_plaintexts`) even while this future
    /// is still running.
    pub async fn from_env_with_loader_hook(
        on_loader: impl FnOnce(&Arc<EncryptedModelLoader>),
    ) -> TeeResult<Option<Self>> {
        // `vars()` panics on a non-UTF-8 value anywhere in the environment (this
        // runs on every node, plain path included); read lossily instead.
        let env: HashMap<String, String> = std::env::vars_os()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.to_string_lossy().into_owned(),
                )
            })
            .collect();
        let Some(cfg) = LiveConfig::resolve(&env, host_tee_enabled())? else {
            return Ok(None);
        };
        tracing::info!(
            target: "tee",
            "attested load path: model {} provider {}",
            hex::encode(cfg.model_id),
            cfg.provider
        );
        let loader = Arc::new(EncryptedModelLoader::from_env());
        on_loader(&loader);
        // tmpfs is a requirement here, not a warning: shutdown unlinks the
        // plaintext without overwriting it (see `detach_for_exit`).
        loader.require_tmpfs_decrypt_dir()?;
        let policy_src = HttpPolicySource::from_env()?;
        let providers = ProviderRegistry::new().with_provider(cfg.model_id, cfg.provider.clone());
        let blob = HttpBlobSource::from_env()?;
        let kbs = HttpKeyBrokerClient::from_env()?;
        let attestation = DstackAttestationProvider::from_env()?;
        let prepared = prepare_attested_model(
            &loader,
            &policy_src,
            &providers,
            &blob,
            &kbs,
            &attestation,
            cfg.model_id,
            cfg.expected_sha256.as_deref(),
        )
        .await?;
        // The proof witness's `model_hash` becomes this id (see
        // `checkpoint_manager::witness_model_hash`).
        crate::tee::model_source::mark_attested_model_id(cfg.model_id);
        let test_release = kbs.last_release_was_test();
        if test_release {
            // Recorded process-wide: the registration metadata and the WS
            // handshake drop `tee-attested` for a test-keyring release.
            crate::tee::model_source::mark_test_release_loaded();
            tracing::warn!(
                target: "tee",
                "CRITICAL: this model was released by a TEST-keyring broker (KBS_GPU_EVIDENCE=canned); \
                 it is a gate run, not a production release; tee-attested will NOT be advertised"
            );
        }
        Ok(Some(Self {
            path: prepared.path.clone(),
            prepared,
            loader,
            test_release,
        }))
    }

    /// Drop the cache reference and securely delete the plaintext (shutdown).
    pub fn release(&self) {
        self.loader
            .release(&self.prepared.model_id, &self.prepared.policy_hash);
        self.loader.evict_unreferenced();
    }

    /// Shutdown variant for a node that has been SERVING: unlink the tmpfs file
    /// without overwriting it, then the caller exits the process.
    ///
    /// Why not [`release`](Self::release) here: WebSocket sessions are upgraded
    /// connections, invisible to the HTTP server's graceful drain, so a
    /// generation can still be decoding when shutdown runs. Zeroing the file in
    /// place under llama.cpp's mmap would feed that generation garbage weights,
    /// and its tokens would still be streamed and tracked. Unlinking keeps the
    /// existing mapping coherent (real tokens until the process ends), removes
    /// the path so nothing new can open it, and the pages are freed on exit;
    /// tmpfs has no medium to scrub, and the live path REQUIRES tmpfs
    /// (`require_tmpfs_decrypt_dir`) for exactly that reason. `release()` stays
    /// for the boot-failure exits, where nothing has mapped the file and zeroing
    /// is safe. Idempotent with `release()`/Drop (a missing file is not an
    /// error there).
    pub fn detach_for_exit(&self) -> std::io::Result<()> {
        let r = match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            r => r,
        };
        if r.is_ok() {
            self.loader.forget_plaintext(&self.path);
        }
        r
    }
}

/// Every exit from `main` after the decrypt — including the `?` returns on a
/// failed P2P or API start, or a failed `ctrl_c()` after serving began — drops
/// this value, so the tmpfs plaintext never outlives the process's intent to
/// serve it. It UNLINKS first (`detach_for_exit`: a generation may still be
/// decoding on a blocking thread the runtime waits for, and zeroing under its
/// mmap would stream garbage) and then runs `release()` for the cache
/// bookkeeping, which finds no file to overwrite. The explicit pre-load
/// `release()` sites keep the zeroing, and the `process::exit` sites call it
/// themselves because `exit` runs no destructors.
impl Drop for AttestedLoad {
    fn drop(&mut self) {
        match self.detach_for_exit() {
            Ok(()) => self.release(),
            // Never fall through to `release()` here: it would overwrite the
            // file in place, the one thing that must not happen under a mapping
            // that may still be decoding. The file stays; the process is ending
            // and the pages go with the mount.
            Err(e) => tracing::warn!(
                target: "tee",
                "CRITICAL: could not unlink the attested plaintext {}: {e}; left in place, NOT overwritten",
                self.path.display()
            ),
        }
    }
}
