// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The narrow CSAM entry points (the only surface production callers use). Each
//! returns a `ModerationResult` only — no raw content / hashes / store leak out.
//! All are fail-closed: any decode/match error or unavailable list ⇒ HOLD.

use super::hashlist::HashListSnapshot;
use super::matcher::Matcher;
use super::ownhash::OwnHashList;
use super::pdq;
use crate::moderation::ingest::IngestItem;
use crate::moderation::types::{MatchResult, ModerationResult, Result, REASON_CSAM_MATCH};

/// Map a single match attempt to a terminal verdict: a hit blocks; an error holds
/// (fail-closed); a clean miss returns `None` so the caller keeps scanning.
fn check(res: Result<MatchResult>) -> Option<ModerationResult> {
    match res {
        Ok(m) if m.is_match => Some(ModerationResult::blocked(REASON_CSAM_MATCH)),
        Ok(_) => None,
        Err(_) => Some(ModerationResult::blocked("moderation unavailable")),
    }
}

/// Decode + hash + match an image/keyframe asset (B8). Exact-match over the file
/// bytes; PDQ near-match over the decoded image. Undecodable-and-unmatched ⇒ HOLD.
pub fn moderate_asset_bytes(
    bytes: &[u8],
    snapshot: &HashListSnapshot,
    ownhash: &OwnHashList,
    max_distance: u32,
) -> ModerationResult {
    let matcher = Matcher::new(snapshot, ownhash);
    // Exact-match is over the file bytes (bit-identical / own-hash).
    let sha = Matcher::sha256(bytes);
    // Decode + PDQ, distinguishing the two failure modes — both fail-closed, like
    // `moderate_frames` (no silent error-swallow): an undecodable image leaves
    // `pdq_hash = None` (still try the exact pre-filter, else hold), while a decoded
    // image whose PDQ compute fails HOLDs immediately with an accurate reason.
    let pdq_hash = match image::load_from_memory(bytes) {
        Err(_) => None,
        Ok(img) => {
            let rgb = img.to_rgb8();
            let (w, h) = (rgb.width(), rgb.height());
            match pdq::compute_pdq_rgb(rgb.as_raw(), w, h) {
                Ok(r) => Some(r.hash),
                Err(_) => return ModerationResult::blocked("asset PDQ computation failed"),
            }
        }
    };
    match matcher.match_content(&sha, pdq_hash.as_ref(), max_distance) {
        Ok(m) if m.is_match => ModerationResult::blocked(REASON_CSAM_MATCH),
        Ok(_) if pdq_hash.is_some() => ModerationResult::cleared(),
        Ok(_) => ModerationResult::blocked("asset could not be decoded for scanning"),
        Err(_) => ModerationResult::blocked("moderation unavailable"),
    }
}

/// Match an ingest item from the transcoder (seam #1). Uses pre-supplied SHA-256 /
/// PDQ hashes directly, and computes PDQ in-node for decoded frames (§3.5). ANY hit
/// blocks; ANY component error (e.g. unavailable list) holds; a frame that cannot
/// be hashed holds. Clean across all ⇒ Cleared.
pub fn moderate_frames(
    item: &IngestItem,
    snapshot: &HashListSnapshot,
    ownhash: &OwnHashList,
    max_distance: u32,
) -> ModerationResult {
    let matcher = Matcher::new(snapshot, ownhash);
    let (sha256, pdq_hashes, frames): (
        &[[u8; 32]],
        &[crate::moderation::types::Pdq256],
        &[crate::moderation::ingest::DecodedFrame],
    ) = match item {
        IngestItem::Hashes { sha256, pdq, .. } => (sha256, pdq, &[]),
        IngestItem::Frames { frames, .. } => (&[], &[], frames),
        IngestItem::Both {
            sha256,
            pdq,
            frames,
            ..
        } => (sha256, pdq, frames),
    };
    // Fail-closed: an item with nothing to scan must NOT clear — we cannot vouch
    // for content we never examined (mirrors the undecodable-asset hold).
    if sha256.is_empty() && pdq_hashes.is_empty() && frames.is_empty() {
        return ModerationResult::blocked("empty ingest item: nothing to scan");
    }
    for s in sha256 {
        if let Some(block) = check(matcher.match_sha256(s)) {
            return block;
        }
    }
    for p in pdq_hashes {
        if let Some(block) = check(matcher.match_pdq(p, max_distance)) {
            return block;
        }
    }
    for f in frames {
        match pdq::compute_pdq_rgb(&f.rgb, f.width, f.height) {
            Ok(r) => {
                if let Some(block) = check(matcher.match_pdq(&r.hash, max_distance)) {
                    return block;
                }
            }
            Err(_) => return ModerationResult::blocked("frame could not be hashed"),
        }
    }
    ModerationResult::cleared()
}
