// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! D8(c): refuse an mp4 that still carries a ComfyUI graph. VHS double-encodes the JSON, so the bytes read
//! `\"class_type\"` (escaped), never `"class_type"`: the guard scans for the BARE tokens. It scans every box
//! except `mdat` (compressed picture data can contain any byte string by chance).

use crate::flow::top_boxes;

/// Tokens that only a ComfyUI workflow/prompt dump contains.
pub const TOKENS: &[&[u8]] = &[b"class_type", b".safetensors"];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Leak {
    #[error("mp4 box {0} carries the ComfyUI graph")]
    Graph(String),
    #[error("mp4 is not a clean box list")]
    Malformed,
}

pub fn check_mp4(b: &[u8]) -> Result<(), Leak> {
    let boxes = top_boxes(b);
    let covered: usize = boxes.iter().map(|(_, _, s)| s).sum();
    if covered != b.len() {
        return Err(Leak::Malformed);
    }
    for (t, off, sz) in boxes {
        if t == "mdat" {
            continue;
        }
        let body = &b[off..off + sz];
        if TOKENS.iter().any(|tok| body.windows(tok.len()).any(|w| w == *tok)) {
            return Err(Leak::Graph(t));
        }
    }
    Ok(())
}
