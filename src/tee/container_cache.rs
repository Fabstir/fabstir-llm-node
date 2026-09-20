// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P5.5 (design S1, S1a, S1c, S1d) — the ciphertext cache on the CVM's
//! disk: one container file per process, keyed by the policy's `encrypted_ref`,
//! written through a `.part` and an atomic rename, validated by its header
//! BEFORE any broker traffic, and sized against the filesystem before the
//! first byte arrives.
//!
//! Nothing here is a trust boundary: the container is ciphertext anyone can
//! fetch from the public blob URL, and its header is unauthenticated. The
//! header check compares a trusted spec (the validated policy) with the file
//! so a stale seal is a MISS (one download) rather than a nonce and an NRAS
//! round trip spent on a container the AEAD would have refused.

use crate::tee::container::{ContainerHeader, HEADER_LEN};
use crate::tee::types::{TeeError, TeeResult};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

/// The loader's space check, invoked by a [`BlobSource`](crate::tee::model_source::BlobSource)
/// once the body length is known and before any byte is written. The `'a`
/// lets the loader pass a closure borrowing its directories; without it the
/// object defaults to `'static` and the borrow is refused.
pub type SpaceCheck<'a> = dyn Fn(u64) -> TeeResult<()> + Send + Sync + 'a;

/// The loader's header check, invoked with the first [`HEADER_LEN`] bytes of
/// the body as soon as they have arrived (before they are written): a
/// container sealed for another model or policy is refused after 98 bytes,
/// not after tens of GB (the day-one mistake: a re-seal not uploaded yet).
pub type HeadCheck<'a> = dyn Fn(&[u8]) -> TeeResult<()> + Send + Sync + 'a;

/// What a [`BlobSource`](crate::tee::model_source::BlobSource) consults while
/// streaming a container.
#[derive(Clone, Copy)]
pub struct FetchHooks<'a> {
    pub on_length: &'a SpaceCheck<'a>,
    pub on_head: &'a HeadCheck<'a>,
}

pub fn accept_length(_: u64) -> TeeResult<()> {
    Ok(())
}

pub fn accept_head(_: &[u8]) -> TeeResult<()> {
    Ok(())
}

impl FetchHooks<'static> {
    /// Hooks that accept everything (tests, plain fetches).
    pub fn accept_all() -> Self {
        FetchHooks {
            on_length: &accept_length,
            on_head: &accept_head,
        }
    }
}

/// The loader's head hook: decode the first bytes and refuse a decodable
/// header bound to another model or policy; anything else (short, garbage)
/// is left to the decrypt, which refuses it as `Crypto`.
pub fn refuse_mismatched_head(
    head: &[u8],
    encrypted_path: &str,
    model_id: &[u8; 32],
    policy_hash: &[u8; 32],
) -> TeeResult<()> {
    if head.len() < HEADER_LEN {
        return Ok(());
    }
    if let Ok(h) = ContainerHeader::decode(&head[..HEADER_LEN]) {
        if h.model_id != *model_id || h.policy_hash != *policy_hash {
            return Err(TeeError::VerificationFailed(format!(
                "the blob at {encrypted_path} is sealed for model {}/policy {}; this boot pinned \
                 {}/{} (the re-seal after a policy pin is not uploaded yet, or the ref points \
                 at another model)",
                hex::encode(h.model_id),
                hex::encode(h.policy_hash),
                hex::encode(model_id),
                hex::encode(policy_hash)
            )));
        }
    }
    Ok(())
}

/// Free space and device identity of the filesystem holding a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FsSpace {
    /// `st_dev` of the directory (two directories on one filesystem share it).
    pub dev: u64,
    /// Bytes available to an unprivileged writer (`f_bavail × f_frsize`;
    /// excludes ext4's root-reserved blocks, so it under-reports for root by
    /// the reserve, which is the safe direction).
    pub avail: u64,
}

/// How the loader learns [`FsSpace`] for a directory; injectable for tests.
pub type SpaceProbe<'a> = dyn Fn(&Path) -> TeeResult<FsSpace> + Send + Sync + 'a;

/// [`FsSpace`] from `statvfs(2)` and `stat(2)`.
pub fn real_space(dir: &Path) -> TeeResult<FsSpace> {
    let dev = std::fs::metadata(dir)?.dev();
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).map_err(|_| {
        TeeError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path contains a NUL byte",
        ))
    })?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated path and `st` is a properly sized
    // out-parameter; statvfs has no other preconditions.
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut st) };
    if rc != 0 {
        return Err(TeeError::Io(std::io::Error::last_os_error()));
    }
    Ok(FsSpace {
        dev,
        avail: (st.f_bavail as u64).saturating_mul(st.f_frsize as u64),
    })
}

/// Design S1c: before the first byte, the container dir and the decrypt dir
/// must each hold `len` bytes; when they sit on ONE filesystem (the production
/// disk-mode layout: both volumes on docker's data-root) that filesystem must
/// hold `2 × len`, because the ciphertext and the plaintext both land on it.
pub fn check_space_for_download(
    probe: &SpaceProbe<'_>,
    container_dir: &Path,
    decrypt_dir: &Path,
    len: u64,
) -> TeeResult<()> {
    let c = probe(container_dir)?;
    let d = probe(decrypt_dir)?;
    if c.dev == d.dev {
        let need = len.saturating_mul(2);
        if c.avail < need {
            return Err(TeeError::Fetch(format!(
                "container of {len} bytes needs {need} bytes on the same filesystem as the \
                 decrypt dir ({} and {} share one filesystem: ciphertext + plaintext); it has \
                 {} — resize the volume or the disk",
                container_dir.display(),
                decrypt_dir.display(),
                c.avail
            )));
        }
        return Ok(());
    }
    if c.avail < len {
        return Err(TeeError::Fetch(format!(
            "container of {len} bytes needs {len} bytes in {}; it has {} — resize the volume",
            container_dir.display(),
            c.avail
        )));
    }
    check_space_for_plaintext(probe, decrypt_dir, len)
}

/// Design S1c on a cache HIT: only the plaintext is written, so only the
/// decrypt dir is checked, against the cached container's length (an upper
/// bound on the plaintext).
pub fn check_space_for_plaintext(
    probe: &SpaceProbe<'_>,
    decrypt_dir: &Path,
    len: u64,
) -> TeeResult<()> {
    let d = probe(decrypt_dir)?;
    if d.avail < len {
        return Err(TeeError::Fetch(format!(
            "plaintext of up to {len} bytes needs {len} bytes in {}; it has {} — in tmpfs mode \
             raise shm_size, in disk mode resize the volume",
            decrypt_dir.display(),
            d.avail
        )));
    }
    Ok(())
}

/// The cache file's stem for a policy's `encrypted_ref`: the first 16 hex of
/// its SHA-256. A new URL is a new key (the old file is pruned, S1d); a new
/// seal at the SAME URL is caught by the header check (S1a).
pub fn cache_key(encrypted_path: &str) -> String {
    hex::encode(Sha256::digest(encrypted_path.as_bytes()))[..16].to_string()
}

/// `<key>.enc` under the container dir.
pub fn cache_path(container_dir: &Path, key: &str) -> PathBuf {
    container_dir.join(format!("{key}.enc"))
}

/// `<dest>.<8 hex>.part`: a per-attempt name, so two concurrent downloads of
/// one container never share a partial file.
pub fn part_path(dest: &Path) -> PathBuf {
    let mut rnd = [0u8; 4];
    OsRng.fill_bytes(&mut rnd);
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dest.with_file_name(format!("{name}.{}.part", hex::encode(rnd)))
}

/// Is `name` a partial download (`*.part`)? The boot sweep removes every one
/// (no loader can be running then); a loader's own prune never touches a
/// same-key `.part`, which may belong to a concurrent load.
pub fn is_part_name(name: &str) -> bool {
    name.ends_with(".part")
}

/// Owns a `.part` file: dropped un-disarmed (an error, a cancelled future) it
/// unlinks the file. The binary's start-up stop `_exit`s without running
/// destructors; the `.part` it leaves is the boot sweep's.
pub struct PartGuard {
    path: PathBuf,
    armed: bool,
}

impl PartGuard {
    /// Create `<dest>.<8 hex>.part` exclusively, `0600`, and own it.
    pub fn create(dest: &Path) -> TeeResult<(Self, std::fs::File)> {
        let path = part_path(dest);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        Ok((Self { path, armed: true }, file))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Atomically rename the completed `.part` over `dest` and disarm.
    pub fn finish(mut self, dest: &Path) -> TeeResult<()> {
        std::fs::rename(&self.path, dest)?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for PartGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Write `bytes` to `dest` through a `.part` and a rename: the ONE helper both
/// the default `get_file_to` body and the streaming override use, so a reader
/// of `dest` never sees a truncated file (a direct truncating write under a
/// concurrent same-key reader would surface as that reader's `Crypto`).
pub fn write_via_part(dest: &Path, bytes: &[u8], hooks: FetchHooks<'_>) -> TeeResult<u64> {
    (hooks.on_length)(bytes.len() as u64)?;
    (hooks.on_head)(&bytes[..bytes.len().min(HEADER_LEN)])?;
    let (guard, mut file) = PartGuard::create(dest)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    guard.finish(dest)?;
    Ok(bytes.len() as u64)
}

/// What the cached container's header says about the spec (design S1a).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderCheck {
    /// `model_id ‖ policy_hash` equal the spec's.
    Matches,
    /// A decodable header bound to another model or policy (a stale seal after
    /// a policy pin; a swap): on a cache hit a MISS, on a fresh download a
    /// refusal before any broker traffic.
    Mismatch {
        model_id: [u8; 32],
        policy_hash: [u8; 32],
    },
    /// Short, bad magic, bad version: a MISS on a cache hit; a fresh file falls
    /// through to the decrypt, which refuses it as `Crypto`.
    Undecodable,
}

/// Read the 98-byte header of the OPEN `file` (from its start; the position
/// is restored to 0) and compare it with the spec. Only a real I/O failure
/// propagates; every decode outcome is a value, never a refusal, so a garbage
/// cache file can never park the CVM. Takes the handle, not a path, so the
/// same descriptor that passed the check is the one the decrypt reads: a
/// concurrent same-key load unlinking the path in between cannot turn this
/// load's read into `NotFound`.
pub fn header_check(
    file: &mut std::fs::File,
    model_id: &[u8; 32],
    policy_hash: &[u8; 32],
) -> TeeResult<HeaderCheck> {
    file.seek(SeekFrom::Start(0))?;
    let mut buf = vec![0u8; HEADER_LEN];
    let mut got = 0;
    let outcome = loop {
        if got == HEADER_LEN {
            break match ContainerHeader::decode(&buf) {
                Ok(h) if h.model_id == *model_id && h.policy_hash == *policy_hash => {
                    HeaderCheck::Matches
                }
                Ok(h) => HeaderCheck::Mismatch {
                    model_id: h.model_id,
                    policy_hash: h.policy_hash,
                },
                Err(_) => HeaderCheck::Undecodable,
            };
        }
        let n = file.read(&mut buf[got..])?;
        if n == 0 {
            break HeaderCheck::Undecodable;
        }
        got += n;
    };
    file.seek(SeekFrom::Start(0))?;
    Ok(outcome)
}

/// Open the cache file for reading; `None` when there is none (no cache yet,
/// or a concurrent same-key load judged it stale and unlinked it first).
pub fn open_cache(path: &Path) -> TeeResult<Option<std::fs::File>> {
    match std::fs::File::open(path) {
        Ok(f) => Ok(Some(f)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(TeeError::Io(e)),
    }
}

/// Design S1d: one model per process, one container in the cache. Remove every
/// entry of `dir` whose name does not start with `<key>.` for any key in
/// `keep` (the loads in flight: never a concurrent loader's `.part` or fresh
/// `.enc`). Regular files and symlinks are unlinked (a symlink is never
/// followed); a directory is left alone and logged. Returns how many entries
/// were removed.
pub fn prune_other_keys(dir: &Path, keep: &std::collections::BTreeSet<String>) -> TeeResult<usize> {
    let mut n = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if keep.iter().any(|k| name.starts_with(&format!("{k}."))) {
            continue;
        }
        let ft = entry.file_type()?;
        if ft.is_dir() {
            tracing::warn!(
                target: "tee",
                "container dir {} holds a directory `{name}`; leaving it (the cache holds files only)",
                dir.display()
            );
            continue;
        }
        std::fs::remove_file(entry.path())?;
        n += 1;
    }
    Ok(n)
}

/// The outcome of the container step of one load (design S4): the word in the
/// log line `container cache: <path> (<outcome>)` and what tests assert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerOutcome {
    /// The cached container matched its header and was decrypted.
    Hit,
    /// No cache; downloaded once.
    Miss,
    /// A cached container whose header did not match (or could not be
    /// decoded): deleted and downloaded, before any challenge.
    Stale,
    /// The cached container passed its header but failed a content check at
    /// decrypt: deleted and downloaded ONCE more under the same DEK.
    Retried,
}

impl std::fmt::Display for ContainerOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Hit => "hit",
            Self::Miss => "miss",
            Self::Stale => "stale",
            Self::Retried => "retried",
        })
    }
}

/// A `Write` that feeds every byte to a SHA-256 hasher on its way to `inner`,
/// so the plaintext's digest comes out of the decrypt for free (design S2)
/// instead of a second full read of the file.
pub struct Sha256Tee<W: Write> {
    inner: W,
    hasher: Sha256,
}

impl<W: Write> Sha256Tee<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
        }
    }

    /// The digest of everything written so far, and the inner writer.
    pub fn finish(self) -> ([u8; 32], W) {
        (self.hasher.finalize().into(), self.inner)
    }
}

impl<W: Write> Write for Sha256Tee<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
