// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The EXR frames index (D9): a deterministic JSON list of frame name -> SHA-256 of the FINAL frame bytes,
//! bound by the standalone sequence manifest's data hash.

use crate::class::Class;
use crate::defs::canon_bytes;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const SCHEMA: &str = "platformlessai.frames-index.v1";
/// Most frames one sequence may list (a 20 s job at 60 fps is 1,200).
pub const MAX_FRAMES: usize = 2_400;

/// The only frame names: `frame_000001.exr` .. contiguous from 1 in delivery order. Fixed by the format so the
/// signed, publicly served index carries no node-chosen text (contract v1.0); the hashes are the only payload.
pub fn frame_name(n: usize) -> String {
    format!("frame_{n:06}.exr")
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IndexError {
    #[error("frames index: {0}")]
    Invalid(String),
}

fn bad(m: &str) -> IndexError {
    IndexError::Invalid(m.to_string())
}

fn class_name(c: Class) -> &'static str {
    match c {
        Class::Generated => "generated",
        Class::Modified => "modified",
        Class::Data => "data",
    }
}

/// Canonical index bytes from (name, final frame bytes) pairs. Names must be unique and well formed.
pub fn build(class: Class, frames: &[(String, Vec<u8>)]) -> Result<Vec<u8>, IndexError> {
    let hashes: Vec<(String, String)> = frames
        .iter()
        .map(|(n, b)| (n.clone(), hex::encode(Sha256::digest(b))))
        .collect();
    build_from_hashes(class, &hashes)
}

/// Same, from precomputed lower-case hex SHA-256 values.
pub fn build_from_hashes(class: Class, hashes: &[(String, String)]) -> Result<Vec<u8>, IndexError> {
    if hashes.is_empty() {
        return Err(bad("no frames"));
    }
    if hashes.len() > MAX_FRAMES {
        return Err(bad("too many frames"));
    }
    let mut m = BTreeMap::new();
    let mut names: Vec<&String> = hashes.iter().map(|(n, _)| n).collect();
    names.sort();
    for (k, n) in names.iter().enumerate() {
        if **n != frame_name(k + 1) {
            return Err(bad("frame names must be frame_000001.exr .. contiguous"));
        }
    }
    for (n, h) in hashes {
        if !is_hex64(h) {
            return Err(bad("hash"));
        }
        if m.insert(n.clone(), h.clone()).is_some() {
            return Err(bad("duplicate frame name"));
        }
    }
    Ok(canon_bytes(
        &json!({"schema": SCHEMA, "class": class_name(class), "frames": m}),
    ))
}

fn is_hex64(h: &str) -> bool {
    h.len() == 64
        && h.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Signer-side validation of index bytes sent by a node: schema, class equals the reservation's, names and
/// hashes well formed, and the bytes are exactly the canonical form (so one index has one byte string).
pub fn validate(bytes: &[u8], class: Class) -> Result<usize, IndexError> {
    let v: Value = serde_json::from_slice(bytes).map_err(|_| bad("not JSON"))?;
    let o = v.as_object().ok_or_else(|| bad("not an object"))?;
    if o.len() != 3 || o.get("schema").and_then(|s| s.as_str()) != Some(SCHEMA) {
        return Err(bad("schema"));
    }
    if o.get("class").and_then(|s| s.as_str()) != Some(class_name(class)) {
        return Err(bad("class differs from the reservation"));
    }
    let frames = o
        .get("frames")
        .and_then(|f| f.as_object())
        .ok_or_else(|| bad("frames"))?;
    let mut pairs = vec![];
    for (n, h) in frames {
        pairs.push((
            n.clone(),
            h.as_str().ok_or_else(|| bad("hash"))?.to_string(),
        ));
    }
    let canon = build_from_hashes(class, &pairs)?;
    if canon != bytes {
        return Err(bad("not in canonical form"));
    }
    Ok(pairs.len())
}

/// The SHA-256 a frame must have to be listed (for the detector and tests).
pub fn lookup(bytes: &[u8], name: &str) -> Option<String> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    v["frames"][name].as_str().map(|s| s.to_string())
}
