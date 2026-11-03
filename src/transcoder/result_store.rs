// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The transcode result store (SCI-C D19(d), IMPLEMENTATION-SESSION-COMPLETION-INTEGRITY).
//!
//! A transcode whose client left still runs, bills and keeps its result, so the
//! client can fetch it later on a new connection. One encrypted file per
//! `(job_id, task_id)` under `TRANSCODE_RESULT_DIR` (default
//! `./data/transcode-results`), holding the state, the plaintext
//! `transcode_complete` payload once paid, and the owner pinned at accept.
//!
//! Every record is encrypted at rest (XChaCha20-Poly1305) under a key derived
//! from the host's private key, which is never written to disk; the record's
//! own `(job_id, task_id)` is the AAD, so a file renamed onto another record's
//! name does not decrypt. Finished records are kept for
//! `TRANSCODE_RESULT_RETENTION_DAYS` (7) and the store is capped at
//! `TRANSCODE_RESULT_MAX_ENTRIES`. At open, records left `InFlight` by a node
//! that stopped are turned into `Failed` (`INTERRUPTED`), and their jobs are
//! kept for the expiry sweeper (D17).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const RETENTION_DAYS_DEFAULT: u64 = 7;
pub const MAX_ENTRIES_DEFAULT: usize = 5_000;
pub const DIR_DEFAULT: &str = "./data/transcode-results";
/// The failure stored when a run ended without recording an outcome (a panic,
/// or a node that stopped mid-run).
pub const INTERRUPTED: &str = "INTERRUPTED";

const EXT: &str = "rec";
const NONCE_LEN: usize = 24;

/// Where a transcode's result stands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ResultState {
    /// Accepted; the sidecar is running.
    InFlight,
    /// Paid; `payload` is the `transcode_complete` JSON exactly as sent.
    Done { payload: Value },
    /// Ended without a releasable result, with the error the client was given.
    Failed { code: String, message: String },
}

impl ResultState {
    fn is_terminal(&self) -> bool {
        !matches!(self, ResultState::InFlight)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResultRecord {
    pub job_id: u64,
    pub task_id: String,
    /// The identity that ordered the work (the signer recovered from the
    /// submitting connection's init), lowercase hex. `None` serves nobody.
    pub owner: Option<String>,
    /// Unix seconds of the last write; retention runs from the terminal write.
    pub stored_at: u64,
    pub state: ResultState,
}

/// What a fetch gets (D19(e)).
#[derive(Debug, Clone, PartialEq)]
pub enum ResultAccess {
    Serve(Value),
    Pending,
    NotFound,
    Expired,
    NotOwner,
    Failed { code: String, message: String },
    Held { code: String, message: String },
}

/// The fetch decision, pure. Order: existence, then owner (a non-owner learns
/// nothing about the state), then retention, then state; a `Done` result is
/// re-checked against moderation at serve time (`held` is consulted only then).
pub fn result_access(
    record: Option<&ResultRecord>,
    signer: Option<&str>,
    now: u64,
    retention_secs: u64,
    held: impl FnOnce() -> Option<(String, String)>,
) -> ResultAccess {
    let Some(record) = record else {
        return ResultAccess::NotFound;
    };
    let owns = match (record.owner.as_deref(), signer) {
        (Some(owner), Some(signer)) => owner.eq_ignore_ascii_case(signer),
        _ => false,
    };
    if !owns {
        return ResultAccess::NotOwner;
    }
    // Retention applies to every state: no run lasts days, so an `InFlight`
    // record that old belongs to a run that is gone (a failed final write).
    if now >= record.stored_at.saturating_add(retention_secs) {
        return ResultAccess::Expired;
    }
    match &record.state {
        ResultState::InFlight => ResultAccess::Pending,
        ResultState::Failed { code, message } => ResultAccess::Failed {
            code: code.clone(),
            message: message.clone(),
        },
        ResultState::Done { payload } => match held() {
            Some((code, message)) => ResultAccess::Held { code, message },
            None => ResultAccess::Serve(payload.clone()),
        },
    }
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

pub struct ResultStore {
    dir: PathBuf,
    key: [u8; 32],
    retention_secs: u64,
    max_entries: usize,
    /// Serialises writes and the cap check.
    lock: Mutex<()>,
    /// Jobs whose run was interrupted by a stop, for the expiry sweeper.
    interrupted: Mutex<Vec<u64>>,
}

impl ResultStore {
    /// The at-rest key: HKDF-SHA256 of the host's private key. Never stored.
    pub fn derive_key(host_private_key: &[u8; 32]) -> [u8; 32] {
        let hk =
            hkdf::Hkdf::<Sha256>::new(Some(b"fabstir-transcode-result-store"), host_private_key);
        let mut key = [0u8; 32];
        hk.expand(b"v1", &mut key)
            .expect("32 bytes is a valid HKDF-SHA256 length");
        key
    }

    /// Open from `TRANSCODE_RESULT_DIR`, `TRANSCODE_RESULT_RETENTION_DAYS` and
    /// `TRANSCODE_RESULT_MAX_ENTRIES`.
    pub fn open_from_env(host_private_key: &[u8; 32]) -> Result<Arc<Self>> {
        let dir = std::env::var("TRANSCODE_RESULT_DIR")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| DIR_DEFAULT.to_string());
        let days = std::env::var("TRANSCODE_RESULT_RETENTION_DAYS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|d| *d > 0)
            .unwrap_or(RETENTION_DAYS_DEFAULT);
        let max = std::env::var("TRANSCODE_RESULT_MAX_ENTRIES")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|m| *m > 0)
            .unwrap_or(MAX_ENTRIES_DEFAULT);
        Self::open(
            Path::new(&dir),
            Self::derive_key(host_private_key),
            days * 86_400,
            max,
            unix_now(),
        )
    }

    /// Open (creating 0700) and reload: expired and unreadable records are
    /// deleted, and records left `InFlight` become `Failed` (`INTERRUPTED`).
    pub fn open(
        dir: &Path,
        key: [u8; 32],
        retention_secs: u64,
        max_entries: usize,
        now: u64,
    ) -> Result<Arc<Self>> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("create result directory {}", dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                .with_context(|| format!("restrict result directory {}", dir.display()))?;
        }
        let store = Arc::new(Self {
            dir: dir.to_path_buf(),
            key,
            retention_secs,
            max_entries,
            lock: Mutex::new(()),
            interrupted: Mutex::new(Vec::new()),
        });
        // A store that cannot be written must refuse work, not discover it later.
        let probe = dir.join(".write-probe");
        std::fs::write(&probe, b"ok")
            .with_context(|| format!("result directory {} is not writable", dir.display()))?;
        let _ = std::fs::remove_file(&probe);
        store.reload(now);
        Ok(store)
    }

    pub fn retention_secs(&self) -> u64 {
        self.retention_secs
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn path_for(&self, job_id: u64, task_id: &str) -> PathBuf {
        let digest = Sha256::digest(task_id.as_bytes());
        self.dir
            .join(format!("{job_id}-{}.{EXT}", hex::encode(&digest[..16])))
    }

    /// The AAD is the file's own name (job id plus task-id hash), so a record
    /// copied onto another record's name does not decrypt.
    fn file_aad(stem: &str) -> Vec<u8> {
        format!("fabstir-transcode-result-v1|{stem}").into_bytes()
    }

    fn record_files(&self) -> Vec<PathBuf> {
        std::fs::read_dir(&self.dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().and_then(|x| x.to_str()) == Some(EXT))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn read_file(&self, path: &Path) -> Option<ResultRecord> {
        let bytes = std::fs::read(path).ok()?;
        if bytes.len() <= NONCE_LEN {
            return None;
        }
        let (nonce, ciphertext) = bytes.split_at(NONCE_LEN);
        let stem = path.file_stem()?.to_str()?;
        let job_id: u64 = stem.split('-').next()?.parse().ok()?;
        let plaintext =
            crate::crypto::decrypt_with_aead(ciphertext, nonce, &Self::file_aad(stem), &self.key)
                .ok()?;
        let record: ResultRecord = serde_json::from_slice(&plaintext).ok()?;
        // The name must be this record's own name.
        if self.path_for(record.job_id, &record.task_id) != path || record.job_id != job_id {
            return None;
        }
        Some(record)
    }

    fn write_record(&self, record: &ResultRecord) -> Result<()> {
        let path = self.path_for(record.job_id, &record.task_id);
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("bad record name"))?
            .to_string();
        let plaintext = serde_json::to_vec(record)?;
        let mut nonce = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce);
        let ciphertext = crate::crypto::encrypt_with_aead(
            &plaintext,
            &nonce,
            &Self::file_aad(&stem),
            &self.key,
        )?;
        let tmp = self.dir.join(format!(".{stem}.tmp"));
        {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(&tmp)
                .with_context(|| format!("open {}", tmp.display()))?;
            file.write_all(&nonce)?;
            file.write_all(&ciphertext)?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp, &path).with_context(|| format!("rename onto {}", path.display()))?;
        // Make the rename itself durable.
        if let Ok(dir) = std::fs::File::open(&self.dir) {
            let _ = dir.sync_all();
        }
        Ok(())
    }

    fn reload(&self, now: u64) {
        let _g = self.guard();
        // Temp files left by a stop between write and rename.
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for path in entries.filter_map(|e| e.ok().map(|e| e.path())) {
                if path.extension().and_then(|x| x.to_str()) == Some("tmp") {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        let mut interrupted = Vec::new();
        for path in self.record_files() {
            match self.read_file(&path) {
                None => {
                    tracing::warn!(
                        "[SCI-C] result store: deleting unreadable record {}",
                        path.display()
                    );
                    let _ = std::fs::remove_file(&path);
                }
                Some(record) if self.expired(&record, now) => {
                    let _ = std::fs::remove_file(&path);
                }
                Some(mut record) if record.state == ResultState::InFlight => {
                    interrupted.push(record.job_id);
                    record.state = ResultState::Failed {
                        code: INTERRUPTED.to_string(),
                        message: "the node stopped while this transcode ran".to_string(),
                    };
                    record.stored_at = now;
                    if let Err(e) = self.write_record(&record) {
                        tracing::error!(
                            "[SCI-C] result store: could not mark job {} task {} interrupted: {}",
                            record.job_id,
                            record.task_id,
                            e
                        );
                    }
                }
                Some(_) => {}
            }
        }
        interrupted.sort_unstable();
        interrupted.dedup();
        if !interrupted.is_empty() {
            tracing::warn!(
                "[SCI-C] result store: {} job(s) were mid-transcode when the node stopped: {:?}",
                interrupted.len(),
                interrupted
            );
        }
        *self.interrupted.lock().unwrap_or_else(|p| p.into_inner()) = interrupted;
    }

    fn expired(&self, record: &ResultRecord, now: u64) -> bool {
        now >= record.stored_at.saturating_add(self.retention_secs)
    }

    /// Every job with a record (from the file names; nothing is decrypted),
    /// for the expiry sweeper's registry (D17(a)).
    pub fn job_ids(&self) -> Vec<u64> {
        let mut jobs: Vec<u64> = self
            .record_files()
            .iter()
            .filter_map(|p| p.file_stem()?.to_str()?.split('-').next()?.parse().ok())
            .collect();
        jobs.sort_unstable();
        jobs.dedup();
        jobs
    }

    /// Jobs found mid-transcode at open (for the expiry sweeper, D17).
    pub fn interrupted_jobs(&self) -> Vec<u64> {
        self.interrupted
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn len(&self) -> usize {
        self.record_files().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Record a transcode as accepted, under the cap: expired records go first,
    /// then the oldest finished ones. A store full of runs in flight refuses.
    pub fn begin(&self, job_id: u64, task_id: &str, owner: Option<String>, now: u64) -> Result<()> {
        let _g = self.guard();
        let files = self.record_files();
        if files.len() >= self.max_entries {
            let mut finished: Vec<(u64, PathBuf)> = Vec::new();
            for path in files {
                match self.read_file(&path) {
                    None => {
                        let _ = std::fs::remove_file(&path);
                    }
                    Some(r) if self.expired(&r, now) => {
                        let _ = std::fs::remove_file(&path);
                    }
                    Some(r) if r.state.is_terminal() => finished.push((r.stored_at, path)),
                    Some(_) => {}
                }
            }
            let mut count = self.record_files().len();
            finished.sort();
            for (_, path) in finished {
                if count < self.max_entries {
                    break;
                }
                let _ = std::fs::remove_file(&path);
                count -= 1;
            }
            if count >= self.max_entries {
                return Err(anyhow!(
                    "result store full ({} runs in flight)",
                    self.max_entries
                ));
            }
        }
        self.write_record(&ResultRecord {
            job_id,
            task_id: task_id.to_string(),
            owner: owner.map(|o| o.to_ascii_lowercase()),
            stored_at: now,
            state: ResultState::InFlight,
        })
    }

    /// Write a terminal state, keeping the owner pinned at accept.
    pub fn finish(
        &self,
        job_id: u64,
        task_id: &str,
        owner: Option<String>,
        state: ResultState,
        now: u64,
    ) -> Result<()> {
        let _g = self.guard();
        self.write_record(&ResultRecord {
            job_id,
            task_id: task_id.to_string(),
            owner: owner.map(|o| o.to_ascii_lowercase()),
            stored_at: now,
            state,
        })
    }

    pub fn get(&self, job_id: u64, task_id: &str) -> Option<ResultRecord> {
        let path = self.path_for(job_id, task_id);
        if !path.exists() {
            return None;
        }
        self.read_file(&path)
            .filter(|r| r.job_id == job_id && r.task_id == task_id)
    }

    pub fn remove(&self, job_id: u64, task_id: &str) {
        let _g = self.guard();
        let _ = std::fs::remove_file(self.path_for(job_id, task_id));
    }

    /// Delete finished records past retention (and unreadable files).
    pub fn sweep(&self, now: u64) -> usize {
        let _g = self.guard();
        let mut removed = 0;
        for path in self.record_files() {
            let drop = match self.read_file(&path) {
                None => true,
                Some(r) => self.expired(&r, now),
            };
            if drop && std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        removed
    }
}

/// One accepted transcode's place in the store. Its outcome is written by
/// `done` or `failed`; dropped without either (a panic, a path that forgot),
/// it records `INTERRUPTED` so a fetch never waits on a run that is gone.
pub struct ResultSlot {
    store: Arc<ResultStore>,
    job_id: u64,
    task_id: String,
    owner: Option<String>,
    written: bool,
}

impl ResultSlot {
    pub fn new(
        store: Arc<ResultStore>,
        job_id: u64,
        task_id: String,
        owner: Option<String>,
    ) -> Self {
        Self {
            store,
            job_id,
            task_id,
            owner,
            written: false,
        }
    }

    fn write(&mut self, state: ResultState) {
        if self.written {
            return;
        }
        let state_is_done = matches!(state, ResultState::Done { .. });
        // One retry; `written` only on success, so a drop retries as INTERRUPTED.
        for attempt in 1..=2 {
            match self.store.finish(
                self.job_id,
                &self.task_id,
                self.owner.clone(),
                state.clone(),
                unix_now(),
            ) {
                Ok(()) => {
                    self.written = true;
                    let metrics = &crate::monitoring::session_completion_metrics::SCI_C;
                    if state_is_done {
                        metrics.transcode_done();
                    } else {
                        metrics.transcode_failed();
                    }
                    return;
                }
                Err(e) => tracing::error!(
                    "[SCI-C] job {} task {}: could not store the transcode result (attempt {}): {}",
                    self.job_id,
                    self.task_id,
                    attempt,
                    e
                ),
            }
        }
    }

    /// The run was paid; `payload` is the `transcode_complete` JSON.
    pub fn done(&mut self, payload: Value) {
        self.write(ResultState::Done { payload });
    }

    /// The run ended without a releasable result.
    pub fn failed(&mut self, code: &str, message: &str) {
        self.write(ResultState::Failed {
            code: code.to_string(),
            message: message.to_string(),
        });
    }
}

impl Drop for ResultSlot {
    fn drop(&mut self) {
        if !self.written {
            self.failed(
                INTERRUPTED,
                "the transcode ended without recording a result",
            );
        }
    }
}
