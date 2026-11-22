// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Cleared results (node 8.65.4): the SHA-256 of the first artefact (the mp4) of every result this host delivered
//! after its own checks, with how it was cleared. The only way in is the host delivering that exact file
//! ([`super::ContentChecks::record_cleared`], called by the generation task just before `ltx_complete`), so a
//! client cannot add an entry, and a file that differs by one byte has another hash. An input that is
//! byte-identical to an entry was judged (or derived from judged inputs) when this host delivered it, and is not
//! sent to the classifier a second time. Bounded (7 days, [`MAX_ENTRIES`]), written atomically, and a missing or
//! damaged file means an empty record: everything is then simply checked.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// How long an entry is honoured.
pub const KEEP_SECS: u64 = 7 * 24 * 3600;
/// The most entries kept; the oldest go first.
pub const MAX_ENTRIES: usize = 4096;

/// How a delivered result was cleared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cleared {
    /// The result check ran and passed (`enforce`).
    C5Pass,
    /// The 8.65.3 Cut-out rule skipped it.
    FixedGraph,
    /// The VFX Passes exemption skipped it.
    Passes,
}

impl Cleared {
    pub fn as_str(self) -> &'static str {
        match self {
            Cleared::C5Pass => "c5-pass",
            Cleared::FixedGraph => "fixed-graph",
            Cleared::Passes => "passes",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        [Cleared::C5Pass, Cleared::FixedGraph, Cleared::Passes]
            .into_iter()
            .find(|c| c.as_str() == s)
    }
}

pub struct ClearedResults {
    path: Option<PathBuf>,
    entries: Mutex<BTreeMap<String, (Cleared, u64)>>,
}

impl ClearedResults {
    /// Load the record at `path`; a missing, unreadable or damaged file is an empty record.
    pub fn load(path: Option<PathBuf>) -> Self {
        let entries = path.as_deref().and_then(read).unwrap_or_default();
        ClearedResults {
            path,
            entries: Mutex::new(entries),
        }
    }

    /// How `sha256` (lower-case hex) was cleared, if it was and the entry has not expired at `now` (Unix seconds).
    pub fn lookup(&self, sha256: &str, now: u64) -> Option<Cleared> {
        let g = self.entries.lock().ok()?;
        let (how, at) = g.get(sha256)?;
        (now.saturating_sub(*at) < KEEP_SECS).then_some(*how)
    }

    /// Record `sha256` as cleared at `now`; drops expired entries, keeps the newest [`MAX_ENTRIES`], writes the
    /// file atomically. The in-memory record is updated even when the write fails.
    pub fn insert(&self, sha256: &str, how: Cleared, now: u64) -> std::io::Result<()> {
        let snapshot = {
            let mut g = self
                .entries
                .lock()
                .map_err(|_| std::io::Error::other("poisoned"))?;
            g.retain(|_, (_, at)| now.saturating_sub(*at) < KEEP_SECS);
            g.insert(sha256.to_string(), (how, now));
            while g.len() > MAX_ENTRIES {
                let oldest = g
                    .iter()
                    .min_by_key(|(_, (_, at))| *at)
                    .map(|(k, _)| k.clone());
                match oldest {
                    Some(k) => g.remove(&k),
                    None => break,
                };
            }
            g.clone()
        };
        match &self.path {
            Some(p) => write_atomic(p, &snapshot),
            None => Ok(()),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.lock().map(|g| g.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn read(path: &std::path::Path) -> Option<BTreeMap<String, (Cleared, u64)>> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let mut out = BTreeMap::new();
    for e in v.get("entries")?.as_array()? {
        let sha = e.get("sha256")?.as_str()?;
        let how = Cleared::parse(e.get("how")?.as_str()?)?;
        let at = e.get("at")?.as_u64()?;
        if sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            out.insert(sha.to_ascii_lowercase(), (how, at));
        }
    }
    Some(out)
}

fn write_atomic(
    path: &std::path::Path,
    entries: &BTreeMap<String, (Cleared, u64)>,
) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let list: Vec<serde_json::Value> = entries
        .iter()
        .map(|(sha, (how, at))| serde_json::json!({"sha256": sha, "how": how.as_str(), "at": at}))
        .collect();
    let body = serde_json::to_vec(&serde_json::json!({"v": 1, "entries": list}))?;
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(&body)?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)
}
