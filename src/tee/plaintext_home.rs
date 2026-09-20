// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 P5.5 (design S3, S3a) — where the decrypted plaintext may live.
//!
//! Two homes, chosen on the deploy form by ONE value (`TEE_DECRYPT_ON_DISK`):
//! tmpfs (today's rule: the pages die with the mount) or the CVM's
//! dstack-encrypted data disk (a named volume, `TEE_PLAINTEXT_VOLUME`), which
//! lets llama.cpp's mmap of a model larger than RAM stay evictable page cache
//! while every layer sits in VRAM. Disk mode is accepted only when the
//! directory's block device is a dm-crypt LUKS device (its sysfs `dm/uuid`
//! begins `CRYPT-LUKS`), directly or through one `slaves/` hop (LVM on LUKS).
//! `/dev/mapper/*` is device-mapper generally (LVM, multipath, dstack's
//! dm-verity rootfs), so the mount source string is logged, never trusted.
//!
//! A disk survives a reboot, so the start-up sweep removes what a SIGKILL
//! mid-serve left behind: only the node's own file names, judged by
//! `DirEntry::file_type()` (a symlink bearing one is unlinked, never opened).

use crate::tee::types::{TeeError, TeeResult};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// What the kernel says about the filesystem under a directory: the
/// `/proc/mounts` source and type (informational) and, when the device is
/// device-mapper, its sysfs `dm/uuid` and those of its `slaves/`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MountInfo {
    pub source: String,
    pub fstype: String,
    /// `st_dev` of the directory, as `major:minor`.
    pub dev: String,
    /// `/sys/dev/block/<dev>/dm/uuid`, if the device is device-mapper.
    pub dm_uuid: Option<String>,
    /// `/sys/dev/block/<dev>/slaves/*/dm/uuid`, for LVM-on-LUKS.
    pub slave_uuids: Vec<String>,
}

impl std::fmt::Display for MountInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "source={} type={} dev={} dm_uuid={} slaves=[{}]",
            self.source,
            self.fstype,
            self.dev,
            self.dm_uuid.as_deref().unwrap_or("-"),
            self.slave_uuids.join(",")
        )
    }
}

/// The `/proc/mounts` `(source, type)` of the deepest mount containing `canon`
/// (on equal depth the LATER entry wins: an over-mount is effective in list
/// order). Pure over the mounts text, for tests.
pub fn mount_entry_from_mounts(canon: &Path, mounts: &str) -> Option<(String, String)> {
    let mut best: Option<(usize, String, String)> = None;
    for line in mounts.lines() {
        let mut cols = line.split_whitespace();
        let (src, mount_point, fstype) = match (cols.next(), cols.next(), cols.next()) {
            (Some(s), Some(m), Some(f)) => (s, m, f),
            _ => continue,
        };
        let mp = Path::new(mount_point);
        if canon.starts_with(mp) {
            let depth = mp.components().count();
            if best.as_ref().is_none_or(|(d, _, _)| depth >= *d) {
                best = Some((depth, src.to_string(), fstype.to_string()));
            }
        }
    }
    best.map(|(_, s, t)| (s, t))
}

fn read_trimmed(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Read [`MountInfo`] for `dir` from `/proc/mounts` and sysfs. Never trusts
/// the source string; the dm uuid is the machine-checkable fact.
pub fn mount_info(dir: &Path) -> TeeResult<MountInfo> {
    let canon = std::fs::canonicalize(dir)?;
    let meta = std::fs::metadata(&canon)?;
    let dev = meta.dev();
    // `libc::major`/`minor`, never a hand-rolled shift: large minors are
    // split across the high bits.
    let major = libc::major(dev);
    let minor = libc::minor(dev);
    let dev_s = format!("{major}:{minor}");
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    let (source, fstype) = mount_entry_from_mounts(&canon, &mounts)
        .unwrap_or_else(|| ("?".to_string(), "?".to_string()));
    let sys = Path::new("/sys/dev/block").join(&dev_s);
    let dm_uuid = read_trimmed(&sys.join("dm/uuid"));
    let mut slave_uuids = Vec::new();
    if let Ok(rd) = std::fs::read_dir(sys.join("slaves")) {
        for e in rd.flatten() {
            if let Some(u) = read_trimmed(&e.path().join("dm/uuid")) {
                slave_uuids.push(u);
            }
        }
    }
    slave_uuids.sort();
    Ok(MountInfo {
        source,
        fstype,
        dev: dev_s,
        dm_uuid,
        slave_uuids,
    })
}

/// cryptsetup names a LUKS mapping `CRYPT-LUKS2-<uuid>-<name>` (LUKS1:
/// `CRYPT-LUKS1-…`); dm-verity is `CRYPT-VERITY-…`, an LV is `LVM-…`.
const LUKS_PREFIX: &str = "CRYPT-LUKS";

/// Design S3, the pure rule. `disk_mode` false = today's tmpfs rule (mount
/// type `tmpfs`, else today's message). `disk_mode` true = the directory's
/// device must be LUKS: `dm_uuid` begins `CRYPT-LUKS`, or begins `LVM-` and a
/// slave's uuid begins `CRYPT-LUKS`. tmpfs and overlay are refused with their
/// own messages (they have no block device at all); a plain block device, a
/// verity device or an LV with no LUKS slave are "not on a LUKS device".
pub fn home_rule(
    disk_mode: bool,
    dir: &Path,
    fstype: &str,
    dm_uuid: Option<&str>,
    slave_uuids: &[String],
) -> TeeResult<()> {
    if !disk_mode {
        if fstype == "tmpfs" {
            return Ok(());
        }
        return Err(TeeError::VerificationFailed(format!(
            "TEE_DECRYPT_DIR {} is not tmpfs: the attested path decrypts only to memory \
             (set it to /dev/shm, sized for the model); refusing to start",
            dir.display()
        )));
    }
    match fstype {
        "tmpfs" => {
            return Err(TeeError::VerificationFailed(format!(
                "TEE_DECRYPT_ON_DISK is set but TEE_PLAINTEXT_VOLUME {} is RAM (tmpfs): disk \
                 mode asked for but the directory is RAM; mount the plaintext volume there or \
                 unset TEE_DECRYPT_ON_DISK; refusing to start",
                dir.display()
            )))
        }
        "overlay" => {
            return Err(TeeError::VerificationFailed(format!(
                "TEE_DECRYPT_ON_DISK is set but TEE_PLAINTEXT_VOLUME {} is the container's \
                 overlay filesystem: put it on a volume (the compose declares fabstir-plaintext); \
                 refusing to start",
                dir.display()
            )))
        }
        _ => {}
    }
    let luks = |u: &str| u.starts_with(LUKS_PREFIX);
    let ok = match dm_uuid {
        Some(u) if luks(u) => true,
        Some(u) if u.starts_with("LVM-") => slave_uuids.iter().any(|s| luks(s)),
        _ => false,
    };
    if ok {
        return Ok(());
    }
    Err(TeeError::VerificationFailed(format!(
        "TEE_DECRYPT_ON_DISK is set but TEE_PLAINTEXT_VOLUME {} is not on a LUKS device: \
         {fstype} on dm uuid {} (slaves: [{}]); disk mode needs dstack's encrypted data disk \
         (dm uuid CRYPT-LUKS…); refusing to start",
        dir.display(),
        dm_uuid.unwrap_or("none: a plain block device"),
        slave_uuids.join(",")
    )))
}

/// Is `name` one of the loader's own plaintext names: `<64hex>.<16hex>.gguf`
/// (`fresh_path`) or a write probe `.tee-write-probe.*`?
pub fn is_own_plaintext_name(name: &str) -> bool {
    if name.starts_with(".tee-write-probe.") {
        return true;
    }
    let Some(stem) = name.strip_suffix(".gguf") else {
        return false;
    };
    let Some((id, suffix)) = stem.split_once('.') else {
        return false;
    };
    let hex = |s: &str, n: usize| s.len() == n && s.chars().all(|c| c.is_ascii_hexdigit());
    hex(id, 64) && hex(suffix, 16)
}

/// Design S3a-3, one directory: remove every entry whose name `matches`,
/// judged by `DirEntry::file_type()`: a regular file is unlinked (zeroed
/// first only when `zero`, which the boot sweep never asks for: a file
/// matched by NAME may belong to another live process's mapping, and an
/// overwrite under a mapping is the one thing P3 forbids; on tmpfs the pages
/// die with the last reference, on the LUKS volume the layer below covers the
/// medium); a symlink is unlinked, never followed, and counts with 0 bytes; a
/// directory is left alone. Returns `(files, bytes)`. A missing directory is
/// `(0, 0)`.
pub fn sweep_dir(
    dir: &Path,
    matches: &dyn Fn(&str) -> bool,
    zero: bool,
) -> TeeResult<(usize, u64)> {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0)),
        Err(e) => return Err(TeeError::Io(e)),
    };
    let mut files = 0usize;
    let mut bytes = 0u64;
    for entry in rd {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !matches(&name) {
            continue;
        }
        let ft = entry.file_type()?;
        let path = entry.path();
        if ft.is_dir() {
            continue;
        }
        if ft.is_symlink() {
            std::fs::remove_file(&path)?;
            files += 1;
            continue;
        }
        let len = entry.metadata()?.len();
        if zero {
            crate::tee::model_source::secure_delete(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
        files += 1;
        bytes += len;
    }
    Ok((files, bytes))
}
