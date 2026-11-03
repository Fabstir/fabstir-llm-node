// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! `{"action": "transcode_result", "taskId": ...}` (SCI-C D19(e)): fetch a
//! transcode's stored result on a new connection after the one that ordered it
//! dropped.
//!
//! It reads the result store and nothing else: it never bills, never begins a
//! unit of work, never spawns and never re-runs the job, and it takes no
//! checkpoint manager, so it cannot. It is served only to the identity pinned
//! at accept (the signer recovered from the submitting connection's init),
//! compared with the signer recovered from THIS connection's init; the answer
//! is encrypted under this connection's session key. It works on a session that
//! has since completed. A paid result is re-checked against moderation at serve
//! time, so a held job is never served.

use serde_json::{json, Value};
use tracing::{info, warn};

use super::transcode::build_encrypted_transcode_response;
use crate::api::server::ApiServer;
use crate::transcoder::result_store::{result_access, unix_now, ResultAccess};

fn fetch_error(
    task_id: &str,
    code: &str,
    message: &str,
    session_key: &[u8; 32],
    session_id: &str,
    message_id: Option<&Value>,
) -> Value {
    build_encrypted_transcode_response(
        &json!({
            "type": "transcode_error",
            "taskId": task_id,
            "error": { "code": code, "message": message },
        }),
        session_key,
        session_id,
        message_id,
    )
}

/// Answer one fetch. `signer` is the address recovered from this connection's
/// `encrypted_session_init`.
pub fn handle_transcode_result(
    server: &ApiServer,
    decrypted_json: &Value,
    session_key: &[u8; 32],
    session_id: &str,
    job_id: Option<u64>,
    signer: Option<&str>,
    message_id: Option<&Value>,
) -> Value {
    let task_id = match decrypted_json.get("taskId").and_then(|v| v.as_str()) {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => {
            return fetch_error(
                "",
                "VALIDATION_FAILED",
                "Missing or empty taskId",
                session_key,
                session_id,
                message_id,
            )
        }
    };
    let Some(store) = server.transcode_result_store() else {
        return fetch_error(
            &task_id,
            "RESULT_STORE_UNAVAILABLE",
            "This host keeps no transcode results",
            session_key,
            session_id,
            message_id,
        );
    };
    let record = job_id.and_then(|jid| store.get(jid, &task_id));
    let access = result_access(
        record.as_ref(),
        signer,
        unix_now(),
        store.retention_secs(),
        || {
            if !server.moderation_enforce() {
                return None;
            }
            match crate::moderation::gate::Gate::transcode_decision(
                server.moderation_store(),
                job_id,
            ) {
                crate::moderation::gate::GateOutcome::Release => None,
                crate::moderation::gate::GateOutcome::Hold { code, message } => {
                    Some((code.to_string(), message))
                }
            }
        },
    );
    let job = job_id.map(|j| j.to_string()).unwrap_or_else(|| "-".into());
    {
        let m = &crate::monitoring::session_completion_metrics::SCI_C;
        match &access {
            ResultAccess::Serve(_) => m.fetch_served(),
            ResultAccess::Pending => m.fetch_pending(),
            ResultAccess::NotFound => m.fetch_not_found(),
            ResultAccess::Expired => m.fetch_expired(),
            ResultAccess::NotOwner => m.fetch_not_owner(),
            ResultAccess::Failed { .. } => m.fetch_failed(),
            ResultAccess::Held { .. } => m.fetch_held(),
        }
    }
    match access {
        ResultAccess::Serve(payload) => {
            info!("[SCI-C] job {} task {}: stored result served", job, task_id);
            build_encrypted_transcode_response(&payload, session_key, session_id, message_id)
        }
        ResultAccess::Pending => build_encrypted_transcode_response(
            &json!({ "type": "transcode_result_pending", "taskId": task_id }),
            session_key,
            session_id,
            message_id,
        ),
        ResultAccess::NotFound => fetch_error(
            &task_id,
            "RESULT_NOT_FOUND",
            "No stored result for this task on this session",
            session_key,
            session_id,
            message_id,
        ),
        ResultAccess::Expired => {
            if let Some(jid) = job_id {
                store.remove(jid, &task_id);
            }
            fetch_error(
                &task_id,
                "RESULT_EXPIRED",
                "The stored result has passed its retention period",
                session_key,
                session_id,
                message_id,
            )
        }
        ResultAccess::NotOwner => {
            warn!(
                "[SCI-C] job {} task {}: result fetch refused, signer is not the owner",
                job, task_id
            );
            fetch_error(
                &task_id,
                "NOT_OWNER",
                "Only the client that ordered this transcode may fetch its result",
                session_key,
                session_id,
                message_id,
            )
        }
        ResultAccess::Failed { code, message } => fetch_error(
            &task_id,
            &code,
            &message,
            session_key,
            session_id,
            message_id,
        ),
        ResultAccess::Held { code, message } => {
            warn!(
                "[SCI-C] job {} task {}: stored result held by moderation at serve time",
                job, task_id
            );
            fetch_error(
                &task_id,
                &code,
                &message,
                session_key,
                session_id,
                message_id,
            )
        }
    }
}
