// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Policy schema 3, the broker's side (`docs/development/IMPLEMENTATION-POLICY-V3-
//! HOST-PROFILES.md` D6–D10, D12): the per-root authority bundle loader, the
//! persisted high-water mark (HWM) against rollback, the write-once archive of
//! accepted bundles, the cached removed-profile ledger, and the candidate
//! filter.
//!
//! One file per root, `{KBS_AUTHORITY_DIR}/{root}.json` (`root` = 40 lowercase
//! hex, no `0x`), read with ONE read and ONE parse per request, so a request never
//! sees a new manifest with an old set (D6). Error kinds (D12): bundle missing =
//! `no_provider`; unparsable JSON = `fault`; wrong `doc`/schema, a rule, a
//! signature, a root equality, the HWM or a window = `verification`; HWM or
//! archive IO = `fault`.

use crate::kbs::error::KbsError;
use crate::tee::host_profile::{
    document_digest, parse_bundle, verify_manifest, verify_set, AuthorityBundle, AuthorityManifest,
    Counted, HostProfile, HostProfileSet, SignedAuthorityManifest, SignedHostProfileSet,
    MAX_ISSUED_AT_SKEW,
};
use crate::tee::http_sources::MAX_POLICY_BYTES;
use crate::tee::types::{HostAuthority, Policy, TeeError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// A counter may move at most this far above the HWM in one load (D10): a
/// compromised signer or a typo cannot burn the counter.
pub const MAX_JUMP: u64 = 16;

/// The persisted high-water mark of one root (D10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hwm {
    pub manifest_version: u32,
    /// Hex SHA-256 of the manifest's canonical unsigned body.
    pub manifest_digest: String,
    pub epoch: u64,
    /// Hex SHA-256 of the set's canonical unsigned body.
    pub set_digest: String,
}

/// `0x` + 40 hex → the 40 hex the file names use.
pub fn root_key(root: &str) -> &str {
    root.strip_prefix("0x").unwrap_or(root)
}

/// Compare `cand` with `current` (D10). Below on either counter refuses; equal
/// requires an equal digest; above is bounded by [`MAX_JUMP`] unless there is
/// no HWM yet (first load). Returns the HWM to persist (the max of both).
pub fn decide(root: &str, current: Option<&Hwm>, cand: &Hwm) -> Result<Hwm, KbsError> {
    let Some(cur) = current else {
        return Ok(cand.clone());
    };
    let step = |what: &str, cur_v: u64, cand_v: u64, cur_d: &str, cand_d: &str| {
        if cand_v < cur_v {
            return Err(KbsError::verification(format!(
                "authority {root}: {what} {cand_v} is below the high-water mark {cur_v} \
                 (an older bundle was served after a newer one was loaded)"
            )));
        }
        if cand_v == cur_v && cand_d != cur_d {
            return Err(KbsError::verification(format!(
                "authority {root}: {what} {cand_v} was already loaded with a different body \
                 (digest {} != {})",
                &cand_d[..16.min(cand_d.len())],
                &cur_d[..16.min(cur_d.len())]
            )));
        }
        if cand_v > cur_v.saturating_add(MAX_JUMP) {
            return Err(KbsError::verification(format!(
                "authority {root}: {what} {cand_v} jumps more than {MAX_JUMP} above the \
                 high-water mark {cur_v}; deploy the intermediate bundles in order"
            )));
        }
        Ok(())
    };
    step(
        "manifest_version",
        u64::from(cur.manifest_version),
        u64::from(cand.manifest_version),
        &cur.manifest_digest,
        &cand.manifest_digest,
    )?;
    step(
        "epoch",
        cur.epoch,
        cand.epoch,
        &cur.set_digest,
        &cand.set_digest,
    )?;
    Ok(cand.clone())
}

/// The per-root HWM files, `{dir}/{root}.json`, behind ONE process-wide mutex
/// (D10, D12): [`HwmStore::raise`] reads, compares and writes inside it, so the
/// below-HWM check and the write can never be separated.
pub struct HwmStore {
    dir: PathBuf,
    read_only: bool,
    lock: Mutex<()>,
}

impl HwmStore {
    pub fn new(dir: impl Into<PathBuf>, read_only: bool) -> Self {
        Self {
            dir: dir.into(),
            read_only,
            lock: Mutex::new(()),
        }
    }

    fn path(&self, root: &str) -> PathBuf {
        self.dir.join(format!("{}.json", root_key(root)))
    }

    /// The HWM of `root`: `None` when no file exists yet; unreadable or corrupt
    /// is a `fault` (fail closed, loud: never treated as "no HWM").
    pub fn read(&self, root: &str) -> Result<Option<Hwm>, KbsError> {
        let path = self.path(root);
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(KbsError::fault(format!("hwm {}: {e}", path.display()))),
        };
        let hwm: Hwm = serde_json::from_slice(&bytes)
            .map_err(|e| KbsError::fault(format!("hwm {} is corrupt: {e}", path.display())))?;
        let is_digest =
            |d: &str| d.len() == 64 && d.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if !is_digest(&hwm.manifest_digest) || !is_digest(&hwm.set_digest) {
            return Err(KbsError::fault(format!(
                "hwm {} is corrupt: a digest is not 64 lowercase hex",
                path.display()
            )));
        }
        Ok(Some(hwm))
    }

    /// Raise `root`'s HWM to `cand` (D10). Read-only stores (every tool path)
    /// compare and never write.
    pub fn raise(&self, root: &str, cand: &Hwm) -> Result<Hwm, KbsError> {
        self.raise_inner(root, cand, || {}, || None, || Ok(None))
    }

    /// [`Self::raise`] with a hook run between the compare and the write, inside
    /// the lock: the test seam that proves the lock spans both.
    #[doc(hidden)]
    pub fn raise_with_hook(
        &self,
        root: &str,
        cand: &Hwm,
        between_compare_and_write: impl FnOnce(),
    ) -> Result<Hwm, KbsError> {
        self.raise_inner(root, cand, between_compare_and_write, || None, || Ok(None))
    }

    /// [`Self::raise`] for the loader, with two more steps in the SAME critical
    /// section. With no HWM yet, `prior_archive()` returning an entry means the
    /// HWM was lost (deleted, or a restore without `hwm/`): refused as a
    /// `fault` naming that entry, rather than taken as a first load, which
    /// would reopen rollback and skip the jump bound. When the HWM moves (a
    /// body pair new to this root), `archive()` writes the bundle write-once
    /// BEFORE the HWM, and a failed HWM write removes the archive file it just
    /// created (unless the HWM did land). So every HWM has its archived bundle
    /// (the D10 recovery path names it), and two requests can never write the
    /// same archive file at once.
    pub fn raise_archiving(
        &self,
        root: &str,
        cand: &Hwm,
        prior_archive: impl FnOnce() -> Option<String>,
        archive: impl FnOnce() -> Result<Option<PathBuf>, KbsError>,
    ) -> Result<Hwm, KbsError> {
        self.raise_inner(root, cand, || {}, prior_archive, archive)
    }

    fn raise_inner(
        &self,
        root: &str,
        cand: &Hwm,
        between_compare_and_write: impl FnOnce(),
        prior_archive: impl FnOnce() -> Option<String>,
        archive: impl FnOnce() -> Result<Option<PathBuf>, KbsError>,
    ) -> Result<Hwm, KbsError> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let current = self.read(root)?;
        if current.is_none() {
            if let Some(entry) = prior_archive() {
                return Err(KbsError::fault(format!(
                    "authority {root}: no high-water mark but the archive holds {entry}; the \
                     HWM was lost (restore hwm/ from the backup taken with the keyring and \
                     check it is not behind that entry; there is no HWM reset)"
                )));
            }
        }
        let next = decide(root, current.as_ref(), cand)?;
        between_compare_and_write();
        if !self.read_only && current.as_ref() != Some(&next) {
            let created = archive()?;
            if let Err(e) = self.write(root, &next) {
                // Undo the archive unless the HWM landed after all (an error
                // after the rename, e.g. the directory fsync).
                if let Some(path) = created {
                    if self.read(root).ok().flatten().as_ref() != Some(&next) {
                        let _ = std::fs::remove_file(path);
                    }
                }
                return Err(e);
            }
        }
        Ok(next)
    }

    fn write(&self, root: &str, hwm: &Hwm) -> Result<(), KbsError> {
        let path = self.path(root);
        let body = serde_json::to_vec_pretty(hwm).expect("hwm serialises");
        write_atomic_0600(&self.dir, &path, &body)
            .map_err(|e| KbsError::fault(format!("hwm {}: {e}", path.display())))
    }

    /// Every root that has an HWM file (for the start-up log and ledger warm-up).
    pub fn roots(&self) -> Result<Vec<String>, KbsError> {
        let rd = match std::fs::read_dir(&self.dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(KbsError::fault(format!("hwm dir: {e}"))),
        };
        let mut v: Vec<String> = rd
            .flatten()
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.strip_suffix(".json").map(str::to_string)
            })
            .filter(|n| n.len() == 40 && n.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(|n| format!("0x{n}"))
            .collect();
        v.sort();
        Ok(v)
    }
}

/// `dir` (created 0700 if absent) ← temp (0600) → write → fsync → rename →
/// fsync dir.
fn write_atomic_0600(dir: &Path, path: &Path, body: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(body)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    std::fs::File::open(dir)?.sync_all()
}

/// A profile present in the latest archived set below the current epoch and
/// absent from the current set (D8's "removed" hint).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    pub profile: HostProfile,
    /// The epoch it vanished at (the current one).
    pub epoch: u64,
    /// The archive file that still holds it.
    pub archive_file: String,
}

/// Indices into `set.profiles` (D12): who is a candidate and why the others
/// are not (the matcher runs over the excluded lists too, for D8's hints).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Candidates {
    pub list: Vec<usize>,
    pub expired: Vec<usize>,
    pub class_filtered: Vec<usize>,
    pub unpinned: Vec<usize>,
}

/// One root's documents as loaded for one request (D12).
#[derive(Debug, Clone)]
pub struct Authority {
    pub manifest: SignedAuthorityManifest,
    pub set: SignedHostProfileSet,
    pub manifest_digest: [u8; 32],
    pub set_digest: [u8; 32],
    pub counted: Counted,
    pub candidates: Candidates,
    pub removed: Vec<Removed>,
}

impl Authority {
    pub fn manifest(&self) -> &AuthorityManifest {
        &self.manifest.manifest
    }
    pub fn profiles(&self) -> &[HostProfile] {
        &self.set.set.profiles
    }
}

/// The candidate filter (D7): window at `now`, class allowed, pinned where the
/// policy requires it. A profile failing several is listed under the first.
pub fn candidates(set: &HostProfileSet, policy: &Policy, now: u64) -> Candidates {
    let mut c = Candidates::default();
    for (i, p) in set.profiles.iter().enumerate() {
        if !p.in_window(now) {
            c.expired.push(i);
        } else if !policy
            .host_authority
            .allowed_classes
            .iter()
            .any(|k| k == &p.class)
        {
            c.class_filtered.push(i);
        } else if (policy.cvm.require_app_id_pinned && p.cvm.app_id.is_none())
            || (policy.cvm.require_key_provider_pinned && p.cvm.key_provider.is_none())
        {
            c.unpinned.push(i);
        } else {
            c.list.push(i);
        }
    }
    c
}

fn tee_to_kbs(e: TeeError) -> KbsError {
    match e {
        TeeError::Fetch(m) => KbsError::fault(m),
        TeeError::VerificationFailed(m) => KbsError::verification(m),
        other => KbsError::verification(other.to_string()),
    }
}

/// (epoch, set digest, manifest digest): one accepted body pair (D10).
type LedgerKey = (u64, [u8; 32], [u8; 32]);

/// The authority bundles of every root, with their HWMs, archive and ledger.
pub struct AuthorityStore {
    dir: PathBuf,
    archive_dir: PathBuf,
    hwm: HwmStore,
    read_only: bool,
    /// root → ((epoch, set digest) the ledger was built for, the ledger).
    ledger: Mutex<HashMap<String, (LedgerKey, Vec<Removed>)>>,
}

impl AuthorityStore {
    /// `read_only` stores (every tool path) never write the HWM or the archive
    /// (L1). A writable store warms the removed-profile ledger of every root
    /// with an HWM from the archive (D8: rebuilt at start).
    pub fn new(
        dir: impl Into<PathBuf>,
        hwm_dir: impl Into<PathBuf>,
        archive_dir: impl Into<PathBuf>,
        read_only: bool,
    ) -> Self {
        let s = Self {
            dir: dir.into(),
            archive_dir: archive_dir.into(),
            hwm: HwmStore::new(hwm_dir, read_only),
            read_only,
            ledger: Mutex::new(HashMap::new()),
        };
        if let Ok(roots) = s.hwm.roots() {
            for root in roots {
                if let Ok(Some(h)) = s.hwm.read(&root) {
                    if let (Ok(set_digest), Ok(manifest_digest)) =
                        (hex32(&h.set_digest), hex32(&h.manifest_digest))
                    {
                        let removed = s.rebuild_ledger(&root, h.epoch, &set_digest, None);
                        s.ledger
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .insert(root, ((h.epoch, set_digest, manifest_digest), removed));
                    }
                }
            }
        }
        s
    }

    pub fn hwm(&self) -> &HwmStore {
        &self.hwm
    }

    pub fn path_for(&self, root: &str) -> PathBuf {
        self.dir.join(format!("{}.json", root_key(root)))
    }

    /// Load `policy.host_authority.root`'s bundle for one request at `now`
    /// (D6, D7, D10). Validates all three documents, the root equalities, the
    /// windows, the policy's minimums and the HWM; raises the HWM only after
    /// all of that; archives a newly accepted bundle; returns the candidates.
    /// Does NOT require a candidate (`challenge` must issue a nonce to a new
    /// host shape so its refusal is captured, D7).
    pub fn load(&self, policy: &Policy, now: u64) -> Result<Authority, KbsError> {
        let root = policy.host_authority.root.as_str();
        let path = self.path_for(root);
        let bytes = match read_bounded(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(KbsError::no_provider(format!(
                    "no authority bundle for root {root} ({})",
                    path.display()
                )))
            }
            // Over the size rule (D5): permanent, so a refusal, not a fault.
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                return Err(KbsError::verification(format!(
                    "authority {}: {e}",
                    path.display()
                )))
            }
            Err(e) => {
                return Err(KbsError::fault(format!(
                    "authority {}: {e}",
                    path.display()
                )))
            }
        };
        let what = format!("authority {}", path.display());
        let bundle = parse_bundle(&bytes, &what).map_err(tee_to_kbs)?;
        let (manifest_digest, set_digest, counted) =
            validate_bundle(&bundle, &policy.host_authority.root, now)?;
        let m = &bundle.manifest.manifest;
        let s = &bundle.set.set;
        let hwm = Hwm {
            manifest_version: m.manifest_version,
            manifest_digest: hex::encode(manifest_digest),
            epoch: s.epoch,
            set_digest: hex::encode(set_digest),
        };
        // The HWM is the ROOT's: raised by any valid bundle of the root, BEFORE
        // this policy's own minimums apply, so a model with stricter minimums
        // never holds a shared root's HWM back (a re-served older bundle stays
        // refused for every model).
        self.hwm.raise_archiving(
            root,
            &hwm,
            || self.prior_archive(root, &archive_name(&hwm)),
            || self.archive(root, &hwm, &bytes),
        )?;
        check_minimums(&bundle, &policy.host_authority)?;
        let removed = if self.read_only {
            self.rebuild_ledger(root, s.epoch, &set_digest, Some(&bundle))
        } else {
            self.accepted(root, &bundle, (s.epoch, set_digest, manifest_digest))
        };
        Ok(Authority {
            candidates: candidates(s, policy, now),
            manifest: bundle.manifest,
            set: bundle.set,
            manifest_digest,
            set_digest,
            counted,
            removed,
        })
    }

    /// Write-once archive of an accepted bundle, keyed by its BODY digests:
    /// co-signature-only changes add nothing (D10). Called only inside the
    /// HWM's critical section when the HWM moves; returns the path when this
    /// call created the file.
    fn archive(&self, root: &str, hwm: &Hwm, bytes: &[u8]) -> Result<Option<PathBuf>, KbsError> {
        let dir = self.archive_dir.join(root_key(root));
        let path = dir.join(archive_name(hwm));
        if path.exists() {
            return Ok(None);
        }
        write_atomic_0600(&dir, &path, bytes)
            .map(|()| Some(path.clone()))
            .map_err(|e| KbsError::fault(format!("archive {}: {e}", path.display())))
    }

    /// The newest archived entry of `root` other than `candidate` (the
    /// evidence that an HWM existed), or why the archive cannot be listed. The
    /// candidate itself is tolerated: a crash between its archive write and its
    /// first HWM write leaves exactly that file, and loading the same bundle
    /// again is not a rollback. Only a MISSING archive directory is empty; any
    /// other listing error counts as an archive (fail closed).
    fn prior_archive(&self, root: &str, candidate: &str) -> Option<String> {
        let dir = self.archive_dir.join(root_key(root));
        match std::fs::read_dir(&dir) {
            Ok(rd) => rd
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".json") && n != candidate)
                .max(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => Some(format!("an archive that cannot be listed ({e})")),
        }
    }

    /// Start-up reconciliation of every root's HWM with its archive (D10).
    ///
    /// Each HWM move writes the archive entry FIRST, inside one critical
    /// section, so a crash (or kill) between the two leaves the archive
    /// exactly ONE entry ahead of the HWM (or one entry and no HWM, on a first
    /// load). That entry is re-verified from the archive (manifest signature
    /// and rules against the root, set signatures and rules against that
    /// manifest, body digests equal to its name) and the HWM rolls FORWARD to
    /// it under the usual `decide` rules: the interrupted raise is completed.
    /// Rolling forward never lowers the HWM, so it re-admits nothing.
    ///
    /// Two or more entries above the HWM cannot come from a crash: it is a
    /// lost or stale HWM (a backup restore). ACCEPTING that HWM would re-admit
    /// the epochs in between; refusing to start would leave no safe recovery
    /// (an older backup is still behind; deleting the archive reopens
    /// rollback). So the HWM is walked forward entry by entry, each
    /// re-verified, each step under `decide`, to the newest state the broker
    /// reached. Any `.json` in an archive directory that is not an archive
    /// entry is refused (fail closed), naming the file. Returns one line per
    /// step, for the CRITICAL log.
    pub fn reconcile_with_archive(&self) -> Result<Vec<String>, KbsError> {
        if self.read_only {
            return Err(KbsError::fault(
                "reconcile_with_archive is serve's (it writes the HWM); a read-only store \
                 cannot reconcile",
            ));
        }
        let mut roots = self.hwm.roots()?;
        roots.extend(self.archive_roots()?);
        roots.sort();
        roots.dedup();
        let mut completed = Vec::new();
        for root in roots {
            let names = self.archive_entries(&root)?;
            let mut entries: Vec<((u64, u32), String)> = Vec::new();
            for n in names {
                match parse_archive_name(&n) {
                    Some(k) => entries.push((k, n)),
                    None => {
                        return Err(KbsError::fault(format!(
                            "authority {root}: unexpected file {n} in its archive directory \
                             (not an archive entry); remove it to start"
                        )))
                    }
                }
            }
            let hwm = self.hwm.read(&root)?;
            let cur = hwm.as_ref().map(|h| (h.epoch, h.manifest_version));
            let mut above: Vec<&((u64, u32), String)> =
                entries.iter().filter(|(k, _)| Some(*k) > cur).collect();
            above.sort();
            // Walk FORWARD through every entry above the HWM, oldest first,
            // re-verifying each from the archive and raising one step at a
            // time under `decide` (below / equal-digest / +16 per step). Each
            // entry passed the full root-level validation and `decide` against
            // its predecessor when it was archived, so the walk re-admits
            // nothing and lands on the newest state the broker reached; a step
            // that does not verify (or jumps) stops it with a `fault`, keeping
            // the progress made (upward only).
            let why = match above.len() {
                0 => continue,
                1 => "an interrupted raise".to_string(),
                n => format!("{n} entries above the HWM: a lost or stale HWM"),
            };
            for (_, name) in above {
                let path = self.archive_dir.join(root_key(&root)).join(name);
                let next = verify_archived(&root, &path, name)?;
                self.hwm.raise(&root, &next)?;
                completed.push(format!(
                    "authority {root}: HWM rolled forward to epoch {} manifest v{} from archive \
                     entry {name} ({why})",
                    next.epoch, next.manifest_version
                ));
            }
        }
        Ok(completed)
    }

    /// The `.json` names in `root`'s archive directory (missing = none; any
    /// other listing error is a `fault`).
    fn archive_entries(&self, root: &str) -> Result<Vec<String>, KbsError> {
        let dir = self.archive_dir.join(root_key(root));
        match std::fs::read_dir(&dir) {
            Ok(rd) => Ok(rd
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".json"))
                .collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
            Err(e) => Err(KbsError::fault(format!(
                "authority {root}: archive {} cannot be listed: {e}",
                dir.display()
            ))),
        }
    }

    /// Every root that has an archive directory.
    fn archive_roots(&self) -> Result<Vec<String>, KbsError> {
        let rd = match std::fs::read_dir(&self.archive_dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(KbsError::fault(format!("archive dir: {e}"))),
        };
        Ok(rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.len() == 40 && n.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
            .map(|n| format!("0x{n}"))
            .collect())
    }

    /// The removed-profile ledger for an accepted body pair (D8). Cached: a
    /// pair this process has seen costs NO IO, so a repeated request, the
    /// refusal path an attacker can trigger with a forged quote after a free
    /// `challenge`, never touches the archive. A NEW pair (archived inside the
    /// HWM's critical section, the only moment the archive changes) rebuilds
    /// it from the archive.
    fn accepted(&self, root: &str, current: &AuthorityBundle, key: LedgerKey) -> Vec<Removed> {
        {
            let cache = self.ledger.lock().unwrap_or_else(|p| p.into_inner());
            if let Some((k, removed)) = cache.get(root) {
                if *k == key {
                    return removed.clone();
                }
            }
        }
        let removed = self.rebuild_ledger(root, key.0, &key.1, Some(current));
        self.ledger
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(root.to_string(), (key, removed.clone()));
        removed
    }

    /// From the archive: the latest archived set below `epoch`; its profiles
    /// absent (by id) from the current set. An archive read error yields an
    /// empty ledger (a hint is never worth turning a refusal into a fault).
    fn rebuild_ledger(
        &self,
        root: &str,
        epoch: u64,
        set_digest: &[u8; 32],
        current: Option<&AuthorityBundle>,
    ) -> Vec<Removed> {
        let dir = self.archive_dir.join(root_key(root));
        let Ok(rd) = std::fs::read_dir(&dir) else {
            return vec![];
        };
        let mut names: Vec<String> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".json"))
            .collect();
        names.sort();
        let epoch_of = |n: &str| n.get(..20).and_then(|e| e.parse::<u64>().ok());
        let current_set = match current {
            Some(b) => b.set.set.clone(),
            None => {
                let want = hex::encode(set_digest);
                let Some(n) = names.iter().find(|n| {
                    epoch_of(n) == Some(epoch) && n.contains(&format!("-{}", &want[..16]))
                }) else {
                    return vec![];
                };
                match read_archived(&dir.join(n)) {
                    Some(b) => b.set.set,
                    None => return vec![],
                }
            }
        };
        let Some(prev_name) = names
            .iter()
            .filter(|n| epoch_of(n).is_some_and(|e| e < epoch))
            .next_back()
        else {
            return vec![];
        };
        let Some(prev) = read_archived(&dir.join(prev_name)) else {
            return vec![];
        };
        prev.set
            .set
            .profiles
            .into_iter()
            .filter(|p| !current_set.profiles.iter().any(|q| q.id == p.id))
            .map(|profile| Removed {
                profile,
                epoch,
                archive_file: prev_name.clone(),
            })
            .collect()
    }
}

/// `{epoch:020}-{manifest_version:010}-{16 hex}-{16 hex}.json` → (epoch,
/// manifest_version); anything else is `None`.
pub fn parse_archive_name(n: &str) -> Option<(u64, u32)> {
    let b = n.as_bytes();
    let hex16 = |r: std::ops::Range<usize>| {
        b.get(r)
            .is_some_and(|x| x.iter().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')))
    };
    let ok = b.len() == 70
        && b[..20].iter().all(u8::is_ascii_digit)
        && b[20] == b'-'
        && b[21..31].iter().all(u8::is_ascii_digit)
        && b[31] == b'-'
        && hex16(32..48)
        && b[48] == b'-'
        && hex16(49..65)
        && n.ends_with(".json");
    if !ok {
        return None;
    }
    Some((n[..20].parse().ok()?, n[21..31].parse().ok()?))
}

/// Re-verify an archived bundle for [`AuthorityStore::reconcile_with_archive`]:
/// it parses, its manifest verifies against `root` (signature and rules; the
/// window is not re-checked, the bundle was accepted when it was valid), its
/// set verifies against that manifest, and its body digests are the ones its
/// name records. Returns the HWM it stands for.
fn verify_archived(root: &str, path: &Path, name: &str) -> Result<Hwm, KbsError> {
    let refuse = |why: String| {
        KbsError::fault(format!(
            "authority {root}: archive entry {name} {why}; the HWM cannot be rolled \
             forward past it (investigate who wrote it: only the broker writes archive/)"
        ))
    };
    let bytes = read_bounded(path).map_err(|e| refuse(format!("cannot be read ({e})")))?;
    let b = parse_bundle(&bytes, name).map_err(|e| refuse(format!("does not parse ({e})")))?;
    verify_manifest(&b.manifest, root).map_err(|e| refuse(format!("does not verify ({e})")))?;
    verify_set(&b.set, &b.manifest.manifest)
        .map_err(|e| refuse(format!("does not verify ({e})")))?;
    let md = document_digest(&b.manifest.manifest).map_err(tee_to_kbs)?;
    let sd = document_digest(&b.set.set).map_err(tee_to_kbs)?;
    let hwm = Hwm {
        manifest_version: b.manifest.manifest.manifest_version,
        manifest_digest: hex::encode(md),
        epoch: b.set.set.epoch,
        set_digest: hex::encode(sd),
    };
    if archive_name(&hwm) != name {
        return Err(refuse(
            "is not named for its contents (digests or counters differ)".into(),
        ));
    }
    Ok(hwm)
}

fn read_archived(path: &Path) -> Option<AuthorityBundle> {
    let bytes = read_bounded(path).ok()?;
    parse_bundle(&bytes, "archive").ok()
}

/// `{epoch:020}-{manifest_version:010}-{manifest_digest16}-{set_digest16}.json`
/// (D10: sortable by epoch for the ledger).
pub fn archive_name(h: &Hwm) -> String {
    format!(
        "{:020}-{:010}-{}-{}.json",
        h.epoch,
        h.manifest_version,
        &h.manifest_digest[..16],
        &h.set_digest[..16]
    )
}

fn hex32(s: &str) -> Result<[u8; 32], ()> {
    hex::decode(s)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or(())
}

fn read_bounded(path: &Path) -> std::io::Result<Vec<u8>> {
    let f = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    f.take(MAX_POLICY_BYTES as u64 + 1).read_to_end(&mut buf)?;
    if buf.len() > MAX_POLICY_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("file over {MAX_POLICY_BYTES} bytes"),
        ));
    }
    Ok(buf)
}

/// Everything ROOT-level about a bundle (D6, D7): the manifest (form,
/// signature, root == `root`, window at `now`) and the set (form, root,
/// signatures at threshold, `issued_at` not in the future). Returns the two
/// body digests and the counted signers. A bundle that passes is valid for
/// the root, whatever one policy's minimums say ([`check_minimums`]).
pub fn validate_bundle(
    b: &AuthorityBundle,
    root: &str,
    now: u64,
) -> Result<([u8; 32], [u8; 32], Counted), KbsError> {
    verify_manifest(&b.manifest, root).map_err(tee_to_kbs)?;
    let m = &b.manifest.manifest;
    if now < m.not_before || now > m.expiry {
        return Err(KbsError::verification(format!(
            "manifest v{} outside its window {}..={} at {now}",
            m.manifest_version, m.not_before, m.expiry
        )));
    }
    let counted = verify_set(&b.set, m).map_err(tee_to_kbs)?;
    let s = &b.set.set;
    if s.issued_at > now.saturating_add(MAX_ISSUED_AT_SKEW) {
        return Err(KbsError::verification(format!(
            "profile set epoch {} issued_at {} is in the future (now {now})",
            s.epoch, s.issued_at
        )));
    }
    let md = document_digest(m).map_err(tee_to_kbs)?;
    let sd = document_digest(s).map_err(tee_to_kbs)?;
    Ok((md, sd, counted))
}

/// One policy's floor on its authority's documents (D3, D7).
pub fn check_minimums(b: &AuthorityBundle, a: &HostAuthority) -> Result<(), KbsError> {
    let m = &b.manifest.manifest;
    let s = &b.set.set;
    if m.manifest_version < a.min_manifest_version {
        return Err(KbsError::verification(format!(
            "manifest v{} below the policy's min_manifest_version {}",
            m.manifest_version, a.min_manifest_version
        )));
    }
    if m.threshold < a.min_threshold {
        return Err(KbsError::verification(format!(
            "manifest threshold {} below the policy's min_threshold {}",
            m.threshold, a.min_threshold
        )));
    }
    if s.epoch < a.min_profile_epoch {
        return Err(KbsError::verification(format!(
            "profile set epoch {} below the policy's min_profile_epoch {}",
            s.epoch, a.min_profile_epoch
        )));
    }
    Ok(())
}

impl Authority {
    /// The PURE part of [`AuthorityStore::load`] (no HWM, archive or ledger):
    /// the root-level validation, the policy's minimums and the candidates at
    /// `now`. For the offline tools (`authority check`, the replay) and the
    /// pure tests; the broker always goes through the store.
    pub fn from_bundle(
        bundle: AuthorityBundle,
        policy: &Policy,
        now: u64,
    ) -> Result<Self, KbsError> {
        let (manifest_digest, set_digest, counted) =
            validate_bundle(&bundle, &policy.host_authority.root, now)?;
        check_minimums(&bundle, &policy.host_authority)?;
        Ok(Self {
            candidates: candidates(&bundle.set.set, policy, now),
            manifest: bundle.manifest,
            set: bundle.set,
            manifest_digest,
            set_digest,
            counted,
            removed: vec![],
        })
    }
}
