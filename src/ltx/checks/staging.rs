// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The staging area (D8): a memory-backed directory the node writes and the service reads. An item is written as
//! `.part`, renamed, judged, and deleted after the answer. Full means `CHECK_UNAVAILABLE`; a start-up sweep clears
//! leftovers.
//!
//! No path leaves a customer's file behind. The write runs on a blocking thread, which a timeout or a dropped
//! check cannot stop; so the guard and the writer share a flag. Dropping the guard sets it and unlinks; a writer
//! that finishes after that sees it and unlinks what it has just created.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// The longest one staging write may take (a memory-backed directory answers in milliseconds).
pub const WRITE_BOUND: Duration = Duration::from_secs(30);
/// A staged file that must outlive the sweep ([`Staged::keep`]) carries this prefix.
pub const KEPT_PREFIX: &str = "kept-";
/// The extensions the node stages (the start-up sweep touches nothing else).
pub const STAGED_EXTENSIONS: &[&str] = &["mp4", "png", "jpg", "webp"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageError {
    /// The item does not fit beside what is already staged (or the filesystem said so).
    Full,
    Io(String),
}

/// One staged item. Dropping it deletes the file, whether or not its write has finished.
#[derive(Debug)]
pub struct Staged {
    node_path: PathBuf,
    part_path: PathBuf,
    service_path: String,
    abandoned: Arc<AtomicBool>,
}

impl Staged {
    /// The path the service is given.
    pub fn service_path(&self) -> &str {
        &self.service_path
    }
    pub fn node_path(&self) -> &Path {
        &self.node_path
    }
    /// Keep the file (the one case: neither a sealed copy nor the `unsealed/` copy could be written and this is
    /// the only one). It is renamed `kept-<name>`, which the start-up sweep does not match, so it survives a
    /// restart until a person deals with it. Returns where it is now.
    pub fn keep(self) -> PathBuf {
        let path = self.node_path.clone();
        std::mem::forget(self);
        let kept = match (path.parent(), path.file_name()) {
            (Some(dir), Some(name)) => dir.join(format!("{KEPT_PREFIX}{}", name.to_string_lossy())),
            _ => return path,
        };
        match std::fs::rename(&path, &kept) {
            Ok(()) => kept,
            Err(_) => path,
        }
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        // tell a writer still at work to clean up after itself, then unlink what exists now
        self.abandoned.store(true, Ordering::SeqCst);
        let _ = std::fs::remove_file(&self.part_path);
        let _ = std::fs::remove_file(&self.node_path);
    }
}

#[derive(Debug, Clone)]
pub struct Staging {
    node_dir: PathBuf,
    service_dir: PathBuf,
    cap_bytes: u64,
    write_bound: Duration,
    /// TEST SEAM: a pause inside the blocking write, to stand in for a slow filesystem. Zero outside tests.
    #[doc(hidden)]
    pub write_delay: Duration,
}

/// Whether `name` is one this node staged: 32 hex characters, a known extension, optionally `.part`.
pub fn is_staged_name(name: &str) -> bool {
    let name = name.strip_suffix(".part").unwrap_or(name);
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    stem.len() == 32
        && stem
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && STAGED_EXTENSIONS.contains(&ext)
}

impl Staging {
    pub fn new(node_dir: &Path, service_dir: &Path, cap_bytes: u64) -> Self {
        Staging {
            node_dir: node_dir.to_path_buf(),
            service_dir: service_dir.to_path_buf(),
            cap_bytes,
            write_bound: WRITE_BOUND,
            write_delay: Duration::ZERO,
        }
    }

    /// A shorter write bound (tests).
    pub fn with_write_bound(mut self, bound: Duration) -> Self {
        self.write_bound = bound;
        self
    }

    /// Bytes staged right now (files only; the directory is flat).
    pub fn used_bytes(&self) -> u64 {
        std::fs::read_dir(&self.node_dir)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| e.metadata().ok())
                    .filter(|m| m.is_file())
                    .map(|m| m.len())
                    .sum()
            })
            .unwrap_or(0)
    }

    /// Start-up sweep: remove leftovers of a previous run. Only names this node stages are touched, so a wrongly
    /// set directory loses nothing that is not ours.
    pub fn sweep(&self) -> usize {
        let mut removed = 0;
        if let Ok(rd) = std::fs::read_dir(&self.node_dir) {
            for e in rd.flatten() {
                let ours = e.file_name().to_str().is_some_and(is_staged_name);
                let is_file = e.file_type().map(|t| t.is_file()).unwrap_or(false);
                if ours && is_file && std::fs::remove_file(e.path()).is_ok() {
                    removed += 1;
                }
            }
        }
        removed
    }

    /// Write `bytes` under a random name with extension `ext`, bounded by [`WRITE_BOUND`]. Whatever happens to
    /// this future (an error, the bound, a drop by an outer timeout), no file is left behind.
    pub async fn stage(&self, bytes: &[u8], ext: &str) -> Result<Staged, StageError> {
        if self.used_bytes().saturating_add(bytes.len() as u64) > self.cap_bytes {
            return Err(StageError::Full);
        }
        let name = format!("{}.{ext}", uuid::Uuid::new_v4().simple());
        let guard = Staged {
            node_path: self.node_dir.join(&name),
            part_path: self.node_dir.join(format!("{name}.part")),
            service_path: self.service_dir.join(&name).to_string_lossy().into_owned(),
            abandoned: Arc::new(AtomicBool::new(false)),
        };
        let (part, dest, abandoned, delay, data) = (
            guard.part_path.clone(),
            guard.node_path.clone(),
            guard.abandoned.clone(),
            self.write_delay,
            bytes.to_vec(),
        );
        let write = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
            // group-readable: the service runs as the same user id, or shares the group of a 0770 directory
            let result = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o640)
                .open(&part)
                .and_then(|mut f| f.write_all(&data))
                .and_then(|_| std::fs::rename(&part, &dest));
            // the guard may have gone while this thread was writing: then nobody else will delete these
            if result.is_err() || abandoned.load(Ordering::SeqCst) {
                let _ = std::fs::remove_file(&part);
                let _ = std::fs::remove_file(&dest);
            }
            result
        });
        let io = |e: std::io::Error| {
            if e.kind() == std::io::ErrorKind::StorageFull || e.raw_os_error() == Some(libc::ENOSPC)
            {
                StageError::Full
            } else {
                StageError::Io(e.kind().to_string())
            }
        };
        match tokio::time::timeout(self.write_bound, write).await {
            Ok(Ok(Ok(()))) => Ok(guard),
            Ok(Ok(Err(e))) => Err(io(e)),
            Ok(Err(_)) => Err(StageError::Io("write task failed".to_string())),
            // dropping `guard` here marks the write abandoned
            Err(_) => Err(StageError::Io("write timed out".to_string())),
        }
    }
}
