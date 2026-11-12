// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The repository (D12): immutable records keyed by manifest uuid, first write wins, written atomically
//! (staged in a private directory, then one `rename` into place; a rename onto an existing record fails).
//!
//! records/<uuid>/manifest.c2pa      the composed manifest as delivered (mp4 uuid box, or a sequence store)
//! records/<uuid>/meta.json          kind, class, has_visual_input, time, public, request body hash
//! records/<uuid>/fingerprint.pfp    packed MPEG-7 set (Picture outputs only)
//! records/<uuid>/frames-index.json  sequence manifests only

use fabstir_provenance::fingerprint::SignatureSet;
use fabstir_provenance::Class;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Mp4,
    Sequence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    pub kind: Kind,
    pub class: Class,
    pub has_visual_input: bool,
    pub time: u64,
    /// false when the manifest store carries a third-party manifest (D6: the public copy must not expose a
    /// camera original's credentials); such records are matchable but not served publicly.
    pub public: bool,
    /// SHA-256 of the sign request body: a retry with the same body gets this record back (contract 6).
    pub request_sha256: String,
}

pub struct Record {
    pub manifest: Vec<u8>,
    pub meta: Meta,
    pub fingerprint: Option<Vec<u8>>,
    pub frames_index: Option<Vec<u8>>,
}

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("record exists")]
    Exists,
    #[error("bad id")]
    BadId,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// One registered fingerprint held in memory for matching.
pub struct Entry {
    pub uuid: String,
    pub meta: Meta,
    pub set: SignatureSet,
}

pub struct Repo {
    root: PathBuf,
    /// Copy-on-write snapshot: a match runs on an `Arc` clone, holding no lock (a long query never blocks a sign).
    index: RwLock<Arc<Vec<Arc<Entry>>>>,
}

/// Lower-case hyphenated UUID only (ids are signer-issued v4).
pub fn valid_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}

impl Repo {
    /// Open for serving: also clears staging leftovers of an interrupted write (never part of a record).
    /// Only the `serve` process may call this; tools use [`Repo::open_shared`].
    pub fn open(root: &Path) -> anyhow::Result<Repo> {
        std::fs::create_dir_all(root.join("records"))?;
        std::fs::create_dir_all(root.join("staging"))?;
        for e in std::fs::read_dir(root.join("staging"))? {
            let _ = std::fs::remove_dir_all(e?.path());
        }
        Self::open_shared(root)
    }

    /// Open alongside a running server (canary, reconcile): touches nothing on disk.
    pub fn open_shared(root: &Path) -> anyhow::Result<Repo> {
        std::fs::create_dir_all(root.join("records"))?;
        std::fs::create_dir_all(root.join("staging"))?;
        let repo = Repo {
            root: root.to_path_buf(),
            index: RwLock::new(Arc::new(vec![])),
        };
        repo.load_index()?;
        Ok(repo)
    }

    fn load_index(&self) -> anyhow::Result<()> {
        let mut idx = vec![];
        for e in std::fs::read_dir(self.root.join("records"))? {
            let e = e?;
            let uuid = e.file_name().to_string_lossy().to_string();
            if !valid_uuid(&uuid) {
                continue;
            }
            let fp = e.path().join("fingerprint.pfp");
            if !fp.exists() {
                continue;
            }
            let meta: Meta = serde_json::from_slice(&std::fs::read(e.path().join("meta.json"))?)?;
            let set = SignatureSet::unpack(&std::fs::read(fp)?)
                .map_err(|e| anyhow::anyhow!("{uuid}: {e}"))?;
            idx.push(Arc::new(Entry { uuid, meta, set }));
        }
        *self.index.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(idx);
        Ok(())
    }

    fn dir(&self, uuid: &str) -> Result<PathBuf, RepoError> {
        if !valid_uuid(uuid) {
            return Err(RepoError::BadId);
        }
        Ok(self.root.join("records").join(uuid))
    }

    /// Store a record atomically; fails with `Exists` if any record has this id.
    pub fn put(&self, uuid: &str, rec: &Record) -> Result<(), RepoError> {
        let dest = self.dir(uuid)?;
        if dest.exists() {
            return Err(RepoError::Exists);
        }
        let set = match &rec.fingerprint {
            Some(f) => Some(SignatureSet::unpack(f).map_err(|_| RepoError::BadId)?),
            None => None,
        };
        let stage = self
            .root
            .join("staging")
            .join(format!("{uuid}.{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&stage)?;
        let write = |name: &str, bytes: &[u8]| -> Result<(), RepoError> {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(stage.join(name))?;
            f.write_all(bytes)?;
            f.sync_all()?;
            Ok(())
        };
        let r = (|| {
            write("manifest.c2pa", &rec.manifest)?;
            write(
                "meta.json",
                &serde_json::to_vec(&rec.meta).expect("meta serialises"),
            )?;
            if let Some(f) = &rec.fingerprint {
                write("fingerprint.pfp", f)?;
            }
            if let Some(i) = &rec.frames_index {
                write("frames-index.json", i)?;
            }
            std::fs::File::open(&stage)?.sync_all()?;
            // rename(2) of a directory onto an existing non-empty one fails (ENOTEMPTY/EEXIST): first write wins
            std::fs::rename(&stage, &dest).map_err(|e| {
                if dest.exists() {
                    RepoError::Exists
                } else {
                    RepoError::Io(e)
                }
            })
        })();
        if r.is_err() {
            let _ = std::fs::remove_dir_all(&stage);
        }
        r?;
        // index first: if the directory fsync below fails, a retry replays this record and it must be matchable
        if let Some(set) = set {
            let mut w = self.index.write().unwrap_or_else(|e| e.into_inner());
            let mut v: Vec<Arc<Entry>> = w.as_ref().clone();
            v.push(Arc::new(Entry {
                uuid: uuid.to_string(),
                meta: rec.meta.clone(),
                set,
            }));
            *w = Arc::new(v);
        }
        // the record's directory entry must be durable before the caller consumes the slot
        std::fs::File::open(self.root.join("records"))?.sync_all()?;
        Ok(())
    }

    pub fn get(&self, uuid: &str) -> Option<Record> {
        let d = self.dir(uuid).ok()?;
        let meta: Meta = serde_json::from_slice(&std::fs::read(d.join("meta.json")).ok()?).ok()?;
        Some(Record {
            manifest: std::fs::read(d.join("manifest.c2pa")).ok()?,
            meta,
            fingerprint: std::fs::read(d.join("fingerprint.pfp")).ok(),
            frames_index: std::fs::read(d.join("frames-index.json")).ok(),
        })
    }

    /// Run `f` over every registered fingerprint.
    pub fn with_index<R>(&self, f: impl FnOnce(&[Arc<Entry>]) -> R) -> R {
        let snap = self.index.read().unwrap_or_else(|e| e.into_inner()).clone();
        f(&snap)
    }

    /// The frames index of a public sequence record.
    pub fn get_frames_index(&self, uuid: &str) -> Option<Vec<u8>> {
        let d = self.dir(uuid).ok()?;
        let meta: Meta = serde_json::from_slice(&std::fs::read(d.join("meta.json")).ok()?).ok()?;
        if !meta.public || meta.kind != Kind::Sequence {
            return None;
        }
        std::fs::read(d.join("frames-index.json")).ok()
    }

    /// Manifest + meta only (the public read; never the fingerprint or index files).
    pub fn get_public(&self, uuid: &str) -> Option<(Vec<u8>, Meta)> {
        let d = self.dir(uuid).ok()?;
        let meta: Meta = serde_json::from_slice(&std::fs::read(d.join("meta.json")).ok()?).ok()?;
        if !meta.public {
            return None;
        }
        Some((std::fs::read(d.join("manifest.c2pa")).ok()?, meta))
    }
}
