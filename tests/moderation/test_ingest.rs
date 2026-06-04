// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 2.1 — seam-#1 frame-ingest adapter + orchestration.

use fabstir_llm_node::moderation::ingest::{
    process, record_pending, DecodedFrame, FrameSource, IngestItem, MockFrameSource,
};
use fabstir_llm_node::moderation::types::{ModerationResult, Pdq256};
use fabstir_llm_node::moderation::verdict_store::VerdictStore;

#[test]
fn mock_frame_source_yields_frames_item() {
    let frame = DecodedFrame {
        width: 1,
        height: 1,
        rgb: vec![0, 0, 0],
    };
    let mut src = MockFrameSource::with_frames(11, vec![frame]);
    match src.next_item() {
        Some(IngestItem::Frames { job_id, frames, .. }) => {
            assert_eq!(job_id, 11);
            assert_eq!(frames.len(), 1);
        }
        _ => panic!("expected a Frames item"),
    }
    assert!(src.next_item().is_none(), "source exhausted after one item");
}

#[test]
fn mock_frame_source_yields_hashes_item() {
    let mut src = MockFrameSource::with_hashes(22, vec![[0u8; 32]], vec![Pdq256([0u8; 32])]);
    match src.next_item() {
        Some(IngestItem::Hashes {
            job_id,
            sha256,
            pdq,
        }) => {
            assert_eq!(job_id, 22);
            assert_eq!(sha256.len(), 1);
            assert_eq!(pdq.len(), 1);
        }
        _ => panic!("expected a Hashes item"),
    }
}

#[test]
fn mock_frame_source_yields_both_item() {
    let frame = DecodedFrame {
        width: 1,
        height: 1,
        rgb: vec![0, 0, 0],
    };
    let mut src =
        MockFrameSource::with_both(33, vec![frame], vec![[1u8; 32]], vec![Pdq256([2u8; 32])]);
    match src.next_item() {
        Some(IngestItem::Both {
            job_id,
            frames,
            sha256,
            pdq,
            ..
        }) => {
            assert_eq!(job_id, 33);
            assert_eq!(frames.len(), 1);
            assert_eq!(sha256.len(), 1);
            assert_eq!(pdq.len(), 1);
        }
        _ => panic!("expected a Both item"),
    }
}

#[test]
fn ingest_records_pending_then_verdict() {
    let store = VerdictStore::new();
    let item = IngestItem::Hashes {
        job_id: 9,
        sha256: vec![],
        pdq: vec![],
    };

    // The moment frames/hashes arrive, the job is marked pending — a HOLD, never
    // absent-then-cleared, so a crash mid-decision still holds.
    record_pending(&store, item.job_id());
    assert!(
        !store.get(9).unwrap().verdict.releases(),
        "a pending job must hold"
    );

    // The decision step stands in for the Track-1 matcher (wired in Phase 4).
    process(&store, &item, |_item| ModerationResult::cleared());
    assert!(
        store.get(9).unwrap().verdict.releases(),
        "the final verdict must overwrite the pending hold"
    );
}
