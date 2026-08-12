// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Deep-conform input (v8.44.x, EXECUTION-DEEP-CONFORM.md).
//!
//! Transport v2 (v8.44.2): `videos[0]` decrypts to a small JSON MANIFEST
//! listing per-frame capability CIDs in delivery order — NOT a tar of frames.
//! The tar transport (v8.44.0/.1) never survived first contact: the helper's
//! s5.js client caps blobs at 32 MiB, so a multi-hundred-MB tar could never
//! upload. Per-frame blobs sit far under the cap (a ZIP-half 4K frame is
//! ~25 MiB), retry individually, and mirror the node's own EXR delivery in
//! reverse. The commitment binds `keccak256(manifest plaintext)`; capability
//! CIDs embed the frames' own plaintext hashes, so the frames are bound
//! transitively through the list the commitment pins.

use serde::Deserialize;

/// EXR magic number (little-endian 20000630).
pub const EXR_MAGIC: [u8; 4] = [0x76, 0x2f, 0x31, 0x01];

/// Hard per-frame ceiling. A 4K half-float ZIP frame is ~25 MiB; 64 MiB is
/// generous headroom without letting one entry balloon the staging loop.
pub const MAX_FRAME_BYTES: usize = 64 << 20;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeepManifestWire {
    deep_frames: Vec<String>,
}

/// Parse + validate the deep-conform manifest. Returns the frame capability
/// CIDs in delivery order.
///
/// Fail-closed rules:
/// - must be JSON of the exact shape `{"deepFrames": [cid, ...]}`
/// - the frame count must be the billed count or billed-1 — the SAME conform
///   ±1 the mp4 path honours (the job bills fps*d+1, a content-true conform
///   carries fps*d)
/// - every CID non-empty, no duplicates (a repeated frame is a malformed
///   conform, not a compression trick)
pub fn parse_deep_manifest(bytes: &[u8], billed_frames: u32) -> Result<Vec<String>, String> {
    let wire: DeepManifestWire = serde_json::from_slice(bytes)
        .map_err(|e| format!("deep manifest is not valid JSON ({e})"))?;
    let cids = wire.deep_frames;
    let billed = billed_frames as usize;
    if cids.len() != billed && cids.len() + 1 != billed {
        return Err(format!(
            "deep manifest lists {} frame(s) but the job bills {billed} — {} or {billed} accepted (the conform ±1)",
            cids.len(),
            billed.saturating_sub(1)
        ));
    }
    for (i, cid) in cids.iter().enumerate() {
        if cid.is_empty() {
            return Err(format!("deep manifest frame {i} has an empty CID"));
        }
        if cids[..i].contains(cid) {
            return Err(format!("deep manifest frame {i} repeats CID {cid}"));
        }
    }
    Ok(cids)
}

/// Per-frame plaintext gate, run on each decrypted frame before it is staged:
/// real EXR bytes, bounded size. The RUNNING total against the bundle's
/// `deepVideoMaxBytes` is the caller's job (it owns the loop).
pub fn check_deep_frame(index: usize, plaintext: &[u8]) -> Result<(), String> {
    if plaintext.len() < 4 || plaintext[0..4] != EXR_MAGIC {
        return Err(format!(
            "deep frame {index} does not start with the EXR magic"
        ));
    }
    if plaintext.len() > MAX_FRAME_BYTES {
        return Err(format!(
            "deep frame {index} is {} bytes — over the {MAX_FRAME_BYTES}-byte per-frame ceiling",
            plaintext.len()
        ));
    }
    Ok(())
}
