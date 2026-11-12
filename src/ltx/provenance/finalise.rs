// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! `finalise` (D2, D3, D8c, D9, D9a): runs between fetching an output from ComfyUI and uploading it, so
//! `outputCID`, the Merkle root and `proofHash` bind the MARKED bytes. Any failure is an error: the caller
//! sends `ltx_error`, uploads nothing and submits no proof (D10: fail closed before billing).

use super::ffmpeg::Ffmpeg;
use super::signer_client::{with_retry, ProvenanceSigner, SignerError};
use anyhow::{anyhow, bail, Context as _, Result};
use base64::Engine;
use fabstir_provenance::c2pa::Context;
use fabstir_provenance::contract::{ErrorCode, ReserveResponse, SignRequest, SignSequenceRequest};
use fabstir_provenance::exr_record::{needs_pad, ExrRecord};
use fabstir_provenance::flow::{self, ReserveOnly};
use fabstir_provenance::{exr_header, fingerprint, frames_index, leak_guard, Class, OutputRole};
use sha2::{Digest, Sha256};
use std::time::Instant;

/// One job's provenance state: the reservation (issued at accept) and what the delivery loop learns.
pub struct ProvenanceCtx<'a> {
    pub reservation: ReserveResponse,
    pub ingredient_formats: Vec<&'static str>,
    /// The job's class-table row (the signer issued the reservation from the same table).
    pub row: &'static fabstir_provenance::TemplateClass,
    pub signer: &'a dyn ProvenanceSigner,
    pub ffmpeg: &'a Ffmpeg,
    pub reserve: usize,
    pub deadline: Instant,
    /// RFC 3339 seconds, the delivery time written into EXR header records.
    pub time: String,
    pub resolver_url: String,
    /// SHA-256 of every marked EXR frame, in delivery order (for the sequence manifest).
    pub exr_hashes: Vec<String>,
    /// The reservation holds ONE mp4 slot.
    pub mp4_done: bool,
    /// Set by `finalise_sequence` once the signer returned the sequence manifest.
    pub sequence_signed: bool,
}

/// The latest instant a signing call may still be retried: the reservation's expiry less a margin, never later
/// than `job_deadline` (relight's whole-job deadline). Each call then retries within min(this, RETRY_WINDOW).
pub fn signing_deadline(expires_at_unix: u64, job_deadline: Option<Instant>) -> Instant {
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // a signer-supplied expiry: never more than the contract's longest reservation (no Instant overflow)
    let longest = fabstir_provenance::contract::MAX_DEADLINE_SECS
        + fabstir_provenance::contract::RESERVATION_GRACE_SECS;
    let left = expires_at_unix
        .saturating_sub(now_unix)
        .saturating_sub(120)
        .min(longest);
    let d = Instant::now() + std::time::Duration::from_secs(left);
    job_deadline.map_or(d, |j| d.min(j))
}

/// Before the proof is built: the mp4 was marked, and a reserved sequence was signed (no delivery whose
/// header records point at a sequence manifest that does not exist).
pub fn check_complete(ctx: &ProvenanceCtx<'_>) -> Result<()> {
    if !ctx.mp4_done {
        bail!("no mp4 output was marked");
    }
    if ctx.reservation.sequence.is_some() && !ctx.sequence_signed {
        bail!("the reserved sequence manifest was not signed");
    }
    Ok(())
}

/// Route one delivered output by its file name: the mp4 (one per job), EXR frames, and nothing else (an output
/// this node cannot mark is never delivered).
pub async fn finalise_output(
    ctx: &mut ProvenanceCtx<'_>,
    filename: &str,
    bytes: Vec<u8>,
) -> Result<Vec<u8>> {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".mp4") {
        if ctx.mp4_done {
            bail!("a second mp4 output has no reserved manifest slot");
        }
        ctx.mp4_done = true;
        finalise_mp4(ctx, bytes).await
    } else if lower.ends_with(".exr") {
        finalise_exr(ctx, &bytes)
    } else {
        bail!("output {filename} is of a type provenance cannot mark")
    }
}

/// The `mdat` boxes of an mp4 (the media payload; must be byte-identical before and after marking).
pub fn mdat_bytes(b: &[u8]) -> Vec<&[u8]> {
    flow::top_boxes(b)
        .into_iter()
        .filter(|(t, _, _)| t == "mdat")
        .map(|(_, o, s)| &b[o..o + s])
        .collect()
}

fn node_context(reserve: usize) -> Result<Context> {
    Ok(Context::new()
        .with_settings(flow::settings(None, false))?
        .with_signer(Box::new(ReserveOnly(reserve))))
}

/// Mark the job's mp4 (`frames[0]` for EXR jobs). `bytes` = the file exactly as ComfyUI wrote it.
pub async fn finalise_mp4(ctx: &mut ProvenanceCtx<'_>, bytes: Vec<u8>) -> Result<Vec<u8>> {
    leak_guard::check_mp4(&bytes).map_err(|e| anyhow!("leak guard: {e}"))?;
    let p = ctx.reservation.params.clone();
    // the fingerprint of the picture as generated: embedding changes no mdat byte, so it is also the
    // fingerprint of the delivered file (AP0.2 evidence-item5; test fingerprint_is_of_delivered_bytes)
    let packed = match ctx.row.mp4_role {
        OutputRole::Picture => Some(
            ctx.ffmpeg
                .fingerprint(&bytes)
                .await
                .context("fingerprint")?
                .pack()?,
        ),
        OutputRole::Data => None,
    };
    if p.soft_binding_alg.is_some() != packed.is_some() {
        bail!("reservation and output role disagree on the fingerprint");
    }
    let digest = packed.as_deref().map(fingerprint::digest);
    let mut reserve = ctx.reserve;
    let mut redone_for_limits = false;
    loop {
        // D9a: never land on an exact 256 KiB multiple (exr::encrypt_frame refuses those). Each step adds 8 bytes
        // of COSE padding, so the second try always clears it; the cap only guards a broken invariant.
        // c2pa hashing of the whole clip is CPU work: off the async workers.
        let (h, prep) = {
            let (p2, d2, f2, b2) = (
                p.clone(),
                digest,
                ctx.ingredient_formats.clone(),
                bytes.clone(),
            );
            let start = reserve;
            let (h, prep, used) = tokio::task::spawn_blocking(move || -> Result<_> {
                let mut r = start;
                for _ in 0..4 {
                    let (h, prep) =
                        flow::prepare_mp4(node_context(r)?, &p2, d2.as_ref(), &f2, &b2, r)?;
                    if !needs_pad(flow::final_len(&prep)) {
                        return Ok((h, prep, r));
                    }
                    r += 8;
                }
                bail!("the placeholder kept landing on a 256 KiB multiple")
            })
            .await
            .map_err(|e| anyhow!("prepare task: {e}"))??;
            reserve = used;
            (h, prep)
        };
        let b64 = base64::engine::general_purpose::STANDARD;
        let req = SignRequest {
            reservation_id: ctx.reservation.reservation_id.clone(),
            slot: "mp4".into(),
            reserve_size: h.reserve,
            placeholder_len: h.placeholder_len,
            bmff_hash_sha256_hex: hex::encode(&h.bmff_hash),
            builder_archive_b64: b64.encode(&h.archive),
            fingerprint_b64: packed.as_ref().map(|f| b64.encode(f)),
        };
        let signer = ctx.signer;
        match with_retry(ctx.deadline, || signer.sign(&req)).await {
            Ok(manifest) => {
                let out = flow::patch(&prep, &manifest)?;
                if mdat_bytes(&out) != mdat_bytes(&bytes) {
                    bail!("marking changed the media payload");
                }
                // reading the whole clip back is CPU work: off the async workers
                let (o2, u2) = (out, p.manifest_uuid.clone());
                let out =
                    tokio::task::spawn_blocking(move || verify_delivered(&o2, &u2).map(|_| o2))
                        .await
                        .map_err(|e| anyhow!("verify task: {e}"))??;
                return Ok(out);
            }
            // contract 4/8: redo the placeholder ONCE with the signer's current limits
            Err(SignerError {
                code: Some(ErrorCode::ReserveTooSmall | ErrorCode::ReserveOutOfRange),
                limits: Some(l),
                ..
            }) if !redone_for_limits => {
                redone_for_limits = true;
                reserve = super::clamp_reserve(l.reserve_recommended.max(l.reserve_min));
            }
            Err(e) => return Err(anyhow!("signing failed: {e}")),
        }
    }
}

/// Before delivery, read the marked file back offline: the active manifest is the reserved one and the hard
/// binding matches these exact bytes (a signer fault returning a well-sized but wrong manifest is caught here,
/// not by the public detector). Trust is not judged here: the node holds no trust list.
pub fn verify_delivered(out: &[u8], manifest_uuid: &str) -> Result<()> {
    let c = Context::new().with_settings(flow::settings(None, false))?;
    let r = fabstir_provenance::c2pa::Reader::from_context(c)
        .with_stream(flow::FMT_MP4, std::io::Cursor::new(out.to_vec()))?;
    if r.active_label() != Some(format!("urn:c2pa:{manifest_uuid}").as_str()) {
        bail!("the signed manifest is not the reserved one");
    }
    let vr = r
        .validation_results()
        .map(|v| serde_json::to_value(v).unwrap_or_default())
        .unwrap_or_default();
    let codes = |k: &str| -> Vec<String> {
        vr["activeManifest"][k]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|c| c["code"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    if !codes("success")
        .iter()
        .any(|c| c == "assertion.bmffHash.match")
    {
        bail!("the hard binding does not match the delivered bytes");
    }
    // without a trust list the signing credential reads untrusted; anything else failing is a fault
    let bad: Vec<String> = codes("failure")
        .into_iter()
        .filter(|c| c != "signingCredential.untrusted")
        .collect();
    if !bad.is_empty() {
        bail!("the delivered manifest does not validate: {bad:?}");
    }
    Ok(())
}

/// Mark one EXR frame with its header record (D9) and remember its final hash for the sequence index.
pub fn finalise_exr(ctx: &mut ProvenanceCtx<'_>, bytes: &[u8]) -> Result<Vec<u8>> {
    let seq = ctx
        .reservation
        .sequence
        .as_ref()
        .ok_or_else(|| anyhow!("EXR output without a sequence reservation"))?;
    let rec = ExrRecord {
        class: sequence_class(ctx),
        node_version: ctx.reservation.params.node_version.clone(),
        time: ctx.time.clone(),
        manifest_uuid: ctx.reservation.params.manifest_uuid.clone(),
        sequence_id: seq.id.clone(),
        sequence_manifest_uuid: seq.manifest_uuid.clone(),
        resolver_url: ctx.resolver_url.clone(),
    };
    let out = exr_header::mark(bytes, &|pad| rec.to_bytes(pad))?;
    ctx.exr_hashes.push(hex::encode(Sha256::digest(&out)));
    Ok(out)
}

/// The class the frames index states: the sequence manifest's class (an alpha matte is data, D1).
fn sequence_class(ctx: &ProvenanceCtx<'_>) -> Class {
    match ctx.row.exr_role {
        Some(OutputRole::Data) => Class::Data,
        _ => ctx.reservation.params.class,
    }
}

/// After the last frame: sign the standalone sequence manifest over the frames index (D9).
pub async fn finalise_sequence(ctx: &mut ProvenanceCtx<'_>) -> Result<()> {
    if ctx.exr_hashes.is_empty() {
        if ctx.reservation.sequence.is_some() {
            bail!("a sequence was reserved but no EXR frame was delivered");
        }
        return Ok(());
    }
    let pairs: Vec<(String, String)> = ctx
        .exr_hashes
        .iter()
        .enumerate()
        .map(|(i, h)| (frames_index::frame_name(i + 1), h.clone()))
        .collect();
    let index = frames_index::build_from_hashes(sequence_class(ctx), &pairs)?;
    let req = SignSequenceRequest {
        reservation_id: ctx.reservation.reservation_id.clone(),
        slot: "sequence".into(),
        frames_index_b64: base64::engine::general_purpose::STANDARD.encode(&index),
    };
    let signer = ctx.signer;
    let manifest = with_retry(ctx.deadline, || signer.sign_sequence(&req))
        .await
        .map_err(|e| anyhow!("sequence signing failed: {e}"))?;
    if manifest.is_empty() {
        bail!("the signer returned an empty sequence manifest");
    }
    ctx.sequence_signed = true;
    Ok(())
}
