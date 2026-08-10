// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Operator-loadable list files (WP-N2) — deploy-time, plaintext, explicit.
//!
//! Scope (rule 8): operator-originated hashes of operator-confirmed illegal
//! content ONLY. NCMEC/NGO-sourced hashes must ride the encrypted
//! [`super::hashlist::NcmecHashStore`] path, never a plaintext file. An empty
//! file is an error unless it declares itself with the sole directive line
//! `#!allow-empty` (rule 2 — a truncated write of a real list can never
//! produce the directive, so no failure mode installs an empty clean list).

use std::collections::HashSet;

use anyhow::{bail, Context, Result};

use super::hashlist::{HashListSnapshot, ListState};
use super::ownhash::OwnHashList;
use crate::moderation::types::Pdq256;

/// Rule-2 directive: exact, case-sensitive, sole non-comment line.
const ALLOW_EMPTY_DIRECTIVE: &str = "#!allow-empty";

/// Decode exactly 64 hex chars (case-insensitive) to 32 bytes; errors carry the
/// 1-based line so a rejected file names its offender (rule 3).
fn decode_hex_32(hex_str: &str, line_no: usize, what: &str) -> Result<[u8; 32]> {
    if hex_str.len() != 64 {
        bail!(
            "line {line_no}: {what} must be exactly 64 hex chars, got {} \
             (inline comments are not allowed)",
            hex_str.len()
        );
    }
    let mut out = [0u8; 32];
    hex::decode_to_slice(hex_str, &mut out)
        .map_err(|e| anyhow::anyhow!("line {line_no}: {what} is not valid hex: {e}"))?;
    Ok(out)
}

/// The shared line grammar (§3): strip a leading UTF-8 BOM (same
/// Windows-transit tolerance as CRLF — an invisible char must not produce a
/// baffling rejection of a visually-valid first line), trim (absorbs CRLF),
/// skip blanks, recognise the rule-2 directive, skip whole-line `#` comments —
/// including directive near-misses (wrong case or spacing), which deliberately
/// do NOT set the flag. Every other line goes to `on_entry` with its 1-based
/// number. Returns the allow-empty flag for [`check_empty_exclusivity`].
fn walk_lines(content: &str, mut on_entry: impl FnMut(&str, usize) -> Result<()>) -> Result<bool> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut allow_empty = false;
    for (idx, raw) in content.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line == ALLOW_EMPTY_DIRECTIVE {
            allow_empty = true;
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        on_entry(line, idx + 1)?;
    }
    Ok(allow_empty)
}

/// Rule 2's epilogue: empty must be declared, and declared-empty must be
/// empty. Exclusive by design — were the directive merely inert beside
/// entries, truncation at the first line boundary could turn a populated list
/// into a valid empty-clears-everything file.
fn check_empty_exclusivity(allow_empty: bool, has_entries: bool, file_noun: &str) -> Result<()> {
    if allow_empty && has_entries {
        bail!("#!allow-empty must be the sole non-comment line, but the file also has entries");
    }
    if !allow_empty && !has_entries {
        bail!("{file_noun} has zero entries and no #!allow-empty directive");
    }
    Ok(())
}

/// Parse `MODERATION_LIST_FILE` content (§3.1): `sha256:`/`pdq:` entries, blank
/// lines and whole-line `#` comments. Malformed line ⇒ reject the whole file
/// (rule 3 — a corrupted hash is a hash that no longer blocks).
pub fn parse_list_file(content: &str) -> Result<(HashSet<[u8; 32]>, Vec<Pdq256>)> {
    let mut sha256 = HashSet::new();
    let mut pdq = Vec::new();
    let allow_empty = walk_lines(content, |line, line_no| {
        if let Some(rest) = line.strip_prefix("sha256:") {
            sha256.insert(decode_hex_32(rest, line_no, "sha256 entry")?);
        } else if let Some(rest) = line.strip_prefix("pdq:") {
            pdq.push(Pdq256(decode_hex_32(rest, line_no, "pdq entry")?));
        } else {
            bail!("line {line_no}: unknown entry prefix (expected sha256: or pdq:)");
        }
        Ok(())
    })?;
    check_empty_exclusivity(
        allow_empty,
        !sha256.is_empty() || !pdq.is_empty(),
        "list file",
    )?;
    Ok((sha256, pdq))
}

/// Parse `MODERATION_OWNHASH_FILE` content (§3.2): one bare 64-hex SHA-256 per
/// line (no prefix — the type is single-purpose). Rules 2 and 3 apply verbatim,
/// with "empty `Loaded`" reading as "accepted empty list" — [`OwnHashList`] has
/// no availability state.
pub fn parse_ownhash_file(content: &str) -> Result<OwnHashList> {
    let mut own = OwnHashList::new();
    let allow_empty = walk_lines(content, |line, line_no| {
        own.add(decode_hex_32(line, line_no, "own-hash entry")?);
        Ok(())
    })?;
    check_empty_exclusivity(allow_empty, !own.is_empty(), "own-hash file")?;
    Ok(own)
}

/// Install parsed entries as a genuine `Loaded` snapshot (§1: own-hash alone can
/// never clear anything — clearing requires `ListState::Loaded`).
///
/// `version` is a content fingerprint (rule 6): the first 8 bytes of the
/// SHA-256 of the file bytes, NON-ORDINAL — never compare with `>`, and log it
/// as hex (`list-fp=…`) so it cannot be misread as a refresh counter. The NCMEC
/// refresh path keeps counter semantics for its own store.
pub fn snapshot_from_parsed(
    sha256: HashSet<[u8; 32]>,
    pdq: Vec<Pdq256>,
    file_bytes: &[u8],
) -> HashListSnapshot {
    let digest = super::matcher::Matcher::sha256(file_bytes);
    HashListSnapshot {
        state: ListState::Loaded,
        sha256,
        pdq,
        version: u64::from_be_bytes(digest[..8].try_into().expect("digest has 32 bytes")),
    }
}

/// Ceiling far above any legitimate operator list (100k entries ≈ 7 MB): a
/// device file or runaway path must degrade loudly, never OOM the node.
const MAX_LIST_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Refuse non-regular files BEFORE opening: `fs::read` on a FIFO blocks in
/// open() forever — a silent wedge is worse than the loud degrade rule 1
/// mandates. Symlinks to regular files pass (`metadata` follows links).
fn check_regular_file(path: &str, what: &str) -> Result<()> {
    let meta = std::fs::metadata(path).with_context(|| format!("{what} {path}"))?;
    if !meta.is_file() {
        bail!("{what} {path} is not a regular file");
    }
    if meta.len() > MAX_LIST_FILE_BYTES {
        bail!(
            "{what} {path} is {} bytes — exceeds the {} byte ceiling",
            meta.len(),
            MAX_LIST_FILE_BYTES
        );
    }
    Ok(())
}

/// One buffer end-to-end: the bytes that are parsed are the bytes that are
/// fingerprinted, so the rule-6 "any change ⇒ visible change" invariant cannot
/// diverge through a re-read or normalisation between the two steps.
fn load_list_snapshot_from(path: &str) -> Result<HashListSnapshot> {
    check_regular_file(path, "moderation list file")?;
    let bytes = std::fs::read(path).with_context(|| format!("moderation list file {path}"))?;
    let content = std::str::from_utf8(&bytes)
        .with_context(|| format!("moderation list file {path} is not UTF-8"))?;
    let (sha256, pdq) =
        parse_list_file(content).with_context(|| format!("moderation list file {path}"))?;
    Ok(snapshot_from_parsed(sha256, pdq, &bytes))
}

/// Resolve `MODERATION_PDQ_MAX_DISTANCE` (rule 7): unset ⇒ 31 (Meta guidance,
/// the previously hardcoded value); set ⇒ must parse as u32 and respect the
/// existing validated cap [`crate::moderation::config::MAX_PDQ_DISTANCE`].
///
/// 🚨 An `Err` here is the plan's ONE fatal case (rule 1): an out-of-range or
/// unparseable knob is static misconfiguration no race or reboot can produce,
/// and degrading on it would silently drop an otherwise-good list file. It is
/// deliberately a SEPARATE call from [`FramesMatchState::from_env_files`] — a
/// single `anyhow::Result` cannot discriminate degrade-vs-die.
pub fn resolve_pdq_max_distance() -> Result<u32> {
    match std::env::var("MODERATION_PDQ_MAX_DISTANCE") {
        Err(std::env::VarError::NotPresent) => Ok(31),
        // Set-but-EMPTY means unset (the shared MODERATION_* convention; a
        // stray `FOO=` env-file line must not take down paid serving)…
        Ok(s) if s.is_empty() => Ok(31),
        // …but set-but-not-unicode is set-and-broken: fatal, not silently 31.
        Err(e @ std::env::VarError::NotUnicode(_)) => {
            Err(e).context("MODERATION_PDQ_MAX_DISTANCE is not valid unicode")
        }
        Ok(s) => {
            let d: u32 = s
                .parse()
                .with_context(|| format!("MODERATION_PDQ_MAX_DISTANCE `{s}` is not a u32"))?;
            if d > crate::moderation::config::MAX_PDQ_DISTANCE {
                bail!(
                    "MODERATION_PDQ_MAX_DISTANCE {} exceeds max {}",
                    d,
                    crate::moderation::config::MAX_PDQ_DISTANCE
                );
            }
            Ok(d)
        }
    }
}

/// The resolved frames/asset match state (§1.2's stored field): what
/// `/v1/moderate/frames` AND the asset moderator match against — one state,
/// two builders, so `/frames` cannot drift from `/asset` (C5).
#[derive(Clone)]
pub struct FramesMatchState {
    pub snapshot: HashListSnapshot,
    pub ownhash: OwnHashList,
    pub max_distance: u32,
}

impl FramesMatchState {
    /// The fail-closed default triple — Unavailable snapshot, empty own-hash,
    /// max_distance 31: byte-for-byte the pre-WP-N2 hardcoded state.
    pub fn fail_closed_default() -> Self {
        Self {
            snapshot: HashListSnapshot::unavailable(),
            ownhash: OwnHashList::new(),
            max_distance: 31,
        }
    }

    /// Load the two optional operator files (rule 5: once, at startup; restart
    /// to reload). Every `Err` out of here is DEGRADABLE (rule 1): the caller
    /// catches it unconditionally, starts with the fail-closed default triple
    /// and surfaces the degradation (/health issue + ERROR log) — a broken
    /// list file never kills the node. The fatal PDQ knob lives in
    /// [`resolve_pdq_max_distance`], a separate call by design.
    pub fn from_env_files(max_distance: u32) -> Result<Self> {
        // Set-but-not-unicode is set-and-broken (a silently-empty ownhash
        // beside a Loaded list would fail OPEN); set-but-empty means unset,
        // the convention every MODERATION_* var here shares.
        let snapshot = match std::env::var("MODERATION_LIST_FILE") {
            Ok(path) if !path.is_empty() => load_list_snapshot_from(&path)?,
            Err(std::env::VarError::NotUnicode(_)) => {
                bail!("MODERATION_LIST_FILE is set but not valid unicode")
            }
            _ => HashListSnapshot::unavailable(),
        };
        let ownhash = match std::env::var("MODERATION_OWNHASH_FILE") {
            Ok(path) if !path.is_empty() => {
                check_regular_file(&path, "own-hash file")?;
                let content = std::fs::read_to_string(&path)
                    .with_context(|| format!("own-hash file {path}"))?;
                parse_ownhash_file(&content).with_context(|| format!("own-hash file {path}"))?
            }
            Err(std::env::VarError::NotUnicode(_)) => {
                bail!("MODERATION_OWNHASH_FILE is set but not valid unicode")
            }
            _ => OwnHashList::new(),
        };
        Ok(Self {
            snapshot,
            ownhash,
            max_distance,
        })
    }

    /// 🧪 TEST-ONLY composition of the two production calls. Production
    /// (`ApiServer::new`) calls [`resolve_pdq_max_distance`] and
    /// [`Self::from_env_files`] directly so the fatal and degradable error
    /// channels stay structurally separate; tests drive this composition.
    pub fn from_env() -> Result<Self> {
        Self::from_env_files(resolve_pdq_max_distance()?)
    }
}
