// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 7.1 — end-to-end fail-closed: ingest→match→gate→quarantine→report,
//! and "kill any in-repo component ⇒ the job holds". 🚨

use chrono::{DateTime, Utc};

use fabstir_llm_node::moderation::csam::hashlist::{HashListSnapshot, HashListSource};
use fabstir_llm_node::moderation::csam::mock_source::MockHashListSource;
use fabstir_llm_node::moderation::csam::ownhash::OwnHashList;
use fabstir_llm_node::moderation::csam::quarantine::Quarantine;
use fabstir_llm_node::moderation::csam::report::{
    confirm_and_file, prepare_report, HumanConfirmation, MockReportSink, NcmecCyberTiplineClient,
};
use fabstir_llm_node::moderation::csam::{moderate_asset_bytes, moderate_frames};
use fabstir_llm_node::moderation::gate::{Gate, GateOutcome};
use fabstir_llm_node::moderation::ingest::{DecodedFrame, IngestItem};
use fabstir_llm_node::moderation::types::{Category, Pdq256, Verdict};
use fabstir_llm_node::moderation::verdict_store::VerdictStore;

const BAD_SHA: [u8; 32] = [0x11; 32];

fn at() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).unwrap()
}
fn loaded() -> HashListSnapshot {
    MockHashListSource::loaded(vec![BAD_SHA], vec![Pdq256([0u8; 32])])
        .refresh()
        .unwrap()
}
fn hashes(job: u64, sha: Vec<[u8; 32]>, pdq: Vec<Pdq256>) -> IngestItem {
    IngestItem::Hashes {
        job_id: job,
        sha256: sha,
        pdq,
    }
}

#[test]
fn e2e_happy_path_match_quarantine_report() {
    let result = moderate_frames(
        &hashes(1, vec![BAD_SHA], vec![]),
        &loaded(),
        &OwnHashList::new(),
        31,
    );
    assert_eq!(
        result.verdict,
        Verdict::Blocked,
        "a known-bad item must block"
    );
    let mut q = Quarantine::new(b"k".to_vec(), 90);
    let case = q.preserve(b"matched bytes", Category::Csam, at()).unwrap();
    let sink = MockReportSink::new();
    let receipt = confirm_and_file(
        &sink,
        prepare_report(&case, Category::Csam),
        &HumanConfirmation::new("alice"),
    )
    .unwrap();
    assert!(!receipt.report_id.is_empty());
    assert!(q.contains(&case), "preserved, never deleted");
}

#[test]
fn e2e_clean_clears_and_gate_releases() {
    let store = VerdictStore::new();
    let result = moderate_frames(
        &hashes(2, vec![[0x99; 32]], vec![]),
        &loaded(),
        &OwnHashList::new(),
        31,
    );
    assert_eq!(result.verdict, Verdict::Cleared);
    store.set(2, result);
    assert!(matches!(
        Gate::transcode_decision(&store, Some(2)),
        GateOutcome::Release
    ));
}

#[test]
fn e2e_kill_hash_list_holds() {
    let r = moderate_frames(
        &hashes(3, vec![[0x99; 32]], vec![Pdq256([0u8; 32])]),
        &HashListSnapshot::unavailable(),
        &OwnHashList::new(),
        31,
    );
    assert_ne!(r.verdict, Verdict::Cleared, "unavailable list ⇒ hold");
}

#[test]
fn e2e_kill_verdict_store_holds() {
    match Gate::transcode_decision(&VerdictStore::new(), Some(7)) {
        GateOutcome::Hold { code, .. } => assert_eq!(code, "MODERATION_UNAVAILABLE"),
        GateOutcome::Release => panic!("absent verdict must hold"),
    }
}

#[test]
fn e2e_kill_report_sink_keeps_item() {
    let mut q = Quarantine::new(b"k".to_vec(), 90);
    let case = q.preserve(b"x", Category::Csam, at()).unwrap();
    assert!(confirm_and_file(
        &NcmecCyberTiplineClient::new(),
        prepare_report(&case, Category::Csam),
        &HumanConfirmation::new("a")
    )
    .is_err());
    assert!(q.contains(&case), "a failed report keeps the item");
}

#[test]
fn e2e_asset_decode_failure_holds() {
    let r = moderate_asset_bytes(b"not an image", &loaded(), &OwnHashList::new(), 31);
    assert_ne!(r.verdict, Verdict::Cleared);
}

#[test]
fn e2e_empty_ingest_item_holds() {
    // Nothing to scan ⇒ HOLD (fail-closed): never clear unverified content.
    let r = moderate_frames(
        &hashes(11, vec![], vec![]),
        &loaded(),
        &OwnHashList::new(),
        31,
    );
    assert_ne!(
        r.verdict,
        Verdict::Cleared,
        "an empty ingest item must hold (nothing scanned)"
    );
}

#[test]
fn e2e_frame_with_bad_dimensions_holds() {
    // A frame whose rgb buffer doesn't match its dims can't be hashed ⇒ HOLD
    // (moderate_frames' PDQ-error path, mirroring the asset path).
    let bad = IngestItem::Frames {
        job_id: 12,
        frames: vec![DecodedFrame {
            width: 4,
            height: 4,
            rgb: vec![0u8; 3], // len != 4*4*3
        }],
        audio: None,
    };
    let r = moderate_frames(&bad, &loaded(), &OwnHashList::new(), 31);
    assert_ne!(r.verdict, Verdict::Cleared, "an unhashable frame must hold");
}

#[test]
fn e2e_both_variant_demuxes_all_components() {
    // IngestItem::Both must check every component (§3.5): a hit in ANY of
    // sha256 / pdq / frames blocks; a Both clean across all clears.
    let matching = IngestItem::Both {
        job_id: 9,
        frames: vec![],
        audio: None,
        sha256: vec![BAD_SHA],
        pdq: vec![],
    };
    assert_eq!(
        moderate_frames(&matching, &loaded(), &OwnHashList::new(), 31).verdict,
        Verdict::Blocked,
        "a Both with a matching SHA must block"
    );

    let clean = IngestItem::Both {
        job_id: 10,
        frames: vec![DecodedFrame {
            width: 8,
            height: 8,
            rgb: vec![100; 8 * 8 * 3],
        }],
        audio: None,
        sha256: vec![[0x99; 32]],
        pdq: vec![],
    };
    assert_eq!(
        moderate_frames(&clean, &loaded(), &OwnHashList::new(), 31).verdict,
        Verdict::Cleared,
        "a Both clean across SHA + PDQ + frames clears"
    );
}

#[test]
fn e2e_own_hash_reupload_auto_blocks() {
    let mut own = OwnHashList::new();
    let confirmed = [0x42; 32];
    own.add(confirmed);
    let r = moderate_frames(&hashes(8, vec![confirmed], vec![]), &loaded(), &own, 31);
    assert_eq!(
        r.verdict,
        Verdict::Blocked,
        "own-hash re-upload auto-blocks"
    );
}
