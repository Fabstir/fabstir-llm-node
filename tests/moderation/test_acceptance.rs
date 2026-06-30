// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 7.2 — host-side legal-hard-gate acceptance (§8). Codifies the criteria
//! this repo owns (the cross-repo dependencies in §8a are NOT asserted here).

use chrono::{DateTime, Duration, Utc};

use fabstir_llm_node::moderation::csam::hashlist::{HashListSnapshot, HashListSource};
use fabstir_llm_node::moderation::csam::matcher::Matcher;
use fabstir_llm_node::moderation::csam::mock_source::MockHashListSource;
use fabstir_llm_node::moderation::csam::ownhash::OwnHashList;
use fabstir_llm_node::moderation::csam::quarantine::Quarantine;
use fabstir_llm_node::moderation::csam::report::{
    confirm_and_file, prepare_report, HumanConfirmation, MockReportSink,
};
use fabstir_llm_node::moderation::gate::Gate;
use fabstir_llm_node::moderation::types::{Category, ModerationError, ModerationResult, Pdq256};

fn at() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).unwrap()
}

#[test]
fn acceptance_1_track1_engine_live_behind_mock() {
    let snap = MockHashListSource::loaded(vec![[0x11; 32]], vec![Pdq256([0u8; 32])])
        .refresh()
        .unwrap();
    let own = OwnHashList::new();
    let m = Matcher::new(&snap, &own);
    assert!(m.match_sha256(&[0x11; 32]).unwrap().is_match, "exact hit");
    assert!(
        m.match_pdq(&Pdq256([0u8; 32]), 31).unwrap().is_match,
        "pdq hit"
    );
    assert!(
        !m.match_sha256(&MockHashListSource::BENIGN_CONTROL_SHA256)
            .unwrap()
            .is_match,
        "benign control does not match"
    );
    let unavailable = HashListSnapshot::unavailable();
    let mu = Matcher::new(&unavailable, &own);
    assert!(
        mu.match_sha256(&[0x11; 32]).is_err(),
        "unavailable list ⇒ hold"
    );
}

#[test]
fn acceptance_2_fail_closed_only_cleared_releases() {
    assert!(Gate::decide(Ok(Some(&ModerationResult::cleared()))).releases());
    assert!(!Gate::decide(Ok(Some(&ModerationResult::blocked("x")))).releases());
    assert!(!Gate::decide(Ok(Some(&ModerationResult::flagged("x")))).releases());
    assert!(!Gate::decide(Ok(None)).releases());
    assert!(!Gate::decide(Err(ModerationError::ListUnavailable)).releases());
}

#[test]
fn acceptance_3_tested_report_path_with_human_confirm() {
    let sink = MockReportSink::new();
    let r = confirm_and_file(
        &sink,
        prepare_report("case-0", Category::Csam),
        &HumanConfirmation::new("reviewer"),
    )
    .unwrap();
    assert!(!r.report_id.is_empty());
    assert_eq!(sink.filed_count(), 1);
}

#[test]
fn acceptance_4_never_auto_delete_5_audited() {
    let mut q = Quarantine::new(b"k".to_vec(), 90);
    let c = q.preserve(b"x", Category::Csam, at()).unwrap();
    assert!(q.contains(&c), "preserved (no delete API exists)");
    assert!(!q.audit_log().is_empty(), "every decision is audited");
    assert!(
        q.retain_until(&c).unwrap() >= at() + Duration::days(90),
        "retention >= 90 days"
    );
}

#[test]
fn acceptance_6_live_path_csam_match_preserves_evidence() {
    // 🚨 B6 live-path closure (§8 #4/#5): a CSAM match through the PRODUCTION handler
    // core preserves evidence — the ORIGINAL received bytes, encrypted, never-deleted,
    // audited, with the kind-derived `Csam` category — not merely in an isolated
    // quarantine unit test. The committed node detected+blocked but never preserved in
    // the live path; this asserts the wiring is now present.
    use std::sync::Mutex;

    use base64::Engine;
    use fabstir_llm_node::api::moderation::{
        moderate_asset_inner_preserving, ModerateAssetRequest,
    };
    use fabstir_llm_node::moderation::asset::{AssetModerator, TextScanList};
    use fabstir_llm_node::moderation::csam::quarantine::Role;

    // A known-bad image (seed its SHA into the mock list ⇒ exact match ⇒ blocked).
    let rgb = image::RgbImage::from_pixel(8, 8, image::Rgb([9, 8, 7]));
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(rgb)
        .write_to(&mut buf, image::ImageFormat::Png)
        .unwrap();
    let png = buf.into_inner();
    let sha = Matcher::sha256(&png);

    let snapshot = MockHashListSource::loaded(vec![sha], vec![])
        .refresh()
        .unwrap();
    let am = AssetModerator::new(
        snapshot,
        OwnHashList::new(),
        31,
        TextScanList::launch_mock(),
    );
    let req = ModerateAssetRequest {
        kind: "image".into(),
        data: base64::engine::general_purpose::STANDARD.encode(&png),
    };
    let q = Mutex::new(Quarantine::new(b"acceptance-key".to_vec(), 90));

    let resp = moderate_asset_inner_preserving(&am, &req, 20 * 1024 * 1024, &q, at()).unwrap();
    assert_eq!(
        resp.verdict, "blocked",
        "a CSAM match blocks in the live path"
    );

    let mut guard = q.lock().unwrap();
    assert_eq!(
        guard.len(),
        1,
        "the blocked content is PRESERVED in the live path (B6), not just detected"
    );
    assert_eq!(
        guard.category("case-0"),
        Some(Category::Csam),
        "kind-derived Csam category (not reason-parsed)"
    );
    assert!(
        guard.retain_until("case-0").unwrap() >= at() + Duration::days(90),
        "retention >= 90 days, never-deleted"
    );
    assert!(
        !guard.audit_log().is_empty(),
        "the live-path preserve is audited"
    );
    let got = guard
        .retrieve("case-0", Role::Reviewer, "acceptance", at())
        .unwrap();
    assert_eq!(
        got, png,
        "the ORIGINAL received bytes are preserved (re-hashable evidence)"
    );
}
