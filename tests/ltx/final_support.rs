// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Shared mocks for the Batch D21 final-proof tests (`test_final_proof.rs`,
//! `test_final_proof_bounds.rs`): a scripted `ProofSubmit` whose calls and
//! receipt polls are counted (and can be made to never resolve), scripted
//! session reads per block tag (optionally failing until a time), and a unix
//! clock on tokio's paused clock.

#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use ethers::types::{Address, H256, U256};
use fabstir_llm_node::ltx::final_proof::{send_relight_final, RelightFinal, SessionReads};
use fabstir_llm_node::ltx::keepalive::UnixClock;
use fabstir_llm_node::ltx::submit::{ProofReceipt, ProofSubmit, ProofSubmitError};
use fabstir_llm_node::training::accept::{SessionSnapshot, SessionStatus};
use tokio::time::Instant;

pub const BASE_UNIX: u64 = 1_790_000_000;
pub const TOKENS: u64 = 754_115; // Full 361
pub const JID: u64 = 42;

pub struct Clock(pub Instant);
impl UnixClock for Clock {
    fn now_unix(&self) -> u64 {
        BASE_UNIX + self.0.elapsed().as_secs()
    }
}

/// Scripted final submitter: typed outcomes per call; receipts per hash become
/// visible at a given offset from `t0`. `hang_submit` makes every submit
/// never resolve; `hang_receipts` every receipt read. `max_polls` panics the
/// test (fails it, never hangs it) once receipts were polled more often.
pub struct Final {
    pub t0: Instant,
    pub script: Mutex<VecDeque<Result<(), ProofSubmitError>>>,
    pub calls: Mutex<Vec<(u64, Duration)>>,
    pub receipts: HashMap<H256, (Duration, ProofReceipt)>,
    pub polls: AtomicU32,
    pub max_polls: u32,
    pub hang_submit: bool,
    pub hang_receipts: bool,
    /// `session_proof_interval` never resolves (round 2: the relight final must
    /// not read it).
    pub hang_interval: bool,
}

impl Final {
    pub fn new(t0: Instant, script: Vec<Result<(), ProofSubmitError>>) -> Self {
        Final {
            t0,
            script: Mutex::new(script.into()),
            calls: Mutex::new(Vec::new()),
            receipts: HashMap::new(),
            polls: AtomicU32::new(0),
            max_polls: u32::MAX,
            hang_submit: false,
            hang_receipts: false,
            hang_interval: false,
        }
    }
    pub fn receipt(mut self, h: H256, after: u64, success: bool, block: u64) -> Self {
        self.receipts.insert(
            h,
            (Duration::from_secs(after), ProofReceipt { success, block }),
        );
        self
    }
    pub fn calls(&self) -> Vec<(u64, Duration)> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl ProofSubmit for Final {
    async fn submit_ltx_proof(
        &self,
        _job_id: u64,
        tokens: u64,
        _h: [u8; 32],
        _c: String,
    ) -> Result<H256, ProofSubmitError> {
        self.calls.lock().unwrap().push((tokens, self.t0.elapsed()));
        if self.hang_submit {
            std::future::pending::<()>().await;
        }
        match self.script.lock().unwrap().pop_front() {
            Some(Err(e)) => Err(e),
            _ => Ok(H256::repeat_byte(0xF1)),
        }
    }
    async fn session_proof_interval(&self, _job_id: u64) -> u64 {
        if self.hang_interval {
            std::future::pending::<()>().await;
        }
        1000
    }
    async fn receipt_of(&self, tx: H256) -> Result<Option<ProofReceipt>, String> {
        let n = self.polls.fetch_add(1, Ordering::SeqCst) + 1;
        assert!(
            n <= self.max_polls,
            "receipt polled {n} times — past the budget"
        );
        if self.hang_receipts {
            std::future::pending::<()>().await;
        }
        Ok(self
            .receipts
            .get(&tx)
            .filter(|(after, _)| self.t0.elapsed() >= *after)
            .map(|(_, r)| *r))
    }
}

/// Scripted session reads: a snapshot per block tag (`None` = `latest`);
/// records every tag asked for; a tag in `fail_until` errors until that
/// offset from `t0`.
#[derive(Default)]
pub struct Reads {
    pub at: HashMap<Option<u64>, SessionSnapshot>,
    pub asked: Mutex<Vec<Option<u64>>>,
    pub fail_until: HashMap<Option<u64>, (Instant, Duration)>,
}

impl Reads {
    pub fn with(mut self, tag: Option<u64>, s: SessionSnapshot) -> Self {
        self.at.insert(tag, s);
        self
    }
    pub fn failing_until(mut self, tag: Option<u64>, t0: Instant, secs: u64) -> Self {
        self.fail_until.insert(tag, (t0, Duration::from_secs(secs)));
        self
    }
}

#[async_trait::async_trait]
impl SessionReads for Reads {
    async fn session_at(
        &self,
        _job_id: u64,
        block: Option<u64>,
    ) -> Result<SessionSnapshot, String> {
        self.asked.lock().unwrap().push(block);
        if let Some((t0, d)) = self.fail_until.get(&block) {
            if t0.elapsed() < *d {
                return Err("rpc down".to_string());
            }
        }
        self.at
            .get(&block)
            .cloned()
            .ok_or_else(|| "no such read".to_string())
    }
}

pub fn snap(used: u64, w9: u64) -> SessionSnapshot {
    SessionSnapshot {
        depositor: Address::repeat_byte(0xD1),
        attempt_address: Address::repeat_byte(0xD1),
        host: Address::repeat_byte(0xB0),
        payment_token: Address::repeat_byte(0xEC),
        // exactly the job: ceil(tokens × price ÷ 1000)
        deposit: (U256::from(TOKENS) * U256::from(3700u64) + U256::from(999u64))
            / U256::from(1000u64),
        price_per_token: U256::from(3700u64),
        tokens_used: U256::from(used),
        max_duration: U256::from(14_400u64),
        start_time: U256::from(BASE_UNIX - 7000),
        last_proof_time: U256::from(w9),
        proof_interval: U256::from(1000u64),
        proof_timeout_window: U256::from(3600u64),
        status: SessionStatus::Active,
    }
}

/// The finalising read: 1,100 keep-alive tokens on chain, w9 10 s ago.
pub const USED: u64 = 1_100;
pub const REMAINDER: u64 = TOKENS - USED;
pub const RW: u64 = 76; // ceil(752,915 ÷ 10,000)
pub const W9: u64 = BASE_UNIX - 10;

pub fn rf<'a>(reads: &'a Reads, clock: &'a Clock, budget: u64) -> RelightFinal<'a> {
    RelightFinal {
        reads,
        clock,
        rate_tokens_per_sec: 10_000,
        final_w9: W9,
        final_used: U256::from(USED),
        budget_end: clock.0 + Duration::from_secs(budget),
        first_attempt_extra: Duration::ZERO,
        proof_interval: 1000,
    }
}

pub async fn send(sub: &Final, rf: &RelightFinal<'_>) -> bool {
    send_relight_final(sub, JID, REMAINDER, [7u8; 32], "uProof".into(), rf).await
}
