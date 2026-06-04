// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! `POST /v1/moderate/review` — access-restricted confirm + NCMEC report of a
//! quarantined item, with an audit entry. 🚨

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::api::server::ApiServer;
use crate::moderation::csam::quarantine::{Quarantine, Role};
use crate::moderation::csam::report::{
    confirm_and_file, prepare_report, HumanConfirmation, ReportSink,
};

/// Launch mock reviewer token (Q-admin): the real auth/credential swaps in at
/// go-live. A request whose token does not match is `Unauthorised`.
pub const AUTHORISED_REVIEWER_TOKEN: &str = "mock-reviewer-token";

/// Resolve a reviewer token to a [`Role`]. Authorisation is decided here (server
/// side), never self-declared by the request body.
pub fn resolve_role(token: &str) -> Role {
    if token == AUTHORISED_REVIEWER_TOKEN {
        Role::Reviewer
    } else {
        Role::Unauthorised
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerateReviewRequest {
    pub case_id: String,
    pub reviewer: String,
    pub reviewer_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerateReviewResponse {
    pub case_id: String,
    #[serde(rename = "reportId")]
    pub report_id: String,
    pub status: String,
}

/// Confirm + report a quarantined item. Access-restricted (unauthorised ⇒ 403,
/// audited); unknown case ⇒ 404; a report-sink failure ⇒ 502 and the item is kept
/// (never cleared). Every outcome is audited.
pub fn moderate_review_inner(
    q: &mut Quarantine,
    sink: &dyn ReportSink,
    role: Role,
    req: &ModerateReviewRequest,
    now: DateTime<Utc>,
) -> Result<ModerateReviewResponse, (StatusCode, String)> {
    if role != Role::Reviewer {
        q.audit_action(
            &req.reviewer,
            &format!("review-denied:{}", req.case_id),
            now,
        );
        return Err((StatusCode::FORBIDDEN, "unauthorised reviewer".into()));
    }
    let category = match q.category(&req.case_id) {
        Some(c) => c,
        None => {
            // Audit the attempt even on an unknown case (every outcome is audited).
            q.audit_action(
                &req.reviewer,
                &format!("review-unknown-case:{}", req.case_id),
                now,
            );
            return Err((
                StatusCode::NOT_FOUND,
                format!("no such case {}", req.case_id),
            ));
        }
    };
    let report = prepare_report(&req.case_id, category);
    let receipt = match confirm_and_file(sink, report, &HumanConfirmation::new(&req.reviewer)) {
        Ok(r) => r,
        Err(e) => {
            // Audit the failed report (the item is kept; the failure is on the record).
            q.audit_action(
                &req.reviewer,
                &format!("review-failed:{}:{e}", req.case_id),
                now,
            );
            return Err((StatusCode::BAD_GATEWAY, e.to_string()));
        }
    };
    q.audit_action(
        &req.reviewer,
        &format!("review-reported:{}:{}", req.case_id, receipt.report_id),
        now,
    );
    Ok(ModerateReviewResponse {
        case_id: req.case_id.clone(),
        report_id: receipt.report_id,
        status: "reported".into(),
    })
}

/// `POST /v1/moderate/review`.
pub async fn moderate_review_handler(
    State(server): State<Arc<ApiServer>>,
    Json(req): Json<ModerateReviewRequest>,
) -> impl IntoResponse {
    let role = resolve_role(&req.reviewer_token);
    let sink = server.moderation_report_sink();
    let mut q = server
        .moderation_quarantine()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    match moderate_review_inner(&mut q, sink.as_ref(), role, &req, Utc::now()) {
        Ok(resp) => {
            server.moderation_metrics().record_report_filed(); // §8 #7
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err((status, msg)) => (status, Json(serde_json::json!({ "error": msg }))).into_response(),
    }
}
