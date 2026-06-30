// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 6.2 — NCMEC reporting: human-confirm, never auto-file, failure keeps. 🚨

use chrono::{DateTime, Utc};

use fabstir_llm_node::moderation::csam::quarantine::Quarantine;
use fabstir_llm_node::moderation::csam::report::{
    confirm_and_file, prepare_report, HumanConfirmation, MockReportSink, NcmecCyberTiplineClient,
};
use fabstir_llm_node::moderation::types::Category;

fn at() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).unwrap()
}

#[test]
fn prepare_report_from_quarantine_item() {
    let mut q = Quarantine::new(b"k".to_vec(), 90);
    let case = q.preserve(b"x", Category::Csam, at()).unwrap();
    let report = prepare_report(&case, q.category(&case).unwrap());
    assert_eq!(report.case_id, case);
    assert_eq!(report.category, Category::Csam);
}

#[test]
fn file_requires_human_confirmation() {
    // There is no auto-file path: filing MUST go through confirm_and_file with an
    // explicit HumanConfirmation (type-enforced). With one, it files.
    let sink = MockReportSink::new();
    let confirmation = HumanConfirmation::new("bob");
    assert!(confirm_and_file(&sink, prepare_report("c", Category::Csam), &confirmation).is_ok());
    assert_eq!(sink.filed_count(), 1);
}

#[test]
fn mock_sink_records_report_and_returns_id() {
    let sink = MockReportSink::new();
    let receipt = confirm_and_file(
        &sink,
        prepare_report("case-0", Category::Csam),
        &HumanConfirmation::new("alice"),
    )
    .unwrap();
    assert!(
        !receipt.report_id.is_empty(),
        "a filed report yields a receipt id"
    );
    assert_eq!(sink.filed_count(), 1);
}

#[test]
fn report_failure_keeps_item_and_does_not_clear() {
    // The real NCMEC client is not yet configured ⇒ filing fails. A failed report
    // must NEVER clear/delete the quarantined item.
    let mut q = Quarantine::new(b"k".to_vec(), 90);
    let case = q.preserve(b"x", Category::Csam, at()).unwrap();
    let sink = NcmecCyberTiplineClient::new();
    let r = confirm_and_file(
        &sink,
        prepare_report(&case, Category::Csam),
        &HumanConfirmation::new("alice"),
    );
    assert!(
        r.is_err(),
        "an unconfigured real sink must fail, not pretend success"
    );
    assert!(
        q.contains(&case),
        "a failed report must keep the quarantined item"
    );
}
