// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! On-chain submit: upload the plaintext attestation to S5, then submit
//! `submitProofOfWork(jobId, tokens, proofHash, proofCID, "")` from the host
//! wallet. No contract change: `proofHash`/`proofCID` are opaque to the contract
//! and auth is `msg.sender == session.host` (v8.14.0; no on-chain signature).

use anyhow::Result;
use ethers::types::{Address, Bytes, TransactionReceipt, H256, U256};
use sha2::{Digest, Sha256};
use tracing::{error, info, warn};

use crate::contracts::checkpoint_manager::encode_checkpoint_call;
use crate::contracts::client::Web3Client;
use crate::ltx::billing::{LtxTracker, MIN_PROVEN_TOKENS};
use crate::ltx::types::Attestation;
use crate::storage::s5_client::S5Storage;

/// On-chain proof submission seam for the LTX spawn. Implemented by
/// `CheckpointManager` (which keeps `Web3Client` encapsulated); mocked in tests
/// so `finalize_clip` is unit-testable without a chain.
#[async_trait::async_trait]
pub trait ProofSubmit: Send + Sync {
    /// Submit `submitProofOfWork(job_id, tokens, proof_hash, proof_cid, "")`
    /// from the host wallet. `Ok` means the proof LANDED: tx confirmed with
    /// `receipt.status == 1` — anything weaker (no receipt, revert) is `Err`.
    async fn submit_ltx_proof(
        &self,
        job_id: u64,
        tokens: u64,
        proof_hash: [u8; 32],
        proof_cid: String,
    ) -> Result<H256>;

    /// The session's `proofInterval` (contract minimum for the FIRST proof).
    async fn session_proof_interval(&self, job_id: u64) -> u64;
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
/// an error because no proof_cid exists at all. `tokens` must be the SAME
/// variable that feeds the wire `billing.tokens` (the §B triple equality).
pub async fn finalize_clip(
    s5: &dyn S5Storage,
    submitter: Option<&dyn ProofSubmit>,
    tracker: &LtxTracker,
    job_id: Option<u64>,
    pending_marked: bool,
    job_tag: &str,
    att: &Attestation,
    tokens: u64,
) -> Result<(String, bool)> {
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
        MIN_PROVEN_TOKENS.max(sub.session_proof_interval(jid).await)
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

    match sub
        .submit_ltx_proof(jid, tokens, proof_hash, proof_cid.clone())
        .await
    {
        Ok(tx) => {
            info!("LTX job {jid}: proof landed ({tokens} tokens, tx {tx:?})");
            tracker.mark_proof_submitted(jid).await;
            Ok((proof_cid, true))
        }
        Err(e) if e.to_string().contains("Too many") => {
            // Rate limit: tokensClaimed ≤ elapsed_secs × 2000. Bounded retry:
            // one wait of ≈ tokens/2000 (+5s). Holds the VRAM permit (~60s worst
            // case on a 110k-token clip) — accepted.
            let wait = tokens / 2000 + 5;
            warn!(
                "LTX job {jid}: rate-limited (\"Too many\"), retrying once in {wait}s \
                 (holding the generation slot)"
            );
            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            match sub
                .submit_ltx_proof(jid, tokens, proof_hash, proof_cid.clone())
                .await
            {
                Ok(tx) => {
                    info!("LTX job {jid}: proof landed on retry ({tokens} tokens, tx {tx:?})");
                    tracker.mark_proof_submitted(jid).await;
                    Ok((proof_cid, true))
                }
                Err(e2) => {
                    error!(
                        "LTX job {jid}: proof submit failed after retry — revenue forfeited: {e2}"
                    );
                    tracker.mark_proof_forfeited(jid).await;
                    Ok((proof_cid, false))
                }
            }
        }
        Err(e) => {
            error!("LTX job {jid}: proof submit failed — revenue forfeited: {e}");
            tracker.mark_proof_forfeited(jid).await;
            Ok((proof_cid, false))
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
