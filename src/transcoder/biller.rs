// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The transcode billing loop (SCI-C D20): proves the work in steps while the
//! sidecar runs, watches the session's on-chain status, and makes the final
//! claim once the job is done. Every proof is awaited to a status-1 receipt.
//! The chain sits behind [`TranscodeChain`] so the loop is testable without one.

use std::time::Duration;

use anyhow::Result;
use ethers::types::H256;
use sha2::{Digest, Sha256};

use super::proof_steps::{
    final_step, interim_amount, interim_due, session_view, steps_can_carry, FinalStep, SessionView,
};
use crate::training::accept::SessionStatus;

/// How often the loop re-reads the session while the sidecar runs (D20(d)).
pub const STATUS_CHECK_EVERY_SECS: u64 = 20;
/// How long, in wall time, the final claim waits for the rate cap to allow it.
pub const FINAL_RATE_WAIT_CAP_SECS: u64 = 120;
/// Transient failures the final claim tolerates before giving up.
pub const FINAL_ATTEMPTS: u32 = 3;
/// Pause between final-claim retries.
pub const FINAL_RETRY_PAUSE_SECS: u64 = 5;

/// What the billing loop needs from the chain and storage.
#[async_trait::async_trait]
pub trait TranscodeChain: Send + Sync {
    /// Raw `sessionJobs(job_id)` return.
    async fn session_raw(&self, job_id: u64) -> Result<Vec<u8>>;
    /// Latest block timestamp (chain time, never the node's clock).
    async fn chain_now(&self) -> Result<u64>;
    /// The session's per-model rate cap in tokens per second.
    async fn rate_limit(&self, job_id: u64) -> Result<u64>;
    /// Store an attestation; returns its CID.
    async fn put_attestation(&self, path: &str, bytes: Vec<u8>) -> Result<String>;
    /// `submitProofOfWork`, `Ok` only on a confirmed status-1 receipt.
    async fn submit_proof(
        &self,
        job_id: u64,
        tokens: u64,
        proof_hash: [u8; 32],
        proof_cid: String,
    ) -> Result<H256>;
}

/// The loop's verdict on a progress report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollOutcome {
    Continue,
    /// The session can no longer be billed: stop the sidecar and bill nothing more.
    Stop(String),
}

/// The final claim's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishOutcome {
    /// Every claimable token is proven on chain; release the result.
    Paid { proven: u64 },
    /// The final claim did not land; withhold the result (D20(c)).
    Unpaid(String),
}

pub struct TranscodeBiller {
    job_id: u64,
    task_id: String,
    proven: u64,
    step: u32,
    last_proved_progress: i32,
    last_proof_at: u64,
    last_status_check: u64,
    estimate_checked: bool,
    /// `tokensUsed` on chain when this job was first read: what is proven for
    /// this transcode is read back from the chain as `tokensUsed - base`, so a
    /// proof that mined after its receipt wait ran out is never claimed twice.
    base_tokens_used: Option<u64>,
    /// The lowest estimate seen, so a changing estimate never over-claims.
    lowest_estimate: Option<u64>,
}

impl TranscodeBiller {
    pub fn new(job_id: u64, task_id: String) -> Self {
        Self {
            job_id,
            task_id,
            proven: 0,
            step: 0,
            last_proved_progress: 0,
            last_proof_at: 0,
            last_status_check: 0,
            estimate_checked: false,
            base_tokens_used: None,
            lowest_estimate: None,
        }
    }

    pub fn proven(&self) -> u64 {
        self.proven
    }

    pub fn job_id(&self) -> u64 {
        self.job_id
    }

    /// Read the session and chain time, and bring `proven` up to what the chain
    /// shows for this transcode.
    async fn read(&mut self, chain: &dyn TranscodeChain) -> Result<(SessionView, u64)> {
        let raw = chain.session_raw(self.job_id).await?;
        let view = session_view(&raw, self.job_id).map_err(anyhow::Error::msg)?;
        let now = chain.chain_now().await?;
        let base = *self.base_tokens_used.get_or_insert(view.tokens_used);
        let on_chain = view.tokens_used.saturating_sub(base);
        if on_chain > self.proven {
            tracing::info!(
                "[SCI] job {}: chain shows {} tokens proven for this transcode (recorded {}); adopting the chain's figure",
                self.job_id,
                on_chain,
                self.proven
            );
            self.proven = on_chain;
        }
        Ok((view, now))
    }

    async fn prove(
        &mut self,
        chain: &dyn TranscodeChain,
        tokens: u64,
        progress: i32,
        now: u64,
    ) -> Result<()> {
        self.step += 1;
        let attestation = serde_json::json!({
            "kind": "transcode-step",
            "jobId": self.job_id,
            "taskId": self.task_id,
            "step": self.step,
            "progress": progress,
            "tokens": tokens,
            "cumulative": self.proven + tokens,
            "chainTime": now,
        });
        let bytes = serde_json::to_vec(&attestation)?;
        let proof_hash: [u8; 32] = Sha256::digest(&bytes).into();
        let path = format!(
            "home/transcode/attest/job_{}_{}_{}.json",
            self.job_id, self.task_id, self.step
        );
        let cid = chain.put_attestation(&path, bytes).await?;
        chain
            .submit_proof(self.job_id, tokens, proof_hash, cid)
            .await?;
        self.proven += tokens;
        self.last_proved_progress = progress;
        self.last_proof_at = now;
        tracing::info!(
            "[SCI] job {}: transcode step {} proved {} tokens at {}% ({} total)",
            self.job_id,
            self.step,
            tokens,
            progress,
            self.proven
        );
        Ok(())
    }

    /// Called on each progress report. `estimate` is the whole job's tokens,
    /// once the sidecar has reported the source duration.
    pub async fn on_progress(
        &mut self,
        chain: &dyn TranscodeChain,
        progress: i32,
        estimate: Option<u64>,
        wall_now: u64,
    ) -> PollOutcome {
        let due_status = wall_now.saturating_sub(self.last_status_check) >= STATUS_CHECK_EVERY_SECS;
        let due_proof = estimate.is_some()
            && interim_due(
                progress,
                self.last_proved_progress,
                wall_now,
                if self.last_proof_at == 0 {
                    wall_now
                } else {
                    self.last_proof_at
                },
                u64::MAX,
            );
        let first_estimate = estimate.is_some() && !self.estimate_checked;
        if !due_status && !due_proof && !first_estimate {
            return PollOutcome::Continue;
        }
        self.last_status_check = wall_now;
        let (view, now) = match self.read(chain).await {
            Ok(read) => read,
            Err(e) => {
                tracing::warn!(
                    "[SCI] job {}: session read failed mid-transcode: {}",
                    self.job_id,
                    e
                );
                return PollOutcome::Continue;
            }
        };
        if view.status != SessionStatus::Active {
            return PollOutcome::Stop(format!("session is {:?}", view.status));
        }
        let Some(estimate) = estimate else {
            return PollOutcome::Continue;
        };
        let estimate = *self.lowest_estimate.insert(
            self.lowest_estimate
                .map_or(estimate, |low| low.min(estimate)),
        );
        let rate = match chain.rate_limit(self.job_id).await {
            Ok(rate) => rate,
            Err(e) => {
                tracing::warn!("[SCI] job {}: rate limit read failed: {}", self.job_id, e);
                return PollOutcome::Continue;
            }
        };
        if !self.estimate_checked {
            self.estimate_checked = true;
            if !steps_can_carry(&view, now, estimate.saturating_sub(self.proven), rate) {
                return PollOutcome::Stop(format!(
                    "the session cannot pay for an estimated {estimate} tokens before it expires"
                ));
            }
        }
        if !interim_due(
            progress,
            self.last_proved_progress,
            now,
            if self.last_proof_at == 0 {
                view.last_proof_time
            } else {
                self.last_proof_at
            },
            view.proof_timeout_window,
        ) {
            return PollOutcome::Continue;
        }
        let target = (estimate as u128 * progress.clamp(0, 100) as u128 / 100) as u64;
        if let Some(amount) = interim_amount(&view, self.proven, target, estimate, now, rate) {
            if let Err(e) = self.prove(chain, amount, progress, now).await {
                tracing::warn!(
                    "[SCI] job {}: interim proof of {} tokens did not land: {}",
                    self.job_id,
                    amount,
                    e
                );
            }
        }
        PollOutcome::Continue
    }

    /// The final claim once the sidecar finished and `actual` is known. A
    /// transient failure (a read, the rate limit, the attestation upload, a
    /// proof that did not land) is retried, re-reading the session each time,
    /// up to [`FINAL_ATTEMPTS`]; a session that is no longer `Active` ends it at
    /// once. Waiting for the rate cap is bounded by [`FINAL_RATE_WAIT_CAP_SECS`]
    /// of wall time.
    pub async fn finish(&mut self, chain: &dyn TranscodeChain, actual: u64) -> FinishOutcome {
        let started = std::time::Instant::now();
        let mut failures = 0u32;
        let mut last_error = String::new();
        loop {
            if failures >= FINAL_ATTEMPTS {
                return FinishOutcome::Unpaid(last_error);
            }
            let (view, now) = match self.read(chain).await {
                Ok(read) => read,
                Err(e) => {
                    failures += 1;
                    last_error = format!("session read failed: {e}");
                    tokio::time::sleep(Duration::from_secs(FINAL_RETRY_PAUSE_SECS)).await;
                    continue;
                }
            };
            if view.status != SessionStatus::Active {
                return FinishOutcome::Unpaid(format!("session is {:?}", view.status));
            }
            let rate = match chain.rate_limit(self.job_id).await {
                Ok(rate) => rate,
                Err(e) => {
                    failures += 1;
                    last_error = format!("rate limit read failed: {e}");
                    tokio::time::sleep(Duration::from_secs(FINAL_RETRY_PAUSE_SECS)).await;
                    continue;
                }
            };
            match final_step(&view, self.proven, actual, now, rate) {
                FinalStep::Done => {
                    return FinishOutcome::Paid {
                        proven: self.proven,
                    }
                }
                FinalStep::Claim(amount) => {
                    if let Err(e) = self.prove(chain, amount, 100, now).await {
                        failures += 1;
                        last_error = format!("final proof of {amount} tokens did not land: {e}");
                        tokio::time::sleep(Duration::from_secs(FINAL_RETRY_PAUSE_SECS)).await;
                    }
                }
                FinalStep::Wait => {
                    if started.elapsed() >= Duration::from_secs(FINAL_RATE_WAIT_CAP_SECS) {
                        return FinishOutcome::Unpaid(
                            "the rate cap did not allow the final claim in time".to_string(),
                        );
                    }
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
}
