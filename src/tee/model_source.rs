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
//!
//! Phase 5 P5.5 (`DESIGN-PHASE5-STREAMING-LOAD.md`): the container is streamed
//! to a ciphertext cache file on the CVM's disk (`container_cache`) and
//! stream-decrypted from there with a SHA-256 tee, so peak RAM is the plaintext
//! plus two chunks instead of two copies of the model; and the plaintext has a
//! second home, the dstack-encrypted data disk (`plaintext_home`), chosen by
//! `TEE_DECRYPT_ON_DISK`, for a model larger than the CVM's RAM.

use crate::tee::container::decrypt_model_to_writer;
use crate::tee::container_cache::{
    cache_key, cache_path, check_space_for_download, check_space_for_plaintext, header_check,
    is_part_name, open_cache, prune_other_keys, real_space, refuse_mismatched_head, write_via_part,
    ContainerOutcome, FetchHooks, HeaderCheck, Sha256Tee, SpaceProbe,
};
use crate::tee::key_broker::{KeyBrokerClient, NodeAttestationClient};
use crate::tee::plaintext_home::{home_rule, is_own_plaintext_name, mount_info, sweep_dir};
use crate::tee::provider::AttestationProvider;
use crate::tee::types::{TeeError, TeeResult};
use async_trait::async_trait;
use rand::{rngs::OsRng, RngCore};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufReader, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};

/// Default tmpfs directory for decrypted weights (overridable via `TEE_DECRYPT_DIR`).
const DEFAULT_DECRYPT_DIR: &str = "/dev/shm";
/// Default ciphertext cache directory (overridable via `TEE_CONTAINER_DIR`).
const DEFAULT_CONTAINER_DIR: &str = "/var/lib/fabstir/containers";
/// Env: `1`/`true` selects the disk home (`TEE_PLAINTEXT_VOLUME`) for the plaintext.
pub const DECRYPT_ON_DISK_ENV: &str = "TEE_DECRYPT_ON_DISK";
/// Env: the named volume's mount path on the dstack-encrypted data disk (a
/// compose literal); the disk-mode home, swept in every mode.
pub const PLAINTEXT_VOLUME_ENV: &str = "TEE_PLAINTEXT_VOLUME";
/// Env: the ciphertext cache directory (a compose literal, its own volume).
pub const CONTAINER_DIR_ENV: &str = "TEE_CONTAINER_DIR";

fn parse_flag(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

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

/// Set once by the attested load when the broker released the DEK under its
/// TEST keyring (`test_release: true`, canned GPU evidence; CPU gate rounds).
static TEST_RELEASE_LOADED: AtomicBool = AtomicBool::new(false);

/// Record that the model behind this process came from a test-keyring release.
pub fn mark_test_release_loaded() {
    TEST_RELEASE_LOADED.store(true, Ordering::SeqCst);
}

/// Whether the model behind this process came from a test-keyring release.
pub fn test_release_loaded() -> bool {
    TEST_RELEASE_LOADED.load(Ordering::SeqCst)
}

/// The on-chain model id behind this process's attested load, once set.
static ATTESTED_MODEL_ID: OnceLock<[u8; 32]> = OnceLock::new();

/// Record the attested model's id (the `TEE_MODEL_ID` the policy and container
/// were bound to). First call wins; one attested model per process.
pub fn mark_attested_model_id(id: [u8; 32]) {
    let _ = ATTESTED_MODEL_ID.set(id);
}

/// The attested model's on-chain id, if this process serves one. The proof
/// witness uses it as `model_hash` (the plain path has only `MODEL_PATH`).
pub fn attested_model_id() -> Option<[u8; 32]> {
    ATTESTED_MODEL_ID.get().copied()
}

/// The advertisement rule, pure for testing: `tee-attested` is claimed only
/// when the flag is on AND the model behind it was not a test-keyring release.
/// A CPU gate node (canned GPU evidence) therefore registers and handshakes as
/// a plain node; it never tells a client or the NodeRegistry that its weights
/// were released against real GPU evidence.
pub fn advertise_tee_attested(tee_enabled: bool, test_release: bool) -> bool {
    tee_enabled && !test_release
}

/// What this process advertises as `tee-attested` (registration metadata and
/// the WS handshake): [`advertise_tee_attested`] over the live values.
pub fn advertises_tee_attested() -> bool {
    advertise_tee_attested(host_tee_enabled(), test_release_loaded())
}

/// Source of encrypted-model container bytes (an S5 blob store in production).
///
/// Phase-2 tests use an in-memory impl; the real `EnhancedS5Client` impl is wired
/// in Phase 4.3. Returns the full container (header + chunked AEAD) for `path`.
#[async_trait]
pub trait BlobSource: Send + Sync {
    async fn get_file(&self, path: &str) -> TeeResult<Vec<u8>>;

    /// Phase 5 P5.5 (design S1): fetch `path` INTO `dest` (bytes written),
    /// calling `hooks.on_length(len)` once the body length is known and
    /// `hooks.on_head(first 98 bytes)` as soon as they arrive, both before any
    /// byte is written to disk, so the loader can refuse for space or for a
    /// wrong binding in seconds rather than after the download. The default
    /// body buffers through [`Self::get_file`] (in-memory sources and every
    /// existing test); the HTTP source overrides it with a true stream. Both
    /// write `dest` through a `.part` and an atomic rename, so a concurrent
    /// reader never sees a truncated file.
    async fn get_file_to(&self, path: &str, dest: &Path, hooks: FetchHooks<'_>) -> TeeResult<u64> {
        let bytes = self.get_file(path).await?;
        write_via_part(dest, &bytes, hooks)
    }
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
    /// SHA-256 of the plaintext, from the decrypt's tee: a cache hit hands
    /// back the digest of the path it returns (design S2).
    digest: [u8; 32],
    refcount: usize,
}

/// Decrypts attested encrypted models to a tmpfs dir, caching by `(model_id, policy_hash)`.
///
/// One instance per node (held in app state). Concurrent loads of the same model
/// share one decrypted file (refcounted); [`Self::release`] drops a reference and
/// [`Self::evict_unreferenced`] securely deletes files no longer in use (the node
/// also runs the latter periodically as a safety net).
pub struct EncryptedModelLoader {
    /// The tmpfs home (`TEE_DECRYPT_DIR`). The home in force is [`Self::home`]:
    /// this, or the plaintext volume when disk mode is effective.
    tmpfs_dir: PathBuf,
    /// Whether this node may load TEE-attested encrypted models (`HOST_TEE_ENABLED`).
    /// **Fail-closed default `false`**: a non-TEE node refuses encrypted models.
    tee_enabled: bool,
    /// The ciphertext cache (design S1): one `<key>.enc` per process.
    container_dir: PathBuf,
    /// The disk-mode home (`TEE_PLAINTEXT_VOLUME`), swept in every mode (S3a-3).
    plaintext_volume: Option<PathBuf>,
    /// `TEE_DECRYPT_ON_DISK`: the plaintext lives on the LUKS volume, purges
    /// are unlink-only (design S3, S3a). Effective only with a volume set
    /// ([`Self::disk_mode`]); `require_decrypt_dir` refuses the flag without one.
    disk_mode: bool,
    /// How free space is measured (design S1c); injectable for tests.
    space_probe: Box<SpaceProbe<'static>>,
    /// The container step's outcome of the LAST `prepare_encrypted_model` call;
    /// `None` until the container step runs (an in-process plaintext hit).
    last_outcome: Mutex<Option<ContainerOutcome>>,
    /// Cache keys of the loads in flight, refcounted (two overlapping loads of
    /// one key: the first to finish must not expose the second to the prune).
    in_flight: Mutex<std::collections::BTreeMap<String, usize>>,
    cache: RwLock<HashMap<CacheKey, CacheEntry>>,
    /// Every plaintext path this loader has created and not yet deleted,
    /// registered BEFORE the decrypt starts. The emergency exit
    /// ([`Self::unlink_live_plaintexts`]) walks it, so a file that exists while
    /// no `AttestedLoad` or cache entry names it yet (the decrypt itself, the
    /// hash step) is still reachable.
    live_plaintexts: std::sync::Mutex<LivePlaintexts>,
}

/// Removes its key from the loader's in-flight set on drop (every exit of
/// the container step, including a dropped future).
struct InFlightKey<'a> {
    loader: &'a EncryptedModelLoader,
    key: String,
}

impl Drop for InFlightKey<'_> {
    fn drop(&mut self) {
        let mut set = self
            .loader
            .in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(n) = set.get_mut(&self.key) {
            *n -= 1;
            if *n == 0 {
                set.remove(&self.key);
            }
        }
    }
}

/// The loader's live-plaintext bookkeeping, under one lock: the paths on disk
/// and whether an emergency exit has begun. A new plaintext file is created
/// ONLY under this lock and only while `stopping` is false, so nothing can
/// appear after [`EncryptedModelLoader::unlink_live_plaintexts`] has run.
#[derive(Default)]
struct LivePlaintexts {
    stopping: bool,
    paths: std::collections::BTreeSet<PathBuf>,
}

impl EncryptedModelLoader {
    /// Build a loader writing decrypted weights under `decrypt_dir` (a tmpfs mount).
    /// TEE loading is **disabled** by default (fail-closed) — enable via
    /// [`Self::with_tee_enabled`] or [`Self::from_env`].
    ///
    /// The container dir defaults to the sibling `<decrypt_dir>.containers`
    /// (disjoint by construction, created on demand); production sets it from
    /// `TEE_CONTAINER_DIR` through [`Self::from_env`].
    pub fn new(decrypt_dir: impl Into<PathBuf>) -> Self {
        let decrypt_dir: PathBuf = decrypt_dir.into();
        // By components, not by string: `/dev/shm/` (trailing slash) must
        // still give `/dev/shm.containers`, never `/dev/shm/.containers`
        // inside the home.
        let mut sibling = decrypt_dir.clone();
        let name = sibling
            .file_name()
            .map(|n| format!("{}.containers", n.to_string_lossy()))
            .unwrap_or_else(|| ".containers".to_string());
        sibling.set_file_name(name);
        Self {
            tmpfs_dir: decrypt_dir,
            tee_enabled: false,
            container_dir: sibling,
            plaintext_volume: None,
            disk_mode: false,
            space_probe: Box::new(real_space),
            last_outcome: Mutex::new(None),
            in_flight: Mutex::new(std::collections::BTreeMap::new()),
            cache: RwLock::new(HashMap::new()),
            live_plaintexts: std::sync::Mutex::new(LivePlaintexts::default()),
        }
    }

    /// Enable/disable loading TEE-attested encrypted models on this node.
    pub fn with_tee_enabled(mut self, enabled: bool) -> Self {
        self.tee_enabled = enabled;
        self
    }

    /// Where the ciphertext cache lives (design S1). Never inside, equal to, or
    /// containing the decrypt dir: `require_decrypt_dir` refuses that.
    pub fn with_container_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.container_dir = dir.into();
        self
    }

    /// The disk-mode home (`TEE_PLAINTEXT_VOLUME`): the plaintext's directory
    /// in disk mode, and swept in every mode.
    pub fn with_plaintext_volume(mut self, dir: impl Into<PathBuf>) -> Self {
        self.plaintext_volume = Some(dir.into());
        self
    }

    /// Disk mode (design S3): the plaintext lives on the LUKS volume and purges
    /// are unlink-only. The home is derived ([`Self::home`]), so the order of
    /// this and [`Self::with_plaintext_volume`] does not matter. The home rule
    /// is checked by `require_decrypt_dir`; a loader that never calls it
    /// (tests) simply uses the directories it has.
    pub fn with_disk_mode(mut self, on: bool) -> Self {
        self.disk_mode = on;
        self
    }

    /// Disk mode in force: the flag AND a volume to be the home. The flag
    /// alone never turns unlink-only purges on for the tmpfs home.
    pub fn disk_mode(&self) -> bool {
        self.disk_mode && self.plaintext_volume.is_some()
    }

    /// The directory the plaintext is written to: the volume in disk mode,
    /// else the tmpfs dir.
    fn home(&self) -> &Path {
        match (&self.plaintext_volume, self.disk_mode) {
            (Some(v), true) => v,
            _ => &self.tmpfs_dir,
        }
    }

    /// Replace the free-space probe (design S1c) for tests.
    pub fn with_space_probe(mut self, probe: Box<SpaceProbe<'static>>) -> Self {
        self.space_probe = probe;
        self
    }

    /// Build a loader from the environment: `HOST_TEE_ENABLED` (default `false`,
    /// fail-closed; accepts `1`/`true`/`yes`/`on`), `TEE_CONTAINER_DIR` (default
    /// `/var/lib/fabstir/containers`), `TEE_PLAINTEXT_VOLUME`, and the home:
    /// `TEE_DECRYPT_ON_DISK` set → the volume, else `TEE_DECRYPT_DIR` (default
    /// `/dev/shm`). Disk mode without a volume is refused by
    /// `require_decrypt_dir`, not here (`from_env` is infallible).
    pub fn from_env() -> Self {
        let dir =
            std::env::var("TEE_DECRYPT_DIR").unwrap_or_else(|_| DEFAULT_DECRYPT_DIR.to_string());
        let container_dir = std::env::var(CONTAINER_DIR_ENV)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_CONTAINER_DIR.to_string());
        let mut loader = Self::new(dir)
            .with_tee_enabled(parse_host_tee_enabled())
            .with_container_dir(container_dir.trim());
        if let Some(v) = std::env::var(PLAINTEXT_VOLUME_ENV)
            .ok()
            .filter(|v| !v.trim().is_empty())
        {
            loader = loader.with_plaintext_volume(v.trim());
        }
        let disk = std::env::var(DECRYPT_ON_DISK_ENV)
            .map(|v| parse_flag(&v))
            .unwrap_or(false);
        loader.with_disk_mode(disk)
    }

    /// The directory the plaintext is written to (the home in force).
    pub fn decrypt_dir(&self) -> &Path {
        self.home()
    }

    /// The ciphertext cache directory.
    pub fn container_dir(&self) -> &Path {
        &self.container_dir
    }

    /// The orchestration reports that the container for `spec` decrypted fine
    /// but its plaintext failed the on-chain hash bind. A CACHED container
    /// (`outcome == Hit`) is evicted, because a re-seal of a corrected model
    /// under the same DEK and policy at the same ref looks exactly like this
    /// (header matches, AEAD passes, old plaintext) and the next boot must
    /// re-download it, whenever the fix lands; a container downloaded fresh
    /// is kept (re-downloading the same bytes cannot help). A wrong expected
    /// hash therefore costs one download every second boot, bounded by
    /// docker's five restarts and the runbook's stop-by-hand row; the
    /// alternative, a once-per-hash record, wedged the corrected re-seal
    /// after the automatic restarts had spent the eviction. Returns whether
    /// the cache was evicted.
    pub fn note_hash_mismatch(
        &self,
        spec: &EncryptedModelSpec,
        outcome: Option<ContainerOutcome>,
    ) -> bool {
        if outcome != Some(ContainerOutcome::Hit) {
            return false;
        }
        let cache = cache_path(&self.container_dir, &cache_key(&spec.encrypted_path));
        match std::fs::remove_file(&cache) {
            Ok(()) => {
                tracing::warn!(
                    target: "tee",
                    "container cache {} evicted after a hash-bind mismatch; the next boot re-downloads \
                     (a corrected re-seal at the same ref is then picked up; a wrong \
                     TEE_EXPECTED_MODEL_SHA256 costs one download every second boot)",
                    cache.display()
                );
                true
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => {
                tracing::warn!(
                    target: "tee",
                    "could not evict container cache {}: {e}",
                    cache.display()
                );
                false
            }
        }
    }

    /// Mark `key` in flight until the returned guard drops.
    fn in_flight_key(&self, key: &str) -> InFlightKey<'_> {
        *self
            .in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(key.to_string())
            .or_insert(0) += 1;
        InFlightKey {
            loader: self,
            key: key.to_string(),
        }
    }

    /// The container step's outcome of the last load, `None` when the last load
    /// was an in-process plaintext-cache hit (the container step never ran).
    pub fn last_container_outcome(&self) -> Option<ContainerOutcome> {
        *self.last_outcome.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Verify the decrypt dir exists, is writable, and (best-effort) is tmpfs.
    ///
    /// Call once at CVM startup. Returns `Err` if the dir cannot be created or
    /// written; a non-tmpfs dir logs CRITICAL (decrypted weights could touch
    /// persistent disk) but does **not** hard-fail, so Phases 1–4 run on ordinary
    /// dev filesystems — the real tmpfs guarantee is a Phase-5 deploy requirement.
    pub fn verify_decrypt_dir(&self) -> TeeResult<()> {
        self.probe_decrypt_dir()?;
        if !self.disk_mode() && !is_tmpfs(self.home()) {
            tracing::warn!(
                target: "tee",
                "CRITICAL: TEE_DECRYPT_DIR {} is not tmpfs — decrypted weights may touch persistent disk",
                self.home().display()
            );
        }
        Ok(())
    }

    /// Create the decrypt dir (0700) and prove it is writable; no tmpfs opinion.
    fn probe_decrypt_dir(&self) -> TeeResult<()> {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(self.home())?;
        // Writability probe (created + removed; surfaces a read-only/over-quota mount).
        // Random name + create_new(0600) so a pre-planted symlink can't redirect the write.
        let mut rnd = [0u8; 8];
        OsRng.fill_bytes(&mut rnd);
        let probe = self
            .home()
            .join(format!(".tee-write-probe.{}", hex::encode(rnd)));
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&probe)?;
        std::fs::remove_file(&probe)?;
        Ok(())
    }

    /// The live attested path's stricter form of [`verify_decrypt_dir`]
    /// (Phase 5): the decrypt dir MUST be tmpfs. The serving-node shutdown
    /// unlinks the plaintext without overwriting it (a generation may still hold
    /// the mapping), which is only safe where the pages die with the mount; on
    /// a persistent filesystem the bytes would stay recoverable. Fail closed at
    /// boot, before anything is decrypted.
    pub fn require_tmpfs_decrypt_dir(&self) -> TeeResult<()> {
        // The probe only: the refusal below is the one message for non-tmpfs.
        self.probe_decrypt_dir()?;
        // The home rule for the mode in force: tmpfs, or a LUKS device in
        // disk mode (the live path's `require_decrypt_dir` adds the
        // container-dir checks and the sweep around the same rule).
        let info = mount_info(self.home())?;
        home_rule(
            self.disk_mode(),
            self.home(),
            &info.fstype,
            info.dm_uuid.as_deref(),
            &info.slave_uuids,
        )
    }

    /// The live attested path's boot check (Phase 5 P5.5, design §5), in this
    /// order: (0) disk mode needs a volume; (1) the container dir is disjoint
    /// from the home and the volume, and the home from the volume in tmpfs mode
    /// (in disk mode the home IS the volume); (2) both directories exist
    /// (`0700`); (3) the home rule (tmpfs, or a LUKS device in disk mode);
    /// (4) the write probe + `chmod 0700`; (5) the start-up sweep over the
    /// deduplicated set {home, container dir, volume}; (6) `mount_info` for
    /// the home and the container dir, logged: that line is the evidence.
    pub fn require_decrypt_dir(&self) -> TeeResult<()> {
        // (0)
        if self.disk_mode && self.plaintext_volume.is_none() {
            return Err(TeeError::VerificationFailed(format!(
                "{DECRYPT_ON_DISK_ENV} is set but {PLAINTEXT_VOLUME_ENV} is not: disk mode needs \
                 the plaintext volume's mount path (the compose literal); refusing to start"
            )));
        }
        // (1) A pure path check, before anything is created, so the refusal is
        // the same on any filesystem.
        let disjoint = |a: &Path, what_a: &str, b: &Path, what_b: &str| -> TeeResult<()> {
            if a == b || a.starts_with(b) || b.starts_with(a) {
                return Err(TeeError::VerificationFailed(format!(
                    "{what_a} {} and {what_b} {} must be distinct directories, neither inside \
                     the other; refusing to start",
                    a.display(),
                    b.display()
                )));
            }
            Ok(())
        };
        let home_dir = self.home().to_path_buf();
        disjoint(
            &self.container_dir,
            "container dir",
            &home_dir,
            "decrypt dir",
        )?;
        if let Some(vol) = &self.plaintext_volume {
            disjoint(
                &self.container_dir,
                "container dir",
                vol,
                "plaintext volume",
            )?;
            if !self.disk_mode() {
                disjoint(&home_dir, "decrypt dir", vol, "plaintext volume")?;
            }
        }
        // (2)
        for d in [&home_dir, &self.container_dir] {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(d)?;
        }
        // (1) again on the CANONICAL paths, now that both exist: a symlinked
        // container dir pointing into the home would pass the lexical check
        // and put the prune inside the plaintext home.
        {
            let c = std::fs::canonicalize(&self.container_dir)?;
            let h = std::fs::canonicalize(&home_dir)?;
            disjoint(&c, "container dir", &h, "decrypt dir")?;
            if let Some(vol) = &self.plaintext_volume {
                if let Ok(v) = std::fs::canonicalize(vol) {
                    disjoint(&c, "container dir", &v, "plaintext volume")?;
                }
            }
        }
        // (3)
        let home = mount_info(&home_dir)?;
        home_rule(
            self.disk_mode(),
            &home_dir,
            &home.fstype,
            home.dm_uuid.as_deref(),
            &home.slave_uuids,
        )?;
        // (4) The write probe; then 0700 on OUR directories (the container dir,
        // the volume root, which arrives 0755), never on a pre-existing tmpfs
        // such as /dev/shm, which is left as-is.
        self.probe_decrypt_dir()?;
        let _ =
            std::fs::set_permissions(&self.container_dir, std::fs::Permissions::from_mode(0o700));
        if self.disk_mode() {
            let _ = std::fs::set_permissions(&home_dir, std::fs::Permissions::from_mode(0o700));
        }
        // (5)
        let started = std::time::Instant::now();
        let (files, bytes) = self.sweep_leftovers()?;
        tracing::info!(
            target: "tee",
            "start-up sweep: {files} leftover file(s), {bytes} bytes, {:.2}s",
            started.elapsed().as_secs_f64()
        );
        // (6)
        let cache = mount_info(&self.container_dir)?;
        tracing::info!(
            target: "tee",
            "plaintext home ({}): {} [{home}]; container cache: {} [{cache}]",
            if self.disk_mode() { "disk" } else { "tmpfs" },
            home_dir.display(),
            self.container_dir.display()
        );
        if cache.fstype == "overlay" {
            tracing::warn!(
                target: "tee",
                "CRITICAL: container dir {} is on the container's overlay filesystem: the \
                 ciphertext cache will not survive a container recreate (mount the \
                 fabstir-containers volume there)",
                self.container_dir.display()
            );
        }
        Ok(())
    }

    /// Design S3a-3: remove what a previous process left behind, over the
    /// deduplicated set {home, container dir, volume}: the node's own
    /// plaintext names in the two plaintext homes and every `*.part` in the
    /// container dir, by unlink only (never an overwrite: another process's
    /// mapping may still hold the file).
    /// Returns `(files, bytes)`; the caller times it. Public so tests can call
    /// it without the home rule.
    pub fn sweep_leftovers(&self) -> TeeResult<(usize, u64)> {
        let mut total = (0usize, 0u64);
        let mut add = |dir: &Path, (f, b): (usize, u64)| {
            tracing::info!(
                target: "tee",
                "sweep {}: {f} file(s), {b} bytes",
                dir.display()
            );
            total.0 += f;
            total.1 += b;
        };
        let own = |n: &str| is_own_plaintext_name(n);
        let home = self.home();
        // Unlink-only in EVERY directory (P3's rule: never overwrite a file a
        // mapping may still hold; "own" is a name pattern, and a second node
        // sharing this /dev/shm would have its live weights zeroed under a
        // running generation). On tmpfs the pages die with the last reference;
        // on the volume the LUKS layer covers the medium.
        add(home, sweep_dir(home, &own, false)?);
        if let Some(vol) = &self.plaintext_volume {
            if vol != home {
                add(vol, sweep_dir(vol, &own, false)?);
            }
        }
        let part = |n: &str| is_part_name(n);
        add(
            &self.container_dir,
            sweep_dir(&self.container_dir, &part, false)?,
        );
        Ok(total)
    }

    /// Fetch → attest → obtain DEK → decrypt the model to a private file in
    /// the home (tmpfs, or the LUKS volume in disk mode).
    ///
    /// Fail-closed: on any error nothing is left on disk (a partially-written file
    /// is purged). Returns the decrypted file's path and takes a cache
    /// reference; the caller must [`Self::release`] it when done. The digest
    /// is dropped here; [`Self::prepare_encrypted_model_with_digest`] keeps it.
    pub async fn prepare_encrypted_model(
        &self,
        s5: &dyn BlobSource,
        kbs: &dyn KeyBrokerClient,
        provider: &dyn AttestationProvider,
        spec: &EncryptedModelSpec,
    ) -> TeeResult<PathBuf> {
        self.prepare_encrypted_model_with_digest(s5, kbs, provider, spec)
            .await
            .map(|(p, _, _)| p)
    }

    /// As [`Self::prepare_encrypted_model`], also returning the plaintext's
    /// SHA-256 from the decrypt's tee (design S2): the hash-bind step compares
    /// it instead of re-reading the file; and THIS load's container outcome
    /// (`None` for an in-process plaintext hit), so overlapping loads on one
    /// loader never read each other's.
    pub async fn prepare_encrypted_model_with_digest(
        &self,
        s5: &dyn BlobSource,
        kbs: &dyn KeyBrokerClient,
        provider: &dyn AttestationProvider,
        spec: &EncryptedModelSpec,
    ) -> TeeResult<(PathBuf, [u8; 32], Option<ContainerOutcome>)> {
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
        // The outcome belongs to THIS call: an in-process hit reads `None`.
        *self.last_outcome.lock().unwrap_or_else(|e| e.into_inner()) = None;

        // Cache keyed on (model_id, policy_hash) — a policy rotation re-decrypts (4.3.1a).
        let key = (spec.model_id, spec.policy_hash);
        if let Some((path, digest)) = self.cache_acquire(&key) {
            return Ok((path, digest, None));
        }

        // 0. (P4.5) Ask the broker whether it can release this model to this node
        //    at all, BEFORE the download: a broker left in a test evidence mode is
        //    found out here, not after tens of GB. Not a security gate (step 2
        //    judges the release itself).
        kbs.preflight(spec.model_id).await?;

        // 1. The ciphertext cache (design S1, S1a, S1c, S1d): both directories
        //    exist (0700: the model_id-bearing names are not even listable by
        //    other local users; a pre-existing /dev/shm is left as-is), the
        //    other keys are pruned, a cached container is judged by its header
        //    BEFORE any broker traffic, and space is checked before the first
        //    byte.
        let home = self.home().to_path_buf();
        for d in [&home, &self.container_dir] {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(d)?;
        }
        let ckey = cache_key(&spec.encrypted_path);
        // This key is in flight until the load returns: the prune (S1d) keeps
        // every in-flight key's files, so overlapping loads of different
        // models in one loader (library callers; the node runs one) never
        // unlink each other's `.part` or fresh `.enc`.
        let _in_flight = self.in_flight_key(&ckey);
        {
            // Under the lock for the whole prune: a load registering between
            // a snapshot and the read_dir would otherwise have its fresh
            // `.part` unlinked mid-write.
            let keep = self.in_flight.lock().unwrap_or_else(|e| e.into_inner());
            let keys: std::collections::BTreeSet<String> = keep.keys().cloned().collect();
            prune_other_keys(&self.container_dir, &keys)?;
        }
        let cache = cache_path(&self.container_dir, &ckey);
        let space = |len: u64| {
            check_space_for_download(&*self.space_probe, &self.container_dir, &home, len)
        };
        let head = |bytes: &[u8]| {
            refuse_mismatched_head(
                bytes,
                &spec.encrypted_path,
                &spec.model_id,
                &spec.policy_hash,
            )
        };
        let hooks = FetchHooks {
            on_length: &space,
            on_head: &head,
        };
        let mut fetched_now = false;
        let mut outcome = ContainerOutcome::Miss;
        let record = |o: ContainerOutcome| {
            *self.last_outcome.lock().unwrap_or_else(|e| e.into_inner()) = Some(o);
        };
        // One open of the cache decides everything, and the descriptor that
        // passed the header check is the one the decrypt reads: a missing
        // file (no cache, or a concurrent same-key load that judged it stale
        // and unlinked it first) is a MISS, never an `Io` refusal, and a
        // concurrent unlink after this point cannot touch this load.
        let mut hit: Option<std::fs::File> = None;
        if let Some(mut f) = open_cache(&cache)? {
            match header_check(&mut f, &spec.model_id, &spec.policy_hash)? {
                HeaderCheck::Matches => hit = Some(f),
                other => {
                    tracing::warn!(
                        target: "tee",
                        "container cache {}: header {} — stale, re-downloading (no challenge spent)",
                        cache.display(),
                        describe_header(&other)
                    );
                    drop(f);
                    remove_if_present(&cache)?;
                    outcome = ContainerOutcome::Stale;
                }
            }
        }
        // Recorded BEFORE the space check and the download, so a load refused
        // inside the container step still reports how far it got (the retry
        // arm updates it); `None` means only "the container step never ran".
        let container = match hit {
            Some(f) => {
                outcome = ContainerOutcome::Hit;
                record(outcome);
                let len = f.metadata()?.len();
                check_space_for_plaintext(&*self.space_probe, &home, len)?;
                f
            }
            None => {
                record(outcome);
                let f = self.fetch_fresh(s5, spec, &cache, hooks).await?;
                fetched_now = true;
                f
            }
        };
        tracing::info!(
            target: "tee",
            "container cache: {} ({outcome})",
            cache.display()
        );

        // 2. Run the attestation → key-release flow: challenge → gather evidence
        //    (binding a fresh pk_att + nonce) → request the wrapped DEK → unwrap.
        //    The KBS withholds the key on a failed/stale attestation (fail-closed).
        let dek = NodeAttestationClient::obtain_dek(provider, kbs, spec.model_id).await?;

        // 3. Decrypt to a fresh private file (design S1a's once-retry): a
        //    CACHED container failing a content check is deleted and fetched
        //    once more under the same DEK (no second challenge); an I/O error
        //    on either attempt never evicts the cache and is returned as is; a
        //    fresh container failing a content check is refused at once.
        let (path, digest) = match self.attempt(container, &dek, spec) {
            Ok(v) => v,
            Err(e1) if !fetched_now && is_content_error(&e1) => {
                tracing::warn!(
                    target: "tee",
                    "container cache stale or corrupt ({e1}); re-downloading once"
                );
                remove_if_present(&cache)?;
                outcome = ContainerOutcome::Retried;
                record(outcome);
                let fresh = self.fetch_fresh(s5, spec, &cache, hooks).await?;
                match self.attempt(fresh, &dek, spec) {
                    Ok(v) => v,
                    Err(e2) if is_content_error(&e2) => {
                        let _ = std::fs::remove_file(&cache);
                        return Err(TeeError::Fetch(format!(
                            "container refused twice: cached ({e1}); fresh ({e2})"
                        )));
                    }
                    Err(e2) => return Err(e2),
                }
            }
            Err(e) => {
                if fetched_now && is_content_error(&e) {
                    let _ = std::fs::remove_file(&cache);
                }
                return Err(e);
            }
        };

        // A stop that began during the write already unlinked the path (it was
        // registered); do not publish a file that is gone.
        if self
            .live_plaintexts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .stopping
        {
            self.purge(&path);
            return Err(TeeError::Io(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "stop in progress: plaintext discarded",
            )));
        }

        // 4. Publish to the cache (decrypt-twice-keep-one if a concurrent load won the race).
        let (path, digest) = self.cache_publish(key, path, digest);
        Ok((path, digest, Some(outcome)))
    }

    /// Download `spec.encrypted_path` into `cache`, open it, and judge the
    /// fresh file's header (design S1a): a decodable header bound to another
    /// model or policy is refused before any (further) broker traffic, naming
    /// both bindings (the head hook refuses it after 98 bytes; this is the
    /// belt-and-braces re-check on the file); an undecodable file falls
    /// through to the decrypt. Returns the open handle the decrypt reads from.
    /// A concurrent same-key load unlinking the path between the rename and
    /// this open is one re-fetch, not a failure.
    async fn fetch_fresh(
        &self,
        s5: &dyn BlobSource,
        spec: &EncryptedModelSpec,
        cache: &Path,
        hooks: FetchHooks<'_>,
    ) -> TeeResult<std::fs::File> {
        s5.get_file_to(&spec.encrypted_path, cache, hooks).await?;
        let mut f = match std::fs::File::open(cache) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!(
                    target: "tee",
                    "container cache {} vanished between the download and its open (a concurrent load); fetching again",
                    cache.display()
                );
                s5.get_file_to(&spec.encrypted_path, cache, hooks).await?;
                std::fs::File::open(cache)?
            }
            Err(e) => return Err(TeeError::Io(e)),
        };
        if let HeaderCheck::Mismatch {
            model_id,
            policy_hash,
        } = header_check(&mut f, &spec.model_id, &spec.policy_hash)?
        {
            drop(f);
            let _ = std::fs::remove_file(cache);
            return Err(TeeError::VerificationFailed(format!(
                "the blob at {} is sealed for model {}/policy {}; this boot pinned {}/{} \
                 (the re-seal after a policy pin is not uploaded yet, or the ref points at \
                 another model)",
                spec.encrypted_path,
                hex::encode(model_id),
                hex::encode(policy_hash),
                hex::encode(spec.model_id),
                hex::encode(spec.policy_hash)
            )));
        }
        Ok(f)
    }

    /// One decrypt attempt: create the plaintext under the live lock, stream-
    /// decrypt the cached container into it with the SHA-256 tee, purge on any
    /// error. Synchronous end to end (design S2): no await sits between the
    /// plaintext's creation and its purge or publication.
    fn attempt(
        &self,
        container: std::fs::File,
        dek: &[u8; 32],
        spec: &EncryptedModelSpec,
    ) -> TeeResult<(PathBuf, [u8; 32])> {
        let path = self.fresh_path(&spec.model_id);
        // Create + register under the live lock, refusing once a stop has begun:
        // the emergency exit's unlink pass and this creation can never interleave
        // so that a file appears after the pass.
        let file = {
            let mut live = self
                .live_plaintexts
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if live.stopping {
                return Err(TeeError::Io(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "stop in progress: refusing to create a plaintext",
                )));
            }
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?;
            live.paths.insert(path.clone());
            file
        };
        match decrypt_to_file(container, dek, spec, file) {
            Ok(digest) => Ok((path, digest)),
            Err(e) => {
                self.purge(&path);
                Err(e)
            }
        }
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
                self.purge(&entry.path);
            }
        }
    }

    /// Delete `path` and forget it. Used on every ordinary deletion: zero then
    /// unlink in tmpfs mode; unlink only in disk mode (design S3a-2: the LUKS
    /// layer covers the medium, and zeroing 100 GB at disk speed is minutes).
    fn purge(&self, path: &Path) {
        if self.disk_mode() {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => tracing::warn!(
                    target: "tee",
                    "CRITICAL: failed to unlink decrypted plaintext {} — it may persist on disk: {e}",
                    path.display()
                ),
            }
        } else {
            purge_or_warn(path);
        }
        self.forget_plaintext(path);
    }

    /// Drop `path` from the live set (the caller removed the file some other way).
    pub fn forget_plaintext(&self, path: &Path) {
        self.live_plaintexts
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .paths
            .remove(path);
    }

    /// Emergency exit: unlink (never overwrite; a mapping may be open) every
    /// plaintext this loader still has on disk, whatever state it is in, and
    /// return how many there were. Marks the loader as stopping, so no new
    /// plaintext can be created afterwards (a load in flight fails closed at
    /// its next step). The cache is left alone; the process is ending.
    /// Idempotent.
    pub fn unlink_live_plaintexts(&self) -> usize {
        let paths: Vec<PathBuf> = {
            let mut live = self
                .live_plaintexts
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            live.stopping = true;
            std::mem::take(&mut live.paths).into_iter().collect()
        };
        let mut n = 0;
        for p in paths {
            match std::fs::remove_file(&p) {
                Ok(()) => n += 1,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                // `raw_stderr`, not `tracing` or `eprintln!`: both take a lock
                // (stdout via the fmt subscriber, the reentrant `Stderr` lock),
                // and the emergency exit calling this must never wait on a main
                // thread blocked in its own `println!`/`eprintln!` on a stalled
                // log pipe.
                Err(e) => raw_stderr(&format!(
                    "CRITICAL: could not unlink decrypted plaintext {}: {e}\n",
                    p.display()
                )),
            }
        }
        n
    }

    /// Cache fast-path: if present, take a reference and return the path and
    /// its digest.
    fn cache_acquire(&self, key: &CacheKey) -> Option<(PathBuf, [u8; 32])> {
        let mut cache = self.cache.write().expect("tee cache poisoned");
        cache.get_mut(key).map(|e| {
            e.refcount += 1;
            (e.path.clone(), e.digest)
        })
    }

    /// Insert a freshly decrypted file, or — if a concurrent load already
    /// published one — reference theirs and delete the redundant copy. The
    /// WINNER's `(path, digest)` comes back: the loser's digest is dropped with
    /// its copy, so the digest always belongs to the path handed back.
    fn cache_publish(&self, key: CacheKey, path: PathBuf, digest: [u8; 32]) -> (PathBuf, [u8; 32]) {
        let mut cache = self.cache.write().expect("tee cache poisoned");
        if let Some(entry) = cache.get_mut(&key) {
            entry.refcount += 1;
            let winner = (entry.path.clone(), entry.digest);
            drop(cache);
            self.purge(&path); // redundant copy from a concurrent decrypt
            return winner;
        }
        cache.insert(
            key,
            CacheEntry {
                path: path.clone(),
                digest,
                refcount: 1,
            },
        );
        (path, digest)
    }

    /// A unique private path in the decrypt dir for one decryption attempt.
    fn fresh_path(&self, model_id: &[u8; 32]) -> PathBuf {
        let mut suffix = [0u8; 8];
        OsRng.fill_bytes(&mut suffix);
        self.home().join(format!(
            "{}.{}.gguf",
            hex::encode(model_id),
            hex::encode(suffix)
        ))
    }
}

/// Stream-decrypt the open cached `container` (positioned at 0) into `file`
/// (a fresh `0600` file, created exclusively) with a SHA-256 tee; returns the
/// plaintext's digest. Peak heap is two chunks (design S2a), never the model.
fn decrypt_to_file(
    mut container: std::fs::File,
    dek: &[u8; 32],
    spec: &EncryptedModelSpec,
    file: std::fs::File,
) -> TeeResult<[u8; 32]> {
    std::io::Seek::seek(&mut container, std::io::SeekFrom::Start(0))?;
    let reader = BufReader::with_capacity(1 << 20, container);
    let mut tee = Sha256Tee::new(file);
    decrypt_model_to_writer(reader, &mut tee, dek, &spec.model_id, &spec.policy_hash)?;
    let (digest, file) = tee.finish();
    file.sync_all()?;
    Ok(digest)
}

/// Unlink `path`, treating an already-missing file as done: two concurrent
/// same-key loads may both judge the cache stale or corrupt and race on the
/// unlink; the loser must not fail the whole load with an `Io`.
fn remove_if_present(path: &Path) -> TeeResult<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(TeeError::Io(e)),
    }
}

/// Design S1a: the errors the once-retry keys on. `Crypto` (a tag, a
/// truncation, a trailing byte, a bad header) and `VerificationFailed` (the
/// header's binding) come from the container's content; `Io` is the node's
/// own disk and never evicts a cache.
fn is_content_error(e: &TeeError) -> bool {
    matches!(e, TeeError::Crypto(_) | TeeError::VerificationFailed(_))
}

fn describe_header(h: &HeaderCheck) -> String {
    match h {
        HeaderCheck::Matches => "matches".into(),
        HeaderCheck::Mismatch {
            model_id,
            policy_hash,
        } => format!(
            "bound to model {}/policy {}",
            hex::encode(model_id),
            hex::encode(policy_hash)
        ),
        HeaderCheck::Undecodable => "undecodable".into(),
    }
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
/// are not decoded, so an exotic mount path with spaces may be skipped. The
/// dominant failure direction is to classify tmpfs as non-tmpfs, which the
/// plain loader turns into a warning and the live attested path
/// (`require_tmpfs_decrypt_dir`) into a refusal to start: fail closed, and
/// `/dev/shm` has no spaces.
fn filesystem_type(path: &Path) -> Option<String> {
    let canon = std::fs::canonicalize(path).ok()?;
    let mounts = std::fs::read_to_string("/proc/mounts").ok()?;
    filesystem_type_from_mounts(&canon, &mounts)
}

/// [`filesystem_type`] over an explicit `/proc/mounts` text (pure, for tests).
/// Picks the deepest mount point that is a prefix of `canon`; on EQUAL depth
/// the LATER entry wins, because an over-mount (the same mount point listed
/// twice, e.g. `/dev/shm` first as tmpfs and then bind-mounted from a
/// persistent directory) is effective in list order and this answer now gates
/// the attested path (`require_tmpfs_decrypt_dir`).
pub fn filesystem_type_from_mounts(canon: &Path, mounts: &str) -> Option<String> {
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
            if best.as_ref().is_none_or(|(d, _)| depth >= *d) {
                best = Some((depth, fstype.to_string()));
            }
        }
    }
    best.map(|(_, t)| t)
}

/// Write `msg` to fd 2 with bare `write(2)` calls: no Rust `Stderr` lock, so
/// an emergency exit can log while the main thread is blocked inside its own
/// `eprintln!` on a full log pipe. Partial writes are continued; errors are
/// ignored (the process is exiting; the line is a courtesy).
pub fn raw_stderr(msg: &str) {
    let mut buf = msg.as_bytes();
    while !buf.is_empty() {
        // SAFETY: fd 2, a valid pointer/length pair into `buf`; write(2) has no
        // other preconditions.
        let n = unsafe { libc::write(2, buf.as_ptr() as *const libc::c_void, buf.len()) };
        if n <= 0 {
            break;
        }
        buf = &buf[n as usize..];
    }
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
