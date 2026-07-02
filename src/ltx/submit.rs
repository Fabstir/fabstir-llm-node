// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! On-chain submit: upload the plaintext attestation to S5, then submit
//! `submitProofOfWork(jobId, tokens, proofHash, proofCID, "")` from the host
//! wallet. No contract change: `proofHash`/`proofCID` are opaque to the contract
//! and auth is `msg.sender == session.host` (v8.14.0; no on-chain signature).

use anyhow::Result;
use ethers::types::{Address, Bytes, H256, U256};
use sha2::{Digest, Sha256};

use crate::contracts::checkpoint_manager::encode_checkpoint_call;
use crate::contracts::client::Web3Client;
use crate::ltx::types::Attestation;
use crate::storage::s5_client::S5Storage;

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

/// Upload the plaintext attestation to S5, then submit its `proofHash`/`proofCID`
/// on-chain from the host wallet. The `proofHash` is SHA256 over the EXACT bytes
/// uploaded (same `Vec`), so the dispute-time `SHA256(fetched) == on-chain` check
/// passes. The node key never enters this path.
pub async fn submit_attestation(
    web3: &Web3Client,
    s5: &dyn S5Storage,
    job_marketplace: Address,
    job_id: u64,
    att: &Attestation,
    tokens: u64,
) -> Result<H256> {
    let bytes = att.stored_bytes();
    let proof_hash: [u8; 32] = Sha256::digest(&bytes).into();
    let proof_cid = s5
        .put(&format!("home/ltx/job_{job_id}_attestation.json"), bytes)
        .await
        .map_err(|e| anyhow::anyhow!("s5 attestation upload failed: {e}"))?;
    let calldata = submit_calldata(job_id, tokens, proof_hash, proof_cid);
    let (tx_hash, _) = web3
        .enqueue_transaction(
            job_marketplace,
            U256::zero(),
            Some(Bytes::from(calldata)),
            &format!("ltx proof job {job_id}"),
            true,
        )
        .await?;
    Ok(tx_hash)
}
