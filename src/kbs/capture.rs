// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Per-request capture (design §12.1, D12): the day-one runbook pins `hwmodel`,
//! driver, VBIOS, TCB status and advisory ids FROM these files.
//!
//! `capture/<ring>/<unix_ms>-<model_id[..8]>/` holding `request.json` (the body as
//! received), `nras.json` (the raw NRAS response, when one was made),
//! `verified.json` (the verified report, the extracted events, decoded EAT claims)
//! and `decision.txt` (kind + every failed row or "released", per-step timings).
//!
//! Two rings, each evicting only its own oldest: `preverify/` (refused before or at
//! the CPU half: malformed, nonce, pre-filter, TDX verify failure) and `verified/`
//! (everything that reached the GPU half), so forged or malformed traffic can never
//! evict the first real EAT. Each file ≤ 1 MiB (over it: `<name>.truncated`); never the
//! DEK, never the wrapped key.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Two requests in one millisecond must not share a directory.
static SEQ: AtomicU64 = AtomicU64::new(0);

pub const FILE_CAP: usize = 1_048_576;
const TRUNCATED: &[u8] = b"\n[truncated at 1 MiB]\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ring {
    Preverify,
    Verified,
}

impl Ring {
    pub fn dir_name(self) -> &'static str {
        match self {
            Ring::Preverify => "preverify",
            Ring::Verified => "verified",
        }
    }
}

/// What one request leaves behind.
#[derive(Debug, Default, Clone)]
pub struct CaptureRecord {
    pub request: Vec<u8>,
    pub nras: Option<Vec<u8>>,
    pub verified: Option<serde_json::Value>,
    pub decision: String,
}

pub struct Capture {
    root: PathBuf,
    /// `0` disables capture.
    max_verified: usize,
    max_preverify: usize,
    /// Evictions are serialised: two concurrent ones would each remove an "oldest"
    /// and drop the ring below its cap.
    evict_lock: std::sync::Mutex<()>,
}

impl Capture {
    /// Also removes any `.tmp-` directory a killed writer left in either ring: no
    /// writer is live at construction, and eviction never sees those names.
    pub fn new(root: PathBuf, max_verified: usize, max_preverify: usize) -> Self {
        let c = Self {
            root,
            max_verified,
            max_preverify,
            evict_lock: std::sync::Mutex::new(()),
        };
        let swept = c.sweep_orphans();
        if swept > 0 {
            tracing::warn!(
                swept,
                "removed capture directories left by an interrupted write"
            );
        }
        c
    }

    /// Remove `.tmp-` directories in both rings; returns how many.
    pub fn sweep_orphans(&self) -> usize {
        let mut n = 0;
        for ring in [Ring::Preverify, Ring::Verified] {
            let Ok(rd) = std::fs::read_dir(self.root.join(ring.dir_name())) else {
                continue;
            };
            for e in rd.filter_map(|e| e.ok()) {
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if is_dir
                    && e.file_name().to_string_lossy().starts_with(".tmp-")
                    && std::fs::remove_dir_all(e.path()).is_ok()
                {
                    n += 1;
                }
            }
        }
        n
    }

    pub fn enabled(&self) -> bool {
        self.max_verified > 0 || self.max_preverify > 0
    }

    /// Write the record into `ring`; evict the ring's oldest beyond its cap.
    /// Returns the directory written, if any. Never fails the request: errors are
    /// logged.
    pub fn write(&self, ring: Ring, model_id: &[u8; 32], rec: &CaptureRecord) -> Option<PathBuf> {
        let cap = match ring {
            Ring::Preverify => self.max_preverify,
            Ring::Verified => self.max_verified,
        };
        if cap == 0 {
            return None;
        }
        let ring_dir = self.root.join(ring.dir_name());
        let name = format!(
            "{:013}-{:06}-{}",
            now_ms(),
            SEQ.fetch_add(1, Ordering::Relaxed) % 1_000_000,
            hex::encode(&model_id[..8])
        );
        // Written under a temp name and renamed when complete: eviction (which sees
        // only final names) can never remove a directory another request is still
        // filling.
        let dir = ring_dir.join(&name);
        let tmp = ring_dir.join(format!(".tmp-{name}"));
        if let Err(e) = self
            .write_files(&tmp, rec)
            .and_then(|()| std::fs::rename(&tmp, &dir))
        {
            tracing::warn!(dir = %dir.display(), error = %e, "capture write failed (non-fatal)");
            let _ = std::fs::remove_dir_all(&tmp);
            return None;
        }
        {
            let _g = self.evict_lock.lock().unwrap_or_else(|p| p.into_inner());
            if let Err(e) = evict_oldest(&ring_dir, cap) {
                tracing::warn!(dir = %ring_dir.display(), error = %e, "capture eviction failed (non-fatal)");
            }
        }
        Some(dir)
    }

    fn write_files(&self, dir: &Path, rec: &CaptureRecord) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        write_capped(&dir.join("request.json"), &rec.request)?;
        if let Some(n) = &rec.nras {
            write_capped(&dir.join("nras.json"), n)?;
        }
        if let Some(v) = &rec.verified {
            write_capped(&dir.join("verified.json"), &serde_json::to_vec_pretty(v)?)?;
        }
        write_capped(&dir.join("decision.txt"), rec.decision.as_bytes())?;
        Ok(())
    }
}

/// Over the cap the file is written as `<name>.truncated` (the first MiB plus a
/// marker): a `.json` name must never hold something a JSON reader cannot parse.
fn write_capped(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if bytes.len() <= FILE_CAP {
        std::fs::write(path, bytes)
    } else {
        let mut v = bytes[..FILE_CAP].to_vec();
        v.extend_from_slice(TRUNCATED);
        let mut name = path.as_os_str().to_owned();
        name.push(".truncated");
        std::fs::write(name, v)
    }
}

/// Remove the oldest directories (by name, which starts with the ms timestamp)
/// until at most `cap` remain.
fn evict_oldest(ring_dir: &Path, cap: usize) -> std::io::Result<()> {
    let mut names: Vec<String> = std::fs::read_dir(ring_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !n.starts_with(".tmp-"))
        .collect();
    if names.len() <= cap {
        return Ok(());
    }
    names.sort();
    let excess = names.len() - cap;
    for n in names.into_iter().take(excess) {
        std::fs::remove_dir_all(ring_dir.join(n))?;
    }
    Ok(())
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
