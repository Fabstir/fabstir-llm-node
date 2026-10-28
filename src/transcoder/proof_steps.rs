// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Transcode billing as it runs (SCI-C D20, IMPLEMENTATION-SESSION-COMPLETION-INTEGRITY).
//!
//! A transcode used to bill once, after the sidecar finished. Until then the
//! session had no proof since its start, so anyone could time it out (or the
//! depositor complete it) and the host was paid nothing for work already done
//! (Defect E; session 1432 was exposed for ~11 minutes). These are the pure
//! rules for proving the work in steps instead, within every contract limit:
//! `MIN_PROVEN_TOKENS` per proof, `proofInterval` for the first, the per-model
//! rate cap ("Too many") and the deposit cap ("Over dep").

use ethers::types::U256;

use crate::training::accept::{decode_session_snapshot, SessionStatus};

/// `JobMarketplace.MIN_PROVEN_TOKENS`.
pub const MIN_PROVEN_TOKENS: u64 = 100;
/// `JobMarketplace.PRICE_PRECISION`.
pub const PRICE_PRECISION: u64 = 1000;

/// The session fields the step rules read, from the raw `sessionJobs` return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionView {
    pub status: SessionStatus,
    pub deposit: u64,
    pub price_per_token: u64,
    pub tokens_used: u64,
    pub max_duration: u64,
    pub start_time: u64,
    pub last_proof_time: u64,
    pub proof_interval: u64,
    pub proof_timeout_window: u64,
}

fn word_u64(raw: &[u8], i: usize) -> Result<u64, String> {
    let v = U256::from_big_endian(&raw[i * 32..(i + 1) * 32]);
    if v > U256::from(u64::MAX) {
        return Err(format!("sessionJobs word {i} overflows u64"));
    }
    Ok(v.as_u64())
}

/// Decode with the same drift canary as completion (word 0 == job id).
pub fn session_view(raw: &[u8], job_id: u64) -> Result<SessionView, String> {
    let snapshot = decode_session_snapshot(raw)?;
    if word_u64(raw, 0)? != job_id {
        return Err(format!(
            "sessionJobs word 0 is not job {job_id} (layout drift)"
        ));
    }
    Ok(SessionView {
        status: snapshot.status,
        deposit: word_u64(raw, 4)?,
        price_per_token: word_u64(raw, 5)?,
        tokens_used: word_u64(raw, 6)?,
        max_duration: word_u64(raw, 7)?,
        start_time: word_u64(raw, 8)?,
        last_proof_time: word_u64(raw, 9)?,
        proof_interval: word_u64(raw, 10)?,
        proof_timeout_window: word_u64(raw, 11)?,
    })
}

impl SessionView {
    pub fn expiry(&self) -> u64 {
        self.start_time.saturating_add(self.max_duration)
    }

    /// Tokens the deposit can still pay for ("Over dep").
    pub fn deposit_room(&self) -> u64 {
        if self.price_per_token == 0 {
            return 0;
        }
        let max_tokens =
            (self.deposit as u128 * PRICE_PRECISION as u128 / self.price_per_token as u128) as u64;
        max_tokens.saturating_sub(self.tokens_used)
    }

    /// Tokens one proof may claim at `chain_now` ("Too many" and "Over dep").
    pub fn claimable(&self, chain_now: u64, rate_per_sec: u64) -> u64 {
        let by_rate = chain_now
            .saturating_sub(self.last_proof_time)
            .saturating_mul(rate_per_sec);
        by_rate.min(self.deposit_room())
    }

    /// The smallest proof the contract accepts next ("Min tokens", "Low first").
    pub fn minimum_proof(&self) -> u64 {
        if self.tokens_used == 0 {
            self.proof_interval.max(MIN_PROVEN_TOKENS)
        } else {
            MIN_PROVEN_TOKENS
        }
    }
}

/// Accept limits (D20(a)), from env with conservative defaults.
#[derive(Debug, Clone, Copy)]
pub struct AcceptLimits {
    /// `TRANSCODE_MIN_PROOF_WINDOW_SECS`: before the sidecar reports progress
    /// there is nothing to prove, so a shorter window can be timed out unpaid.
    pub min_proof_window_secs: u64,
    /// `TRANSCODE_MIN_LIFETIME_SECS`: remaining session lifetime required.
    pub min_lifetime_secs: u64,
}

impl AcceptLimits {
    pub fn from_env() -> Self {
        let read = |name: &str, default: u64| {
            std::env::var(name)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        };
        Self {
            min_proof_window_secs: read("TRANSCODE_MIN_PROOF_WINDOW_SECS", 300),
            min_lifetime_secs: read("TRANSCODE_MIN_LIFETIME_SECS", 600),
        }
    }
}

/// Why a transcode is refused at accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptRefusal {
    NotActive,
    ProofWindowTooShort,
    LifetimeTooShort,
    DepositExhausted,
}

impl AcceptRefusal {
    pub fn code(&self) -> &'static str {
        match self {
            AcceptRefusal::NotActive => "SESSION_NOT_ACTIVE",
            AcceptRefusal::ProofWindowTooShort => "PROOF_WINDOW_TOO_SHORT",
            AcceptRefusal::LifetimeTooShort => "SESSION_LIFETIME_TOO_SHORT",
            AcceptRefusal::DepositExhausted => "DEPOSIT_EXHAUSTED",
        }
    }
}

/// D20(a): may a transcode start on this session now? The cost estimate needs
/// the source duration, which the sidecar reports only once it is running, so
/// the deposit and rate checks against the estimate happen at the first
/// progress report (`steps_can_carry`).
pub fn transcode_accept(
    view: &SessionView,
    chain_now: u64,
    limits: &AcceptLimits,
) -> Result<(), AcceptRefusal> {
    if view.status != SessionStatus::Active {
        return Err(AcceptRefusal::NotActive);
    }
    if view.proof_timeout_window < limits.min_proof_window_secs {
        return Err(AcceptRefusal::ProofWindowTooShort);
    }
    if view.expiry().saturating_sub(chain_now) < limits.min_lifetime_secs {
        return Err(AcceptRefusal::LifetimeTooShort);
    }
    if view.deposit_room() < view.minimum_proof() {
        return Err(AcceptRefusal::DepositExhausted);
    }
    Ok(())
}

/// Once the estimate is known: can the rate cap carry it before expiry? The
/// deposit is not checked here: work beyond the deposit is simply not
/// claimable, and a deposit-capped job bills what the deposit pays and is
/// released (D20(c)).
pub fn steps_can_carry(
    view: &SessionView,
    chain_now: u64,
    estimate: u64,
    rate_per_sec: u64,
) -> bool {
    let remaining = view.expiry().saturating_sub(chain_now);
    let by_rate = (remaining as u128) * (rate_per_sec as u128)
        + view.claimable(chain_now, rate_per_sec) as u128;
    (estimate.min(view.deposit_room()) as u128) <= by_rate
}

/// D20(b): is an interim proof due? Every 25 % of progress, or when half the
/// session's own proof window has passed since the last proof.
pub fn interim_due(
    progress: i32,
    last_proved_progress: i32,
    now: u64,
    last_proof_at: u64,
    proof_timeout_window: u64,
) -> bool {
    if progress <= last_proved_progress {
        return false;
    }
    progress / 25 > last_proved_progress.max(0) / 25
        || now.saturating_sub(last_proof_at) >= proof_timeout_window / 2
}

/// D20(b): how much the next interim proof claims. `target` is the work done so
/// far (the estimate scaled by progress), `estimate` the whole job. Never more
/// than the contract allows now, never below its minimum, and never so much
/// that less than `MIN_PROVEN_TOKENS` is left for the final proof. `None`
/// defers the step to the next tick.
pub fn interim_amount(
    view: &SessionView,
    proven: u64,
    target: u64,
    estimate: u64,
    chain_now: u64,
    rate_per_sec: u64,
) -> Option<u64> {
    let ceiling = estimate.saturating_sub(MIN_PROVEN_TOKENS);
    let wanted = target.min(ceiling).saturating_sub(proven);
    let amount = wanted.min(view.claimable(chain_now, rate_per_sec));
    (amount >= view.minimum_proof()).then_some(amount)
}

/// The next final-proof action (D20(c)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalStep {
    /// Submit a proof for this many tokens now.
    Claim(u64),
    /// More is owed but the rate cap does not allow it yet.
    Wait,
    /// Nothing more can be claimed: all proven, the deposit is spent, or a
    /// remainder below the contract minimum is left.
    Done,
}

/// D20(c): the final proof(s), after the sidecar finished and `actual` is
/// known. On a session with no proof yet the claim is raised to the
/// contract's first-proof minimum (`proofInterval`), the same minimum billing
/// the old single checkpoint applied. Work beyond the deposit is not
/// claimable, so a deposit-capped job ends with what the deposit pays.
pub fn final_step(
    view: &SessionView,
    proven: u64,
    actual: u64,
    chain_now: u64,
    rate_per_sec: u64,
) -> FinalStep {
    let mut owed = actual.saturating_sub(proven);
    if owed == 0 {
        return FinalStep::Done;
    }
    if view.tokens_used == 0 {
        owed = owed.max(view.minimum_proof());
    }
    let owed = owed.min(view.deposit_room());
    if owed < view.minimum_proof() {
        return FinalStep::Done;
    }
    let amount = owed.min(view.claimable(chain_now, rate_per_sec));
    if amount < view.minimum_proof() {
        return FinalStep::Wait;
    }
    FinalStep::Claim(amount)
}
