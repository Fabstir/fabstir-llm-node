// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 9: encrypted WebSocket handler for LTX 2.3 generation (`ltx_generate`).
//!
//! Mirrors `transcode.rs`: a two-phase reply (an immediate encrypted `ltx_accepted`
//! ack plus a background [`LtxGenerateTask`] that streams encrypted progress and a
//! terminal `ltx_complete`/`ltx_error`). The background task reuses the SAME S5
//! access the transcode spawn uses (`CheckpointManager::get_s5_storage`) and (M1
//! economics) submits one `submitProofOfWork` per clip through the
//! [`ProofSubmit`](crate::ltx::submit::ProofSubmit) seam on `CheckpointManager`,
//! with a pending/deferred race machine on `LtxTracker` so a WS disconnect can
//! never settle the session at 0 tokens under an in-flight proof.

use crate::api::server::ApiServer;
use crate::ltx::attestation::EnvMeta;
use crate::ltx::client::Progress;
use crate::ltx::template::Bounds;
use crate::ltx::types::{FrameManifest, LtxJob};
use crate::ltx::{attestation, exr, patcher, submit, ComfyClient};
use ethers::types::U256;
use futures::FutureExt;
use rand::RngCore;
use serde_json::{json, Value};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use tokio::sync::{mpsc, OwnedSemaphorePermit};
use tracing::{error, info, warn};

/// Fallback `modelId` until a registered LTX model id is wired (GPU-acceptance).
const ZERO_BYTES32: &str = "0x0000000000000000000000000000000000000000000000000000000000000000";

// ---------------------------------------------------------------------------
// Pure inner-message builders (testable without decryption)
// ---------------------------------------------------------------------------

/// `{type:"ltx_progress", stage, pct, requestId?}`. Stage ∈ generating|encrypting|
/// uploading|finalising.
pub fn ltx_progress_inner(stage: &str, pct: u32, request_id: Option<&str>) -> Value {
    let mut v = json!({ "type": "ltx_progress", "stage": stage, "pct": pct });
    if let Some(r) = request_id {
        v["requestId"] = json!(r);
    }
    v
}

/// `{type:"ltx_complete", outputCID, proofCID, frames:[capabilityCIDs],
/// manifest:{keyless}, billing:{unit, tokens, pricePerToken}, requestId?}`.
pub fn ltx_complete_inner(
    output_cid: &str,
    proof_cid: &str,
    frames: &[String],
    manifest: &FrameManifest,
    tokens: u64,
    price_per_token: &str,
    request_id: Option<&str>,
) -> Value {
    let mut v = json!({
        "type": "ltx_complete",
        "outputCID": output_cid,
        "proofCID": proof_cid,
        "frames": frames,
        "manifest": serde_json::to_value(manifest).unwrap_or_else(|_| json!({})),
        "billing": { "unit": "megapixel-frame", "tokens": tokens, "pricePerToken": price_per_token },
    });
    if let Some(r) = request_id {
        v["requestId"] = json!(r);
    }
    v
}

/// `{type:"ltx_error", error:{code, message}, requestId?}` — an error path carries
/// NO proof (no `proofCID`).
pub fn ltx_error_inner(code: &str, message: &str, request_id: Option<&str>) -> Value {
    let mut v = json!({ "type": "ltx_error", "error": { "code": code, "message": message } });
    if let Some(r) = request_id {
        v["requestId"] = json!(r);
    }
    v
}

// ---------------------------------------------------------------------------
// Encryption envelope (byte-for-byte mirror of transcode; fixed per-handler AAD)
// ---------------------------------------------------------------------------

/// Build an encrypted `encrypted_response` envelope around `inner` with the fixed
/// per-handler AAD `encrypted_ltx_response` (carried in `payload.aadHex`; there is
/// no `message_<index>` AAD in these handlers, exactly like transcode).
pub fn build_encrypted_ltx_response(
    inner: &Value,
    session_key: &[u8; 32],
    session_id: &str,
    message_id: Option<&Value>,
) -> Value {
    let plaintext = serde_json::to_vec(inner).unwrap_or_default();
    let mut nonce = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce);
    let aad = b"encrypted_ltx_response";
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
            error!("Failed to encrypt ltx response: {}", e);
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

/// What one bounded WebSocket write did. See [`send_ws_frame_bounded`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsWriteOutcome {
    /// The frame reached the socket.
    Sent,
    /// The write errored — the peer has gone.
    Failed,
    /// The write did not complete within the bound — the peer is still
    /// connected but is not reading (OQ-L24).
    TimedOut,
}

impl WsWriteOutcome {
    /// Both abnormal outcomes mean the same thing to the progress drain: stop
    /// draining and drop the receiver, so the generation task's existing
    /// disconnect gates classify the client as gone.
    ///
    /// Breaking on `Failed` only would reopen OQ-L24 exactly, so this is a
    /// method rather than a `== Failed` comparison at the call site.
    pub fn client_is_gone(self) -> bool {
        !matches!(self, WsWriteOutcome::Sent)
    }
}

/// Write one frame to a WebSocket sink, bounded by `write_timeout`.
///
/// **OQ-L24.** The unbounded form (`sink.send(frame).await`) strands session
/// escrow. A client that completes the handshake, holds the socket open and
/// then stops reading TCP closes its receive window, so the write never
/// returns. The LTX progress drain then stops draining its bounded 32-slot
/// channel; the channel fills; and every `send_err`/`send_stage` inside the
/// generation core parks on it. The core never returns, so `catch_unwind`
/// never resolves, the VRAM permit is never released and the single-exit
/// cleanup never runs — the clip's pending proof stays unresolved for the
/// process lifetime, `defer_completion` returns `true` for ever, and the
/// disconnect path returns without `completeSessionJob`. The user then pays an
/// on-chain `triggerSessionTimeout` reclaim to recover their own escrow, and
/// the single generation slot is pinned throughout. No panic is involved and
/// the trigger is entirely client-controlled.
///
/// Bounding the write here fixes every one of the core's terminal sends at
/// once, because it feeds machinery that already exists and is already
/// correct: a broken drain drops `progress_rx`, which makes `progress_tx`
/// fail immediately, which is precisely what `send_stage`'s `false` return
/// and the `client_gone` exit are written to detect.
pub async fn send_ws_frame_bounded<S, M>(
    sink: &mut S,
    frame: M,
    write_timeout: std::time::Duration,
) -> WsWriteOutcome
where
    S: futures::Sink<M> + Unpin,
{
    use futures::SinkExt;
    match tokio::time::timeout(write_timeout, sink.send(frame)).await {
        Ok(Ok(())) => WsWriteOutcome::Sent,
        Ok(Err(_)) => WsWriteOutcome::Failed,
        Err(_elapsed) => WsWriteOutcome::TimedOut,
    }
}

/// How long one WebSocket write may take before the client is treated as gone
/// (`LTX_WS_WRITE_TIMEOUT_SECS`, default 300 s).
///
/// **The bound being finite is the fix; its tightness is not.** Five minutes is
/// deliberately loose because a false positive is expensive and silent: the
/// core's `client_gone` exit sends nothing (the channel is already gone), so a
/// still-connected client simply receives silence, and if the stall lands after
/// `finalize_clip` the proof is already on-chain and the user is billed while
/// the `ltx_complete` frame — the ONLY carrier of the output and capability
/// CIDs — is dropped. Renders run 5–15 minutes, and laptop suspend, a VPN
/// re-key or a helper blocked on a large decode can all stall a legitimate
/// client for a minute or more. Bounding at 300 s still closes the unbounded
/// strand while leaving those survivable.
pub fn ltx_ws_write_timeout() -> std::time::Duration {
    parse_ws_write_timeout(std::env::var("LTX_WS_WRITE_TIMEOUT_SECS").ok().as_deref())
}

/// The parse half of [`ltx_ws_write_timeout`], split out so the DEFAULT is
/// testable without mutating process-global environment.
///
/// The default's *magnitude* is the fix: widening 60 to (say) `u64::MAX`
/// silently restores the unbounded behaviour OQ-L24 describes, so it is pinned
/// by test. `0` is rejected rather than honoured — an operator setting it
/// means "disabled", but as a literal bound it would abort every render at its
/// first progress frame with a 0-token refund.
pub fn parse_ws_write_timeout(raw: Option<&str>) -> std::time::Duration {
    std::time::Duration::from_secs(
        raw.and_then(|v| v.parse().ok())
            .filter(|s| *s > 0)
            .unwrap_or(300),
    )
}

/// Build an encrypted `ltx_error` envelope.
pub fn build_ltx_error(
    code: &str,
    message: &str,
    session_key: &[u8; 32],
    session_id: &str,
    request_id: Option<&str>,
    message_id: Option<&Value>,
) -> Value {
    build_encrypted_ltx_response(
        &ltx_error_inner(code, message, request_id),
        session_key,
        session_id,
        message_id,
    )
}

/// Validate job params against the advertised allow-list bounds.
fn validate_bounds(job: &LtxJob, b: &Bounds) -> bool {
    job.frames >= b.frames.min
        && job.frames <= b.frames.max
        && b.fps.contains(&job.fps)
        && b.resolutions.contains(&job.resolution)
}

/// Enforce the clip-duration contract on top of [`validate_bounds`]. The bundle's
/// `frames` min/max and `fps` membership are already checked there; here `frames`
/// must land on an exact whole second at the job's fps and the derived duration
/// must be 5..=15 s. The range is checked on the integer-divided second count
/// FIRST (so a sub-5 s clip reports a duration-range error even when it also fails
/// divisibility), then the exact-whole-second divisibility. `fps == 0` is guarded
/// so this never divides by zero even if a future bounds change lets 0 through.
fn validate_duration(job: &LtxJob) -> Result<(), String> {
    // `duration_secs()` owns the (frames-1)/fps derivation and its zero-guard, so
    // the patcher and this check can never diverge. `None` = zero fps/frames.
    let secs = job
        .duration_secs()
        .ok_or_else(|| format!("invalid frames/fps: frames={}, fps={}", job.frames, job.fps))?;
    if !(5..=15).contains(&secs) {
        return Err(format!(
            "clip duration {secs}s is out of range (must be 5..=15s)"
        ));
    }
    if (job.frames - 1) % job.fps != 0 {
        return Err(format!(
            "frames {} is not a whole number of seconds at {} fps",
            job.frames, job.fps
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Handler: validate + accept (immediate ack) or reject
// ---------------------------------------------------------------------------

/// Handle an encrypted `ltx_generate` request. Returns `(immediate_ack, optional
/// background task)`. Seam codes: VALIDATION_FAILED, SIDECAR_UNAVAILABLE, CAPACITY,
/// GENERATION_FAILED, TIMEOUT (rate-limit maps to CAPACITY — there is no RATE_LIMIT).
pub async fn handle_encrypted_ltx_generate(
    server: &ApiServer,
    decrypted_json: &Value,
    session_key: &[u8; 32],
    session_id: &str,
    job_id: Option<u64>,
    message_id: Option<&Value>,
) -> (Value, Option<LtxGenerateTask>) {
    let request_id = decrypted_json
        .get("requestId")
        .and_then(|v| v.as_str())
        .map(String::from);
    let rid = request_id.as_deref();
    let reject = |code: &str, msg: &str| -> (Value, Option<LtxGenerateTask>) {
        (
            build_ltx_error(code, msg, session_key, session_id, rid, message_id),
            None,
        )
    };

    // Deserialise the job (ignores extra action/requestId keys).
    let job: LtxJob = match serde_json::from_value(decrypted_json.clone()) {
        Ok(j) => j,
        Err(e) => return reject("VALIDATION_FAILED", &format!("invalid ltx job: {e}")),
    };

    // Rate-limit (maps to CAPACITY).
    if !server.ltx_rate_limiter().check_rate_limit(session_id) {
        return reject("CAPACITY", "LTX generation rate limit exceeded");
    }

    // Sidecar availability (the caller hands the spawn a client; here we only gate).
    if server.get_ltx_client().await.is_none() {
        return reject("SIDECAR_UNAVAILABLE", "LTX sidecar not configured (503)");
    }

    // Template allow-list pin + bounds.
    let store = match server.get_ltx_template_store().await {
        Some(s) => s,
        None => return reject("VALIDATION_FAILED", "LTX template store not configured"),
    };
    let graph = match store.verify(&job.template_id, &job.template_hash) {
        Ok(g) => g,
        Err(e) => return reject("VALIDATION_FAILED", &format!("template rejected: {e}")),
    };
    if !validate_bounds(&job, &store.bundle().bounds) {
        return reject("VALIDATION_FAILED", "job params out of allow-list bounds");
    }
    // Clip-duration contract (5..=15 s, exact whole seconds at the job's fps) —
    // enforced after the bundle bounds so fps membership / frame min-max fire
    // first with their own message.
    if let Err(msg) = validate_duration(&job) {
        return reject("VALIDATION_FAILED", &msg);
    }

    // Input-image validation (M1a), fail-closed BEFORE a slot is spent. The
    // template's `imageInputs` is the commitment format selector: the job MUST
    // carry exactly that many images (0 for t2v). Each image's size is checked
    // from the capability CID's own length field — parse only, NO network fetch —
    // so an oversize image is rejected without a wasted portal GET.
    if let Err(msg) = validate_input_cids(
        &job.template_id,
        "image",
        "imageMaxBytes",
        job.images.as_deref().unwrap_or_default(),
        store.image_inputs(&job.template_id).unwrap_or(0),
        store.bundle().bounds.image_max_bytes,
    ) {
        return reject("VALIDATION_FAILED", &msg);
    }
    // Input-video validation (BL3): same gate, `videoInputs` is the v3 commitment
    // selector.
    if let Err(msg) = validate_input_cids(
        &job.template_id,
        "video",
        "videoMaxBytes",
        job.videos.as_deref().unwrap_or_default(),
        store.video_inputs(&job.template_id).unwrap_or(0),
        store.bundle().bounds.video_max_bytes,
    ) {
        return reject("VALIDATION_FAILED", &msg);
    }

    // Patch scalar params into the pinned graph (substitution only). Image/video
    // inputs (LoadImage/LoadVideo) are patched post-accept in the spawn, once the
    // inputs are fetched from S5 and uploaded to ComfyUI (they need the stored
    // filenames).
    let patched_graph = match patcher::patch(&graph, &job, &[], &[]) {
        Ok(g) => g,
        Err(e) => return reject("VALIDATION_FAILED", &format!("patch failed: {e}")),
    };

    // VRAM admission — hold the owned permit for the job's lifetime.
    let permit = match server.ltx_semaphore().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => return reject("CAPACITY", "all generation slots are in use"),
    };

    // M1 economics: mark the clip's proof PENDING at accept — HERE in the
    // handler, not the detached spawn body, which would leave an accept→spawn
    // gap a disconnect could settle through (0-token settle under a rendering
    // clip). Sessionless requests and cm-less nodes (tests) mark nothing. Also
    // cancels a stale deferral on a reconnected session (see LtxTracker).
    // ATOMIC accept gate: the mark fails while a completeSessionJob dispatched
    // within the latch window is in flight (disconnect + fast reconnect) — a
    // clip accepted mid-completion would be settled under (proof reverts, clip
    // delivers free, session dead). Maps to CAPACITY (retryable; the latch
    // self-expires). The permit drops with the reject.
    let latch = std::time::Duration::from_secs(crate::ltx::billing::COMPLETING_LATCH_SECS);
    let pending_marked = match job_id {
        Some(jid) if server.get_checkpoint_manager().await.is_some() => {
            if !server.ltx_tracker().mark_proof_pending(jid, latch).await {
                return reject("CAPACITY", "session settlement in progress — retry shortly");
            }
            true
        }
        _ => false,
    };

    // Accept.
    server.ltx_rate_limiter().record_request(session_id);
    let allow_list_version = store.bundle().allow_list_version;
    let timeout_secs = std::env::var("LTX_JOB_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1800);

    let mut ack_inner = json!({
        "type": "ltx_accepted",
        "status": "processing",
        "sessionId": session_id,
        "allowListVersion": allow_list_version,
    });
    if let Some(r) = rid {
        ack_inner["requestId"] = json!(r);
    }
    let ack = build_encrypted_ltx_response(&ack_inner, session_key, session_id, message_id);

    let task = LtxGenerateTask {
        job,
        patched_graph,
        request_id,
        allow_list_version,
        timeout_secs,
        job_id,
        permit,
        pending_marked,
        panic_seam: None,
    };
    (ack, Some(task))
}

// ---------------------------------------------------------------------------
// Background generation task
// ---------------------------------------------------------------------------

/// Accepted-but-not-yet-run generation. Holds the VRAM `permit` until the spawned
/// task finishes.
pub struct LtxGenerateTask {
    pub job: LtxJob,
    pub patched_graph: crate::ltx::Graph,
    pub request_id: Option<String>,
    pub allow_list_version: u32,
    pub timeout_secs: u64,
    pub job_id: Option<u64>,
    pub permit: OwnedSemaphorePermit,
    /// The handler marked `mark_proof_pending(job_id)` at accept. The spawn's
    /// single-exit cleanup resolves it; if the caller DROPS this task without
    /// spawning it, the caller must `mark_proof_forfeited(job_id)` first.
    pub pending_marked: bool,
    /// TEST SEAM (A.0) — the accept path always constructs this `None` and
    /// nothing in `src/` ever produces a `Some`. It makes the panic-safety path
    /// (DL16(a): `catch_unwind` → terminal frame → [`finish_ltx_task`])
    /// exercisable from an integration crate without a live ComfyUI/S5 harness.
    /// It exists because `tests/ltx_api_tests.rs` is a separate crate that can
    /// see neither `pub(crate)` items nor `#[cfg(test)]` code, so the seam must
    /// be plain `pub` or the test cannot compile.
    #[doc(hidden)]
    pub panic_seam: Option<LtxPanicSeam>,
}

/// Which panic [`LtxGenerateTask::run`]'s core injects at entry. Test-only;
/// see [`LtxGenerateTask::panic_seam`].
///
/// Two variants because the cleanup's forfeit decision turns on
/// `pending_resolved`, and the two sides of that branch are otherwise
/// indistinguishable from a test: both real `pending_resolved = true`
/// assignments sit after `finalize_clip` has been REACHED (its `Ok` arm and its
/// proof-upload-failure `Err` arm), which needs the live ComfyUI + S5 harness
/// deferred to Phase B2.0.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LtxPanicSeam {
    /// Panic with the clip's proof still UNRESOLVED — an exit before
    /// `finalize_clip`. The cleanup must forfeit the pending proof.
    BeforeResolve,
    /// Mark the clip's proof resolved exactly as the `finalize_clip` arms do,
    /// then panic. The cleanup must NOT forfeit it a second time, or it would
    /// consume a CONCURRENT clip's pending mark and let the disconnect path
    /// settle the session under that clip's in-flight proof.
    AfterResolve,
}

/// The LTX generation task's SINGLE-EXIT cleanup — every exit of
/// [`LtxGenerateTask::run`]'s core funnels through here, including (since
/// DL16(a)) a caught panic.
///
/// (a) An exit before `finalize_clip` leaves the clip's pending unresolved:
///     forfeit it (no work delivered ⇒ that clip settles at 0 — correct).
/// (b) If a disconnect deferred completion and no proof is pending any more,
///     THIS task owns `completeSessionJob`: wait out the dispute window since
///     the last landed proof (a host caller reverts "Dispute wait" inside it),
///     then complete — one belt-and-braces retry (cm may also retry
///     internally). PEEK (read-only) before the sleep, TAKE only at wake: a
///     clip accepted on a reconnected session mid-sleep clears the flag at
///     accept, so the take fails and completion ownership moves to that clip's
///     own lifecycle — never settle under it.
///
/// Extracted (A.0) so the already-resolved path is unit-testable without a
/// live render; plain `pub` for the same integration-crate visibility reason as
/// [`LtxGenerateTask::panic_seam`].
pub async fn finish_ltx_task(
    server: &ApiServer,
    job_id: Option<u64>,
    pending_marked: bool,
    pending_resolved: bool,
) {
    let Some(jid) = job_id else {
        return;
    };
    if pending_marked && !pending_resolved {
        server.ltx_tracker().mark_proof_forfeited(jid).await;
    }
    if !server.ltx_tracker().deferred_idle(jid).await {
        return;
    }
    let Some(cm) = server.get_checkpoint_manager().await else {
        error!("LTX job {jid}: completion deferred but no checkpoint manager");
        return;
    };
    let window =
        cm.dispute_window_secs() + crate::contracts::checkpoint_manager::DISPUTE_WINDOW_BUFFER_SECS;
    let wait = server.ltx_tracker().proof_wait_remaining(jid, window).await;
    if !wait.is_zero() {
        info!(
            "LTX job {jid}: deferred completion waiting {}s dispute window",
            wait.as_secs()
        );
        tokio::time::sleep(wait).await;
    }
    // The take also SETS the completing latch (same lock): accepts are
    // rejected while the completion runs.
    if !server.ltx_tracker().take_deferred_if_idle(jid).await {
        info!("LTX job {jid}: deferred completion ownership moved to a newer clip — skipping");
        return;
    }
    info!("LTX job {jid}: running deferred session completion");
    // complete_session_job's error is a non-Send Box<dyn Error>: stringify
    // before holding it across an await.
    let first = cm
        .complete_session_job(jid)
        .await
        .map_err(|e| e.to_string());
    if let Err(e) = first {
        warn!("LTX job {jid}: deferred completion failed ({e}); retrying once after {window}s");
        tokio::time::sleep(std::time::Duration::from_secs(window)).await;
        // Atomically re-latch for the retry; false = a newer clip owns
        // completion now.
        if !server.ltx_tracker().mark_completing_if_idle(jid).await {
            info!(
                "LTX job {jid}: a newer clip is in flight — abandoning the completion retry to \
                 its lifecycle"
            );
            return;
        }
        let retry = cm
            .complete_session_job(jid)
            .await
            .map_err(|e| e.to_string());
        if let Err(e2) = retry {
            error!("LTX job {jid}: deferred completion retry failed: {e2}");
        }
    }
}

async fn send_stage(
    tx: &mpsc::Sender<Value>,
    stage: &str,
    pct: u32,
    key: &[u8; 32],
    sid: &str,
    rid: Option<&str>,
) -> bool {
    let m = build_encrypted_ltx_response(&ltx_progress_inner(stage, pct, rid), key, sid, None);
    tx.send(m).await.is_ok()
}

async fn send_err(
    tx: &mpsc::Sender<Value>,
    code: &str,
    msg: &str,
    key: &[u8; 32],
    sid: &str,
    rid: Option<&str>,
) {
    let _ = tx
        .send(build_ltx_error(code, msg, key, sid, rid, None))
        .await;
}

fn env_or(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}

/// The base URL the input-image ciphertext is fetched from. This is the LOCAL S5
/// BRIDGE (`ENHANCED_S5_URL`, default `http://localhost:5522`), whose
/// `GET /s5/blob/{cid}` route resolves the blob over the S5 protocol
/// (`downloadByCID`, P2P) — the same transport the node's uploads and the
/// transcoder's client-source downloads already use. A raw portal HTTP GET is NOT
/// a supported transport (it 500s even for blobs that exist), so we go through the
/// bridge's real S5 client. NOTE (deploy): the bridge must peer with the portal
/// the client uploads to (`S5_INITIAL_PEERS` must include the
/// `s5.platformlessai.ai` P2P node) so it can pull a client blob on demand.
fn s5_blob_source_url() -> String {
    std::env::var("ENHANCED_S5_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://localhost:5522".to_string())
}

/// Pre-accept gate for one media kind (M1a images / BL3 videos), fail-closed
/// BEFORE a slot is spent: count == the template's selector (`imageInputs`/
/// `videoInputs` — also the commitment format selector), a missing size bound
/// rejects (unset must NOT mean "unlimited": that would re-open an unbounded
/// fetch on a client-influenced size), and each capability CID's own claimed
/// length is checked — parse only, NO network fetch.
fn validate_input_cids(
    template_id: &str,
    kind: &str,
    bound_name: &str,
    cids: &[String],
    expected: u32,
    max_bytes: u64,
) -> Result<(), String> {
    if cids.len() != expected as usize {
        return Err(format!(
            "template {:?} expects {} input {}(s), got {}",
            template_id,
            expected,
            kind,
            cids.len()
        ));
    }
    if !cids.is_empty() && max_bytes == 0 {
        return Err(format!(
            "template accepts {kind}s but no {bound_name} bound is configured"
        ));
    }
    for cid in cids {
        let env = crate::ltx::input_image::parse_capability_cid(cid)
            .map_err(|e| format!("invalid {kind} capability CID: {e}"))?;
        if env.plaintext_len as u64 > max_bytes {
            return Err(format!(
                "input {} is {} bytes, exceeds {} {}",
                kind, env.plaintext_len, bound_name, max_bytes
            ));
        }
    }
    Ok(())
}

/// One staged input: fetch + integrity-check + decrypt the capability CID from
/// the S5 portal, run the kind-specific plaintext check, upload to ComfyUI under
/// the content-addressed name (`keccak256(plaintext)` ⇒ stable under
/// `overwrite`). Returns (stored name, plaintext keccak) — the commitment input.
async fn stage_input(
    client: &ComfyClient,
    blob_source: &str,
    cid: &str,
    kind: &str,
    ext: &str,
    check: impl Fn(&[u8]) -> Result<(), String>,
) -> Result<(String, [u8; 32]), String> {
    let (hash, plaintext) = crate::ltx::input_image::fetch_image_hash(blob_source, cid)
        .await
        .map_err(|e| format!("input {kind} fetch failed: {e}"))?;
    check(&plaintext)?;
    let name = client
        .upload_input(&format!("{}.{ext}", hex::encode(hash)), plaintext)
        .await
        .map_err(|e| format!("input {kind} upload failed: {e}"))?;
    Ok((name, hash))
}

/// Resolve a conditioned job's inputs (M1a images + BL3 videos). For each
/// ordered capability CID: fetch + integrity-check + decrypt it from the S5
/// portal, upload the plaintext to ComfyUI under a content-addressed name
/// (`keccak256(plaintext)` ⇒ stable under `overwrite`), and record
/// `keccak256(plaintext)` into `image_hashes`/`video_hashes` (the v2/v3
/// commitment inputs). Finally patch the `LoadImage`/`LoadVideo` nodes with the
/// stored names. A t2v job (no inputs) returns the graph untouched and leaves
/// both hash vecs empty. Pre-accept validation has already checked the counts
/// and per-input sizes, so any failure here is post-accept (`GENERATION_FAILED`).
/// The control-video plaintext gate, run on the decrypted bytes BEFORE any
/// ComfyUI upload or GPU work: (1) the bundle's `videoFormats` is `["mp4"]` —
/// enforce the container (an ISO BMFF file has "ftyp" at offset 4; the
/// capability CID itself carries no format, so this is the earliest the node
/// can check it); (2) the clip's own sample count must be AT LEAST the billed
/// count minus one (the conform ±1: content-true fps·d vs billed fps·d+1) —
/// an under-length clip would render fewer frames than billed (overbilling).
/// Over-length clips are accepted as of v8.36.1: both pinned-graph families
/// crop server-side by construction (the BL4 trio's `VHS_LoadVideo` carries
/// `frame_load_cap` patched to the billed count; iclora's `Video Slice` takes
/// the first `duration` seconds), so extra footage cannot inflate the rendered
/// count — this lets clients submit an off-grid-length clip (e.g. 14.76 s)
/// against the rounded-down whole-second job without client-side re-encoding.
/// The clip's fps must still match the job exactly (conditioning + output
/// timing derive from it); the TS helper enforces that via ffprobe.
pub fn check_control_video(plaintext: &[u8], billed_frames: u32) -> Result<(), String> {
    if plaintext.len() < 12 || &plaintext[4..8] != b"ftyp" {
        return Err("input video is not an mp4 (ISO BMFF) container".to_string());
    }
    let samples = crate::ltx::mp4::video_sample_count(plaintext).map_err(|e| {
        format!("input video frame count unreadable ({e}) — refusing unbounded render")
    })?;
    let billed = u64::from(billed_frames);
    if samples + 1 < billed {
        return Err(format!(
            "control video has {samples} frame(s) but the job bills {billed} — \
             the clip must carry at least {} frame(s) at the job's fps",
            billed.saturating_sub(1)
        ));
    }
    Ok(())
}

async fn prepare_inputs(
    client: &ComfyClient,
    job: &LtxJob,
    graph: crate::ltx::Graph,
    image_hashes: &mut Vec<[u8; 32]>,
    video_hashes: &mut Vec<[u8; 32]>,
) -> Result<crate::ltx::Graph, String> {
    let images = job.images.as_deref().unwrap_or_default();
    let videos = job.videos.as_deref().unwrap_or_default();
    if images.is_empty() && videos.is_empty() {
        return Ok(graph);
    }
    let blob_source = s5_blob_source_url();
    let mut image_names = Vec::with_capacity(images.len());
    for cid in images {
        let (name, hash) =
            stage_input(client, &blob_source, cid, "image", "png", |_| Ok(())).await?;
        image_names.push(name);
        image_hashes.push(hash);
    }
    let mut video_names = Vec::with_capacity(videos.len());
    for cid in videos {
        let (name, hash) = stage_input(client, &blob_source, cid, "video", "mp4", |plaintext| {
            check_control_video(plaintext, job.frames)
        })
        .await?;
        video_names.push(name);
        video_hashes.push(hash);
    }
    patcher::patch(&graph, job, &image_names, &video_names)
        .map_err(|e| format!("input patch failed: {e}"))
}

impl LtxGenerateTask {
    /// Submit the pinned graph to ComfyUI, stream progress, then run the EXR
    /// pipeline (collect → encrypt+upload → manifest → attestation) over the same
    /// S5 access transcode uses, ending in an encrypted `ltx_complete` (or
    /// `ltx_error`, which submits NO proof).
    pub fn spawn(
        self,
        client: Arc<ComfyClient>,
        session_key: [u8; 32],
        session_id: String,
        server: Arc<ApiServer>,
        progress_tx: mpsc::Sender<Value>,
    ) {
        tokio::spawn(self.run(client, session_key, session_id, server, progress_tx));
    }

    /// The body [`Self::spawn`] detaches. Kept awaitable and plain `pub` (A.0)
    /// so an integration crate can drive one full generation lifecycle to
    /// completion deterministically — `spawn` discards its `JoinHandle`, so a
    /// test driven through it can only poll.
    ///
    /// NOT CANCELLATION-SAFE: dropping this future part-way (a `select!` arm, a
    /// `tokio::time::timeout`, a cancelled parent task) skips the single-exit
    /// cleanup entirely and strands the clip's pending proof — the very failure
    /// this task's structure exists to prevent. Production must go through
    /// [`Self::spawn`], which detaches it onto its own task.
    pub async fn run(
        self,
        client: Arc<ComfyClient>,
        session_key: [u8; 32],
        session_id: String,
        server: Arc<ApiServer>,
        progress_tx: mpsc::Sender<Value>,
    ) {
        let LtxGenerateTask {
            job,
            patched_graph,
            request_id,
            allow_list_version: _,
            timeout_secs,
            job_id,
            permit,
            pending_marked,
            panic_seam,
        } = self;
        // This block is a no-op scope kept deliberately: it was the body of the
        // old `tokio::spawn(async move { … })`, and preserving it keeps A.0's
        // diff to the extraction itself rather than re-indenting ~490 lines of
        // a live billing path.
        {
            let _permit = permit; // hold the VRAM slot for the whole job
            let rid = request_id.as_deref();
            let (key, sid) = (&session_key, session_id.as_str());
            // Set once `finalize_clip` has run (it resolves the pending itself
            // on every internal path); the single-exit cleanup below forfeits
            // the pending for any exit taken BEFORE it.
            let mut pending_resolved = false;

            // The core runs as an inner future so that EVERY exit (there are
            // ~10 early returns) funnels through the ONE cleanup below —
            // without it, any error path taken after a disconnect deferred
            // completion would leave the session unsettled until job timeout.
            let core = async {
                // Inert in any release binary: `debug_assertions` is off under
                // `--release`, so the shipped artefact cannot panic here even
                // if a future construction site sets the field.
                if cfg!(debug_assertions) {
                    match panic_seam {
                        Some(LtxPanicSeam::BeforeResolve) => {
                            panic!("LTX test seam: panic before resolve")
                        }
                        Some(LtxPanicSeam::AfterResolve) => {
                            pending_resolved = true;
                            panic!("LTX test seam: panic after resolve")
                        }
                        None => {}
                    }
                }
                // Conditioned templates: fetch each input image/video from S5,
                // upload to ComfyUI, patch the LoadImage/LoadVideo nodes, and
                // collect the per-input `keccak256(plaintext)` for the v2/v3
                // commitment. Both empty for t2v.
                let mut image_hashes: Vec<[u8; 32]> = Vec::new();
                let mut video_hashes: Vec<[u8; 32]> = Vec::new();
                let patched_graph = match prepare_inputs(
                    &client,
                    &job,
                    patched_graph,
                    &mut image_hashes,
                    &mut video_hashes,
                )
                .await
                {
                    Ok(g) => g,
                    Err(e) => {
                        send_err(&progress_tx, "GENERATION_FAILED", &e, key, sid, rid).await;
                        return;
                    }
                };

                // 1. Submit graph → prompt_id.
                let prompt_id = match client.submit(&patched_graph).await {
                    Ok(p) => p,
                    Err(e) => {
                        send_err(
                            &progress_tx,
                            "GENERATION_FAILED",
                            &format!("submit failed: {e}"),
                            key,
                            sid,
                            rid,
                        )
                        .await;
                        return;
                    }
                };

                // 2. Watch + stream generating progress. A failed send_stage means
                // the OUTER progress channel is gone (WS disconnect) — but watch_rx
                // would stay alive, so `watch` would block until job timeout (the
                // 64-slot channel fills) or complete and bill an undeliverable
                // clip. Make abandonment DETERMINISTIC instead: drop the receiver
                // (watch's sender then errors → watch returns promptly), interrupt
                // ComfyUI (stop wasting GPU), and take a "client disconnected"
                // error exit BEFORE any billing/submit — the single-exit cleanup
                // turns that into forfeit → deferred completion at 0 tokens (user
                // refunded, host eats the GPU time; deliver-after-disconnect is a
                // future milestone).
                let (watch_tx, mut watch_rx) = mpsc::channel::<Progress>(64);
                let (wc, pid) = (client.clone(), prompt_id.clone());
                let watch_handle =
                    tokio::spawn(async move { wc.watch(&pid, watch_tx, timeout_secs).await });
                let mut client_gone = false;
                while let Some(p) = watch_rx.recv().await {
                    if let Progress::Progress { value, max } = p {
                        let pct = if max > 0 {
                            ((value as u64 * 100) / max as u64) as u32
                        } else {
                            0
                        };
                        if !send_stage(&progress_tx, "generating", pct, key, sid, rid).await {
                            client_gone = true;
                            break;
                        }
                    }
                }
                if client_gone {
                    drop(watch_rx);
                    let _ = client.interrupt().await;
                    let _ = watch_handle.await;
                    warn!("LTX job abandoned mid-render: client disconnected (render interrupted)");
                    return;
                }
                match watch_handle.await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        let code = if e.to_string().contains("timed out") {
                            "TIMEOUT"
                        } else {
                            "GENERATION_FAILED"
                        };
                        send_err(
                            &progress_tx,
                            code,
                            &format!("generation failed: {e}"),
                            key,
                            sid,
                            rid,
                        )
                        .await;
                        return;
                    }
                    Err(_) => {
                        send_err(
                            &progress_tx,
                            "GENERATION_FAILED",
                            "watch task panicked",
                            key,
                            sid,
                            rid,
                        )
                        .await;
                        return;
                    }
                }

                // 3. EXR pipeline. Enumerate THIS prompt's outputs (scoped by prompt_id) —
                // NOT a glob of the shared output dir, which would leak other concurrent
                // jobs' frames into this manifest/capability set.
                let mut output_refs = match client.outputs(&prompt_id).await {
                    Ok(r) => r,
                    Err(e) => {
                        send_err(
                            &progress_tx,
                            "GENERATION_FAILED",
                            &format!("output enumeration failed: {e}"),
                            key,
                            sid,
                            rid,
                        )
                        .await;
                        return;
                    }
                };
                // Keep only final "output" frames (drop "temp" previews) so a preview can't
                // pollute the manifest or trip the count check.
                output_refs.retain(|r| r.type_ == "output");
                // Deterministic order (ComfyUI writes zero-padded frame indices).
                output_refs.sort_by(|a, b| a.filename.cmp(&b.filename));
                if output_refs.is_empty() {
                    send_err(
                        &progress_tx,
                        "GENERATION_FAILED",
                        "no frames produced",
                        key,
                        sid,
                        rid,
                    )
                    .await;
                    return;
                }
                // The output count is advisory this pass: the pinned graph controls clip
                // length (a single video file, or an EXR sequence), so `job.frames` need not
                // equal the delivered count. Billing still uses `job.frames`; just surface any
                // divergence in the logs rather than failing a legitimate video artefact.
                if output_refs.len() != job.frames as usize {
                    warn!(
                        "LTX output count {} differs from requested frames {} (advisory this pass)",
                        output_refs.len(),
                        job.frames
                    );
                }
                let cm = match server.get_checkpoint_manager().await {
                    Some(cm) => cm,
                    None => {
                        send_err(
                            &progress_tx,
                            "GENERATION_FAILED",
                            "storage unavailable",
                            key,
                            sid,
                            rid,
                        )
                        .await;
                        return;
                    }
                };
                let s5 = cm.get_s5_storage();
                // Fold the always-present, always-unique prompt_id into the S5 path so
                // concurrent jobs (or job_id-less requests) can never collide on the same
                // `home/ltx/...` prefix; job_id stays in front for operator correlation.
                let job_tag = format!(
                    "{}-{}",
                    job_id
                        .map(|j| j.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    prompt_id
                );

                // Stage-transition sends double as DISCONNECT GATES: a failed
                // send means the client is gone, and every capability CID
                // delivered only in `ltx_complete` would die unsent — billing
                // an undeliverable clip. Abandon at each cheap gate BEFORE the
                // next irreversible step (S5 uploads / the proof submit); the
                // single-exit cleanup forfeits and settles at 0 for this clip.
                // (The per-frame pct sends stay fire-and-forget; the next gate
                // catches a mid-loop disconnect before the submit.) Residual
                // accepted window: a disconnect after the finalising gate, i.e.
                // during upload+submit itself.
                if !send_stage(&progress_tx, "encrypting", 0, key, sid, rid).await {
                    warn!("LTX job abandoned post-render: client disconnected before delivery");
                    return;
                }
                let total = output_refs.len();
                let mut caps = Vec::with_capacity(total);
                let mut hashes = Vec::with_capacity(total);
                for (i, r) in output_refs.iter().enumerate() {
                    let dest = format!("home/ltx/job_{job_tag}/frame_{i:05}.bin");
                    // Pull the rendered file over ComfyUI's /view (no shared volume), then
                    // encrypt+upload its bytes.
                    let bytes = match client.download(r).await {
                        Ok(b) => b,
                        Err(e) => {
                            send_err(
                                &progress_tx,
                                "GENERATION_FAILED",
                                &format!("fetch {} from comfyui failed: {e}", r.filename),
                                key,
                                sid,
                                rid,
                            )
                            .await;
                            return;
                        }
                    };
                    match exr::encrypt_bytes_and_upload(bytes, s5, &dest).await {
                        Ok((cap, h)) => {
                            caps.push(cap);
                            hashes.push(h);
                        }
                        Err(e) => {
                            send_err(
                                &progress_tx,
                                "GENERATION_FAILED",
                                &format!("frame upload failed: {e}"),
                                key,
                                sid,
                                rid,
                            )
                            .await;
                            return;
                        }
                    }
                    let pct = (((i + 1) * 100) / total) as u32;
                    let _ = send_stage(&progress_tx, "encrypting", pct, key, sid, rid).await;
                }

                if !send_stage(&progress_tx, "uploading", 0, key, sid, rid).await {
                    warn!("LTX job abandoned post-encrypt: client disconnected before delivery");
                    return;
                }
                let manifest = match exr::build_manifest(&hashes, &job) {
                    Ok(m) => m,
                    Err(e) => {
                        send_err(
                            &progress_tx,
                            "GENERATION_FAILED",
                            &format!("manifest build failed: {e}"),
                            key,
                            sid,
                            rid,
                        )
                        .await;
                        return;
                    }
                };
                let output_cid = match exr::upload_manifest(
                    &manifest,
                    s5,
                    &format!("home/ltx/job_{job_tag}/manifest.json"),
                )
                .await
                {
                    Ok(c) => c,
                    Err(e) => {
                        send_err(
                            &progress_tx,
                            "GENERATION_FAILED",
                            &format!("manifest upload failed: {e}"),
                            key,
                            sid,
                            rid,
                        )
                        .await;
                        return;
                    }
                };

                // The LAST gate before the proof submit — after this the clip
                // is billed, so an unsendable ltx_complete must be ruled out
                // as late as detection allows.
                if !send_stage(&progress_tx, "finalising", 0, key, sid, rid).await {
                    warn!("LTX job abandoned pre-submit: client disconnected before delivery");
                    return;
                }
                // TODO(GPU-acceptance): real reproduction hashes hydrate envHash.
                let env_meta = EnvMeta {
                    weights_hash: env_or("LTX_WEIGHTS_HASH"),
                    lora_hash: env_or("LTX_LORA_HASH"),
                    comfy_commit: env_or("LTX_COMFY_COMMIT"),
                    node_commit: env_or("LTX_NODE_COMMIT"),
                    cuda_version: env_or("LTX_CUDA_VERSION"),
                    gpu_class: env_or("LTX_GPU_CLASS"),
                };
                let env_hash = attestation::env_hash(&env_meta);
                // TODO(GPU-acceptance): real registered modelId + node signing key (None ⇒ unsigned).
                let model_id =
                    std::env::var("LTX_MODEL_ID").unwrap_or_else(|_| ZERO_BYTES32.to_string());
                let host = cm.get_host_address();
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let att = match attestation::assemble(
                    model_id,
                    job.template_hash.clone(),
                    env_hash,
                    &job,
                    &image_hashes,
                    &video_hashes,
                    output_cid.clone(),
                    manifest.clone(),
                    session_id.clone(),
                    host,
                    timestamp,
                    None,
                ) {
                    Ok(a) => a,
                    Err(e) => {
                        send_err(
                            &progress_tx,
                            "GENERATION_FAILED",
                            &format!("attestation failed: {e}"),
                            key,
                            sid,
                            rid,
                        )
                        .await;
                        return;
                    }
                };
                // M1 economics: upload the attestation (proofCID) and submit ONE
                // submitProofOfWork per clip through the ProofSubmit seam on cm
                // (strict success: tx confirmed with receipt status 1). `tokens` is
                // the ONE variable feeding tokensClaimed, the wire billing.tokens
                // and the tracker (§B triple equality). finalize_clip resolves the
                // pending on every internal path; a submit failure still returns
                // the proof_cid (clip delivery ≥ revenue — the client paid for and
                // receives its clip; the node forfeits that clip's revenue).
                let tokens = submit::ltx_tokens(job.frames, job.resolution.w, job.resolution.h);
                let proof_cid = match submit::finalize_clip(
                    s5,
                    Some(&*cm),
                    server.ltx_tracker(),
                    job_id,
                    pending_marked,
                    &job_tag,
                    &att,
                    tokens,
                )
                .await
                {
                    Ok((cid, _submitted)) => {
                        pending_resolved = true;
                        cid
                    }
                    Err(e) => {
                        // Upload failure: no proofCID exists at all — the one
                        // finalize failure that stays an error (pending already
                        // forfeited inside finalize_clip).
                        pending_resolved = true;
                        send_err(
                            &progress_tx,
                            "GENERATION_FAILED",
                            &format!("proof upload failed: {e}"),
                            key,
                            sid,
                            rid,
                        )
                        .await;
                        return;
                    }
                };
                let price =
                    std::env::var("LTX_PRICE_PER_TOKEN").unwrap_or_else(|_| "0".to_string());
                if let Some(jid) = job_id {
                    let ppt = U256::from_dec_str(&price).unwrap_or_default();
                    let cost = U256::from(tokens).checked_mul(ppt).unwrap_or(U256::MAX);
                    // Metrics/observability totals: includes clips whose submit
                    // was forfeited (delivered but unproven) — the on-chain
                    // claim is what finalize_clip actually submitted.
                    server
                        .ltx_tracker()
                        .track(jid, Some(session_id.clone()), tokens, cost)
                        .await;
                }
                let inner = ltx_complete_inner(
                    &output_cid,
                    &proof_cid,
                    &caps,
                    &manifest,
                    tokens,
                    &price,
                    rid,
                );
                let _ = progress_tx
                    .send(build_encrypted_ltx_response(&inner, key, sid, None))
                    .await;
                info!(
                    "LTX job {job_tag} complete: {} frames, outputCID={output_cid}",
                    caps.len()
                );
            };
            // DL16(a): a panic anywhere in the core used to unwind straight out
            // of the spawned task, skipping the single-exit cleanup below — the
            // clip's pending proof was then never forfeited, `pending_count`
            // stayed at 1 forever, and the later disconnect path took
            // `defer_completion` (true) and returned WITHOUT completing the
            // session. Catching the unwind restores the single exit. (`panic =
            // unwind` is in force; no profile sets `panic = "abort"`.)
            let panicked = AssertUnwindSafe(core).catch_unwind().await.is_err();
            // Render is over on every core exit — release the VRAM slot before
            // any settlement sleeps (the deferred path can wait 35s+).
            drop(_permit);

            if panicked {
                error!(
                    "LTX job {:?} (session {sid}): generation task PANICKED — forfeiting the \
                     clip and running the single-exit cleanup",
                    job_id
                );
            }

            // ORDER MATTERS: the money cleanup runs BEFORE the terminal frame.
            // `send_err` awaits a send on a BOUNDED channel (capacity 32,
            // `api/server.rs:2916`) whose drain loop blocks inside
            // `ws_sender.send(...).await` with no write timeout — a client that
            // holds the socket open but stops reading TCP fills that channel and
            // parks the send forever. Sending first would therefore skip the
            // forfeit on exactly the input a hostile client controls.
            // SCOPE: ordering matters here for the PANIC exit. The wider
            // wedge — the core's other terminal exits parking inside the core
            // on a full channel — was OQ-L24, and is now closed separately by
            // bounding the WebSocket writes themselves (`send_ws_frame_bounded`),
            // so a non-reading client can no longer park any of them.
            // Nothing is lost by
            // going second: the deferred branch of the cleanup only runs when a
            // disconnect already happened (`completion_deferred`), in which case
            // this task's `progress_tx` belongs to a connection that is gone and
            // the frame is undeliverable anyway; when the client IS connected the
            // cleanup returns immediately after the forfeit.
            finish_ltx_task(&server, job_id, pending_marked, pending_resolved).await;

            if panicked {
                // Otherwise the client waits out LTX_JOB_TIMEOUT_SECS. Reuses
                // GENERATION_FAILED (DL9): the addon renders only the leading
                // ALLCAPS token, so a distinguishable code needs helper/addon
                // work (OQ-L8) and is outside Slice A.
                send_err(
                    &progress_tx,
                    "GENERATION_FAILED",
                    "generation failed unexpectedly",
                    key,
                    sid,
                    rid,
                )
                .await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::validate_duration;
    use crate::ltx::types::{LtxJob, OutputKind, Resolution};

    fn job(frames: u32, fps: u32) -> LtxJob {
        LtxJob {
            template_id: "ltx-t2v-hdr".to_string(),
            template_hash: "0x00".to_string(),
            prompt: "p".to_string(),
            seed: "1".to_string(),
            frames,
            fps,
            resolution: Resolution { w: 1280, h: 720 },
            lora: "ltx-iclora-hdr@v1".to_string(),
            output: OutputKind::ExrSequence,
            images: None,
            videos: None,
        }
    }

    #[test]
    fn test_validate_duration_guards_zero() {
        // The one validate_duration case the handler pipeline can't reach
        // (validate_bounds rejects a 0 frame count first); the accept /
        // divisibility / range matrix is exercised end-to-end in
        // tests/ltx_api/test_ws.rs::test_duration_{accepts,rejects}_matrix.
        assert!(validate_duration(&job(121, 0)).is_err(), "fps 0 guarded");
        assert!(validate_duration(&job(0, 24)).is_err(), "frames 0 guarded");
    }
}
