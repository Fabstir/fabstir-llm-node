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
    catch_up_due, final_owed, final_step_batched, finished_after_expiry, interim_amount,
    interim_due, session_view, steps_can_carry, FinalStep, SessionView,
};
use crate::training::accept::SessionStatus;

/// How often the loop re-reads the session while the sidecar runs (D20(d)).
pub const STATUS_CHECK_EVERY_SECS: u64 = 20;
/// Slack, in seconds, the final claim may wait beyond what the rate needs for
/// what is owed (8.62.2: the wait was this flat 120 s, so a job owing more than
/// 120 s of rate ended BILLING_FAILED with its result withheld).
pub const FINAL_RATE_WAIT_CAP_SECS: u64 = 120;
/// How long the final claim may wait for the rate cap: the time the rate needs
/// for `owed`, plus `FINAL_RATE_WAIT_CAP_SECS`. 8.62.6: no fixed ceiling. A 3,600 s
/// ceiling (8.62.2) withheld the result of any fully paid job owing more than the
/// rate could claim in an hour, after most of it had been paid. What bounds it is
/// the job itself: the accept and estimate checks only take a job the session can
/// pay before it expires, and a session that stops being Active ends the claim.
/// A rate of 0 can never pay, so it gets the slack only.
pub fn final_wait_cap_secs(owed: u64, rate_per_sec: u64) -> u64 {
    if rate_per_sec == 0 {
        return FINAL_RATE_WAIT_CAP_SECS;
    }
    owed / rate_per_sec + FINAL_RATE_WAIT_CAP_SECS
}
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
    /// The session expired mid-run (D19(b)): stop, bill nothing more; steps
    /// proven before expiry stay paid.
    Expired,
}

/// The final claim's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishOutcome {
    /// Every claimable token is proven on chain; release the result.
    Paid { proven: u64 },
    /// The final claim did not land; withhold the result (D20(c)).
    Unpaid(String),
    /// 8.62.2: the session stopped being payable before the final claim (its
    /// depositor or another party completed it, or it timed out). Steps already
    /// proven stay paid; the client is told SESSION_NOT_BILLABLE, as for the same
    /// event mid-run, not BILLING_FAILED.
    NotBillable(String),
    /// The work finished after the session expired: not billed (D19(b)), the
    /// result withheld. Steps proven before expiry stay paid.
    Expired { proven: u64 },
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
    /// No quarter-driven proof attempt before this wall time.
    retry_proof_after: u64,
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
            retry_proof_after: 0,
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
                "[SCI-C] job {}: chain shows {} tokens proven for this transcode (recorded {}); adopting the chain's figure",
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
            "[SCI-C] job {}: transcode step {} proved {} tokens at {}% ({} total)",
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
        // After a proof that could not be made, wait for the status tick rather
        // than trying again on every poll.
        let due_proof = due_proof && wall_now >= self.retry_proof_after;
        if !due_status && !due_proof && !first_estimate {
            return PollOutcome::Continue;
        }
        self.last_status_check = wall_now;
        let (view, now) = match self.read(chain).await {
            Ok(read) => read,
            Err(e) => {
                tracing::warn!(
                    "[SCI-C] job {}: session read failed mid-transcode: {}",
                    self.job_id,
                    e
                );
                return PollOutcome::Continue;
            }
        };
        if view.status != SessionStatus::Active {
            return PollOutcome::Stop(format!("session is {:?}", view.status));
        }
        // D19(b): nothing is proved for work done after expiry, and a run
        // still going then can never be billed, so it stops.
        if finished_after_expiry(&view, now) {
            return PollOutcome::Expired;
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
                tracing::warn!("[SCI-C] job {}: rate limit read failed: {}", self.job_id, e);
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
        let last_proof_at = if self.last_proof_at == 0 {
            view.last_proof_time
        } else {
            self.last_proof_at
        };
        let target = (estimate as u128 * progress.clamp(0, 100) as u128 / 100) as u64;
        if !interim_due(
            progress,
            self.last_proved_progress,
            now,
            last_proof_at,
            view.proof_timeout_window,
        ) && !catch_up_due(
            self.proven,
            target,
            now,
            last_proof_at,
            view.proof_timeout_window,
        ) {
            return PollOutcome::Continue;
        }
        let Some(amount) = interim_amount(&view, self.proven, target, estimate, now, rate) else {
            self.retry_proof_after = wall_now + STATUS_CHECK_EVERY_SECS;
            return PollOutcome::Continue;
        };
        if let Err(e) = self.prove(chain, amount, progress, now).await {
            tracing::warn!(
                "[SCI-C] job {}: interim proof of {} tokens did not land: {}",
                self.job_id,
                amount,
                e
            );
            self.retry_proof_after = wall_now + STATUS_CHECK_EVERY_SECS;
        }
        PollOutcome::Continue
    }

    /// The final claim once the sidecar finished and `actual` is known. A
    /// transient failure (a read, the rate limit, the attestation upload, a
    /// proof that did not land) is retried, re-reading the session each time,
    /// up to [`FINAL_ATTEMPTS`]; a session that is no longer `Active` ends it at
    /// once as `NotBillable`. The claim is batched (`final_step_batched`) and its
    /// wait for the rate cap is bounded by [`final_wait_cap_secs`].
    pub async fn finish(&mut self, chain: &dyn TranscodeChain, actual: u64) -> FinishOutcome {
        if self.proven > actual {
            tracing::error!(
                "[SCI-C] job {}: {} tokens already proved but the job's final bill is {}: the reported duration fell during the run, and the difference stays paid",
                self.job_id,
                self.proven,
                actual
            );
        }
        // tokio's clock (8.62.2): identical in production, and paused tests can
        // reach the cap (with std's clock they never could).
        let started = tokio::time::Instant::now();
        let mut wait_cap: Option<Duration> = None;
        let mut failures = 0u32;
        let mut last_error = String::new();
        // When the work finished, on chain time: the first successful read.
        // Later retries never re-judge it, so a claim for work finished in time
        // still lands after expiry.
        let mut judged_expiry = false;
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
                return FinishOutcome::NotBillable(format!("session is {:?}", view.status));
            }
            if !judged_expiry {
                judged_expiry = true;
                if finished_after_expiry(&view, now) {
                    return FinishOutcome::Expired {
                        proven: self.proven,
                    };
                }
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
            let cap = *wait_cap.get_or_insert_with(|| {
                Duration::from_secs(final_wait_cap_secs(
                    final_owed(&view, self.proven, actual),
                    rate,
                ))
            });
            match final_step_batched(&view, self.proven, actual, now, rate) {
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
                    if started.elapsed() >= cap {
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
