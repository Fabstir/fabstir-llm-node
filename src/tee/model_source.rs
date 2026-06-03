// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 2.4/2.5 — encrypted-model source orchestration.
//!
//! [`EncryptedModelLoader::prepare_encrypted_model`] fetches an encrypted
//! container from an S5 [`BlobSource`], runs the attestation → key-release flow
//! ([`NodeAttestationClient::obtain_dek`]: challenge → evidence → ECIES-wrapped
//! DEK → unwrap) against a [`KeyBrokerClient`], and streams the decrypted weights
//! to a private (`0600`) file in a tmpfs decrypt dir — **fail-closed**: on any
//! error nothing is left on disk. Decrypted files are cached by model identity and
//! refcounted; [`secure_delete`] zeroizes + unlinks them once no longer referenced.
//!
//! Phase 4.3.1a adds policy/version revalidation on every cache lookup; the real
//! `EnhancedS5Client` [`BlobSource`] impl is wired in Phase 4.3.

use crate::tee::container::decrypt_model;
use crate::tee::key_broker::{KeyBrokerClient, NodeAttestationClient};
use crate::tee::provider::AttestationProvider;
use crate::tee::types::{TeeError, TeeResult};
use async_trait::async_trait;
use rand::{rngs::OsRng, RngCore};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

/// Default tmpfs directory for decrypted weights (overridable via `TEE_DECRYPT_DIR`).
const DEFAULT_DECRYPT_DIR: &str = "/dev/shm";

/// Parse `HOST_TEE_ENABLED` (accepts `1`/`true`/`yes`/`on`; anything else / unset → `false`).
fn parse_host_tee_enabled() -> bool {
    std::env::var("HOST_TEE_ENABLED")
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// Whether this node honors TEE-attested encrypted models (`HOST_TEE_ENABLED`),
/// read **once** at first access and cached for the process (Phase 4.3.3a).
///
/// This is the single source of truth that ties the fail-closed model-load
/// enforcement (`prepare_encrypted_model`) to the advertised `tee-attested`
/// capability (Phase 4.2): a node can never advertise a capability it won't honor.
/// (Per-loader [`EncryptedModelLoader::from_env`] parses the same var freshly so
/// it stays independently testable.)
pub fn host_tee_enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(parse_host_tee_enabled)
}

/// Source of encrypted-model container bytes (an S5 blob store in production).
///
/// Phase-2 tests use an in-memory impl; the real `EnhancedS5Client` impl is wired
/// in Phase 4.3. Returns the full container (header + chunked AEAD) for `path`.
#[async_trait]
pub trait BlobSource: Send + Sync {
    async fn get_file(&self, path: &str) -> TeeResult<Vec<u8>>;
}

/// Identifies + binds one encrypted model for [`EncryptedModelLoader::prepare_encrypted_model`].
#[derive(Debug, Clone)]
pub struct EncryptedModelSpec {
    /// The model the container holds (checked against the decrypted header).
    pub model_id: [u8; 32],
    /// SHA-256 of the canonical signed policy (checked against the header).
    pub policy_hash: [u8; 32],
    /// S5 path of the encrypted container.
    pub encrypted_path: String,
}

/// Cache key for a decrypted model file: `(model_id, policy_hash)` (Phase 4.3.1a).
///
/// Keying on `policy_hash` (not `model_id` alone) means a **policy rotation** — a
/// new signed policy for the same model — is a cache *miss*, forcing a fresh
/// attested decrypt under the new policy instead of silently serving the file
/// decrypted under the old one. (Expiry/version revocation is enforced upstream:
/// the orchestration re-fetches + re-validates the *current* signed policy on every
/// load, so an expired/revoked policy fails closed before the cache is consulted —
/// which subsumes a per-entry TTL.)
type CacheKey = ([u8; 32], [u8; 32]);

struct CacheEntry {
    path: PathBuf,
    refcount: usize,
}

/// Decrypts attested encrypted models to a tmpfs dir, caching by `(model_id, policy_hash)`.
///
/// One instance per node (held in app state). Concurrent loads of the same model
/// share one decrypted file (refcounted); [`Self::release`] drops a reference and
/// [`Self::evict_unreferenced`] securely deletes files no longer in use (the node
/// also runs the latter periodically as a safety net).
pub struct EncryptedModelLoader {
    decrypt_dir: PathBuf,
    /// Whether this node may load TEE-attested encrypted models (`HOST_TEE_ENABLED`).
    /// **Fail-closed default `false`**: a non-TEE node refuses encrypted models.
    tee_enabled: bool,
    cache: RwLock<HashMap<CacheKey, CacheEntry>>,
}

impl EncryptedModelLoader {
    /// Build a loader writing decrypted weights under `decrypt_dir` (a tmpfs mount).
    /// TEE loading is **disabled** by default (fail-closed) — enable via
    /// [`Self::with_tee_enabled`] or [`Self::from_env`].
    pub fn new(decrypt_dir: impl Into<PathBuf>) -> Self {
        Self {
            decrypt_dir: decrypt_dir.into(),
            tee_enabled: false,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Enable/disable loading TEE-attested encrypted models on this node.
    pub fn with_tee_enabled(mut self, enabled: bool) -> Self {
        self.tee_enabled = enabled;
        self
    }

    /// Build a loader from `TEE_DECRYPT_DIR` (default `/dev/shm`) and `HOST_TEE_ENABLED`
    /// (default `false` — fail-closed; accepts `1`/`true`/`yes`/`on`).
    pub fn from_env() -> Self {
        let dir =
            std::env::var("TEE_DECRYPT_DIR").unwrap_or_else(|_| DEFAULT_DECRYPT_DIR.to_string());
        Self::new(dir).with_tee_enabled(parse_host_tee_enabled())
    }

    /// Verify the decrypt dir exists, is writable, and (best-effort) is tmpfs.
    ///
    /// Call once at CVM startup. Returns `Err` if the dir cannot be created or
    /// written; a non-tmpfs dir logs CRITICAL (decrypted weights could touch
    /// persistent disk) but does **not** hard-fail, so Phases 1–4 run on ordinary
    /// dev filesystems — the real tmpfs guarantee is a Phase-5 deploy requirement.
    pub fn verify_decrypt_dir(&self) -> TeeResult<()> {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.decrypt_dir)?;
        // Writability probe (created + removed; surfaces a read-only/over-quota mount).
        // Random name + create_new(0600) so a pre-planted symlink can't redirect the write.
        let mut rnd = [0u8; 8];
        OsRng.fill_bytes(&mut rnd);
        let probe = self
            .decrypt_dir
            .join(format!(".tee-write-probe.{}", hex::encode(rnd)));
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&probe)?;
        std::fs::remove_file(&probe)?;
        if !is_tmpfs(&self.decrypt_dir) {
            tracing::warn!(
                target: "tee",
                "CRITICAL: TEE_DECRYPT_DIR {} is not tmpfs — decrypted weights may touch persistent disk",
                self.decrypt_dir.display()
            );
        }
        Ok(())
    }

    /// Fetch → attest → obtain DEK → decrypt the model to a private tmpfs file.
    ///
    /// Fail-closed: on any error nothing is left on disk (a partially-written file
    /// is [`secure_delete`]d). Returns the decrypted file's path and takes a cache
    /// reference; the caller must [`Self::release`] it when done.
    pub async fn prepare_encrypted_model(
        &self,
        s5: &dyn BlobSource,
        kbs: &dyn KeyBrokerClient,
        provider: &dyn AttestationProvider,
        spec: &EncryptedModelSpec,
    ) -> TeeResult<PathBuf> {
        // 4.3.3 — fail-closed: a non-TEE node must never load an encrypted model.
        // Checked first: no cache, no S5 fetch, no plaintext on a non-TEE host.
        if !self.tee_enabled {
            tracing::warn!(
                target: "tee",
                "CRITICAL: refusing encrypted model {} on a non-TEE node (HOST_TEE_ENABLED=false)",
                hex::encode(spec.model_id)
            );
            return Err(TeeError::NonTeeNodeRefusesEncrypted);
        }

        // Cache keyed on (model_id, policy_hash) — a policy rotation re-decrypts (4.3.1a).
        let key = (spec.model_id, spec.policy_hash);
        if let Some(path) = self.cache_acquire(&key) {
            return Ok(path);
        }

        // 1. Fetch the encrypted container.
        let container = s5.get_file(&spec.encrypted_path).await?;

        // 2. Run the attestation → key-release flow: challenge → gather evidence
        //    (binding a fresh pk_att + nonce) → request the wrapped DEK → unwrap.
        //    The KBS withholds the key on a failed/stale attestation (fail-closed).
        let dek = NodeAttestationClient::obtain_dek(provider, kbs, spec.model_id).await?;

        // 3. Decrypt to a fresh private file; clean up on any failure (write nothing).
        // 0700 so the model_id-bearing filenames aren't even listable by other local
        // users (mode applies only to dirs we create; a pre-existing /dev/shm is left
        // as-is). Inside the CVM, CC-encrypted RAM is the real confidentiality boundary.
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.decrypt_dir)?;
        let path = self.fresh_path(&spec.model_id);
        if let Err(e) = decrypt_to_file(&container, &dek, spec, &path) {
            purge_or_warn(&path);
            return Err(e);
        }

        // 4. Publish to the cache (decrypt-twice-keep-one if a concurrent load won the race).
        Ok(self.cache_publish(key, path))
    }

    /// Drop one reference to the `(model_id, policy_hash)` entry; at zero the file is
    /// eligible for [`Self::evict_unreferenced`].
    pub fn release(&self, model_id: &[u8; 32], policy_hash: &[u8; 32]) {
        let mut cache = self.cache.write().expect("tee cache poisoned");
        if let Some(entry) = cache.get_mut(&(*model_id, *policy_hash)) {
            entry.refcount = entry.refcount.saturating_sub(1);
        }
    }

    /// Securely delete + evict every cached model with refcount == 0.
    pub fn evict_unreferenced(&self) {
        let mut cache = self.cache.write().expect("tee cache poisoned");
        let dead: Vec<CacheKey> = cache
            .iter()
            .filter(|(_, e)| e.refcount == 0)
            .map(|(k, _)| *k)
            .collect();
        for k in dead {
            if let Some(entry) = cache.remove(&k) {
                purge_or_warn(&entry.path);
            }
        }
    }

    /// Cache fast-path: if present, take a reference and return the path.
    fn cache_acquire(&self, key: &CacheKey) -> Option<PathBuf> {
        let mut cache = self.cache.write().expect("tee cache poisoned");
        cache.get_mut(key).map(|e| {
            e.refcount += 1;
            e.path.clone()
        })
    }

    /// Insert a freshly decrypted file, or — if a concurrent load already
    /// published one — reference theirs and securely delete the redundant copy.
    fn cache_publish(&self, key: CacheKey, path: PathBuf) -> PathBuf {
        let mut cache = self.cache.write().expect("tee cache poisoned");
        if let Some(entry) = cache.get_mut(&key) {
            entry.refcount += 1;
            let winner = entry.path.clone();
            drop(cache);
            purge_or_warn(&path); // redundant copy from a concurrent decrypt
            return winner;
        }
        cache.insert(
            key,
            CacheEntry {
                path: path.clone(),
                refcount: 1,
            },
        );
        path
    }

    /// A unique private path in the decrypt dir for one decryption attempt.
    fn fresh_path(&self, model_id: &[u8; 32]) -> PathBuf {
        let mut suffix = [0u8; 8];
        OsRng.fill_bytes(&mut suffix);
        self.decrypt_dir.join(format!(
            "{}.{}.gguf",
            hex::encode(model_id),
            hex::encode(suffix)
        ))
    }
}

/// Decrypt `container` to a new `0600` file at `path` (created exclusively).
fn decrypt_to_file(
    container: &[u8],
    dek: &[u8; 32],
    spec: &EncryptedModelSpec,
    path: &Path,
) -> TeeResult<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    decrypt_model(container, dek, &spec.model_id, &spec.policy_hash, &mut file)?;
    file.sync_all()?;
    Ok(())
}

/// Best-effort: is `path` on a `tmpfs` (RAM-backed) mount?
///
/// Resolves the longest mount-point prefix of `path` in `/proc/mounts` and checks
/// its filesystem type. Returns `false` if the path can't be canonicalized or the
/// type can't be determined — fail-safe for the startup warning, never panics.
pub fn is_tmpfs(path: &Path) -> bool {
    filesystem_type(path).as_deref() == Some("tmpfs")
}

/// Filesystem type of the mount containing `path`, from `/proc/mounts` (Linux).
/// `None` if `path` can't be canonicalized or `/proc/mounts` is unavailable.
///
/// Best-effort: mount points containing octal-escaped whitespace (`\040` etc.)
/// are not decoded, so an exotic mount path with spaces may be skipped — it only
/// affects the startup tmpfs *warning*, never a security decision, and the
/// dominant failure direction is to over-warn (classify tmpfs as non-tmpfs).
fn filesystem_type(path: &Path) -> Option<String> {
    let canon = std::fs::canonicalize(path).ok()?;
    let mounts = std::fs::read_to_string("/proc/mounts").ok()?;
    // Pick the deepest mount point that is a prefix of the canonical path.
    let mut best: Option<(usize, String)> = None;
    for line in mounts.lines() {
        let mut cols = line.split_whitespace();
        let (mount_point, fstype) = match (cols.next(), cols.next(), cols.next()) {
            (Some(_dev), Some(m), Some(f)) => (m, f),
            _ => continue,
        };
        let mp = Path::new(mount_point);
        if canon.starts_with(mp) {
            let depth = mp.components().count();
            if best.as_ref().is_none_or(|(d, _)| depth > *d) {
                best = Some((depth, fstype.to_string()));
            }
        }
    }
    best.map(|(_, t)| t)
}

/// Overwrite `path` with zeros once, then unlink it.
///
/// A single zeroize pass suffices for RAM-backed tmpfs (the decrypt dir) — the
/// pages are also TEE-encrypted, so this is defense-in-depth; multi-pass overwrite
/// is a magnetic-disk technique, pointless on RAM. Idempotent: a missing path is `Ok`.
pub fn secure_delete(path: &Path) -> TeeResult<()> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(TeeError::Io(e)),
    };
    let mut file = OpenOptions::new().write(true).open(path)?;
    let zeros = [0u8; 64 * 1024];
    let mut remaining = meta.len();
    while remaining > 0 {
        let n = (remaining as usize).min(zeros.len());
        file.write_all(&zeros[..n])?;
        remaining -= n as u64;
    }
    file.sync_all()?;
    drop(file);
    std::fs::remove_file(path)?;
    Ok(())
}

/// [`secure_delete`] `path`, logging CRITICAL on failure instead of returning it.
///
/// Used on cleanup paths where the caller is already returning the underlying
/// error: a `secure_delete` failure may leave plaintext on disk, so it must never
/// be swallowed silently — fail-closed *alerting* even when we can't fail the call.
fn purge_or_warn(path: &Path) {
    if let Err(e) = secure_delete(path) {
        tracing::warn!(
            target: "tee",
            "CRITICAL: failed to secure-delete decrypted plaintext {} — it may persist on disk: {e}",
            path.display()
        );
    }
}
