// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! On-chain submit: upload the plaintext attestation to S5, then submit
//! `submitProofOfWork(jobId, tokens, proofHash, proofCID, "")` from the host
//! wallet. No contract change: `proofHash`/`proofCID` are opaque to the contract
//! and auth is `msg.sender == session.host` (v8.14.0; no on-chain signature).

use anyhow::Result;
use ethers::types::{Address, Bytes, TransactionReceipt, H256, U256, U64};
use sha2::{Digest, Sha256};
use tracing::{error, info, warn};

use crate::contracts::checkpoint_manager::encode_checkpoint_call;
use crate::contracts::client::Web3Client;
use crate::ltx::billing::{LtxTracker, MIN_PROVEN_TOKENS};
use crate::ltx::final_proof::send_relight_final;
pub use crate::ltx::final_proof::SubmitPolicy;
use crate::ltx::types::Attestation;
use crate::storage::s5_client::S5Storage;

/// Batch D21: why a proof did not land, classified ONCE (inside
/// `CheckpointManager::submit_ltx_proof`, via [`classify_submit_outcome`]); no
/// caller reads error text afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProofSubmitError {
    /// The gas estimate failed "Too many" (the proof rate limit): nothing was
    /// broadcast.
    RateLimited,
    /// A mined status-0 receipt at `block` (it never carries a reason).
    Reverted { block: u64 },
    /// Broadcast, but no receipt within the queue's 60 s wait: it may still mine.
    Unconfirmed(H256),
    /// Anything else (not broadcast, or unclassifiable).
    Other(String),
}

impl std::fmt::Display for ProofSubmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProofSubmitError::RateLimited => write!(f, "rate-limited (\"Too many\")"),
            ProofSubmitError::Reverted { block } => {
                write!(f, "reverted (status 0) at block {block}")
            }
            ProofSubmitError::Unconfirmed(h) => write!(f, "unconfirmed (no receipt, tx {h:?})"),
            ProofSubmitError::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ProofSubmitError {}

/// Batch D21: a mined transaction's outcome (`ProofSubmit::receipt_of`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProofReceipt {
    /// `status == 1`.
    pub success: bool,
    pub block: u64,
}

/// A receipt as a [`ProofReceipt`]; `None` until it carries a block number.
pub fn proof_receipt(r: &TransactionReceipt) -> Option<ProofReceipt> {
    Some(ProofReceipt {
        success: r.status == Some(U64::from(1)),
        block: r.block_number?.as_u64(),
    })
}

/// Batch D21: the ONE place a proof submission's failure is classified. `sent`
/// is the tx queue's answer: `Err(text)` = nothing broadcast (the queue's
/// `Failed`), `Ok((hash, receipt))` = broadcast, with the receipt if one came
/// within the wait. `Ok` is STRICT: only a status-1 receipt.
pub fn classify_submit_outcome(
    sent: std::result::Result<(H256, Option<TransactionReceipt>), String>,
) -> std::result::Result<H256, ProofSubmitError> {
    match sent {
        Err(text) if text.contains("Too many") => Err(ProofSubmitError::RateLimited),
        Err(text) => Err(ProofSubmitError::Other(text)),
        Ok((hash, None)) => Err(ProofSubmitError::Unconfirmed(hash)),
        Ok((hash, Some(r))) => match proof_receipt(&r) {
            Some(ProofReceipt { success: true, .. }) => Ok(hash),
            Some(ProofReceipt { block, .. }) => Err(ProofSubmitError::Reverted { block }),
            None => Err(ProofSubmitError::Other(format!(
                "receipt for tx {hash:?} carries no block number"
            ))),
        },
    }
}

/// On-chain proof submission seam for the LTX spawn. Implemented by
/// `CheckpointManager` (which keeps `Web3Client` encapsulated); mocked in tests
/// so `finalize_clip` is unit-testable without a chain.
#[async_trait::async_trait]
pub trait ProofSubmit: Send + Sync {
    /// Submit `submitProofOfWork(job_id, tokens, proof_hash, proof_cid, "")`
    /// from the host wallet. `Ok` means the proof LANDED: tx confirmed with
    /// `receipt.status == 1` — anything weaker is a typed [`ProofSubmitError`].
    async fn submit_ltx_proof(
        &self,
        job_id: u64,
        tokens: u64,
        proof_hash: [u8; 32],
        proof_cid: String,
    ) -> std::result::Result<H256, ProofSubmitError>;

    /// The session's `proofInterval` (contract minimum for the FIRST proof).
    async fn session_proof_interval(&self, job_id: u64) -> u64;

    /// Batch D21: the receipt of an earlier proof transaction (`None` = not
    /// mined yet), for polling an `Unconfirmed` final without resending.
    async fn receipt_of(&self, tx: H256) -> std::result::Result<Option<ProofReceipt>, String>;
}

/// Megapixel-frame token count: `ceil(frames*w*h / 1000)`, computed in u128 so
/// large clips (720p×121 = 111,514,000 px) never overflow.
pub fn ltx_tokens(frames: u32, w: u32, h: u32) -> u64 {
    (((frames as u128) * (w as u128) * (h as u128) + 999) / 1000) as u64
}

/// Build `submitProofOfWork(jobId, tokensClaimed, proofHash, proofCID, "")`
/// calldata (5-param, v8.14.0; empty `deltaCID`). Reuses the audited encoder.
pub fn submit_calldata(
    job_id: u64,
    tokens: u64,
    proof_hash: [u8; 32],
    proof_cid: String,
) -> Vec<u8> {
    encode_checkpoint_call(job_id, tokens, proof_hash, proof_cid, String::new())
}

/// Upload the plaintext attestation to S5 at the CALLER-SUPPLIED path (the
/// spawn's per-clip `job_{job_tag}` path — two clips of one session must not
/// collide). Returns `(proof_cid, proof_hash)` where `proof_hash` is SHA256
/// over the EXACT bytes uploaded (same `Vec`), so the dispute-time
/// `SHA256(fetched) == on-chain` check passes.
pub async fn upload_attestation(
    s5: &dyn S5Storage,
    s5_path: &str,
    att: &Attestation,
) -> Result<(String, [u8; 32])> {
    let bytes = att.stored_bytes();
    let proof_hash: [u8; 32] = Sha256::digest(&bytes).into();
    let proof_cid = s5
        .put(s5_path, bytes)
        .await
        .map_err(|e| anyhow::anyhow!("s5 attestation upload failed: {e}"))?;
    Ok((proof_cid, proof_hash))
}

/// The per-clip payout sequence (M1 economics), extracted from the spawn so it
/// is unit-testable with a mock [`ProofSubmit`] + mock S5:
///
/// 1. upload the attestation to the per-clip `job_{job_tag}` S5 path;
/// 2. if a pending was marked for this clip: gate on the contract minimums
///    (FIRST proof ≥ `proofInterval`, else `MIN_PROVEN_TOKENS`) and submit
///    `submitProofOfWork` — on a "Too many" rate-limit revert, wait
///    ≈ `tokens/2000` (+5s buffer) and retry ONCE (this holds the VRAM permit;
///    accepted and logged);
/// 3. return `(proof_cid, submitted)` — the spawn ALWAYS sends `ltx_complete`
///    with the proof_cid (clip delivery ≥ revenue: a failed tx forfeits the
///    clip's revenue, it never errors a session whose artefact exists).
///
/// RESOLVES the pending it was handed on EVERY internal path (submitted or
/// forfeited) — including the upload-failure `Err`, the one failure that stays
/// an error because no proof_cid exists at all. `claim` is the ON-CHAIN claim:
/// the job's tokens for the LTX modes (then also the wire `billing.tokens`, the
/// §B triple equality); for a relight job after keep-alives, Batch D21's
/// remainder (the wire carries the cumulative total instead). `policy` picks the
/// send rules ([`SubmitPolicy`]).
#[allow(clippy::too_many_arguments)]
pub async fn finalize_clip(
    s5: &dyn S5Storage,
    submitter: Option<&dyn ProofSubmit>,
    tracker: &LtxTracker,
    job_id: Option<u64>,
    pending_marked: bool,
    job_tag: &str,
    att: &Attestation,
    claim: u64,
    policy: SubmitPolicy<'_>,
) -> Result<(String, bool)> {
    let tokens = claim;
    let s5_path = format!("home/ltx/job_{job_tag}_attestation.json");
    let (proof_cid, proof_hash) = match upload_attestation(s5, &s5_path, att).await {
        Ok(pair) => pair,
        Err(e) => {
            if pending_marked {
                if let Some(jid) = job_id {
                    tracker.mark_proof_forfeited(jid).await;
                }
            }
            return Err(e);
        }
    };

    let (jid, sub) = match (pending_marked, job_id, submitter) {
        (true, Some(jid), Some(sub)) => (jid, sub),
        (true, Some(jid), None) => {
            // Defensive: pending implies a cm existed at accept; if the
            // submitter is gone the pending must still resolve.
            error!("LTX job {jid}: no submitter for a marked pending — forfeiting");
            tracker.mark_proof_forfeited(jid).await;
            return Ok((proof_cid, false));
        }
        _ => return Ok((proof_cid, false)), // job_id-less request: upload only
    };

    // Contract minimums: only proof 0 is gated by proofInterval ("Low first");
    // every proof needs the MIN_PROVEN_TOKENS floor. Skip a doomed tx.
    let first_proof = tracker.proofs_submitted(jid).await == 0;
    let min_required = if first_proof {
        let interval = match &policy {
            // Batch D21 round 2: the relight final uses the admission snapshot's
            // w10 and never reads the chain here.
            SubmitPolicy::Relight(rf) => rf.proof_interval,
            // Pre-existing (other LTX modes): an UNBOUNDED read — its
            // `query_session_jobs_raw` has no timeout. Left as it was.
            SubmitPolicy::Ltx => sub.session_proof_interval(jid).await,
        };
        MIN_PROVEN_TOKENS.max(interval)
    } else {
        MIN_PROVEN_TOKENS
    };
    if tokens < min_required {
        warn!(
            "LTX job {jid}: {tokens} tokens < required {min_required} \
             (first_proof={first_proof}) — skipping submit, revenue forfeited"
        );
        tracker.mark_proof_forfeited(jid).await;
        return Ok((proof_cid, false));
    }

    // Batch D21: a relight final after keep-alives has its own send rules.
    if let SubmitPolicy::Relight(rf) = &policy {
        let landed = send_relight_final(sub, jid, tokens, proof_hash, proof_cid.clone(), rf).await;
        if landed {
            tracker.mark_proof_submitted(jid).await;
        } else {
            tracker.mark_proof_forfeited(jid).await;
        }
        return Ok((proof_cid, landed));
    }

    // Submit with a bounded rate-limit retry loop. "Too many" means
    // tokensClaimed > elapsed×2000 since lastProofTime — and lastProofTime
    // MOVES if a sibling clip's proof lands during our sleep
    // (MAX_CONCURRENT_GENERATIONS > 1), so one fixed wait is not always
    // enough. Recompute-and-retry, at most MAX_SUBMIT_ATTEMPTS; the sleeps
    // hold the VRAM permit (worst ≈2min on a 110k-token clip) — accepted and
    // logged. Non-rate-limit errors never retry (a mined status-0 revert
    // carries no reason string, so it lands here too — conservative forfeit).
    const MAX_SUBMIT_ATTEMPTS: u32 = 3;
    let mut attempt = 1u32;
    loop {
        match sub
            .submit_ltx_proof(jid, tokens, proof_hash, proof_cid.clone())
            .await
        {
            Ok(tx) => {
                info!(
                    "LTX job {jid}: proof landed ({tokens} tokens, tx {tx:?}, \
                     attempt {attempt})"
                );
                tracker.mark_proof_submitted(jid).await;
                return Ok((proof_cid, true));
            }
            Err(ProofSubmitError::RateLimited) if attempt < MAX_SUBMIT_ATTEMPTS => {
                let wait = tokens / 2000 + 5;
                warn!(
                    "LTX job {jid}: rate-limited (\"Too many\"), attempt \
                     {attempt}/{MAX_SUBMIT_ATTEMPTS} — retrying in {wait}s \
                     (holding the generation slot)"
                );
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                attempt += 1;
            }
            Err(e) => {
                error!(
                    "LTX job {jid}: proof submit failed (attempt {attempt}) — \
                     revenue forfeited: {e}"
                );
                tracker.mark_proof_forfeited(jid).await;
                return Ok((proof_cid, false));
            }
        }
    }
}

/// Submit `proofHash`/`proofCID` on-chain from the host wallet via the tx
/// queue (wait-for-confirmation). Returns the receipt UNINSPECTED — the queue's
/// `Success` does not check `receipt.status`, so the caller (the `ProofSubmit`
/// impl) must gate success on receipt present + `status == 1`. The node key
/// never enters this path.
pub async fn submit_proof(
    web3: &Web3Client,
    job_marketplace: Address,
    job_id: u64,
    tokens: u64,
    proof_hash: [u8; 32],
    proof_cid: String,
) -> Result<(H256, Option<TransactionReceipt>)> {
    let calldata = submit_calldata(job_id, tokens, proof_hash, proof_cid);
    web3.enqueue_transaction(
        job_marketplace,
        U256::zero(),
        Some(Bytes::from(calldata)),
        &format!("ltx proof job {job_id}"),
        true,
    )
    .await
}

/// VFX Passes D3: total attempts per relight output frame (one 90 s S5 hiccup must not kill a 25 min job).
pub const RELIGHT_UPLOAD_ATTEMPTS: u32 = 3;

/// VFX Passes D3: encrypt + upload one frame, retried up to `attempts` in total (each attempt draws a fresh frame key
/// inside `encrypt_bytes_and_upload`, so a retry never reuses a key). Returns `(capability, frame hash)`.
pub async fn upload_frame_with_retry(
    s5: &dyn S5Storage,
    dest: &str,
    bytes: Vec<u8>,
    attempts: u32,
) -> Result<(String, String)> {
    let attempts = attempts.max(1);
    let mut last = None;
    let mut bytes = Some(bytes);
    for attempt in 1..=attempts {
        // clone only while a retry may still need the plaintext; the last attempt moves it
        let plaintext = if attempt == attempts {
            bytes.take().expect("present until the last attempt")
        } else {
            bytes
                .as_ref()
                .expect("present until the last attempt")
                .clone()
        };
        match crate::ltx::exr::encrypt_bytes_and_upload(plaintext, s5, dest).await {
            Ok(pair) => return Ok(pair),
            Err(e) => {
                warn!("frame upload {dest} failed (attempt {attempt}/{attempts}): {e:#}");
                last = Some(e);
            }
        }
    }
    Err(last.expect("at least one attempt"))
}
