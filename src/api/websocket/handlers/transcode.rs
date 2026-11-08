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

/// Default for `TRANSCODE_OUTPUT_WAIT_SECS` (8.62.1): how long a run that has
/// reached progress 100 waits for the sidecar to list its outputs. The sidecar
/// reports 100 while it is still uploading to S5 (about 5 MB/s on host2, so a
/// 121 MB output took ~25 s); 8.62.0 gave up after 5 polls (10 s) and held
/// finished jobs unbilled. 120 s covers ~600 MB.
pub const OUTPUT_WAIT_SECS_DEFAULT: u64 = 120;

/// What the poll loop does at progress 100.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionWait {
    /// The sidecar's metadata is the output list (a JSON array, possibly empty).
    Ready,
    /// Not listed yet; poll again.
    KeepWaiting,
    /// Not listed within the wait: fail the run as `OUTPUTS_NOT_READY`.
    GiveUp,
}

/// `waited` is the time since the run first reported progress 100.
pub fn completion_wait(metadata: &str, waited: Duration, max_wait: Duration) -> CompletionWait {
    if metadata.trim_start().starts_with('[') {
        CompletionWait::Ready
    } else if waited < max_wait {
        CompletionWait::KeepWaiting
    } else {
        CompletionWait::GiveUp
    }
}

/// The output wait actually used: the configured value, capped so it cannot
/// outlast the session's proof window. No interim proof is made at 100 (the
/// final claim needs the actual total), and while below 100 the biller proves at
/// least every half window, so at 100 the last proof may be half a window old.
/// The accept gate guarantees the window is at least `min_proof_window_secs`;
/// the wait keeps 20 s of margin inside the other half, and never drops below
/// 8.62.0's 10 s.
pub fn output_wait_secs(configured: u64, min_proof_window_secs: u64) -> u64 {
    configured
        .min((min_proof_window_secs / 2).saturating_sub(20))
        .max(10)
}

/// The sidecar wants `network://cid` (e.g. `s5://u...`) and silently drops any
/// other form while its status keeps saying "in progress", so the node refuses
/// it before submitting (sessions 1459 and 1460 waited 30 minutes for nothing).
pub fn source_cid_names_network(cid: &str) -> bool {
    cid.split_once("://")
        .map(|(network, rest)| !network.is_empty() && !rest.is_empty())
        .unwrap_or(false)
}

/// 8.62.2: the sidecar (d645145 and later) reports a job that failed for good in
/// GetTranscoded's `status_code`, with HTTP 200: 400 is bad input (an invalid
/// source CID, for example), any other non-200 is any other permanent failure.
/// `None` for 200. Checked before progress, which means nothing on a failed
/// job; `metadata` then holds a plain-text reason for logs only, never parsed.
pub fn sidecar_failure_code(status_code: i32) -> Option<&'static str> {
    match status_code {
        200 => None,
        400 => Some("VALIDATION_FAILED"),
        _ => Some("TRANSCODE_FAILED"),
    }
}

/// The refusal text for a CID without its network, shared by WS and HTTP.
pub const SOURCE_CID_NETWORK_MESSAGE: &str =
    "sourceCid must name its storage network, e.g. s5://<cid>";

/// The sidecar reports `duration: 0.0` while a job is queued or downloading (and
/// for its whole run before 5783b1a). That means "not known yet", never a zero
/// length job: taken as an estimate it would pin the run's lowest estimate at
/// zero and no step would ever be proved.
pub fn known_duration(reported: Option<f64>) -> Option<f64> {
    reported.filter(|d| *d > 0.0)
}

/// How long the sidecar may stay unreachable before a run is failed: a crash or
/// a container recreate takes seconds, so the old "three polls" (about 6 s) would
/// fail a job that is still running. `TRANSCODE_POLL_GRACE_SECS`, default 120.
pub const POLL_GRACE_SECS_DEFAULT: u64 = 120;

pub fn poll_grace_secs(configured: Option<&str>) -> u64 {
    configured
        .and_then(|v| v.parse().ok())
        .unwrap_or(POLL_GRACE_SECS_DEFAULT)
}

/// Has the sidecar been unreachable for too long? `failing_for` is the time
/// since the first of the current run of failed polls.
pub fn poll_gives_up(failing_for: Duration, grace: Duration) -> bool {
    failing_for >= grace
}

/// `ceil(units × 1000)`, after rounding away floating-point noise: 1,800 s at
/// factor 1.1 sums to 1980.0000000000002 units, which must bill 1,980,000 tokens,
/// not 1,980,001. A real fraction of a token still rounds up.
pub fn tokens_for_units(units: f64) -> u64 {
    (((units * 1000.0) * 1e6).round() / 1e6).ceil() as u64
}

/// The share of the source a format renders: a preview (`trim_percent`, which
/// the SDK sets from `previewPercent`) renders only that share.
fn rendered_fraction(format: &VideoFormat) -> f64 {
    match format.trim_percent {
        Some(p) if p > 0 && p < 100 => p as f64 / 100.0,
        _ => 1.0,
    }
}

/// The running estimate's tokens, from the source's probed duration: each
/// format counts only the share it renders, so a preview's interim proofs never
/// run ahead of its final bill. From transcoder 6cc694e the final duration is
/// what was really encoded (the trimmed length for a preview-only job), and the
/// final bill stays [`transcode_tokens`] on that value, so this never exceeds it.
pub fn transcode_estimate_tokens(
    duration: f64,
    formats: &[VideoFormat],
    is_encrypted: bool,
) -> u64 {
    let units: f64 = formats
        .iter()
        .map(|fmt| {
            calculate_transcode_units(
                duration * rendered_fraction(fmt),
                resolution_factor_from_vf(fmt.vf.as_deref().unwrap_or("")),
                codec_factor(fmt.vcodec.as_deref().unwrap_or("")),
                is_encrypted,
            )
        })
        .sum();
    tokens_for_units(units)
}

/// The final bill (8.62.7). From transcoder e0c789d each published output carries
/// `encodedDuration`, its own measured length, so each format is billed by what
/// it really rendered: a preview at its trimmed length, a format that produced no
/// output at nothing. When no output carries it (older transcoders), every
/// format is billed at the job's `duration`, as before.
pub fn final_transcode_tokens(
    duration: f64,
    formats: &[VideoFormat],
    outputs: &Value,
    is_encrypted: bool,
) -> (f64, u64) {
    let measured: Vec<(Option<u64>, f64, &Value)> = outputs
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|o| {
                    let secs = o.get("encodedDuration")?.as_f64()?;
                    (secs.is_finite() && secs >= 0.0).then_some((
                        o.get("id").and_then(|v| v.as_u64()),
                        secs,
                        o,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    if measured.is_empty() {
        return transcode_tokens(duration, formats, is_encrypted);
    }
    let units: f64 = measured
        .iter()
        .map(|(id, secs, output)| {
            // The client's own format decides the factors; the output echoes it.
            let format = id.and_then(|id| formats.iter().find(|f| f.id as u64 == id));
            let vf = format
                .and_then(|f| f.vf.clone())
                .or_else(|| output.get("vf").and_then(|v| v.as_str()).map(String::from))
                .unwrap_or_default();
            let vcodec = format
                .and_then(|f| f.vcodec.clone())
                .or_else(|| {
                    output
                        .get("vcodec")
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
                .unwrap_or_default();
            calculate_transcode_units(
                *secs,
                resolution_factor_from_vf(&vf),
                codec_factor(&vcodec),
                is_encrypted,
            )
        })
        .sum();
    ((units * 1e9).round() / 1e9, tokens_for_units(units))
}

/// The transcode's billing units and tokens for `duration` seconds rendered to
/// `formats`: the bill when the outputs carry no measured lengths.
pub fn transcode_tokens(duration: f64, formats: &[VideoFormat], is_encrypted: bool) -> (f64, u64) {
    let units: f64 = formats
        .iter()
        .map(|fmt| {
            calculate_transcode_units(
                duration,
                resolution_factor_from_vf(fmt.vf.as_deref().unwrap_or("")),
                codec_factor(fmt.vcodec.as_deref().unwrap_or("")),
                is_encrypted,
            )
        })
        .sum();
    ((units * 1e9).round() / 1e9, tokens_for_units(units))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Background progress polling task spawned after transcode submission.
pub struct TranscodeProgressTask {
    pub task_id: String,
    pub poll_interval_ms: u64,
    pub timeout_seconds: u64,
    /// SCI-C D15: this transcode's unit of work on its job, held until the task
    /// ends on any path. `None` for a job-less request.
    pub work: Option<crate::api::job_work::WorkGuard>,
    /// SCI-C D19(d): where this run's outcome is stored for a later fetch.
    /// `None` for a job-less request.
    pub result: Option<crate::transcoder::result_store::ResultSlot>,
    /// SCI-C D19(a): an explicit `transcode_cancel`. A client that merely
    /// disconnects does not cancel; the run carries on and bills.
    pub cancel: tokio_util::sync::CancellationToken,
}

/// How long a terminal frame waits for a listener that has stopped reading.
/// Progress frames never wait (`try_send`): a stalled socket must not stall
/// billing (D19(a)).
const TERMINAL_SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// Deliver a terminal frame to the listener, if one is left, without waiting
/// long for one that has stopped reading.
async fn send_terminal(progress_tx: &mpsc::Sender<Value>, frame: Value) {
    let _ = tokio::time::timeout(TERMINAL_SEND_TIMEOUT, progress_tx.send(frame)).await;
}

/// Send a failure to the listener (if any is left) and store it for a fetch.
async fn emit_failure(
    progress_tx: &mpsc::Sender<Value>,
    result: &mut Option<crate::transcoder::result_store::ResultSlot>,
    code: &str,
    message: &str,
    session_key: &[u8; 32],
    session_id: &str,
) {
    if let Some(slot) = result.as_mut() {
        slot.failed(code, message);
    }
    send_terminal(
        progress_tx,
        build_encrypted_transcode_error(code, message, session_key, session_id, None),
    )
    .await;
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
        let work = self.work;
        let mut result = self.result;
        let cancel = self.cancel;
        tokio::spawn(async move {
            // SCI-C D20: bill in steps as the sidecar runs, on a job with a chain.
            let chain = match job_id {
                Some(_) => server.get_checkpoint_manager().await,
                None => None,
            };
            let mut biller = job_id
                .map(|jid| crate::transcoder::biller::TranscodeBiller::new(jid, task_id.clone()));
            let start = std::time::Instant::now();
            let mut last_progress = -1i32;
            // 8.62.1: when the run first reported 100, and how long it may then
            // wait for the sidecar to list its outputs (see `output_wait_secs`).
            let mut reached_hundred_at: Option<std::time::Instant> = None;
            let output_wait = Duration::from_secs(output_wait_secs(
                std::env::var("TRANSCODE_OUTPUT_WAIT_SECS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(OUTPUT_WAIT_SECS_DEFAULT),
                crate::transcoder::proof_steps::AcceptLimits::from_env().min_proof_window_secs,
            ));
            let mut poll_failing_since: Option<std::time::Instant> = None;
            let mut last_estimate: Option<u64> = None;
            let mut last_seen_progress = 0i32;
            let poll_grace = Duration::from_secs(poll_grace_secs(
                std::env::var("TRANSCODE_POLL_GRACE_SECS").ok().as_deref(),
            ));

            loop {
                if start.elapsed() > timeout {
                    if let Err(e) = transcoder_client.cancel_transcode(&task_id).await {
                        warn!("Sidecar cancel failed for {}: {}", task_id, e);
                    }
                    emit_failure(
                        &progress_tx,
                        &mut result,
                        "TIMEOUT",
                        "Transcoding timed out",
                        &session_key,
                        &session_id,
                    )
                    .await;
                    break;
                }

                tokio::select! {
                    _ = tokio::time::sleep(poll_interval) => {}
                    _ = cancel.cancelled() => {}
                }
                // D19(a)(i): an explicit cancel stops the sidecar and bills
                // nothing more; steps already proven stay paid.
                if cancel.is_cancelled() {
                    info!("[SCI-C] transcode {} cancelled by the client", task_id);
                    if let Err(e) = transcoder_client.cancel_transcode(&task_id).await {
                        warn!("Sidecar cancel failed for {}: {}", task_id, e);
                    }
                    emit_failure(
                        &progress_tx,
                        &mut result,
                        "CANCELLED",
                        "Transcode cancelled by the client",
                        &session_key,
                        &session_id,
                    )
                    .await;
                    break;
                }

                match transcoder_client.get_status(&task_id).await {
                    Ok(status) => {
                        poll_failing_since = None;
                        last_seen_progress = status.progress;
                        // 8.62.2: a non-200 status_code is a permanent failure,
                        // checked before progress (meaningless on a failed job).
                        // Steps already proven stay paid (after the loop).
                        if let Some(code) = sidecar_failure_code(status.status_code) {
                            warn!(
                                "Transcode {} failed in the sidecar (status_code {}, job {:?}): {:?}",
                                task_id,
                                status.status_code,
                                job_id,
                                &status.metadata[..status.metadata.len().min(200)]
                            );
                            emit_failure(
                                &progress_tx,
                                &mut result,
                                code,
                                &format!(
                                    "The transcoder reported the job failed (status {}); nothing more is billed",
                                    status.status_code
                                ),
                                &session_key,
                                &session_id,
                            )
                            .await;
                            break;
                        }
                        // D20(b)/(d): prove the work so far and watch the session.
                        if let (Some(b), Some(cm), true) =
                            (biller.as_mut(), chain.as_ref(), status.progress < 100)
                        {
                            let estimate = known_duration(status.duration)
                                .map(|d| transcode_estimate_tokens(d, &formats, is_encrypted));
                            last_estimate = estimate.or(last_estimate);
                            let polled = b
                                .on_progress(cm.as_ref(), status.progress, estimate, unix_now())
                                .await;
                            if polled == crate::transcoder::biller::PollOutcome::Expired {
                                warn!(
                                    "[SCI-C] transcode {} stopped: the session expired mid-run (billed {} tokens before expiry)",
                                    task_id,
                                    b.proven()
                                );
                                if let Err(e) = transcoder_client.cancel_transcode(&task_id).await {
                                    warn!("Sidecar cancel failed for {}: {}", task_id, e);
                                }
                                emit_failure(
                                    &progress_tx,
                                    &mut result,
                                    "SESSION_EXPIRED",
                                    &format!(
                                        "Transcode stopped: the session expired before it finished ({} tokens billed for the work done before expiry)",
                                        b.proven()
                                    ),
                                    &session_key,
                                    &session_id,
                                )
                                .await;
                                break;
                            }
                            if let crate::transcoder::biller::PollOutcome::Stop(reason) = polled {
                                warn!(
                                    "Transcode {} stopped: {} (billed {} tokens so far)",
                                    task_id,
                                    reason,
                                    b.proven()
                                );
                                if let Err(e) = transcoder_client.cancel_transcode(&task_id).await {
                                    warn!("Sidecar cancel failed for {}: {}", task_id, e);
                                }
                                emit_failure(
                                    &progress_tx,
                                    &mut result,
                                    "SESSION_NOT_BILLABLE",
                                    &format!(
                                        "Transcode stopped: {} ({} tokens billed for the work done)",
                                        reason,
                                        b.proven()
                                    ),
                                    &session_key,
                                    &session_id,
                                )
                                .await;
                                break;
                            }
                        }

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
                            // D19(a): a listener that left, or stopped reading, does
                            // not end or stall the run; it finishes, bills and stores
                            // its result for a fetch. A dropped progress frame is fine.
                            let _ = progress_tx.try_send(progress_msg);
                        }

                        // Check completion (progress 100 only — metadata is unreliable
                        // because "Transcoding in progress" is non-empty/non-[])
                        if status.progress >= 100 {
                            // The sidecar reports 100 before its S5 uploads finish;
                            // the metadata becomes the output list only after them.
                            // 8.62.1: wait for it (bounded), and never treat "not
                            // listed yet" as "no outputs" (that held finished jobs
                            // unbilled as MODERATION_UNAVAILABLE in 8.62.0).
                            let since =
                                *reached_hundred_at.get_or_insert_with(std::time::Instant::now);
                            match completion_wait(&status.metadata, since.elapsed(), output_wait) {
                                CompletionWait::Ready => {}
                                CompletionWait::KeepWaiting => {
                                    debug!(
                                        "Transcode {} at 100% but outputs not listed yet ({}s of {}s): {:?}",
                                        task_id,
                                        since.elapsed().as_secs(),
                                        output_wait.as_secs(),
                                        &status.metadata[..status.metadata.len().min(80)]
                                    );
                                    continue; // poll again after sleep
                                }
                                CompletionWait::GiveUp => {
                                    warn!(
                                        "Transcode {} reached 100% but listed no outputs within {}s (job {:?}); not billed: {:?}",
                                        task_id,
                                        output_wait.as_secs(),
                                        job_id,
                                        &status.metadata[..status.metadata.len().min(80)]
                                    );
                                    emit_failure(
                                        &progress_tx,
                                        &mut result,
                                        "OUTPUTS_NOT_READY",
                                        &format!(
                                            "The transcode finished but its outputs were not listed within {}s, so it was not billed",
                                            output_wait.as_secs()
                                        ),
                                        &session_key,
                                        &session_id,
                                    )
                                    .await;
                                    break;
                                }
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
                                emit_failure(
                                    &progress_tx,
                                    &mut result,
                                    "MODERATION_UNAVAILABLE",
                                    "transcode held: no publishable outputs",
                                    &session_key,
                                    &session_id,
                                )
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
                                        emit_failure(
                                            &progress_tx,
                                            &mut result,
                                            code,
                                            &message,
                                            &session_key,
                                            &session_id,
                                        )
                                        .await;
                                        break; // skip billing, proof, transcode_complete
                                    }
                                }
                            } else {
                                warn!("⚠️ Moderation enforcement DISABLED (set MODERATION_ENFORCE=true to enable); transcode completing WITHOUT the host-slice moderation gate (§8a)");
                            }

                            // Calculate billing (one formula with the running estimate)
                            let duration = status.duration.unwrap_or(0.0);
                            let (total_units, tokens) =
                                final_transcode_tokens(duration, &formats, &outputs, is_encrypted);

                            // Track billing (in-process record, for reporting)
                            if let Some(jid) = job_id {
                                server
                                    .transcoding_tracker()
                                    .track(jid, Some(session_id.clone()), total_units)
                                    .await;
                            }

                            // SCI-C D20(c): the final claim ON CHAIN, awaited to a confirmed
                            // receipt. Placed below the zero-output and moderation guards, so a
                            // held job claims nothing more (steps already proven stay paid). The
                            // result is released only once every claimable token is proven; if
                            // the final claim does not land, the client gets a billing error and
                            // the outputs are withheld.
                            if job_id.is_some() && tokens == 0 {
                                error!(
                                    "TRANSCODE BILLING FAILED for job {:?}: no source duration reported, nothing to bill; result withheld",
                                    job_id
                                );
                                emit_failure(
                                    &progress_tx,
                                    &mut result,
                                    "BILLING_FAILED",
                                    "Transcode finished but its duration was not reported, so it could not be billed",
                                    &session_key,
                                    &session_id,
                                )
                                .await;
                                break;
                            }
                            if let (Some(b), Some(cm)) = (biller.as_mut(), chain.as_ref()) {
                                if tokens > 0 {
                                    match b.finish(cm.as_ref(), tokens).await {
                                        crate::transcoder::biller::FinishOutcome::Paid {
                                            proven,
                                        } => {
                                            info!(
                                                "Transcode billing: {} tokens proven on chain ({:.1} units) for job {}",
                                                proven, total_units, b.job_id()
                                            );
                                            if let Some(work) = work.as_ref() {
                                                work.mark_billed();
                                            }
                                        }
                                        crate::transcoder::biller::FinishOutcome::Unpaid(
                                            reason,
                                        ) => {
                                            error!(
                                                "TRANSCODE BILLING FAILED for job {}: {} (proven {} of {} tokens); result withheld",
                                                b.job_id(), reason, b.proven(), tokens
                                            );
                                            emit_failure(
                                                &progress_tx,
                                                &mut result,
                                                "BILLING_FAILED",
                                                &format!("Transcode finished but billing did not settle: {}", reason),
                                                &session_key,
                                                &session_id,
                                            )
                                            .await;
                                            break;
                                        }
                                        crate::transcoder::biller::FinishOutcome::NotBillable(
                                            reason,
                                        ) => {
                                            // 8.62.2 (session 1469): the same code as
                                            // when this happens mid-run.
                                            warn!(
                                                "[SCI-C] job {}: session stopped being payable before the final claim ({}); proven {} of {} tokens stay paid, result withheld",
                                                b.job_id(), reason, b.proven(), tokens
                                            );
                                            emit_failure(
                                                &progress_tx,
                                                &mut result,
                                                "SESSION_NOT_BILLABLE",
                                                &format!(
                                                    "The session stopped being payable before the final claim ({}); the {} tokens already proven stay paid",
                                                    reason,
                                                    b.proven()
                                                ),
                                                &session_key,
                                                &session_id,
                                            )
                                            .await;
                                            break;
                                        }
                                        crate::transcoder::biller::FinishOutcome::Expired {
                                            proven,
                                        } => {
                                            warn!(
                                                "[SCI-C] job {}: transcode finished after the session expired; not billed ({} tokens proven before expiry), result withheld",
                                                b.job_id(), proven
                                            );
                                            emit_failure(
                                                &progress_tx,
                                                &mut result,
                                                "SESSION_EXPIRED",
                                                &format!("Transcode finished after the session expired, so it was not billed; {} tokens were billed for the work done before expiry", proven),
                                                &session_key,
                                                &session_id,
                                            )
                                            .await;
                                            break;
                                        }
                                    }
                                }
                            } else if job_id.is_some() {
                                // Unreachable past the accept gate; never release unpaid.
                                error!(
                                    "TRANSCODE BILLING FAILED for job {:?}: no checkpoint manager, {} tokens unbilled; result withheld",
                                    job_id, tokens
                                );
                                emit_failure(
                                    &progress_tx,
                                    &mut result,
                                    "BILLING_FAILED",
                                    "Transcode finished but this host had no chain connection to bill it",
                                    &session_key,
                                    &session_id,
                                )
                                .await;
                                break;
                            }

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
                                    // SCI-C: what is proven on chain for this run
                                    // (the first-proof minimum and a deposit cap
                                    // make it differ from `tokens`).
                                    "provenTokens": biller.as_ref().map(|b| b.proven()),
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

                            // D19(d)/D20(c): stored `Done` only now, after the final
                            // proof is confirmed, so a fetch serves only paid work.
                            if let Some(slot) = result.as_mut() {
                                slot.done(complete_payload.clone());
                            }
                            let complete_msg = build_encrypted_transcode_response(
                                &complete_payload,
                                &session_key,
                                &session_id,
                                None,
                            );
                            send_terminal(&progress_tx, complete_msg).await;
                            break;
                        }
                    }
                    Err(e) => {
                        let failing_for = poll_failing_since
                            .get_or_insert_with(std::time::Instant::now)
                            .elapsed();
                        if !poll_gives_up(failing_for, poll_grace) {
                            // The sidecar being away must not let the session's
                            // proof window lapse: keep proving the work seen so
                            // far. A session that has ended is acted on once
                            // polls recover.
                            if let (Some(b), Some(cm)) = (biller.as_mut(), chain.as_ref()) {
                                b.on_progress(
                                    cm.as_ref(),
                                    last_seen_progress,
                                    last_estimate,
                                    unix_now(),
                                )
                                .await;
                            }
                            warn!(
                                "Transcoder status poll failed for {} ({}s of {}s): {}",
                                task_id,
                                failing_for.as_secs(),
                                poll_grace.as_secs(),
                                e
                            );
                            continue;
                        }
                        error!("Transcoder status poll failed for {}: {}", task_id, e);
                        if let Err(e) = transcoder_client.cancel_transcode(&task_id).await {
                            warn!("Sidecar cancel failed for {}: {}", task_id, e);
                        }
                        emit_failure(
                            &progress_tx,
                            &mut result,
                            "POLL_FAILED",
                            &format!("Failed to poll transcoder status: {}", e),
                            &session_key,
                            &session_id,
                        )
                        .await;
                        break;
                    }
                }
            }
            // SCI-C D15/D20: steps already proven are paid on chain, whatever
            // ended the run (stopped, held, final claim failed), so the job may be
            // settled by the node like any billed job.
            if let (Some(b), Some(w)) = (biller.as_ref(), work.as_ref()) {
                if b.proven() > 0 {
                    w.mark_billed();
                }
            }
            // Log billing freeze on any exit path
            if let Some(jid) = job_id {
                info!("Transcode progress loop ended for job {} — billing frozen at last checkpoint (progress={})", jid, last_progress);
            }
            // The outcome is stored before the unit of work ends, so D15's
            // completion never runs ahead of the record a fetch reads.
            drop(result);
            drop(work);
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
    handle_encrypted_transcode_for(
        server,
        decrypted_json,
        session_key,
        session_id,
        job_id,
        None,
        message_id,
    )
    .await
}

/// [`handle_encrypted_transcode`] with the identity that ordered the work
/// (SCI-C D19(c)): the signer recovered from the submitting connection's
/// `encrypted_session_init`. Only that identity may fetch the stored result.
pub async fn handle_encrypted_transcode_for(
    server: &ApiServer,
    decrypted_json: &Value,
    session_key: &[u8; 32],
    session_id: &str,
    job_id: Option<u64>,
    owner: Option<String>,
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

    // 8.62.1: a CID without its storage network would be dropped by the
    // sidecar while its status keeps saying "in progress". Refused here, before
    // any session gate or unit of work, so nothing waits on it.
    if !source_cid_names_network(&source_cid) {
        return (
            build_encrypted_transcode_error(
                "VALIDATION_FAILED",
                SOURCE_CID_NETWORK_MESSAGE,
                session_key,
                session_id,
                message_id,
            ),
            None,
        );
    }

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

    // Step 5 (SCI-C D15): hold a unit of work on the job before the sidecar is
    // called. A job already settled, or whose completion has been decided,
    // takes no new work.
    let work = match job_id {
        Some(jid) => {
            let settled = match server.get_checkpoint_manager().await {
                // 8.63.0: also once the node has decided to settle it.
                Some(cm) => {
                    cm.is_job_settled(jid).await
                        || server.settled_sessions().reason_for(jid).is_some()
                }
                None => false,
            };
            // D20(a): refuse what the session cannot pay for safely. Unreadable
            // fails closed, like the training accept gate.
            let Some(cm) = server.get_checkpoint_manager().await else {
                return (
                    build_encrypted_transcode_error(
                        "BILLING_UNAVAILABLE",
                        "This host cannot bill a session transcode (no chain connection)",
                        session_key,
                        session_id,
                        message_id,
                    ),
                    None,
                );
            };
            {
                use crate::transcoder::biller::TranscodeChain;
                let gate = || async {
                    let raw = cm.session_raw(jid).await?;
                    let view = crate::transcoder::proof_steps::session_view(&raw, jid)
                        .map_err(anyhow::Error::msg)?;
                    let now = cm.chain_now().await?;
                    Ok::<_, anyhow::Error>(crate::transcoder::proof_steps::transcode_accept(
                        &view,
                        now,
                        &crate::transcoder::proof_steps::AcceptLimits::from_env(),
                    ))
                };
                // One retry on a read failure before failing closed.
                let first = gate().await;
                let outcome = match first {
                    Err(_) => gate().await,
                    decided => decided,
                };
                let refusal = match outcome {
                    Ok(Ok(())) => None,
                    Ok(Err(refusal)) => Some((refusal.code(), format!("{:?}", refusal))),
                    Err(e) => Some(("SESSION_UNREADABLE", e.to_string())),
                };
                if let Some((code, detail)) = refusal {
                    warn!("Transcode refused for job {}: {} ({})", jid, code, detail);
                    return (
                        build_encrypted_transcode_error(
                            code,
                            &format!("Transcode refused: {}", detail),
                            session_key,
                            session_id,
                            message_id,
                        ),
                        None,
                    );
                }
            }
            // D19(c): a session transcode must have an owner to serve its
            // result to (an encrypted init on this connection).
            if owner.is_none() {
                return (
                    build_encrypted_transcode_error(
                        "SESSION_INIT_REQUIRED",
                        "A session transcode needs an encrypted session_init on this connection",
                        session_key,
                        session_id,
                        message_id,
                    ),
                    None,
                );
            }
            // D19(d): a run that cannot keep its result for a fetch is not
            // started (no in-memory fallback).
            if server.transcode_result_store().is_none() {
                return (
                    build_encrypted_transcode_error(
                        "RESULT_STORE_UNAVAILABLE",
                        "This host cannot keep transcode results (result store unavailable)",
                        session_key,
                        session_id,
                        message_id,
                    ),
                    None,
                );
            }
            if settled {
                return (
                    build_encrypted_transcode_error(
                        "SESSION_SETTLED",
                        "This session has already been settled; open a new session",
                        session_key,
                        session_id,
                        message_id,
                    ),
                    None,
                );
            }
            match server.job_work().begin(jid) {
                Ok(guard) => Some(guard),
                Err(_) => {
                    return (
                        build_encrypted_transcode_error(
                            "SESSION_CLOSING",
                            "This session is being settled; open a new session",
                            session_key,
                            session_id,
                            message_id,
                        ),
                        None,
                    );
                }
            }
        }
        None => None,
    };

    // Step 6: Submit to transcoder
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

            // D19(c)/(d): record the run as in flight, owned by whoever ordered
            // it, before anything can finish. A record that cannot be written
            // stops the run: its result would be unreachable after a drop.
            let result = match (job_id, server.transcode_result_store()) {
                (Some(jid), Some(store)) => {
                    match store.begin(
                        jid,
                        &resp.task_id,
                        owner.clone(),
                        crate::transcoder::result_store::unix_now(),
                    ) {
                        Ok(()) => Some(crate::transcoder::result_store::ResultSlot::new(
                            store,
                            jid,
                            resp.task_id.clone(),
                            owner.clone(),
                        )),
                        Err(e) => {
                            error!(
                                "[SCI-C] job {}: could not record transcode {}: {}; cancelling it",
                                jid, resp.task_id, e
                            );
                            if let Err(e) = transcoder_client.cancel_transcode(&resp.task_id).await
                            {
                                warn!("Sidecar cancel failed for {}: {}", resp.task_id, e);
                            }
                            return (
                                build_encrypted_transcode_error(
                                    "RESULT_STORE_UNAVAILABLE",
                                    &format!("This host could not record the transcode: {}", e),
                                    session_key,
                                    session_id,
                                    message_id,
                                ),
                                None,
                            );
                        }
                    }
                }
                _ => None,
            };

            let task = TranscodeProgressTask {
                task_id: resp.task_id.clone(),
                poll_interval_ms,
                timeout_seconds,
                work,
                result,
                cancel: tokio_util::sync::CancellationToken::new(),
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

pub fn build_encrypted_transcode_error(
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
