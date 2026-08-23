// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The `train` action handler (TD13: thin — every decision lives in
//! `crate::training::core`). Two-phase like LTX: this handler validates,
//! takes the TD14 GPU permit, runs SESSION-level acceptance (A.3), and
//! returns `(encrypted ack, Option<TrainTask>)`; the caller writes the ack
//! (bounded), then spawns the task, whose dataset legs + slice loop land at
//! T4 (progress frames cover staging/scanning/counting per the protocol).
//!
//! Reject placement rules (recorded; realigned to C.3's universal rule in
//! the T3 converge round):
//! - No chain job id / unparseable `TrainingJob` → `VALIDATION_FAILED` (the
//!   A.1 numeric wire rule surfaces HERE as a serde failure); training
//!   disabled → `SIDECAR_UNAVAILABLE`. Both are PRE-READ: no session was
//!   verified, so nothing is consumed or settled.
//! - The TD14 GPU permit is taken AFTER `accept_session` (the converge round
//!   proved permit-before-reads let cheap garbage requests starve LTX
//!   through the shared semaphore): a permit failure then consumes + settles
//!   the session per C.3's CAPACITY row — sessions are host-bound, so an
//!   unsettled capacity reject would strand the deposit until timeout.

use std::sync::Arc;

use rand::RngCore;
use serde_json::{json, Value};
use tokio::sync::OwnedSemaphorePermit;
use tracing::error;

use crate::training::core::{
    accept_session, AcceptedSession, TrainReject, TrainingDeps, CAPACITY, SIDECAR_UNAVAILABLE,
    VALIDATION_FAILED,
};
use crate::training::types::TrainingJob;

/// The spawned half of a `train` (T4.2 gives it `spawn`): holds the accepted
/// session, the wire job, and the TD14 permit for the run's whole lifetime.
pub struct TrainTask {
    pub job_id: u64,
    pub job: TrainingJob,
    pub accepted: AcceptedSession,
    pub permit: OwnedSemaphorePermit,
}

/// `{type:"train_accepted", status, sessionId, allowListVersion, billing,
/// schedule}` (interface protocol item 1).
pub fn train_accepted_inner(
    job_id: u64,
    allow_list_version: u64,
    accepted: &AcceptedSession,
    slice_tokens: u64,
    request_id: Option<&str>,
) -> Value {
    let mut v = json!({
        "type": "train_accepted",
        "status": "processing",
        "sessionId": job_id,
        "allowListVersion": allow_list_version,
        "billing": {
            "unit": "training-token",
            "tokens": accepted.training_tokens,
            // The VERIFIED on-chain session price (A.3) — never an env value.
            "pricePerToken": accepted.snapshot.price_per_token.to_string(),
        },
        "schedule": {
            "sliceTokens": slice_tokens,
            "slices": accepted.schedule.len(),
        },
    });
    if let Some(r) = request_id {
        v["requestId"] = json!(r);
    }
    v
}

/// `{type:"train_error", error:{code, message, detail?}}` (protocol item 4).
pub fn train_error_inner(reject: &TrainReject, request_id: Option<&str>) -> Value {
    let mut error = json!({ "code": reject.code, "message": reject.detail });
    let mut detail = serde_json::Map::new();
    if let Some(reason) = reject.reason {
        detail.insert("reason".to_string(), json!(reason));
    }
    if let Some((declared, actual)) = reject.declared_actual {
        detail.insert("declared".to_string(), json!(declared));
        detail.insert("actual".to_string(), json!(actual));
    }
    if !detail.is_empty() {
        error["detail"] = Value::Object(detail);
    }
    let mut v = json!({ "type": "train_error", "error": error });
    if let Some(r) = request_id {
        v["requestId"] = json!(r);
    }
    v
}

/// Encrypted `encrypted_response` envelope with the fixed per-handler AAD
/// `encrypted_train_response` (interface-fixed; the LTX/transcode mirror).
pub fn build_encrypted_train_response(
    inner: &Value,
    session_key: &[u8; 32],
    session_id: &str,
    message_id: Option<&Value>,
) -> Value {
    let plaintext = serde_json::to_vec(inner).unwrap_or_default();
    let mut nonce = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce);
    let aad = b"encrypted_train_response";
    match crate::crypto::encrypt_with_aead(&plaintext, &nonce, aad, session_key) {
        Ok(ciphertext) => {
            let mut msg = json!({
                "type": "encrypted_response",
                "payload": {
                    "ciphertextHex": hex::encode(&ciphertext),
                    "nonceHex": hex::encode(nonce),
                    "aadHex": hex::encode(aad),
                },
                "session_id": session_id,
            });
            if let Some(mid) = message_id {
                msg["id"] = mid.clone();
            }
            msg
        }
        Err(e) => {
            error!("Failed to encrypt train response: {}", e);
            let mut msg = json!({
                "type": "error",
                "code": "ENCRYPTION_FAILED",
                "message": format!("Failed to encrypt response: {}", e),
                "session_id": session_id,
            });
            if let Some(mid) = message_id {
                msg["id"] = mid.clone();
            }
            msg
        }
    }
}

/// `TRAIN_WS_WRITE_TIMEOUT_SECS` (default 900 — a 300 s stall at hour 3 must
/// not kill a paid run; the interface pins the training default).
pub fn train_ws_write_timeout() -> std::time::Duration {
    let secs = std::env::var("TRAIN_WS_WRITE_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(900);
    std::time::Duration::from_secs(secs)
}

/// Handle one decrypted `{"action":"train", …}` message. Returns the
/// ENCRYPTED ack/error envelope and, on acceptance, the task to spawn.
#[allow(clippy::too_many_arguments)]
pub async fn handle_encrypted_train(
    deps: Option<Arc<TrainingDeps>>,
    gpu_semaphore: Arc<tokio::sync::Semaphore>,
    allow_list_version: u64,
    decrypted: &Value,
    session_key: &[u8; 32],
    session_id: &str,
    chain_job_id: Option<u64>,
    message_id: Option<&Value>,
    now_secs: u64,
) -> (Value, Option<TrainTask>) {
    let request_id = decrypted.get("requestId").and_then(|v| v.as_str());
    let reject_only = |reject: TrainReject| {
        (
            build_encrypted_train_response(
                &train_error_inner(&reject, request_id),
                session_key,
                session_id,
                message_id,
            ),
            None,
        )
    };

    let Some(job_id) = chain_job_id else {
        return reject_only(TrainReject {
            code: VALIDATION_FAILED,
            reason: None,
            detail: "train requires an on-chain session job id".to_string(),
            declared_actual: None,
        });
    };
    let Some(deps) = deps else {
        return reject_only(TrainReject {
            code: SIDECAR_UNAVAILABLE,
            reason: None,
            detail: "training is not enabled on this host".to_string(),
            declared_actual: None,
        });
    };
    // A.1 numeric wire rule: null / missing / non-number members FAIL here.
    let job: TrainingJob = match serde_json::from_value(decrypted.clone()) {
        Ok(job) => job,
        Err(e) => {
            return reject_only(TrainReject {
                code: VALIDATION_FAILED,
                reason: None,
                detail: format!("train job failed validation: {e}"),
                declared_actual: None,
            })
        }
    };
    match accept_session(&deps, job_id, &job, now_secs).await {
        Ok(accepted) => {
            // TD14: one GPU workload at a time — the SAME semaphore LTX
            // holds, taken only now that the session is verified (never
            // held across the RPC reads). A busy GPU is a consuming
            // CAPACITY reject per C.3.
            let Ok(permit) = gpu_semaphore.try_acquire_owned() else {
                let reject = TrainReject {
                    code: CAPACITY,
                    reason: None,
                    detail: "GPU busy (cross-workload exclusion)".to_string(),
                    declared_actual: None,
                };
                crate::training::core::terminal_reject_effects(
                    &deps,
                    job_id,
                    &accepted.snapshot,
                    now_secs,
                    false, // capacity back-off: consume + settle, no cooldown arm
                )
                .await;
                return reject_only(reject);
            };
            let ack = build_encrypted_train_response(
                &train_accepted_inner(
                    job_id,
                    allow_list_version,
                    &accepted,
                    deps.template.slice_tokens,
                    request_id,
                ),
                session_key,
                session_id,
                message_id,
            );
            (
                ack,
                Some(TrainTask {
                    job_id,
                    job,
                    accepted,
                    permit,
                }),
            )
        }
        Err(reject) => reject_only(reject),
    }
}

// ---------------------------------------------------------------------------
// T4.f — the task execution layer: the accepted task runs the dataset legs +
// slice loop, serialises RunProgress into the interface's wire frames
// (encrypted), self-ticks the ≤60 s liveness heartbeat, and owns the
// end-of-run completion after the dispute wait. The WS dispatch drains the
// returned channel with the bounded writer + the `train_cancel` select.
// ---------------------------------------------------------------------------

use crate::training::core::{
    prepare_dataset, run_training_session, terminal_reject_effects, RunEnd, RunProgress,
};

/// `{type:"train_progress", stage, slice?/checkpoint?/adapter?}` frames.
fn train_progress_inner(stage: &str, extra: Option<(&str, Value)>) -> Value {
    let mut v = json!({ "type": "train_progress", "stage": stage });
    if let Some((key, value)) = extra {
        v[key] = value;
    }
    v
}

fn frame_for_progress(progress: &RunProgress) -> Value {
    match progress {
        RunProgress::Stage { stage } => train_progress_inner(stage, None),
        RunProgress::Uploading {
            manifest_cid,
            manifest_sha256,
            size_bytes,
            ..
        } => train_progress_inner(
            "uploading",
            Some((
                "checkpoint",
                json!({ "manifestCID": manifest_cid, "manifestSha256": manifest_sha256, "sizeBytes": size_bytes }),
            )),
        ),
        RunProgress::FinalisingAdapter {
            manifest_cid,
            manifest_sha256,
        } => train_progress_inner(
            "finalising",
            Some((
                "adapter",
                json!({ "manifestCID": manifest_cid, "manifestSha256": manifest_sha256 }),
            )),
        ),
        RunProgress::SliceSettled {
            index,
            step_from,
            step_to,
            tokens_delta,
            cumulative_tokens,
            checkpoint,
            proof_cid,
            submitted,
        } => train_progress_inner(
            "training",
            Some((
                "slice",
                json!({
                    "index": index,
                    "stepFrom": step_from,
                    "stepTo": step_to,
                    "tokensDelta": tokens_delta,
                    "cumulativeTokens": cumulative_tokens,
                    "checkpoint": {
                        "manifestCID": checkpoint.manifest_cid,
                        "manifestSha256": checkpoint.manifest_sha256,
                        "sizeBytes": checkpoint.total_bytes,
                    },
                    "proof": { "proofCID": proof_cid, "submitted": submitted },
                }),
            )),
        ),
    }
}

fn billing_json(billing: &crate::training::core::TrainRunEndBilling, price: &str) -> Value {
    json!({
        "unit": "training-token",
        "tokens": billing.billed_tokens,
        "pricePerToken": price,
    })
}

impl TrainTask {
    /// Run the accepted task to its end. Every outbound wire frame (already
    /// ENCRYPTED) goes to `out`; the dispatch layer writes them with the
    /// bounded writer. `cancel` is the `train_cancel`/disconnect flag.
    /// `heartbeat` is the ≤60 s liveness cadence (tests shrink it).
    #[allow(clippy::too_many_arguments)]
    pub async fn execute(
        self,
        deps: Arc<TrainingDeps>,
        session_key: [u8; 32],
        session_id: String,
        request_id: Option<String>,
        out: tokio::sync::mpsc::Sender<Value>,
        cancel: Arc<std::sync::atomic::AtomicBool>,
        heartbeat: std::time::Duration,
        silence_timeout: std::time::Duration,
        now_secs: u64,
    ) {
        let TrainTask {
            job_id,
            job,
            accepted,
            permit,
        } = self;
        let request_id_ref = request_id.as_deref();
        let send_inner = |inner: Value| {
            let out = out.clone();
            let envelope = build_encrypted_train_response(&inner, &session_key, &session_id, None);
            async move {
                let _ = out.send(envelope).await;
            }
        };

        // Progress plumbing + the ≤60 s heartbeat (re-emits the last stage).
        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel::<RunProgress>(64);
        let last_stage = Arc::new(std::sync::Mutex::new("staging"));
        let heartbeat_stage = last_stage.clone();
        let heartbeat_out = out.clone();
        let heartbeat_key = session_key;
        let heartbeat_session = session_id.clone();
        // Abort-on-drop guard: if this future unwinds/cancels anywhere, the
        // heartbeat must die with it — a surviving heartbeat holds the frame
        // channel open and wedges the dispatch drain forever (T4 converge
        // round, the zombie chain).
        struct AbortOnDrop(tokio::task::JoinHandle<()>);
        impl Drop for AbortOnDrop {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let heartbeat_task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(heartbeat).await;
                let stage = *heartbeat_stage.lock().unwrap();
                let envelope = build_encrypted_train_response(
                    &train_progress_inner(stage, None),
                    &heartbeat_key,
                    &heartbeat_session,
                    None,
                );
                if heartbeat_out.send(envelope).await.is_err() {
                    return;
                }
            }
        });
        let heartbeat_guard = AbortOnDrop(heartbeat_task);
        let forward_stage = last_stage.clone();
        let forward_out = out.clone();
        let forward_key = session_key;
        let forward_session = session_id.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(progress) = progress_rx.recv().await {
                if let RunProgress::Stage { stage } = &progress {
                    *forward_stage.lock().unwrap() = stage;
                }
                let envelope = build_encrypted_train_response(
                    &frame_for_progress(&progress),
                    &forward_key,
                    &forward_session,
                    None,
                );
                if forward_out.send(envelope).await.is_err() {
                    return;
                }
            }
        });

        // 1. Dataset legs (staging/scanning/counting frames from core).
        let prepared =
            match prepare_dataset(&deps, job_id, &job, &accepted, now_secs, Some(&progress_tx))
                .await
            {
                Ok(prepared) => prepared,
                Err(reject) => {
                    // Terminal frame LAST: drain the forwarder AND stop the
                    // heartbeat before sending it — the heartbeat writes
                    // straight into `out`, bypassing the forwarder, so
                    // draining alone left it able to trail the terminal
                    // (round-2 F2, completed in round 3).
                    drop(progress_tx);
                    let _ = forwarder.await;
                    drop(heartbeat_guard);
                    send_inner(train_error_inner(&reject, request_id_ref)).await;
                    drop(permit);
                    return;
                }
            };

        // 2. The sidecar stream + the slice loop.
        let wire = crate::training::trainer_client::TrainWireRequest {
            job_id,
            dataset_path: prepared.staged_dataset.to_string_lossy().to_string(),
            declared_tokens: job.dataset.declared_tokens,
            epochs: job.epochs,
            hyper: job.hyper.clone(),
        };
        let stream = match deps.trainer.train(&wire, silence_timeout).await {
            Ok(stream) => stream,
            Err(failure) => {
                // Pre-stream sidecar failure AFTER acceptance: consuming
                // (C.3) — SLOT_BUSY → CAPACITY, else the §3.7 class.
                let reject = match &failure {
                    crate::training::trainer_client::SidecarFailure::Envelope {
                        kind,
                        detail,
                        ..
                    } if kind == "SLOT_BUSY" => TrainReject {
                        code: crate::training::core::CAPACITY,
                        reason: None,
                        detail: detail.clone(),
                        declared_actual: None,
                    },
                    other => TrainReject {
                        code: crate::training::core::SIDECAR_UNAVAILABLE,
                        reason: None,
                        detail: crate::training::redact::opaque(
                            "train open failed",
                            format!("{other:?}"),
                        ),
                        declared_actual: None,
                    },
                };
                // C.6: a post-pipeline SIDECAR_UNAVAILABLE did real work —
                // it ARMS the cooldown; only the SLOT_BUSY capacity back-off
                // does not (converge round F7).
                let arm = reject.code != crate::training::core::CAPACITY;
                terminal_reject_effects(&deps, job_id, &accepted.snapshot, now_secs, arm).await;
                // Terminal frame LAST — forwarder drained AND heartbeat
                // stopped first (round-2 F2, completed in round 3).
                drop(progress_tx);
                let _ = forwarder.await;
                drop(heartbeat_guard);
                send_inner(train_error_inner(&reject, request_id_ref)).await;
                drop(permit);
                return;
            }
        };
        let price = prepared.price_per_token.to_string();
        let end = run_training_session(
            &deps,
            job_id,
            &job,
            &prepared,
            &accepted,
            stream,
            progress_tx,
            cancel,
            now_secs,
        )
        .await;
        // The GPU is free the moment the run ends — chain bookkeeping and
        // frame delivery must not hold the cross-workload permit through a
        // dispute wait or a wedged writer (converge round F4).
        drop(permit);
        let _ = forwarder.await; // progress_tx moved into the loop; drained
        drop(heartbeat_guard); // stops the ≤60 s ticker

        // 3. Terminal frame + the end-of-run completion.
        let window = deps.sessions.dispute_window_secs().await + deps.settle_buffer_secs;
        match &end {
            RunEnd::Complete {
                adapter,
                billing,
                proof_cids,
                warnings,
            } => {
                let mut inner = json!({
                    "type": "train_complete",
                    "adapter": {
                        "manifestCID": adapter.manifest_cid,
                        "manifestSha256": adapter.manifest_sha256,
                    },
                    "billing": billing_json(billing, &price),
                    "proofCIDs": proof_cids,
                    "moderation": {
                        "status": prepared.verdict,
                        "policyVersion": prepared.policy_version,
                    },
                });
                if !warnings.is_empty() {
                    inner["warnings"] = json!(warnings);
                }
                if let Some(r) = request_id_ref {
                    inner["requestId"] = json!(r);
                }
                send_inner(inner).await;
            }
            RunEnd::Failed {
                code,
                detail,
                billing,
                last_checkpoint,
            } => {
                let mut error = json!({ "code": code, "message": detail });
                let mut extra = serde_json::Map::new();
                // C.1: k = EXECUTED slices (landed + forfeited) — the wire
                // bill is Σ delta[0..k−1]; landed-only here broke the frozen
                // formula when a slice forfeited (converge round F3).
                extra.insert(
                    "settledSlices".into(),
                    json!(billing.settled_slices + billing.forfeited_slices),
                );
                extra.insert("billedTokens".into(), json!(billing.billed_tokens));
                if let Some(checkpoint) = last_checkpoint {
                    extra.insert(
                        "lastCheckpoint".into(),
                        json!({
                            "manifestCID": checkpoint.manifest_cid,
                            "manifestSha256": checkpoint.manifest_sha256,
                        }),
                    );
                }
                error["detail"] = Value::Object(extra);
                let mut inner = json!({ "type": "train_error", "error": error });
                if let Some(r) = request_id_ref {
                    inner["requestId"] = json!(r);
                }
                send_inner(inner).await;
            }
            RunEnd::Cancelled {
                billing,
                last_checkpoint,
            } => {
                let mut error = json!({
                    "code": "CANCELLED",
                    "message": "run aborted at a slice boundary (cancel/disconnect)",
                });
                let mut extra = serde_json::Map::new();
                // C.1: k = EXECUTED slices (landed + forfeited) — the wire
                // bill is Σ delta[0..k−1]; landed-only here broke the frozen
                // formula when a slice forfeited (converge round F3).
                extra.insert(
                    "settledSlices".into(),
                    json!(billing.settled_slices + billing.forfeited_slices),
                );
                extra.insert("billedTokens".into(), json!(billing.billed_tokens));
                if let Some(checkpoint) = last_checkpoint {
                    extra.insert(
                        "lastCheckpoint".into(),
                        json!({
                            "manifestCID": checkpoint.manifest_cid,
                            "manifestSha256": checkpoint.manifest_sha256,
                        }),
                    );
                }
                error["detail"] = Value::Object(extra);
                let mut inner = json!({ "type": "train_error", "error": error });
                if let Some(r) = request_id_ref {
                    inner["requestId"] = json!(r);
                }
                send_inner(inner).await;
            }
        }

        // Completion: k = 0 Failed already zero-settled inside the loop; every
        // other end completes here after the dispute wait, guarded by the
        // tracker latch. The attempt record closes (Completed keeps the
        // one-train-ever consumption WITHOUT arming a cooldown).
        // TD15 on EVERY end: no plaintext survives a finished run — the
        // staged dataset AND the work dirs go (converge round F5: Complete
        // left the staging plaintext; Cancelled/Failed left both).
        let _ = tokio::fs::remove_dir_all(deps.staging_root.join(format!("job-{job_id}"))).await;
        let _ = tokio::fs::remove_dir_all(deps.work_root.join(format!("job-{job_id}"))).await;

        let settled_any = match &end {
            RunEnd::Complete { .. } | RunEnd::Cancelled { .. } => true,
            RunEnd::Failed { code, .. } => *code != crate::training::core::SIDECAR_UNAVAILABLE,
        };
        if settled_any {
            // C.3 timing: never earlier than creation + dispute + buffer.
            // With landed proofs the chain gates on lastProofTime (the
            // tracker's wait); with NONE (a k=0 cancel) the creation floor
            // applies — without it the completion reverts "Dispute wait"
            // and, un-retried, stranded the deposit (converge round F2/F5).
            let proof_wait = deps.tracker.proof_wait_remaining(job_id, window).await;
            let wait = if deps
                .tracker
                .info(job_id)
                .await
                .map(|i| i.slices_submitted)
                .unwrap_or(0)
                == 0
            {
                let now_real = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or_default();
                let created = if accepted.snapshot.start_time > ethers::types::U256::from(u64::MAX)
                {
                    u64::MAX
                } else {
                    accepted.snapshot.start_time.as_u64()
                };
                let due = created.saturating_add(window);
                std::cmp::max(
                    proof_wait,
                    std::time::Duration::from_secs(due.saturating_sub(now_real)),
                )
            } else {
                proof_wait
            };
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
            if deps.tracker.mark_completing_if_idle(job_id).await {
                // Bounded retry, mirroring the zero-settle (a single
                // log-and-forget attempt stranded the deposit on any
                // transient RPC error — converge round F5).
                for attempt in 1..=5u32 {
                    match deps.completer.complete_session(job_id).await {
                        Ok(()) => break,
                        Err(error) if attempt < 5 => {
                            tracing::warn!(
                                "training completion for job {job_id} failed (attempt {attempt}/5): {error}"
                            );
                            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                        }
                        Err(error) => {
                            tracing::error!(
                                "training completion for job {job_id} EXHAUSTED retries: {error} — the chain max_duration timeout is the client's backstop"
                            );
                        }
                    }
                }
            }
            deps.attempts.finish(
                job_id,
                accepted.snapshot.depositor,
                now_secs,
                crate::training::accept::AttemptOutcome::Completed,
            );
        }
    }
}
