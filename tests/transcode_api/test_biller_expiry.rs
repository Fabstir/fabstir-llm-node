// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! SCI-C D19(b): work that finishes after the session's expiry is not billed;
//! steps proven before expiry stay paid. Judged once, on chain time, when the
//! final claim first reads the session.

use anyhow::{anyhow, Result};
use ethers::types::H256;
use fabstir_llm_node::training::accept::SessionStatus;
use fabstir_llm_node::transcoder::biller::{
    FinishOutcome, PollOutcome, TranscodeBiller, TranscodeChain,
};
use fabstir_llm_node::transcoder::proof_steps::{finished_after_expiry, SessionView};
use std::sync::Mutex;

const JOB: u64 = 1433;
const START: u64 = 1_000_000;
const MAX_DURATION: u64 = 3_600;

struct Chain {
    now: u64,
    tokens_used: u64,
    last_proof_time: u64,
    proofs: Vec<u64>,
}

struct Fake(Mutex<Chain>);

fn word(v: u64) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&v.to_be_bytes());
    w
}

#[async_trait::async_trait]
impl TranscodeChain for Fake {
    async fn session_raw(&self, job_id: u64) -> Result<Vec<u8>> {
        let c = self.0.lock().unwrap();
        let words = [
            job_id,
            1,
            2,
            3,
            500_000,
            5_000,
            c.tokens_used,
            MAX_DURATION,
            START,
            c.last_proof_time,
            1_000,
            300,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        Ok(words.iter().flat_map(|w| word(*w)).collect())
    }
    async fn chain_now(&self) -> Result<u64> {
        Ok(self.0.lock().unwrap().now)
    }
    async fn rate_limit(&self, _job_id: u64) -> Result<u64> {
        Ok(2_000)
    }
    async fn put_attestation(&self, path: &str, _bytes: Vec<u8>) -> Result<String> {
        Ok(format!("cid:{path}"))
    }
    async fn submit_proof(&self, _j: u64, tokens: u64, _h: [u8; 32], _c: String) -> Result<H256> {
        let mut c = self.0.lock().unwrap();
        if tokens > (c.now - c.last_proof_time) * 2_000 {
            return Err(anyhow!("Too many"));
        }
        c.tokens_used += tokens;
        c.last_proof_time = c.now;
        c.proofs.push(tokens);
        Ok(H256::zero())
    }
}

fn fake() -> Fake {
    Fake(Mutex::new(Chain {
        now: START,
        tokens_used: 0,
        last_proof_time: START,
        proofs: Vec::new(),
    }))
}

#[test]
fn expiry_is_strictly_after_start_plus_max_duration() {
    let view = SessionView {
        status: SessionStatus::Active,
        deposit: 500_000,
        price_per_token: 5_000,
        tokens_used: 0,
        max_duration: MAX_DURATION,
        start_time: START,
        last_proof_time: START,
        proof_interval: 1_000,
        proof_timeout_window: 300,
    };
    assert!(!finished_after_expiry(&view, START + MAX_DURATION));
    assert!(finished_after_expiry(&view, START + MAX_DURATION + 1));
}

#[tokio::test]
async fn work_finished_after_expiry_is_not_billed_but_earlier_steps_stay_paid() {
    let chain = fake();
    let mut b = TranscodeBiller::new(JOB, "task".into());
    // An interim step while the session is live.
    chain.0.lock().unwrap().now = START + 10;
    assert_eq!(
        b.on_progress(&chain, 30, Some(10_000), START + 10).await,
        PollOutcome::Continue
    );
    let before = chain.0.lock().unwrap().proofs.clone();
    assert!(!before.is_empty(), "an interim step was proven");
    // The sidecar reports done one second after expiry.
    chain.0.lock().unwrap().now = START + MAX_DURATION + 1;
    let outcome = b.finish(&chain, 10_000).await;
    assert_eq!(
        outcome,
        FinishOutcome::Expired {
            proven: before.iter().sum()
        }
    );
    assert_eq!(
        chain.0.lock().unwrap().proofs,
        before,
        "no proof after expiry"
    );
}

#[tokio::test]
async fn work_finished_at_the_last_second_is_billed() {
    let chain = fake();
    let mut b = TranscodeBiller::new(JOB, "task".into());
    chain.0.lock().unwrap().now = START + MAX_DURATION;
    assert_eq!(
        b.finish(&chain, 10_000).await,
        FinishOutcome::Paid { proven: 10_000 }
    );
}

#[tokio::test]
async fn a_run_still_going_after_expiry_proves_nothing_more_and_stops() {
    let chain = fake();
    let mut b = TranscodeBiller::new(JOB, "task".into());
    chain.0.lock().unwrap().now = START + 10;
    assert_eq!(
        b.on_progress(&chain, 30, Some(10_000), START + 10).await,
        PollOutcome::Continue
    );
    let before = chain.0.lock().unwrap().proofs.clone();
    // 25 % further on, past expiry: a step would be due, but nothing is proved.
    chain.0.lock().unwrap().now = START + MAX_DURATION + 30;
    assert_eq!(
        b.on_progress(&chain, 60, Some(10_000), START + MAX_DURATION + 30)
            .await,
        PollOutcome::Expired
    );
    assert_eq!(chain.0.lock().unwrap().proofs, before);
}
