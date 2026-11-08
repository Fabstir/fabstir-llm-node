// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Settled-session notices (node 8.63.0, feature `session-settled-notice`).
//!
//! The node settles sessions on its own: when a job's last connection closes,
//! after the transcode grace, at the end of an LTX clip or a training run, on
//! the expiry sweep, or when a chain read finds the session already ended. A
//! client still connected to the session had no way to know, so it could send
//! work the node would answer but never bill.
//!
//! Every node completion goes through `CheckpointManager::complete_session_job`,
//! which records the job here the moment completion is decided, and every
//! connection is subscribed. A connection carrying that job is then sent one
//! plaintext `session_settled` frame, and new work on the job is refused with
//! `SESSION_SETTLED`. `encrypted_session_init` itself is not refused, because a
//! client fetching a dropped transcode's result inits on a completed session;
//! the init succeeds and the notice follows it. The result fetch is never refused.
//!
//! In memory only: after a restart the node no longer knows which sessions it
//! settled, and the client's own chain read covers that.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Value};
use tokio::sync::broadcast;

/// Why a session was settled, as sent in the frame's `reason`.
pub mod reason {
    pub const LAST_CONNECTION_CLOSED: &str = "last_connection_closed";
    pub const WORK_FINISHED: &str = "work_finished";
    pub const LTX_FINISHED: &str = "ltx_finished";
    pub const TRAINING_FINISHED: &str = "training_finished";
    pub const EXPIRED: &str = "expired";
    pub const SETTLED_ON_CHAIN: &str = "settled_on_chain";
    pub const COMPLETION: &str = "completion";
}

/// How many notices a slow connection may fall behind before it re-reads the
/// registry instead.
const CHANNEL_CAPACITY: usize = 256;

pub struct SettledSessions {
    jobs: Mutex<HashMap<u64, &'static str>>,
    tx: broadcast::Sender<(u64, &'static str)>,
}

impl Default for SettledSessions {
    fn default() -> Self {
        Self::new()
    }
}

impl SettledSessions {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self {
            jobs: Mutex::new(HashMap::new()),
            tx,
        }
    }

    /// The node has decided to settle `job_id`. The first reason recorded is
    /// kept; only the first announcement is broadcast.
    pub fn announce(&self, job_id: u64, why: &'static str) {
        let first = {
            let mut jobs = self.jobs.lock().unwrap_or_else(|p| p.into_inner());
            if jobs.contains_key(&job_id) {
                false
            } else {
                jobs.insert(job_id, why);
                true
            }
        };
        if first {
            tracing::info!(
                "[SCI-C] job {}: session settling ({}), notifying its connections",
                job_id,
                why
            );
            let _ = self.tx.send((job_id, why));
        }
    }

    /// Why `job_id` was settled, if the node has settled it.
    pub fn reason_for(&self, job_id: u64) -> Option<&'static str> {
        self.jobs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&job_id)
            .copied()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<(u64, &'static str)> {
        self.tx.subscribe()
    }
}

/// The job a request bills: its own `job_id`, else its session id (the SDK
/// sends `"139n"`, so a trailing `n` is stripped). The inference cores derive
/// it with this rule, and the settled gates check the same job (8.63.1).
pub fn request_job(job_id: Option<u64>, session_id: Option<&str>) -> Option<u64> {
    job_id.or_else(|| session_id.and_then(|sid| sid.trim_end_matches('n').parse::<u64>().ok()))
}

/// The refusal body of `POST /v1/inference`, sent with HTTP 409 (8.63.1).
pub fn refused_body(job_id: u64) -> Value {
    json!({
        "error": "SESSION_SETTLED",
        "code": "SESSION_SETTLED",
        "job_id": job_id.to_string(),
        "message": format!("session {job_id} has been settled; open a new session for new work"),
    })
}

/// The plaintext notice. It carries no secret.
pub fn settled_frame(session_id: Option<&str>, job_id: u64, why: &str) -> Value {
    json!({
        "type": "session_settled",
        "session_id": session_id.unwrap_or("unknown"),
        "job_id": job_id.to_string(),
        "reason": why,
    })
}

/// The refusal sent for new work on a settled session.
pub fn refused_frame(session_id: Option<&str>, msg_id: Option<&Value>, job_id: u64) -> Value {
    let mut frame = json!({
        "type": "error",
        "code": "SESSION_SETTLED",
        "session_id": session_id.unwrap_or("unknown"),
        "message": format!("session {job_id} has been settled; open a new session for new work"),
    });
    if let Some(id) = msg_id {
        frame["id"] = id.clone();
    }
    frame
}

/// Does this frame start new work, and so get refused on a settled session?
/// `frame_type` is the outer frame's `type`; `action` the decrypted action of an
/// `encrypted_message` (`None` for a prompt). Inits, cancels, the result fetch and
/// anything else that starts no work pass; a transcode is refused by its own
/// accept gate instead.
pub fn refused_when_settled(frame_type: &str, action: Option<&str>) -> bool {
    match frame_type {
        "prompt" | "inference" | "uploadVectors" | "searchVectors" | "searchRequest" => true,
        // A transcode keeps the encrypted SESSION_SETTLED it has had since
        // 8.62.0, sent by its own accept gate.
        "encrypted_message" => !matches!(
            action,
            Some("transcode")
                | Some("transcode_result")
                | Some("transcode_cancel")
                | Some("ltx_cancel")
                | Some("train_cancel")
        ),
        _ => false,
    }
}
