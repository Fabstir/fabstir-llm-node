// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 6.3 — review-action endpoint logic (access-restricted, audited). 🚨

use axum::http::StatusCode;
use chrono::{DateTime, Utc};

use fabstir_llm_node::api::moderation::{
    moderate_review_inner, resolve_role, ModerateReviewRequest, AUTHORISED_REVIEWER_TOKEN,
};
use fabstir_llm_node::moderation::csam::quarantine::{Quarantine, Role};
use fabstir_llm_node::moderation::csam::report::{MockReportSink, NcmecCyberTiplineClient};
use fabstir_llm_node::moderation::types::Category;

fn at() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).unwrap()
}

fn seeded() -> (Quarantine, String) {
    let mut q = Quarantine::new(b"k".to_vec(), 90);
    let case = q.preserve(b"x", Category::Csam, at()).unwrap();
    (q, case)
}

fn req(case: &str, reviewer: &str) -> ModerateReviewRequest {
    ModerateReviewRequest {
        case_id: case.to_string(),
        reviewer: reviewer.to_string(),
        reviewer_token: "ignored-here".to_string(),
    }
}

#[test]
fn authorised_reviewer_can_confirm_and_report() {
    let (mut q, case) = seeded();
    let sink = MockReportSink::new();
    let resp =
        moderate_review_inner(&mut q, &sink, Role::Reviewer, &req(&case, "alice"), at()).unwrap();
    assert_eq!(resp.case_id, case);
    assert!(!resp.report_id.is_empty());
    assert_eq!(sink.filed_count(), 1);
}

#[test]
fn unauthorised_rejected() {
    let (mut q, case) = seeded();
    let sink = MockReportSink::new();
    let err = moderate_review_inner(
        &mut q,
        &sink,
        Role::Unauthorised,
        &req(&case, "mallory"),
        at(),
    )
    .unwrap_err();
    assert_eq!(err.0, StatusCode::FORBIDDEN);
    assert_eq!(sink.filed_count(), 0, "unauthorised must not file");
}

#[test]
fn action_is_audited() {
    let (mut q, case) = seeded();
    let sink = MockReportSink::new();
    let before = q.audit_log().len();
    moderate_review_inner(&mut q, &sink, Role::Reviewer, &req(&case, "alice"), at()).unwrap();
    assert!(
        q.audit_log().len() > before,
        "a review action must be audited"
    );
    assert!(q.audit_log().iter().any(|e| e.action.contains("review")));
}

#[test]
fn unauthorised_attempt_is_audited() {
    let (mut q, case) = seeded();
    let sink = MockReportSink::new();
    let _ = moderate_review_inner(
        &mut q,
        &sink,
        Role::Unauthorised,
        &req(&case, "mallory"),
        at(),
    );
    assert!(q.audit_log().iter().any(|e| e.action.contains("denied")));
}

#[test]
fn unknown_case_not_found() {
    let mut q = Quarantine::new(b"k".to_vec(), 90);
    let sink = MockReportSink::new();
    let err = moderate_review_inner(&mut q, &sink, Role::Reviewer, &req("nope", "alice"), at())
        .unwrap_err();
    assert_eq!(err.0, StatusCode::NOT_FOUND);
    assert!(
        q.audit_log()
            .iter()
            .any(|e| e.action.contains("unknown-case")),
        "an unknown-case attempt must still be audited"
    );
}

#[test]
fn report_failure_returns_error_and_keeps_item() {
    let (mut q, case) = seeded();
    let sink = NcmecCyberTiplineClient::new();
    let err = moderate_review_inner(&mut q, &sink, Role::Reviewer, &req(&case, "alice"), at())
        .unwrap_err();
    assert_eq!(err.0, StatusCode::BAD_GATEWAY);
    assert!(q.contains(&case), "a failed report must keep the item");
    assert!(
        q.audit_log()
            .iter()
            .any(|e| e.action.contains("review-failed")),
        "a failed report must still be audited"
    );
}

#[test]
fn resolve_role_maps_token() {
    assert_eq!(resolve_role(AUTHORISED_REVIEWER_TOKEN), Role::Reviewer);
    assert_eq!(resolve_role("not-the-token"), Role::Unauthorised);
}
