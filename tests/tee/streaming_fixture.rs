// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P5.5 — fixtures shared by `test_streaming_load.rs` and
//! `test_plaintext_home.rs`: a blob store whose contents the test can swap
//! between loads, a broker wrapper, ONE shared event log both push their
//! names onto (the ORDER of `get_file_to` vs `challenge` is what the header
//! check's tests assert), and a loader with explicit directories under one
//! tempdir.

use async_trait::async_trait;
use fabstir_llm_node::tee::container::encrypt_model;
use fabstir_llm_node::tee::container_cache::{cache_key, write_via_part, FetchHooks};
use fabstir_llm_node::tee::key_broker::KeyBrokerClient;
use fabstir_llm_node::tee::mock::{MockAttestationProvider, MockKeyBroker};
use fabstir_llm_node::tee::model_source::{BlobSource, EncryptedModelLoader, EncryptedModelSpec};
use fabstir_llm_node::tee::types::{
    CcMode, CvmPolicy, Evidence, GpuPolicy, Policy, TeeError, TeeResult, WrappedKey,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub(super) const SKU: &str = "H100";
pub(super) const MEASUREMENT: [u8; 48] = [0x42u8; 48];
pub(super) const REF: &str = "models/target.enc";
pub(super) const CHUNK: u32 = 1024;

pub(super) fn test_policy(model_id: [u8; 32]) -> Policy {
    Policy {
        schema_version: 2,
        policy_version: 1,
        model_id,
        not_before: 0,
        expiry: u64::MAX - 1,
        cvm: CvmPolicy {
            mrtd: hex::encode(MEASUREMENT),
            rtmr0: "00".repeat(48),
            rtmr1: "00".repeat(48),
            rtmr2: "00".repeat(48),
            os_image_hash: "00".repeat(32),
            compose_hash: "00".repeat(32),
            app_id: None,
            key_provider: None,
            require_td_debug_off: true,
            allowed_tcb_status: vec!["UpToDate".to_string()],
            allowed_advisory_ids: vec![],
        },
        gpu: GpuPolicy {
            allowed_hwmodels: vec![SKU.to_string()],
            require_cc_mode: Some(CcMode::On),
            require_secure_boot: true,
            require_debug_disabled: true,
            min_driver_version: None,
            min_vbios_version: None,
        },
    }
}

pub(super) fn good_provider() -> MockAttestationProvider {
    MockAttestationProvider::new(SKU, MEASUREMENT, CcMode::On)
}

/// The shared event log.
pub(super) type Log = Arc<Mutex<Vec<&'static str>>>;

pub(super) fn log_of(log: &Log) -> Vec<&'static str> {
    log.lock().unwrap().clone()
}

/// A blob store the test can re-point between loads, logging `get_file_to`,
/// with an optional side effect run BEFORE the bytes are served (the
/// read-only-dir-during-the-retry arm).
pub(super) struct SwappableBlobs {
    blobs: Mutex<HashMap<String, Vec<u8>>>,
    pub(super) log: Log,
    pub(super) get_file_to_calls: AtomicUsize,
    pub(super) on_fetch: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
    /// Yield to the runtime AFTER the file is in place, so two overlapping
    /// loads interleave at the point where one's fresh `.enc` exists.
    pub(super) yield_after_fetch: std::sync::atomic::AtomicBool,
}

impl SwappableBlobs {
    pub(super) fn new(log: Log, path: &str, bytes: Vec<u8>) -> Self {
        Self {
            blobs: Mutex::new(HashMap::from([(path.to_string(), bytes)])),
            log,
            get_file_to_calls: AtomicUsize::new(0),
            on_fetch: Mutex::new(None),
            yield_after_fetch: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub(super) fn insert(&self, path: &str, bytes: Vec<u8>) {
        self.blobs.lock().unwrap().insert(path.to_string(), bytes);
    }
}

#[async_trait]
impl BlobSource for SwappableBlobs {
    async fn get_file(&self, path: &str) -> TeeResult<Vec<u8>> {
        self.blobs
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or_else(|| {
                TeeError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    path.to_string(),
                ))
            })
    }

    async fn get_file_to(&self, path: &str, dest: &Path, hooks: FetchHooks<'_>) -> TeeResult<u64> {
        self.log.lock().unwrap().push("get_file_to");
        self.get_file_to_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(f) = self.on_fetch.lock().unwrap().as_ref() {
            f();
        }
        let bytes = self.get_file(path).await?;
        let n = write_via_part(dest, &bytes, hooks)?;
        if self.yield_after_fetch.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
        Ok(n)
    }
}

/// Logs `challenge`; delegates the release flow to the mock broker.
pub(super) struct LoggingBroker {
    pub(super) inner: MockKeyBroker,
    pub(super) log: Log,
    pub(super) challenges: AtomicUsize,
}

impl LoggingBroker {
    pub(super) fn new(log: Log, model_id: [u8; 32], dek: [u8; 32]) -> Self {
        Self {
            inner: MockKeyBroker::new(HashMap::from([(model_id, (dek, test_policy(model_id)))])),
            log,
            challenges: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl KeyBrokerClient for LoggingBroker {
    async fn challenge(&self, model_id: [u8; 32], pk_att: &[u8]) -> TeeResult<[u8; 32]> {
        self.log.lock().unwrap().push("challenge");
        self.challenges.fetch_add(1, Ordering::SeqCst);
        self.inner.challenge(model_id, pk_att).await
    }
    async fn request_key(&self, model_id: [u8; 32], ev: &Evidence) -> TeeResult<WrappedKey> {
        self.inner.request_key(model_id, ev).await
    }
}

/// One tempdir holding `decrypt/`, `containers/` and `vol/`.
pub(super) struct Dirs {
    pub(super) _tmp: tempfile::TempDir,
    pub(super) decrypt: PathBuf,
    pub(super) containers: PathBuf,
    pub(super) vol: PathBuf,
}

pub(super) fn dirs() -> Dirs {
    let tmp = tempfile::tempdir().expect("tempdir");
    let d = Dirs {
        decrypt: tmp.path().join("decrypt"),
        containers: tmp.path().join("containers"),
        vol: tmp.path().join("vol"),
        _tmp: tmp,
    };
    std::fs::create_dir_all(&d.decrypt).unwrap();
    std::fs::create_dir_all(&d.containers).unwrap();
    std::fs::create_dir_all(&d.vol).unwrap();
    d
}

/// A TEE-enabled tmpfs-mode loader over `dirs` (no `require_decrypt_dir`:
/// the home rule is tested on its own).
pub(super) fn loader(d: &Dirs) -> EncryptedModelLoader {
    EncryptedModelLoader::new(&d.decrypt)
        .with_tee_enabled(true)
        .with_container_dir(&d.containers)
}

pub(super) fn spec(model_id: [u8; 32], policy_hash: [u8; 32]) -> EncryptedModelSpec {
    EncryptedModelSpec {
        model_id,
        policy_hash,
        encrypted_path: REF.to_string(),
    }
}

pub(super) fn seal(
    plaintext: &[u8],
    dek: &[u8; 32],
    model_id: [u8; 32],
    policy_hash: [u8; 32],
) -> Vec<u8> {
    encrypt_model(plaintext, dek, model_id, policy_hash, CHUNK).unwrap()
}

pub(super) fn plaintext(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

/// `<containers>/<key>.enc` for `REF`.
pub(super) fn cache_file(d: &Dirs) -> PathBuf {
    d.containers.join(format!("{}.enc", cache_key(REF)))
}

pub(super) fn plant(d: &Dirs, bytes: &[u8]) {
    std::fs::write(cache_file(d), bytes).unwrap();
}

pub(super) fn regular_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}
