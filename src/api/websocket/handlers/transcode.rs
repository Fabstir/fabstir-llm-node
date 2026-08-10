// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Encrypted WebSocket handler for transcoding (v8.25.0)
//!
//! Handles `"action": "transcode"` messages. Long-running: uses background task
//! with mpsc channel for progress streaming.

use crate::api::server::ApiServer;
use crate::transcoder::billing::{
    calculate_transcode_units, codec_factor, resolution_factor_from_vf,
};
use crate::transcoder::gop::gop_info_from_progress;
use crate::transcoder::merkle::MerkleTree;
use crate::transcoder::proof::{
    compute_codec_params_hash, compute_proof_hash, generate_gop_stark_proof, serialize_proof_for_s5,
};
use crate::transcoder::types::{QualityMetrics, VideoFormat};
use crate::transcoder::TranscoderClient;
use rand::RngCore;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// A transcode completion is *publishable* iff `outputs` is a **non-empty JSON
/// array** (F3b). Empty / null / non-array / (unparseable, already coerced to `[]`
/// upstream) ⇒ NOT publishable ⇒ HOLD (fail-closed). Phrased as "non-empty array",
/// never "confirmed `[]`", so a parse failure and a genuine empty array are treated
/// identically (both hold).
pub fn is_publishable_completion(outputs: &Value) -> bool {
    outputs.as_array().map(|a| !a.is_empty()).unwrap_or(false)
}

/// Background progress polling task spawned after transcode submission.
pub struct TranscodeProgressTask {
    pub task_id: String,
    pub poll_interval_ms: u64,
    pub timeout_seconds: u64,
}

impl TranscodeProgressTask {
    /// Spawn a background task that polls transcoder status and sends encrypted
    /// progress messages through the channel.
    pub fn spawn(
        self,
        transcoder_client: Arc<TranscoderClient>,
        session_key: [u8; 32],
        session_id: String,
        job_id: Option<u64>,
        server: Arc<ApiServer>,
        progress_tx: mpsc::Sender<Value>,
        formats: Vec<VideoFormat>,
        is_encrypted: bool,
    ) {
        let task_id = self.task_id.clone();
        let poll_interval = Duration::from_millis(self.poll_interval_ms);
        let timeout = Duration::from_secs(self.timeout_seconds);
        tokio::spawn(async move {
            let start = std::time::Instant::now();
            let mut last_progress = -1i32;
            let mut completion_retries = 0u32;
            const MAX_COMPLETION_RETRIES: u32 = 5;

            loop {
                if start.elapsed() > timeout {
                    let err = build_encrypted_transcode_error(
                        "TIMEOUT",
                        "Transcoding timed out",
                        &session_key,
                        &session_id,
                        None,
                    );
                    let _ = progress_tx.send(err).await;
                    break;
                }

                tokio::time::sleep(poll_interval).await;

                match transcoder_client.get_status(&task_id).await {
                    Ok(status) => {
                        // Send progress only when it changes
                        if status.progress != last_progress && status.progress > 0 {
                            last_progress = status.progress;
                            let mut progress_data = json!({
                                "type": "transcode_progress",
                                "taskId": task_id,
                                "progress": status.progress
                            });
                            if let Some(dur) = status.duration {
                                let elapsed = start.elapsed().as_secs_f64();
                                let gop = gop_info_from_progress(status.progress, dur, elapsed);
                                progress_data["gopInfo"] = json!({
                                    "currentGop": gop.current_gop,
                                    "totalGops": gop.total_gops,
                                    "elapsedSeconds": gop.elapsed_seconds
                                });
                            }
                            let progress_msg = build_encrypted_transcode_response(
                                &progress_data,
                                &session_key,
                                &session_id,
                                None,
                            );
                            if progress_tx.send(progress_msg).await.is_err() {
                                break; // Client disconnected
                            }
                        }

                        // Check completion (progress 100 only — metadata is unreliable
                        // because "Transcoding in progress" is non-empty/non-[])
                        if status.progress >= 100 {
                            // Sidecar may report progress=100 before uploads finish.
                            // Retry if metadata isn't a valid JSON array yet.
                            let meta_looks_ready = status.metadata.starts_with('[');
                            if !meta_looks_ready && completion_retries < MAX_COMPLETION_RETRIES {
                                completion_retries += 1;
                                debug!(
                                    "Transcode progress=100 but metadata not ready (attempt {}/{}): {:?}",
                                    completion_retries, MAX_COMPLETION_RETRIES,
                                    &status.metadata[..status.metadata.len().min(80)]
                                );
                                continue; // poll again after sleep
                            }
                            if !meta_looks_ready {
                                warn!(
                                    "Transcode metadata still not ready after {} retries, completing with available data: {:?}",
                                    MAX_COMPLETION_RETRIES, &status.metadata[..status.metadata.len().min(80)]
                                );
                            }

                            // F3b: parse `outputs` ONCE here — hoisted above billing so the
                            // zero-output guard (below) and the completion message share one
                            // parse. A parse failure is coerced to `[]` ⇒ treated as
                            // non-publishable (HOLD), indistinguishable from a genuine empty
                            // array (both fail-closed).
                            let outputs: Value =
                                serde_json::from_str(&status.metadata).unwrap_or(json!([]));

                            // F3a/F3b zero-output guard (defensive, INDEPENDENT of
                            // MODERATION_ENFORCE): a completion with no publishable outputs
                            // (empty/null/non-array/unparseable) is a held/failed job — do
                            // NOT bill/prove/complete it. Placed strictly BELOW the
                            // metadata-not-ready retry so a transient never trips it (F3a),
                            // and ABOVE billing so it gates billing+proof+complete. It catches
                            // the transcoder's coupled `metadata="[]"` held signal even with
                            // enforcement off; it does NOT close the non-empty-Blocked case
                            // (that needs MODERATION_ENFORCE=true at go-live — accepted
                            // dark-launch gap F3c). This slice does NOT fully close the gate.
                            if !is_publishable_completion(&outputs) {
                                warn!(
                                    "Transcode completion has no publishable outputs (job_id={:?}); withholding billing/proof/completion (held)",
                                    job_id
                                );
                                server.moderation_metrics().record_held(); // §8 #7 (parity with the gate Hold)
                                let _ = progress_tx
                                    .send(build_encrypted_transcode_error(
                                        "MODERATION_UNAVAILABLE",
                                        "transcode held: no publishable outputs",
                                        &session_key,
                                        &session_id,
                                        None,
                                    ))
                                    .await;
                                break; // skip billing, proof, transcode_complete
                            }

                            // ── Moderation gate (host-reachable half of seam #2) ──
                            // Withholds completion/billing/proof for non-cleared jobs.
                            // Placed BEFORE billing/proof so a blocked job leaks no S5
                            // proof artifact (R6). Does NOT stop the external
                            // transcoder's HLS upload (Part A/A4) or the SDK publish
                            // (seam #3) — all three are required to fully close the gate
                            // (§8a). Absent verdict / missing job_id ⇒ HOLD (fail-closed).
                            //
                            // Activation is dark-launched behind MODERATION_ENFORCE: the
                            // verdict-producing ingest (seam #1) lives in another repo, so
                            // until it is wired, enforcing would hold ALL transcodes. Flip
                            // MODERATION_ENFORCE=true at go-live. The gate LOGIC is always
                            // fail-closed; only its activation is toggled.
                            if server.moderation_enforce() {
                                match crate::moderation::gate::Gate::transcode_decision(
                                    server.moderation_store(),
                                    job_id,
                                ) {
                                    crate::moderation::gate::GateOutcome::Release => {}
                                    crate::moderation::gate::GateOutcome::Hold {
                                        code,
                                        message,
                                    } => {
                                        server.moderation_metrics().record_held(); // §8 #7
                                        let _ = progress_tx
                                            .send(build_encrypted_transcode_error(
                                                code,
                                                &message,
                                                &session_key,
                                                &session_id,
                                                None,
                                            ))
                                            .await;
                                        break; // skip billing, proof, transcode_complete
                                    }
                                }
                            } else {
                                warn!("⚠️ Moderation enforcement DISABLED (set MODERATION_ENFORCE=true to enable); transcode completing WITHOUT the host-slice moderation gate (§8a)");
                            }

                            // Calculate billing
                            let duration = status.duration.unwrap_or(0.0);
                            let mut total_units = 0.0;

                            for fmt in &formats {
                                let res_factor =
                                    resolution_factor_from_vf(fmt.vf.as_deref().unwrap_or(""));
                                let c_factor = codec_factor(fmt.vcodec.as_deref().unwrap_or(""));
                                let units = calculate_transcode_units(
                                    duration,
                                    res_factor,
                                    c_factor,
                                    is_encrypted,
                                );
                                total_units += units;
                            }

                            // Track billing
                            if let Some(jid) = job_id {
                                server
                                    .transcoding_tracker()
                                    .track(jid, Some(session_id.clone()), total_units)
                                    .await;
                            }

                            let tokens = (total_units * 1000.0).ceil() as u64;

                            // (`outputs` is parsed once, hoisted above the zero-output
                            // guard near the top of this completion block — F3b.)

                            // Build proof pipeline (best-effort — fields stay null on failure)
                            let mut quality_val: Value = json!(null);
                            let mut proof_cid_val: Value = json!(null);
                            let mut proof_root_val: Value = json!(null);

                            if let Some(jid) = job_id {
                                let codec_hash = compute_codec_params_hash(&formats);
                                // Use codec hash as both input and output hash for the job-level proof
                                // (GOP-level source/output hashing requires ffmpeg segment access — future work)
                                let input_hash = codec_hash;
                                let output_hash = compute_proof_hash(status.metadata.as_bytes());

                                match generate_gop_stark_proof(
                                    jid,
                                    codec_hash,
                                    input_hash,
                                    output_hash,
                                ) {
                                    Ok(stark_bytes) => {
                                        let metrics = QualityMetrics {
                                            psnr_db: 0.0,
                                            ssim: None,
                                            actual_bitrate: 0,
                                        };
                                        let mut gop_proof =
                                            crate::transcoder::proof::build_gop_proof(
                                                0,
                                                input_hash,
                                                output_hash,
                                                &metrics,
                                            );
                                        let proof_hash = compute_proof_hash(&stark_bytes);
                                        gop_proof.stark_proof_hash = hex::encode(proof_hash);

                                        // Build Merkle tree with single leaf
                                        let mut tree = MerkleTree::new();
                                        tree.add_leaf(proof_hash);
                                        let root = tree.root();
                                        let tree_bytes = tree.serialize();

                                        // Upload proof + tree to S5
                                        let proof_data =
                                            serialize_proof_for_s5(&gop_proof, &stark_bytes);
                                        if let Some(cm) = server.get_checkpoint_manager().await {
                                            let s5 = cm.get_s5_storage();
                                            let tree_path = format!(
                                                "home/transcode/proof-tree/{}.json",
                                                hex::encode(root)
                                            );
                                            match s5.put(&tree_path, tree_bytes).await {
                                                Ok(cid) => {
                                                    info!("Proof tree uploaded to S5: CID={}", cid);
                                                    proof_cid_val = json!(cid);
                                                    proof_root_val =
                                                        json!(format!("0x{}", hex::encode(root)));
                                                }
                                                Err(e) => warn!(
                                                    "Failed to upload proof tree to S5: {}",
                                                    e
                                                ),
                                            }
                                            let proof_path = format!(
                                                "home/transcode/gop-proof/job_{}_gop_0.bin",
                                                jid
                                            );
                                            if let Err(e) = s5.put(&proof_path, proof_data).await {
                                                warn!("Failed to upload GOP proof to S5: {}", e);
                                            }
                                        }
                                    }
                                    Err(e) => warn!(
                                        "STARK proof generation failed for job {}: {}",
                                        jid, e
                                    ),
                                }
                            }

                            // Seam #3 (WP-N1): surface the recorded verdict on the
                            // completion so the SDK's publish gate has a signal even in
                            // dark launch, where the gate above does not hold. Absent
                            // verdict ⇒ field omitted — the SDK treats absence as "not
                            // moderated", never as "clean" (DESIGN-SDK-SEAM3 §2.2).
                            let mut complete_payload = json!({
                                "type": "transcode_complete",
                                "taskId": task_id,
                                "outputs": outputs,
                                "billing": {
                                    "units": total_units,
                                    "tokens": tokens,
                                },
                                "duration": duration,
                                "qualityMetrics": quality_val,
                                "proofTreeCID": proof_cid_val,
                                "proofTreeRootHash": proof_root_val
                            });
                            if let Some(m) =
                                moderation_completion_field(server.moderation_store(), job_id)
                            {
                                complete_payload["moderation"] = m;
                            }

                            let complete_msg = build_encrypted_transcode_response(
                                &complete_payload,
                                &session_key,
                                &session_id,
                                None,
                            );
                            let _ = progress_tx.send(complete_msg).await;
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Transcoder status poll failed for {}: {}", task_id, e);
                        let err = build_encrypted_transcode_error(
                            "POLL_FAILED",
                            &format!("Failed to poll transcoder status: {}", e),
                            &session_key,
                            &session_id,
                            None,
                        );
                        let _ = progress_tx.send(err).await;
                        break;
                    }
                }
            }
            // Log billing freeze on any exit path
            if let Some(jid) = job_id {
                info!("Transcode progress loop ended for job {} — billing frozen at last checkpoint (progress={})", jid, last_progress);
            }
        });
    }
}

/// Handle an encrypted transcode request.
///
/// Returns (immediate_ack, optional_progress_task).
pub async fn handle_encrypted_transcode(
    server: &ApiServer,
    decrypted_json: &Value,
    session_key: &[u8; 32],
    session_id: &str,
    job_id: Option<u64>,
    message_id: Option<&Value>,
) -> (Value, Option<TranscodeProgressTask>) {
    // Step 1: Parse and validate request
    let source_cid = match decrypted_json.get("sourceCid").and_then(|v| v.as_str()) {
        Some(cid) if !cid.is_empty() => cid.to_string(),
        _ => {
            return (
                build_encrypted_transcode_error(
                    "VALIDATION_FAILED",
                    "Missing or empty sourceCid",
                    session_key,
                    session_id,
                    message_id,
                ),
                None,
            );
        }
    };

    let formats: Vec<VideoFormat> = match decrypted_json.get("mediaFormats") {
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(fmts) => fmts,
            Err(e) => {
                return (
                    build_encrypted_transcode_error(
                        "VALIDATION_FAILED",
                        &format!("Invalid mediaFormats: {}", e),
                        session_key,
                        session_id,
                        message_id,
                    ),
                    None,
                );
            }
        },
        None => {
            return (
                build_encrypted_transcode_error(
                    "VALIDATION_FAILED",
                    "Missing mediaFormats",
                    session_key,
                    session_id,
                    message_id,
                ),
                None,
            );
        }
    };

    if formats.is_empty() {
        return (
            build_encrypted_transcode_error(
                "VALIDATION_FAILED",
                "mediaFormats must not be empty",
                session_key,
                session_id,
                message_id,
            ),
            None,
        );
    }

    let is_gpu = decrypted_json
        .get("isGpu")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let is_encrypted = decrypted_json
        .get("isEncrypted")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let preview_percent: Option<u32> = decrypted_json
        .get("previewPercent")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    // Step 2: Rate limit check
    if !server
        .transcoding_rate_limiter()
        .check_rate_limit(session_id)
    {
        warn!("Transcoding rate limit exceeded for session {}", session_id);
        return (
            build_encrypted_transcode_error(
                "RATE_LIMIT_EXCEEDED",
                "Transcoding rate limit exceeded",
                session_key,
                session_id,
                message_id,
            ),
            None,
        );
    }

    // Step 3: Check sidecar availability
    let transcoder_client = match server.get_transcoder_client().await {
        Some(client) => client,
        None => {
            return (
                build_encrypted_transcode_error(
                    "SIDECAR_UNAVAILABLE",
                    "Transcoder sidecar not configured (503)",
                    session_key,
                    session_id,
                    message_id,
                ),
                None,
            );
        }
    };

    // Step 4: Capacity check — ask sidecar if it can accept work
    if !server.has_sidecar_capacity().await {
        warn!("Sidecar capacity full for session {}", session_id);
        return (
            build_encrypted_transcode_error(
                "TRANSCODE_CAPACITY_FULL",
                "All transcode slots are in use — try again later",
                session_key,
                session_id,
                message_id,
            ),
            None,
        );
    }

    // Step 5: Submit to transcoder
    match transcoder_client
        .submit_transcode(&source_cid, &formats, is_encrypted, is_gpu, preview_percent)
        .await
    {
        Ok(resp) => {
            info!(
                "Transcode submitted: task_id={}, session={}",
                resp.task_id, session_id
            );

            // Seam-#1 (C1 / task 2.1.3): map the transcoder's task_id → our job_id at
            // submit — BEFORE the transcoder begins POSTing keyframes to
            // /v1/moderate/frames, so the mapping is already present when they arrive.
            // A `None` job_id records nothing ⇒ a later /frames POST ⇒ 404 ⇒ HOLD.
            if let Some(j) = job_id {
                server.record_task_job(resp.task_id.clone(), j);
            }

            // Record rate limit
            server.transcoding_rate_limiter().record_request(session_id);

            let poll_interval_ms: u64 = std::env::var("TRANSCODE_POLL_INTERVAL_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2000);
            let timeout_seconds: u64 = std::env::var("TRANSCODE_TIMEOUT_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1800);

            let task = TranscodeProgressTask {
                task_id: resp.task_id.clone(),
                poll_interval_ms,
                timeout_seconds,
            };

            let ack = build_encrypted_transcode_response(
                &json!({
                    "type": "transcode_accepted",
                    "taskId": resp.task_id,
                    "status": "accepted"
                }),
                session_key,
                session_id,
                message_id,
            );

            (ack, Some(task))
        }
        Err(e) => {
            error!("Failed to submit transcode: {}", e);
            (
                build_encrypted_transcode_error(
                    "SUBMIT_FAILED",
                    &format!("Failed to submit transcode: {}", e),
                    session_key,
                    session_id,
                    message_id,
                ),
                None,
            )
        }
    }
}

/// Build an encrypted response for transcode messages.
pub fn build_encrypted_transcode_response(
    inner_json: &Value,
    session_key: &[u8; 32],
    session_id: &str,
    message_id: Option<&Value>,
) -> Value {
    let plaintext = serde_json::to_vec(inner_json).unwrap_or_default();

    let mut nonce = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce);

    let aad = b"encrypted_transcode_response";

    match crate::crypto::encrypt_with_aead(&plaintext, &nonce, aad, session_key) {
        Ok(ciphertext) => {
            let mut msg = json!({
                "type": "encrypted_response",
                "payload": {
                    "ciphertextHex": hex::encode(&ciphertext),
                    "nonceHex": hex::encode(&nonce),
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
            error!("Failed to encrypt transcode response: {}", e);
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

/// Build an encrypted transcode error response.
/// Seam #3 (WP-N1): the `moderation` field for `transcode_complete`, from the
/// recorded verdict. `None` (no job id, or no verdict recorded) means the field
/// is OMITTED — the SDK treats absence as "not moderated", never as "clean"
/// (DESIGN-SDK-SEAM3-PUBLISH-GATE.md §2.2). The verdict string comes from
/// `Verdict::as_str()`, the same single source the frames endpoint uses, so the
/// two wire surfaces cannot drift.
pub fn moderation_completion_field(
    store: &crate::moderation::verdict_store::VerdictStore,
    job_id: Option<u64>,
) -> Option<Value> {
    let r = job_id.and_then(|jid| store.get(jid))?;
    Some(json!({
        "verdict": r.verdict.as_str(),
        "reason": r.reason,
    }))
}

fn build_encrypted_transcode_error(
    code: &str,
    message: &str,
    session_key: &[u8; 32],
    session_id: &str,
    message_id: Option<&Value>,
) -> Value {
    let inner = json!({
        "type": "transcode_error",
        "error": {
            "code": code,
            "message": message,
        }
    });
    build_encrypted_transcode_response(&inner, session_key, session_id, message_id)
}
