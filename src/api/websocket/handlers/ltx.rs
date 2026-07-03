// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 9: encrypted WebSocket handler for LTX 2.3 generation (`ltx_generate`).
//!
//! Mirrors `transcode.rs`: a two-phase reply (an immediate encrypted `ltx_accepted`
//! ack plus a background [`LtxGenerateTask`] that streams encrypted progress and a
//! terminal `ltx_complete`/`ltx_error`). The background task reuses the SAME S5
//! access the transcode spawn uses (`CheckpointManager::get_s5_storage`); the
//! on-chain submit is TODO'd (GPU-acceptance) because `Web3Client` is not exposed
//! from `CheckpointManager`, exactly as the transcode spawn defers chain submit.

use crate::api::server::ApiServer;
use crate::ltx::attestation::EnvMeta;
use crate::ltx::client::Progress;
use crate::ltx::template::Bounds;
use crate::ltx::types::{FrameManifest, LtxJob};
use crate::ltx::{attestation, exr, patcher, submit, ComfyClient};
use ethers::types::U256;
use rand::RngCore;
use serde_json::{json, Value};
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

    // Input-image validation (M1a), fail-closed BEFORE a slot is spent. The
    // template's `imageInputs` is the commitment format selector: the job MUST
    // carry exactly that many images (0 for t2v). Each image's size is checked
    // from the capability CID's own length field — parse only, NO network fetch —
    // so an oversize image is rejected without a wasted portal GET.
    let image_inputs = store.image_inputs(&job.template_id).unwrap_or(0);
    let images = job.images.clone().unwrap_or_default();
    if images.len() != image_inputs as usize {
        return reject(
            "VALIDATION_FAILED",
            &format!(
                "template {:?} expects {} input image(s), got {}",
                job.template_id,
                image_inputs,
                images.len()
            ),
        );
    }
    let image_max_bytes = store.bundle().bounds.image_max_bytes;
    // Fail closed if a template accepts images but advertises no size bound —
    // `imageMaxBytes` unset (0) must NOT mean "unlimited" (that would re-open an
    // unbounded fetch on a client-influenced size). A correctly configured image
    // template always sets it.
    if !images.is_empty() && image_max_bytes == 0 {
        return reject(
            "VALIDATION_FAILED",
            "template accepts images but no imageMaxBytes bound is configured",
        );
    }
    for cid in &images {
        let env = match crate::ltx::input_image::parse_capability_cid(cid) {
            Ok(e) => e,
            Err(e) => {
                return reject(
                    "VALIDATION_FAILED",
                    &format!("invalid image capability CID: {e}"),
                )
            }
        };
        if env.plaintext_len as u64 > image_max_bytes {
            return reject(
                "VALIDATION_FAILED",
                &format!(
                    "input image is {} bytes, exceeds imageMaxBytes {}",
                    env.plaintext_len, image_max_bytes
                ),
            );
        }
    }

    // Patch scalar params into the pinned graph (substitution only). Image inputs
    // (LoadImage) are patched post-accept in the spawn, once the images are
    // fetched from S5 and uploaded to ComfyUI (they need the stored filenames).
    let patched_graph = match patcher::patch(&graph, &job, &[]) {
        Ok(g) => g,
        Err(e) => return reject("VALIDATION_FAILED", &format!("patch failed: {e}")),
    };

    // VRAM admission — hold the owned permit for the job's lifetime.
    let permit = match server.ltx_semaphore().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => return reject("CAPACITY", "all generation slots are in use"),
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

/// Resolve an image-conditioned job's inputs (M1a). For each ordered capability
/// CID: fetch + integrity-check + decrypt it from the S5 portal, upload the
/// plaintext to ComfyUI under a content-addressed name (`keccak256(plaintext)` ⇒
/// stable under `overwrite`), and record `keccak256(plaintext)` into
/// `image_hashes` (the v2 commitment input). Finally patch the `LoadImage` nodes
/// with the stored names. A t2v job (no `images`) returns the graph untouched and
/// leaves `image_hashes` empty. Pre-accept validation has already checked the
/// image count and per-image size, so any failure here is post-accept
/// (`GENERATION_FAILED`).
async fn prepare_input_images(
    client: &ComfyClient,
    job: &LtxJob,
    graph: crate::ltx::Graph,
    image_hashes: &mut Vec<[u8; 32]>,
) -> Result<crate::ltx::Graph, String> {
    let images = match &job.images {
        Some(imgs) if !imgs.is_empty() => imgs,
        _ => return Ok(graph),
    };
    let blob_source = s5_blob_source_url();
    let mut stored_names = Vec::with_capacity(images.len());
    for cid in images {
        let (hash, plaintext) = crate::ltx::input_image::fetch_image_hash(&blob_source, cid)
            .await
            .map_err(|e| format!("input image fetch failed: {e}"))?;
        let filename = format!("{}.png", hex::encode(hash));
        let name = client
            .upload_image(&filename, plaintext)
            .await
            .map_err(|e| format!("input image upload failed: {e}"))?;
        stored_names.push(name);
        image_hashes.push(hash);
    }
    patcher::patch(&graph, job, &stored_names).map_err(|e| format!("image patch failed: {e}"))
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
        let LtxGenerateTask {
            job,
            patched_graph,
            request_id,
            allow_list_version: _,
            timeout_secs,
            job_id,
            permit,
        } = self;
        tokio::spawn(async move {
            let _permit = permit; // hold the VRAM slot for the whole job
            let rid = request_id.as_deref();
            let (key, sid) = (&session_key, session_id.as_str());

            // Image-conditioned templates (M1a): fetch each input image from S5,
            // upload it to ComfyUI, patch the LoadImage nodes, and collect the
            // per-image `keccak256(plaintext)` for the v2 commitment. Empty for t2v.
            let mut image_hashes: Vec<[u8; 32]> = Vec::new();
            let patched_graph =
                match prepare_input_images(&client, &job, patched_graph, &mut image_hashes).await {
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

            // 2. Watch + stream generating progress.
            let (watch_tx, mut watch_rx) = mpsc::channel::<Progress>(64);
            let (wc, pid) = (client.clone(), prompt_id.clone());
            let watch_handle =
                tokio::spawn(async move { wc.watch(&pid, watch_tx, timeout_secs).await });
            while let Some(p) = watch_rx.recv().await {
                if let Progress::Progress { value, max } = p {
                    let pct = if max > 0 {
                        ((value as u64 * 100) / max as u64) as u32
                    } else {
                        0
                    };
                    if !send_stage(&progress_tx, "generating", pct, key, sid, rid).await {
                        break;
                    }
                }
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

            let _ = send_stage(&progress_tx, "encrypting", 0, key, sid, rid).await;
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

            let _ = send_stage(&progress_tx, "uploading", 0, key, sid, rid).await;
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

            let _ = send_stage(&progress_tx, "finalising", 0, key, sid, rid).await;
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
            // proofCID = S5-stored attestation (same S5 access transcode uses).
            // TODO(GPU-acceptance): submit::submit_attestation on-chain — Web3Client is
            // NOT exposed by CheckpointManager, so the spawn does S5 only, exactly like
            // the transcode spawn (which S5-puts its Merkle tree and defers chain submit).
            let proof_cid = match s5
                .put(
                    &format!("home/ltx/job_{job_tag}_attestation.json"),
                    att.stored_bytes(),
                )
                .await
            {
                Ok(c) => c,
                Err(e) => {
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

            let tokens = submit::ltx_tokens(job.frames, job.resolution.w, job.resolution.h);
            let price = std::env::var("LTX_PRICE_PER_TOKEN").unwrap_or_else(|_| "0".to_string());
            if let Some(jid) = job_id {
                let ppt = U256::from_dec_str(&price).unwrap_or_default();
                let cost = U256::from(tokens).checked_mul(ppt).unwrap_or(U256::MAX);
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
        });
    }
}
