// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Batch D21: the relight FINAL proof, after keep-alive interims.
//!
//! - Claim: `remainder = tokens − max(on-chain delta, interim_sent)` (saturating),
//!   where the on-chain delta = `tokensUsed` at the finalising re-read − at the D20
//!   admission read. Subtracting every interim ever handed to the queue means a
//!   lagging re-read or an interim mining after it can never push the cumulative
//!   claim above the estimate; the delta covers anything larger (another clip on
//!   a shared session). The cost is a rare under-claim of interims that never
//!   mined (the host's loss; accepted).
//! - Timing: first sent at ≥ the on-chain `lastProofTime` (w9 from the
//!   finalising re-read) + remainder ÷ rate.
//! - Typed outcomes ([`ProofSubmitError`]): `RateLimited` ⇒ wait (remainder ÷
//!   rate) plus 5 s and resend; `Reverted { block }` ⇒ read the session AT that block and,
//!   if w9 moved past the one this attempt was timed from and `tokensUsed` has
//!   not grown by the remainder, resend from the NEW w9; `Unconfirmed(hash)` ⇒
//!   send NOTHING new, poll that hash's receipt to the budget (status 1 = landed,
//!   status 0 = the `Reverted` rule, none = forfeit); `Other` ⇒ forfeit.
//! - At most 3 sends; before EVERY resend a re-read (at the latest receipt's
//!   block, or `latest` when no attempt has a receipt) stops once `tokensUsed`
//!   has grown by the remainder; an attempt (its wait + 5 s + 60 s receipt) that
//!   would end past the budget (deadline + 120 s + tokens ÷ rate) is not started.

use std::time::Duration;

use ethers::types::{H256, U256};
use tokio::time::Instant;
use tracing::{error, info, warn};

use crate::ltx::keepalive::UnixClock;
use crate::ltx::submit::{ProofReceipt, ProofSubmit, ProofSubmitError};
use crate::training::accept::SessionSnapshot;

/// At most this many `submitProofOfWork` sends for one final proof.
pub const MAX_FINAL_SENDS: u32 = 3;
/// Added to every resend's rate wait.
pub const RETRY_PAD_SECS: u64 = 5;
/// The tx queue's receipt wait, counted into every attempt's end.
pub const RECEIPT_WAIT_SECS: u64 = 60;
/// How often an `Unconfirmed` final's receipt is polled.
pub const RECEIPT_POLL: Duration = Duration::from_secs(5);

/// `tokens − max(final_used − admission_used, interim_sent)`, both saturating.
pub fn final_remainder(
    tokens: u64,
    admission_used: U256,
    final_used: U256,
    interim_sent: u64,
) -> u64 {
    let delta = final_used.saturating_sub(admission_used);
    let delta = delta.min(U256::from(u64::MAX)).as_u64();
    tokens.saturating_sub(delta.max(interim_sent))
}

/// The session read at a block (`None` = `latest`).
#[async_trait::async_trait]
pub trait SessionReads: Send + Sync {
    async fn session_at(&self, job_id: u64, block: Option<u64>) -> Result<SessionSnapshot, String>;
}

/// The relight final proof's inputs (from the finalising re-read and AdmitCfg).
pub struct RelightFinal<'a> {
    pub reads: &'a dyn SessionReads,
    pub clock: &'a dyn UnixClock,
    pub rate_tokens_per_sec: u64,
    /// w9 at the finalising re-read.
    pub final_w9: u64,
    /// `tokensUsed` at the finalising re-read.
    pub final_used: U256,
    /// `started + deadline + 120 s + ceil(tokens ÷ rate)`.
    pub budget_end: Instant,
    /// Added to the FIRST attempt's end: one receipt wait when an interim was
    /// still in the serial tx queue at the keep-alive's stop (the final can be
    /// broadcast that much later).
    pub first_attempt_extra: Duration,
    /// The admission snapshot's w10 `proofInterval`: the first-proof floor
    /// (round 2: never re-read on this path — that read has no timeout).
    pub proof_interval: u64,
}

/// How `finalize_clip` sends the proof.
pub enum SubmitPolicy<'a> {
    /// The LTX modes: a "Too many" (`RateLimited`) retry at `claim / 2000 + 5` s,
    /// 3 attempts; anything else forfeits.
    Ltx,
    /// Batch D21: relight jobs — [`send_relight_final`].
    Relight(RelightFinal<'a>),
}

/// Batch D21: what `run` derives for a relight final from its two reads (pure;
/// `run` calls it and tested on its own).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelightFinalInputs {
    /// The on-chain claim: [`final_remainder`].
    pub claim: u64,
    pub final_w9: u64,
    pub final_used: U256,
    pub budget_end: Instant,
    pub first_attempt_extra: Duration,
    pub rate_tokens_per_sec: u64,
    /// The ADMISSION snapshot's w10 (the first-proof floor).
    pub proof_interval: u64,
}

/// Batch D21: the claim (`tokens − max(delta since the ADMISSION read,
/// interim_sent)`) and the final's timing from the finalising read: budget =
/// `started_at + deadline + 120 s + ceil(tokens ÷ rate)`; one more receipt wait
/// on the first attempt when an interim was in flight at the stop. With no
/// admission snapshot (never on a relight job with a session) the delta is 0.
#[allow(clippy::too_many_arguments)]
pub fn relight_final_inputs(
    tokens: u64,
    admission: Option<&SessionSnapshot>,
    final_snap: &SessionSnapshot,
    interim_sent: u64,
    interim_in_flight: bool,
    cfg: &crate::ltx::relight::AdmitCfg,
    template_id: &str,
    frames: u32,
    started_at: Instant,
) -> RelightFinalInputs {
    let admission_used = admission.map_or(final_snap.tokens_used, |a| a.tokens_used);
    let proof_interval =
        u256_secs(admission.map_or(final_snap.proof_interval, |a| a.proof_interval));
    RelightFinalInputs {
        claim: final_remainder(tokens, admission_used, final_snap.tokens_used, interim_sent),
        final_w9: u256_secs(final_snap.last_proof_time),
        final_used: final_snap.tokens_used,
        budget_end: started_at
            + Duration::from_secs(cfg.final_budget_secs(template_id, frames, tokens)),
        first_attempt_extra: if interim_in_flight {
            Duration::from_secs(RECEIPT_WAIT_SECS)
        } else {
            Duration::ZERO
        },
        rate_tokens_per_sec: cfg.rate_tokens_per_sec,
        proof_interval,
    }
}

/// Batch D21 round 3: THE on-chain claim of a clip's final proof — the relight
/// remainder when there are relight inputs, else `tokens − interim_sent`
/// (saturating; `interim_sent` is 0 outside relight jobs, so this only guards
/// against interims ever running under the Ltx policy).
pub fn final_claim(tokens: u64, relight: Option<&RelightFinalInputs>, interim_sent: u64) -> u64 {
    match relight {
        Some(i) => i.claim,
        None => tokens.saturating_sub(interim_sent),
    }
}

/// Batch D21: a relight final (`Some` inputs) takes the relight send rules;
/// everything else the LTX ones.
pub fn final_policy<'a>(
    inputs: Option<&RelightFinalInputs>,
    reads: &'a dyn SessionReads,
    clock: &'a dyn UnixClock,
) -> SubmitPolicy<'a> {
    match inputs {
        Some(i) => SubmitPolicy::Relight(RelightFinal {
            reads,
            clock,
            rate_tokens_per_sec: i.rate_tokens_per_sec,
            final_w9: i.final_w9,
            final_used: i.final_used,
            budget_end: i.budget_end,
            first_attempt_extra: i.first_attempt_extra,
            proof_interval: i.proof_interval,
        }),
        None => SubmitPolicy::Ltx,
    }
}

fn u256_secs(v: U256) -> u64 {
    v.min(U256::from(u64::MAX)).as_u64()
}

/// Time left before `budget_end` (zero once past it).
fn left(budget_end: Instant) -> Duration {
    budget_end.saturating_duration_since(Instant::now())
}

/// The wait until `at_unix` on the chain clock, saturating (an absurd w9 gives
/// an absurd wait, which the budget check then refuses — never a panic).
fn wait_until_unix(clock: &dyn UnixClock, at_unix: u64) -> Duration {
    Duration::from_secs(at_unix.saturating_sub(clock.now_unix()))
}

/// How often the read at a revert's block is retried while it fails.
pub const REVERT_READ_RETRY: Duration = Duration::from_secs(5);

enum AfterRevert {
    Landed,
    Resend { new_w9: u64 },
    GiveUp,
}

/// Judge a mined status-0 attempt from a read AT its block. A failing read is
/// retried (nothing is sent meanwhile) until a resend — its rate wait + 5 s +
/// the receipt wait — could no longer fit inside the budget.
async fn judge_revert(
    rf: &RelightFinal<'_>,
    jid: u64,
    claim: u64,
    block: u64,
    timed_from_w9: u64,
    resend_cost: Duration,
) -> AfterRevert {
    loop {
        match rf.reads.session_at(jid, Some(block)).await {
            Ok(s) if s.tokens_used >= rf.final_used.saturating_add(U256::from(claim)) => {
                return AfterRevert::Landed
            }
            Ok(s) if u256_secs(s.last_proof_time) > timed_from_w9 => {
                return AfterRevert::Resend {
                    new_w9: u256_secs(s.last_proof_time),
                }
            }
            Ok(_) => {
                warn!(
                    "LTX job {jid}: final proof reverted at block {block} with no newer proof — \
                     forfeiting"
                );
                return AfterRevert::GiveUp;
            }
            Err(e) => {
                if left(rf.budget_end) < resend_cost.saturating_add(REVERT_READ_RETRY) {
                    warn!(
                        "LTX job {jid}: final proof reverted; the read at block {block} still \
                         fails ({e}) and a resend no longer fits the budget — forfeiting"
                    );
                    return AfterRevert::GiveUp;
                }
                warn!("LTX job {jid}: the read at block {block} failed ({e}); retrying");
                tokio::time::sleep(REVERT_READ_RETRY).await;
            }
        }
    }
}

/// Batch D21: send the relight final proof (`claim` = the remainder). Returns
/// whether it LANDED (the tracker is the caller's business). Every await is
/// bounded by the budget.
pub async fn send_relight_final(
    sub: &dyn ProofSubmit,
    jid: u64,
    claim: u64,
    proof_hash: [u8; 32],
    proof_cid: String,
    rf: &RelightFinal<'_>,
) -> bool {
    let rate_wait = Duration::from_secs(claim.div_ceil(rf.rate_tokens_per_sec.max(1)));
    let pad = Duration::from_secs(RETRY_PAD_SECS);
    let receipt_wait = Duration::from_secs(RECEIPT_WAIT_SECS);
    let resend_cost = rate_wait.saturating_add(pad).saturating_add(receipt_wait);
    let mut timed_from_w9 = rf.final_w9;
    // the first send: at ≥ w9 + remainder ÷ rate (chain time)
    let mut wait = wait_until_unix(rf.clock, timed_from_w9.saturating_add(rate_wait.as_secs()));
    let mut sends = 0u32;
    let mut last_receipt_block: Option<u64> = None;
    loop {
        let mut attempt = wait
            .max(rate_wait.saturating_add(pad))
            .saturating_add(receipt_wait);
        if sends == 0 {
            attempt = attempt.saturating_add(rf.first_attempt_extra);
        }
        if sends >= MAX_FINAL_SENDS || attempt > left(rf.budget_end) {
            error!(
                "LTX job {jid}: final proof forfeited after {sends} send(s) — another attempt \
                 would pass the budget or the {MAX_FINAL_SENDS}-send cap"
            );
            return false;
        }
        tokio::time::sleep(wait).await;
        sends += 1;
        // bounded by the budget; a timeout may have been broadcast ⇒ forfeit,
        // never a resend
        let outcome = match tokio::time::timeout(
            left(rf.budget_end),
            sub.submit_ltx_proof(jid, claim, proof_hash, proof_cid.clone()),
        )
        .await
        {
            Ok(o) => o,
            Err(_) => {
                error!("LTX job {jid}: final proof got no answer within the budget — forfeited");
                return false;
            }
        };
        // what a mined status-0 attempt leads to
        let mut reverted_at: Option<u64> = None;
        match outcome {
            Ok(tx) => {
                info!(
                    "LTX job {jid}: final proof landed ({claim} tokens, tx {tx:?}, send {sends})"
                );
                return true;
            }
            Err(ProofSubmitError::RateLimited) => {
                // nothing broadcast: re-read before the resend
                let landed = match rf.reads.session_at(jid, last_receipt_block).await {
                    Ok(s) => s.tokens_used >= rf.final_used.saturating_add(U256::from(claim)),
                    Err(e) => {
                        warn!("LTX job {jid}: re-read before the resend failed ({e})");
                        false
                    }
                };
                if landed {
                    info!("LTX job {jid}: the remainder is on chain — no resend");
                    return true;
                }
                wait = rate_wait.saturating_add(pad);
                warn!(
                    "LTX job {jid}: final proof rate-limited (send {sends}); resending in {wait:?}"
                );
            }
            Err(ProofSubmitError::Reverted { block }) => reverted_at = Some(block),
            Err(ProofSubmitError::Unconfirmed(hash)) => {
                // never a second send while this one may still mine
                match poll_receipt(sub, jid, hash, rf.budget_end).await {
                    Some(r) if r.success => return true,
                    Some(r) => reverted_at = Some(r.block),
                    None => return false,
                }
            }
            Err(ProofSubmitError::Other(e)) => {
                error!("LTX job {jid}: final proof failed ({e}) — revenue forfeited");
                return false;
            }
        }
        if let Some(block) = reverted_at {
            last_receipt_block = Some(block);
            match judge_revert(rf, jid, claim, block, timed_from_w9, resend_cost).await {
                AfterRevert::Landed => return true,
                AfterRevert::GiveUp => return false,
                AfterRevert::Resend { new_w9 } => {
                    timed_from_w9 = new_w9;
                    wait = wait_until_unix(
                        rf.clock,
                        new_w9
                            .saturating_add(rate_wait.as_secs())
                            .saturating_add(RETRY_PAD_SECS),
                    );
                    warn!(
                        "LTX job {jid}: final proof reverted at block {block} after a newer proof \
                         (w9 {new_w9}); resending in {wait:?}"
                    );
                }
            }
        }
    }
}

/// One receipt read is bounded by this (and by what is left of the budget).
pub const RECEIPT_READ_BOUND: Duration = Duration::from_secs(10);

/// Poll an `Unconfirmed` final's receipt until `budget_end`; `None` = still
/// none at the cap (forfeit). Each read is bounded (a timeout reads as none)
/// and every sleep is at least 100 ms (never a spin).
async fn poll_receipt(
    sub: &dyn ProofSubmit,
    jid: u64,
    hash: H256,
    budget_end: Instant,
) -> Option<ProofReceipt> {
    loop {
        let bound = RECEIPT_READ_BOUND.min(left(budget_end));
        match tokio::time::timeout(bound, sub.receipt_of(hash)).await {
            Ok(Ok(Some(r))) => return Some(r),
            Ok(Ok(None)) | Err(_) => {}
            Ok(Err(e)) => warn!("LTX job {jid}: receipt read for {hash:?} failed ({e})"),
        }
        let remaining = left(budget_end);
        if remaining.is_zero() {
            error!(
                "LTX job {jid}: final proof {hash:?} still unconfirmed at the budget — forfeited"
            );
            return None;
        }
        tokio::time::sleep(RECEIPT_POLL.min(remaining).max(Duration::from_millis(100))).await;
    }
}

/// The production [`SessionReads`]: `sessionJobs` at a block through the
/// checkpoint manager, retried like every D20 read and decoded fail-closed.
pub struct CmSessionReads<'a>(pub &'a crate::contracts::checkpoint_manager::CheckpointManager);

#[async_trait::async_trait]
impl SessionReads for CmSessionReads<'_> {
    async fn session_at(&self, job_id: u64, block: Option<u64>) -> Result<SessionSnapshot, String> {
        crate::ltx::template::read_with_retry(
            3,
            Duration::from_secs(2),
            Duration::from_secs(10),
            || self.0.query_session_jobs_raw(job_id, block),
        )
        .await
        .and_then(|raw| crate::training::accept::decode_session_snapshot(&raw))
    }
}
