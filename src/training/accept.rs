// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Accept-time validation (interface A.3) + abuse bounds (C.6) + the
//! drift-proof `sessionJobs` snapshot decode.
//!
//! DECODE RULE (the 17-field trap): the deployed struct has **18 fields**;
//! this module decodes ONLY fixed head words at fixed offsets — never a
//! typed full-tuple parse — and **fails CLOSED** on anything short or
//! unknown, per A.3. Word layout — verified LIVE against the DEPLOYED
//! contract (`sessionJobs(931)` on 0xD067…adA4, 2026-08-23; the raw return
//! is pinned as `tests/training_api/fixtures/sessionjobs_931.hex`):
//! w0 id · w1 depositor · w2 host · w3 paymentToken · w4 deposit ·
//! w5 pricePerToken · w6 tokensUsed · w7 maxDuration · w8 startTime ·
//! w9 lastProofTime · w10 proofInterval(TOKENS) ·
//! w11 proofTimeoutWindow(SECONDS) · w12 status(u8) · w13 withdrawnByHost ·
//! w14 refundedToUser · w15 offset(conversationCID) · w16 lastProofHash ·
//! w17 offset(lastProofCID).
//!
//! DO NOT "verify" this layout against
//! `contracts/JobMarketplaceWithModels-CLIENT-ABI.json` — that artifact is
//! STALE (it carries a phantom `requester` at w2 that exists nowhere on the
//! deployed contract; the T3 converge round proved every A.3 field except
//! depositor/status mis-offset when built from it). The pinned live fixture
//! is the authority; semantic anchors in it: w3 = the documented USDC
//! address, w5 = 904, w12 = 1 (Completed).

use std::collections::HashMap;
use std::sync::Mutex;

use ethers::types::{Address, U256};

/// The deployed session struct's head word count (18 fields; the two string
/// members occupy their head slots as offsets).
pub const SESSION_JOBS_HEAD_WORDS: usize = 18;

/// On-chain session status (values pinned from the SDK's own
/// `enum SessionStatus { Active = 0, Completed = 1, TimedOut = 2 }`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Active,
    Completed,
    TimedOut,
}

impl TryFrom<u8> for SessionStatus {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(SessionStatus::Active),
            1 => Ok(SessionStatus::Completed),
            2 => Ok(SessionStatus::TimedOut),
            other => Err(format!("unknown session status {other} (fail closed)")),
        }
    }
}

/// The A.3-relevant slice of `sessionJobs(jobId)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub depositor: Address,
    pub host: Address,
    pub payment_token: Address,
    pub deposit: U256,
    pub price_per_token: U256,
    pub tokens_used: U256,
    pub max_duration: U256,
    pub start_time: U256,
    /// w11 — the per-proof SILENCE window in SECONDS (A.3's ≥ 3600 gate).
    /// NOT w10 `proofInterval`, which is a TOKEN count (the first-proof
    /// floor, 1000 on live sessions).
    pub proof_timeout_window: U256,
    pub status: SessionStatus,
}

/// Decode the raw `eth_call` return of `sessionJobs(uint256)` by fixed word
/// offsets. Fails CLOSED: a short head or an unknown status is an error,
/// never a default.
pub fn decode_session_snapshot(ret: &[u8]) -> Result<SessionSnapshot, String> {
    if ret.len() < SESSION_JOBS_HEAD_WORDS * 32 {
        return Err(format!(
            "sessionJobs return too short: {} bytes < {} head words (fail closed)",
            ret.len(),
            SESSION_JOBS_HEAD_WORDS
        ));
    }
    let word = |i: usize| &ret[i * 32..(i + 1) * 32];
    let addr = |i: usize| Address::from_slice(&word(i)[12..32]);
    let uint = |i: usize| U256::from_big_endian(word(i));
    Ok(SessionSnapshot {
        depositor: addr(1),
        host: addr(2),
        payment_token: addr(3),
        deposit: uint(4),
        price_per_token: uint(5),
        tokens_used: uint(6),
        max_duration: uint(7),
        start_time: uint(8),
        proof_timeout_window: uint(11),
        status: SessionStatus::try_from(word(12)[31])?,
    })
}

/// Accept-time constants (interface A.3 / C.5; env-plumbed at T4.5).
#[derive(Debug, Clone)]
pub struct AcceptConfig {
    /// `TRAIN_JOB_TIMEOUT_SECS` (M0 default 12,600).
    pub train_job_timeout_secs: u64,
    /// The settle margin (dispute window + completion tx) — 600.
    pub settle_margin_secs: u64,
    /// The M0 proof-window floor — 3,600 (== the chain maximum).
    pub min_proof_timeout_window_secs: u64,
}

impl Default for AcceptConfig {
    fn default() -> Self {
        AcceptConfig {
            train_job_timeout_secs: 12_600,
            settle_margin_secs: 600,
            min_proof_timeout_window_secs: 3_600,
        }
    }
}

/// A terminal accept-time rejection and the wire reason it maps to
/// (`VALIDATION_FAILED` with `detail.reason`, interface A.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptReject {
    /// `detail.reason: "sessionParams"` — one of the six A.3 gates failed.
    SessionParams { detail: String },
    /// `detail.reason: "sessionReused"` — one `train` per session, EVER.
    SessionReused,
    /// `detail.reason: "trainActive"` — a `train` already runs on this session.
    TrainActive,
    /// C.6 plausibility / one-per-address / cooldown.
    Plausibility { detail: String },
}

/// The pure A.3 gate over an already-read snapshot. `session_model` is the
/// separate `sessionModel(jobId)` read; `training_tokens` = declared × epochs.
/// The host-binding check is defence beyond A.3's six (proof auth on-chain is
/// `msg.sender == session.host` — accepting a foreign session trains free).
#[allow(clippy::too_many_arguments)]
pub fn validate_session(
    snap: &SessionSnapshot,
    session_model: [u8; 32],
    now_secs: u64,
    training_tokens: U256,
    expected_price: U256,
    priced_tokens: &[Address],
    expected_model: [u8; 32],
    host_address: Address,
    cfg: &AcceptConfig,
) -> Result<(), AcceptReject> {
    let fail = |detail: String| Err(AcceptReject::SessionParams { detail });
    if snap.status != SessionStatus::Active {
        return fail(format!("session is not Active (status {:?})", snap.status));
    }
    if snap.host != host_address {
        return fail(format!("session host {:?} is not this host", snap.host));
    }
    if session_model != expected_model {
        return fail("sessionModel does not match the training model id".to_string());
    }
    if snap.price_per_token != expected_price || snap.price_per_token.is_zero() {
        return fail(format!(
            "session price {} != registered price {}",
            snap.price_per_token, expected_price
        ));
    }
    if !priced_tokens.contains(&snap.payment_token) {
        return fail(format!(
            "payment token {:?} is not one this host prices",
            snap.payment_token
        ));
    }
    if snap.proof_timeout_window < U256::from(cfg.min_proof_timeout_window_secs) {
        return fail(format!(
            "proofTimeoutWindow {} below the {} floor",
            snap.proof_timeout_window, cfg.min_proof_timeout_window_secs
        ));
    }
    // Remaining lifetime: startTime + maxDuration − now ≥ timeout + margin.
    let deadline = snap.start_time.saturating_add(snap.max_duration);
    let need = U256::from(cfg.train_job_timeout_secs + cfg.settle_margin_secs);
    let now = U256::from(now_secs);
    if deadline <= now || deadline - now < need {
        return fail(format!(
            "remaining lifetime {} s < required {} s (TRAIN_JOB_TIMEOUT + settle margin)",
            deadline.saturating_sub(now),
            need
        ));
    }
    // Headroom NET of tokensUsed: deposit × 1000 / price − used ≥ required
    // (the ×1000 is the marketplace's price scaling; price-zero guarded above).
    let capacity = snap.deposit.saturating_mul(U256::from(1000u64)) / snap.price_per_token;
    let remaining = capacity.saturating_sub(snap.tokens_used);
    if remaining < training_tokens {
        return fail(format!(
            "remaining headroom {remaining} tokens < required {training_tokens}"
        ));
    }
    Ok(())
}

/// C.6 plausibility gate over manifest numbers, BEFORE any shard fetch.
pub fn plausibility_gate(
    manifest_total_bytes: u64,
    declared_tokens: u64,
) -> Result<(), AcceptReject> {
    let bound = declared_tokens.saturating_mul(8);
    if manifest_total_bytes > bound {
        return Err(AcceptReject::Plausibility {
            detail: format!(
                "totalBytes {manifest_total_bytes} > declaredTokens × 8 = {bound} (implausible dataset)"
            ),
        });
    }
    Ok(())
}

/// Outcome of asking the registry to begin an attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptClaim {
    Ok,
    /// This session already carries an attempt that ENDED (completed or
    /// terminally rejected) — one `train` per session, ever (A.3).
    SessionReused,
    /// This session's attempt is still running.
    TrainActive,
    /// This address already has a training job in flight (C.6).
    AddressBusy,
    /// This address is inside the post-reject cooldown (C.6).
    Cooldown,
}

/// How an attempt ended (drives the C.6 cooldown, which follows REJECTS only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptOutcome {
    Completed,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionAttempt {
    Active,
    Ended,
}

#[derive(Default)]
struct AttemptState {
    by_session: HashMap<u64, SessionAttempt>,
    active_by_address: HashMap<Address, u32>,
    last_reject_at: HashMap<Address, u64>,
}

/// The node-local attempt record (A.3's one-train-per-session-ever + C.6's
/// one-per-address + cooldown). NODE-LOCAL is the point: a terminal reject
/// leaves the chain session Active, and ONLY this record blocks its reuse.
#[derive(Default)]
pub struct AttemptRegistry {
    inner: Mutex<AttemptState>,
}

impl AttemptRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Advisory session-history check WITHOUT claiming: `SessionReused` /
    /// `TrainActive` for a session the registry already knows, else `Ok`.
    /// Callers use it to short-circuit BEFORE side-effectful stages (the
    /// converge round proved a consumed session re-entering the sidecar
    /// consult scheduled a SECOND zero-settle); `try_begin` remains the
    /// atomic claim.
    pub fn peek(&self, job_id: u64) -> AttemptClaim {
        let state = self.inner.lock().expect("attempt registry poisoned");
        match state.by_session.get(&job_id) {
            Some(SessionAttempt::Active) => AttemptClaim::TrainActive,
            Some(SessionAttempt::Ended) => AttemptClaim::SessionReused,
            None => AttemptClaim::Ok,
        }
    }

    /// Claim the right to process a `train` for (session, client address).
    /// Session-history checks come FIRST: a reused session reports
    /// `SessionReused` even when the address's cooldown has long expired.
    pub fn try_begin(
        &self,
        job_id: u64,
        address: Address,
        now_secs: u64,
        cooldown_secs: u64,
    ) -> AttemptClaim {
        let mut state = self.inner.lock().expect("attempt registry poisoned");
        match state.by_session.get(&job_id) {
            Some(SessionAttempt::Active) => return AttemptClaim::TrainActive,
            Some(SessionAttempt::Ended) => return AttemptClaim::SessionReused,
            None => {}
        }
        if state.active_by_address.get(&address).copied().unwrap_or(0) > 0 {
            return AttemptClaim::AddressBusy;
        }
        if let Some(&rejected_at) = state.last_reject_at.get(&address) {
            if now_secs < rejected_at.saturating_add(cooldown_secs) {
                return AttemptClaim::Cooldown;
            }
        }
        state.by_session.insert(job_id, SessionAttempt::Active);
        *state.active_by_address.entry(address).or_insert(0) += 1;
        AttemptClaim::Ok
    }

    /// Record the end of a previously-begun attempt. The session is consumed
    /// FOREVER either way (A.3); only a REJECT starts the C.6 cooldown.
    pub fn finish(&self, job_id: u64, address: Address, now_secs: u64, outcome: AttemptOutcome) {
        let mut state = self.inner.lock().expect("attempt registry poisoned");
        if matches!(state.by_session.get(&job_id), Some(SessionAttempt::Active)) {
            state.by_session.insert(job_id, SessionAttempt::Ended);
            let count = state.active_by_address.entry(address).or_insert(1);
            *count = count.saturating_sub(1);
            if outcome == AttemptOutcome::Rejected {
                state.last_reject_at.insert(address, now_secs);
            }
        }
    }

    /// Record a CONSUMING terminal reject whether or not an attempt was ever
    /// begun (C.3's universal rule: template-shape, sidecar-consult, TD14
    /// capacity and A.3 rejects ALL consume the session — one `train` per
    /// session, ever). Returns `true` iff this call is the FIRST terminal
    /// record for the session — the caller's exactly-once settle key (round-2
    /// catch: two concurrent consult failures double-scheduled the settle).
    ///
    /// `arm_cooldown` — C.6's cooldown protects the PIPELINE, so only rejects
    /// that did real work arm it. Pure back-off refusals (Cooldown itself,
    /// AddressBusy, GPU/sidecar capacity) pass `false`: round 2 proved the
    /// unconditional stamp let a premature retry EXTEND its own lockout
    /// indefinitely, and AddressBusy refusals during a legit run poisoned the
    /// post-completion window.
    pub fn record_terminal_reject(
        &self,
        job_id: u64,
        address: Address,
        now_secs: u64,
        arm_cooldown: bool,
    ) -> bool {
        let mut state = self.inner.lock().expect("attempt registry poisoned");
        let newly_recorded = match state.by_session.get(&job_id) {
            Some(SessionAttempt::Ended) => false,
            Some(SessionAttempt::Active) => {
                let count = state.active_by_address.entry(address).or_insert(1);
                *count = count.saturating_sub(1);
                true
            }
            None => true,
        };
        state.by_session.insert(job_id, SessionAttempt::Ended);
        if arm_cooldown && newly_recorded {
            state.last_reject_at.insert(address, now_secs);
        }
        newly_recorded
    }
}
