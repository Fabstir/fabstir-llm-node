// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! M1-economics tests for the payout path: the upload/submit split
//! (`upload_attestation`), and `finalize_clip` ordering with a mock
//! [`ProofSubmit`] (the §B same-variable rule, the first-proof/interval gate,
//! the "Too many" bounded retry, and failure-still-returns-proof_cid).

use anyhow::Result;
use ethers::types::H256;
use fabstir_llm_node::ltx::billing::LtxTracker;
use fabstir_llm_node::ltx::submit::{finalize_clip, upload_attestation, ProofSubmit};
use fabstir_llm_node::ltx::types::{Attestation, FrameManifest, Resolution};
use fabstir_llm_node::storage::s5_client::{MockS5Backend, S5Storage};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::sync::Mutex;

fn sample_attestation() -> Attestation {
    Attestation {
        model_id: "0x01".to_string(),
        template_hash: "0x02".to_string(),
        env_hash: "0x03".to_string(),
        input_commitment: "0x04".to_string(),
        output_cid: "uManifestCidPlaceholder".to_string(),
        manifest: FrameManifest {
            frame_count: 1,
            fps: 24,
            resolution: Resolution { w: 1280, h: 720 },
            colour_encoding: "linear-HDR-from-LogC3".to_string(),
            frame_hashes: vec!["0xaa".to_string()],
            merkle_root: "0xbb".to_string(),
        },
        session_id: "0x05".to_string(),
        host: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266".to_string(),
        timestamp: 1_790_000_000,
        signature: None,
    }
}

// -----------------------------------------------------------------------------
// E1 — upload half: caller-supplied path, SHA256 over the exact uploaded bytes
// -----------------------------------------------------------------------------

#[tokio::test]
async fn test_upload_attestation_returns_cid_and_sha256() {
    let s5 = MockS5Backend::new();
    let att = sample_attestation();
    let path = "home/ltx/job_42-promptxyz_attestation.json";

    let (proof_cid, proof_hash) = upload_attestation(&s5, path, &att).await.unwrap();

    // The hash is SHA256 over the EXACT stored bytes (dispute-time equality).
    let expected: [u8; 32] = Sha256::digest(att.stored_bytes()).into();
    assert_eq!(proof_hash, expected);
    assert!(!proof_cid.is_empty(), "upload must yield a CID");

    // The caller-supplied path is used verbatim (per-clip job_tag paths must not
    // collapse onto a shared job_id path).
    let stored = s5.get(path).await.unwrap();
    assert_eq!(stored, att.stored_bytes());
}

// -----------------------------------------------------------------------------
// E4 — finalize_clip with a mock ProofSubmit
// -----------------------------------------------------------------------------

/// Scripted mock: each call pops the next outcome (`Some(msg)` → that error,
/// `None`/exhausted → success) and records `(job_id, tokens, hash, cid)`.
struct MockSubmit {
    interval: u64,
    script: Mutex<VecDeque<Option<String>>>,
    calls: Mutex<Vec<(u64, u64, [u8; 32], String)>>,
}

impl MockSubmit {
    fn ok(interval: u64) -> Self {
        Self::scripted(interval, vec![])
    }
    fn scripted(interval: u64, script: Vec<Option<String>>) -> Self {
        Self {
            interval,
            script: Mutex::new(script.into()),
            calls: Mutex::new(Vec::new()),
        }
    }
    fn calls(&self) -> Vec<(u64, u64, [u8; 32], String)> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl ProofSubmit for MockSubmit {
    async fn submit_ltx_proof(
        &self,
        job_id: u64,
        tokens: u64,
        proof_hash: [u8; 32],
        proof_cid: String,
    ) -> Result<H256> {
        self.calls
            .lock()
            .unwrap()
            .push((job_id, tokens, proof_hash, proof_cid));
        match self.script.lock().unwrap().pop_front().flatten() {
            Some(msg) => Err(anyhow::anyhow!(msg)),
            None => Ok(H256::zero()),
        }
    }

    async fn session_proof_interval(&self, _job_id: u64) -> u64 {
        self.interval
    }
}

/// Marks a pending for job 42 (as the handler does at accept) and returns the
/// pieces every finalize test needs.
async fn pending_setup() -> (MockS5Backend, LtxTracker, Attestation) {
    let tracker = LtxTracker::new();
    tracker.mark_proof_pending(42).await;
    (MockS5Backend::new(), tracker, sample_attestation())
}

/// The tracker's pending count, read back through the public API: a leftover
/// pending is exactly the state in which a disconnect would defer.
async fn pending_unresolved(tracker: &LtxTracker, job_id: u64) -> bool {
    tracker.defer_completion(job_id).await
}

#[tokio::test]
async fn test_finalize_clip_success_upload_then_submit_same_tokens() {
    let (s5, tracker, att) = pending_setup().await;
    let mock = MockSubmit::ok(1000);
    let tokens = 9_831u64; // the §G worked example

    let (proof_cid, submitted) = finalize_clip(
        &s5,
        Some(&mock),
        &tracker,
        Some(42),
        true,
        "42-promptxyz",
        &att,
        tokens,
    )
    .await
    .unwrap();

    assert!(submitted);
    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    let (jid, claimed, hash, cid) = calls[0].clone();
    assert_eq!(jid, 42);
    // §B same-variable rule: tokensClaimed == the tokens finalize_clip was
    // handed (the ONE variable that also feeds ltx_complete/tracker).
    assert_eq!(claimed, tokens);
    // The on-chain hash/CID are the UPLOADED attestation's.
    let expected: [u8; 32] = Sha256::digest(att.stored_bytes()).into();
    assert_eq!(hash, expected);
    assert_eq!(cid, proof_cid);
    // The upload went to the per-clip job_tag path.
    assert!(s5
        .get("home/ltx/job_42-promptxyz_attestation.json")
        .await
        .is_ok());
    // Pending resolved as submitted.
    assert_eq!(tracker.proofs_submitted(42).await, 1);
    assert!(!pending_unresolved(&tracker, 42).await);
}

#[tokio::test]
async fn test_finalize_clip_first_proof_below_interval_skips_submit() {
    // Contract gotcha: the FIRST proof must be ≥ proofInterval (live 1000) or
    // it reverts "Low first" — don't burn gas on a doomed tx.
    let (s5, tracker, att) = pending_setup().await;
    let mock = MockSubmit::ok(1000);

    let (proof_cid, submitted) = finalize_clip(
        &s5,
        Some(&mock),
        &tracker,
        Some(42),
        true,
        "42-p",
        &att,
        394,
    )
    .await
    .unwrap();

    assert!(!submitted, "394 < proofInterval 1000 ⇒ submit skipped");
    assert!(mock.calls().is_empty(), "no doomed tx");
    assert!(!proof_cid.is_empty(), "clip delivery is preserved");
    assert_eq!(tracker.proofs_submitted(42).await, 0);
    assert!(
        !pending_unresolved(&tracker, 42).await,
        "skip still resolves the pending (forfeited)"
    );
}

#[tokio::test]
async fn test_finalize_clip_later_proof_below_interval_submits() {
    // Only proof 0 is gated by proofInterval; later clips only need the
    // MIN_PROVEN_TOKENS floor (100).
    let (s5, tracker, att) = pending_setup().await;
    // A first proof already landed on this session.
    tracker.mark_proof_pending(42).await;
    tracker.mark_proof_submitted(42).await;
    let mock = MockSubmit::ok(1000);

    let (_cid, submitted) = finalize_clip(
        &s5,
        Some(&mock),
        &tracker,
        Some(42),
        true,
        "42-p",
        &att,
        394,
    )
    .await
    .unwrap();

    assert!(submitted, "394 ≥ MIN_PROVEN_TOKENS and not the first proof");
    assert_eq!(mock.calls().len(), 1);
}

#[tokio::test]
async fn test_finalize_clip_submit_failure_still_returns_proof_cid() {
    // Policy: clip delivery ≥ revenue — a failed tx must NOT turn a rendered
    // clip into an ltx_error; the node forfeits and the client still gets it.
    let (s5, tracker, att) = pending_setup().await;
    let mock = MockSubmit::scripted(1000, vec![Some("execution reverted".into())]);

    let (proof_cid, submitted) = finalize_clip(
        &s5,
        Some(&mock),
        &tracker,
        Some(42),
        true,
        "42-p",
        &att,
        9_831,
    )
    .await
    .unwrap();

    assert!(!submitted);
    assert!(!proof_cid.is_empty());
    assert_eq!(
        mock.calls().len(),
        1,
        "a non-rate-limit error is NOT retried"
    );
    assert_eq!(tracker.proofs_submitted(42).await, 0);
    assert!(!pending_unresolved(&tracker, 42).await, "pending forfeited");
}

#[tokio::test(start_paused = true)]
async fn test_finalize_clip_too_many_retries_once_then_succeeds() {
    // Rate limit: tokensClaimed ≤ elapsed_secs × 2000 reverts "Too many"; the
    // node waits ≈ tokens/2000 (+5s buffer) and retries ONCE.
    let (s5, tracker, att) = pending_setup().await;
    let mock = MockSubmit::scripted(1000, vec![Some("revert: Too many".into()), None]);

    let (_cid, submitted) = finalize_clip(
        &s5,
        Some(&mock),
        &tracker,
        Some(42),
        true,
        "42-p",
        &att,
        111_514,
    )
    .await
    .unwrap();

    assert!(submitted, "retry after the rate-limit wait must land");
    assert_eq!(mock.calls().len(), 2);
    assert_eq!(tracker.proofs_submitted(42).await, 1);
    assert!(!pending_unresolved(&tracker, 42).await);
}

#[tokio::test(start_paused = true)]
async fn test_finalize_clip_too_many_twice_forfeits() {
    let (s5, tracker, att) = pending_setup().await;
    let mock = MockSubmit::scripted(
        1000,
        vec![
            Some("revert: Too many".into()),
            Some("revert: Too many".into()),
        ],
    );

    let (proof_cid, submitted) = finalize_clip(
        &s5,
        Some(&mock),
        &tracker,
        Some(42),
        true,
        "42-p",
        &att,
        111_514,
    )
    .await
    .unwrap();

    assert!(!submitted, "ONE retry only — then forfeit");
    assert!(!proof_cid.is_empty());
    assert_eq!(mock.calls().len(), 2);
    assert!(!pending_unresolved(&tracker, 42).await);
}

#[tokio::test]
async fn test_finalize_clip_without_job_id_uploads_only() {
    // job_id-less requests (no session) upload the attestation for the client
    // but have nothing to submit or settle.
    let (s5, tracker, att) = (
        MockS5Backend::new(),
        LtxTracker::new(),
        sample_attestation(),
    );
    let mock = MockSubmit::ok(1000);

    let (proof_cid, submitted) = finalize_clip(
        &s5,
        Some(&mock),
        &tracker,
        None,
        false,
        "unknown-p",
        &att,
        9_831,
    )
    .await
    .unwrap();

    assert!(!submitted);
    assert!(!proof_cid.is_empty());
    assert!(mock.calls().is_empty());
}

#[tokio::test]
async fn test_finalize_clip_upload_failure_forfeits_and_errors() {
    // No proof_cid exists at all ⇒ this is the one finalize failure that stays
    // an error (the spawn sends ltx_error) — but the pending is still resolved.
    let (s5, tracker, att) = pending_setup().await;
    s5.inject_error(
        fabstir_llm_node::storage::s5_client::StorageError::NetworkError("s5 down".into()),
    )
    .await;
    let mock = MockSubmit::ok(1000);

    let result = finalize_clip(
        &s5,
        Some(&mock),
        &tracker,
        Some(42),
        true,
        "42-p",
        &att,
        9_831,
    )
    .await;

    assert!(result.is_err());
    assert!(mock.calls().is_empty());
    assert!(
        !pending_unresolved(&tracker, 42).await,
        "even the error path resolves the pending"
    );
}
