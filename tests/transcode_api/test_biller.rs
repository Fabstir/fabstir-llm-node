// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! SCI-C D20: the transcode billing loop against a fake contract that applies
//! the real `submitProofOfWork` rules (Min tokens, Low first, Too many, Over dep,
//! Not active).

use anyhow::{anyhow, Result};
use ethers::types::H256;
use fabstir_llm_node::transcoder::biller::{
    FinishOutcome, PollOutcome, TranscodeBiller, TranscodeChain,
};
use std::sync::Mutex;

const JOB: u64 = 1433;
const RATE: u64 = 2000;

struct Contract {
    status: u8,
    deposit: u64,
    price: u64,
    tokens_used: u64,
    start: u64,
    max_duration: u64,
    last_proof_time: u64,
    proof_interval: u64,
    window: u64,
    now: u64,
    proofs: Vec<u64>,
    fail_next_proof: bool,
    /// Proofs that fail on submit (count down).
    failing_proofs: u32,
    /// Apply the proof on chain but report failure (a receipt wait that ran out).
    mined_but_unconfirmed: bool,
}

struct FakeChain(Mutex<Contract>);

impl FakeChain {
    fn new() -> Self {
        FakeChain(Mutex::new(Contract {
            status: 0,
            deposit: 500_000,
            price: 5_000,
            tokens_used: 0,
            start: 1_000_000,
            max_duration: 3_600,
            last_proof_time: 1_000_000,
            proof_interval: 1_000,
            window: 300,
            now: 1_000_000,
            proofs: Vec::new(),
            fail_next_proof: false,
            failing_proofs: 0,
            mined_but_unconfirmed: false,
        }))
    }
    fn at(&self, now: u64) {
        self.0.lock().unwrap().now = now;
    }
    fn with<R>(&self, f: impl FnOnce(&mut Contract) -> R) -> R {
        f(&mut self.0.lock().unwrap())
    }
}

fn word(v: u64) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&v.to_be_bytes());
    w
}

#[async_trait::async_trait]
impl TranscodeChain for FakeChain {
    async fn session_raw(&self, job_id: u64) -> Result<Vec<u8>> {
        let c = self.0.lock().unwrap();
        let words = [
            job_id,
            1,
            2,
            3,
            c.deposit,
            c.price,
            c.tokens_used,
            c.max_duration,
            c.start,
            c.last_proof_time,
            c.proof_interval,
            c.window,
            c.status as u64,
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
        Ok(RATE)
    }
    async fn put_attestation(&self, path: &str, _bytes: Vec<u8>) -> Result<String> {
        Ok(format!("cid:{path}"))
    }
    async fn submit_proof(
        &self,
        _job: u64,
        tokens: u64,
        _h: [u8; 32],
        _cid: String,
    ) -> Result<H256> {
        let mut c = self.0.lock().unwrap();
        if std::mem::take(&mut c.fail_next_proof) {
            return Err(anyhow!("reverted"));
        }
        if c.failing_proofs > 0 {
            c.failing_proofs -= 1;
            return Err(anyhow!("reverted"));
        }
        if c.status != 0 {
            return Err(anyhow!("Not active"));
        }
        if tokens < 100 {
            return Err(anyhow!("Min tokens"));
        }
        if c.tokens_used == 0 && tokens < c.proof_interval {
            return Err(anyhow!("Low first"));
        }
        if tokens > (c.now - c.last_proof_time) * RATE {
            return Err(anyhow!("Too many"));
        }
        if c.tokens_used + tokens > c.deposit * 1000 / c.price {
            return Err(anyhow!("Over dep"));
        }
        c.tokens_used += tokens;
        c.last_proof_time = c.now;
        c.proofs.push(tokens);
        if std::mem::take(&mut c.mined_but_unconfirmed) {
            return Err(anyhow!("no receipt within the wait"));
        }
        Ok(H256::zero())
    }
}

async fn progress(
    b: &mut TranscodeBiller,
    chain: &FakeChain,
    now: u64,
    pct: i32,
    est: u64,
) -> PollOutcome {
    chain.at(now);
    b.on_progress(chain, pct, Some(est), now).await
}

#[tokio::test]
async fn a_full_run_is_proved_in_steps_and_settles_to_the_unit() {
    let chain = FakeChain::new();
    let mut b = TranscodeBiller::new(JOB, "task".into());
    let est = 28_875;
    for (t, pct) in [
        (1_000_010, 10),
        (1_000_020, 25),
        (1_000_030, 50),
        (1_000_040, 75),
        (1_000_045, 90),
    ] {
        assert_eq!(
            progress(&mut b, &chain, t, pct, est).await,
            PollOutcome::Continue
        );
    }
    chain.at(1_000_050);
    assert_eq!(
        b.finish(&chain, est).await,
        FinishOutcome::Paid { proven: est }
    );
    chain.with(|c| {
        assert_eq!(c.tokens_used, est, "on chain, exactly the bill");
        assert!(c.proofs.len() >= 4, "proved in steps: {:?}", c.proofs);
        assert!(c.proofs[0] >= 1_000, "first proof met proofInterval");
    });
}

#[tokio::test]
async fn interim_proofs_keep_the_session_from_timing_out_unpaid() {
    let chain = FakeChain::new();
    let mut b = TranscodeBiller::new(JOB, "task".into());
    // A long job: the last proof must never be older than the window.
    let est = 90_000;
    let mut t = 1_000_000;
    for pct in (5..=95).step_by(5) {
        t += 60;
        progress(&mut b, &chain, t, pct, est).await;
        chain.with(|c| assert!(t - c.last_proof_time <= c.window, "exposed at {pct}%"));
    }
}

#[tokio::test]
async fn a_session_ended_mid_run_stops_the_job() {
    let chain = FakeChain::new();
    let mut b = TranscodeBiller::new(JOB, "task".into());
    progress(&mut b, &chain, 1_000_020, 25, 28_875).await;
    chain.with(|c| c.status = 1); // the depositor completed it
    assert!(matches!(
        progress(&mut b, &chain, 1_000_045, 40, 28_875).await,
        PollOutcome::Stop(_)
    ));
}

#[tokio::test]
async fn an_estimate_beyond_the_deposit_still_runs_and_bills_the_deposit() {
    let chain = FakeChain::new();
    let mut b = TranscodeBiller::new(JOB, "task".into());
    assert_eq!(
        progress(&mut b, &chain, 1_000_010, 5, 250_000).await,
        PollOutcome::Continue
    );
}

#[tokio::test]
async fn an_estimate_the_rate_cannot_carry_before_expiry_stops_the_job() {
    let chain = FakeChain::new();
    chain.with(|c| c.deposit = 500_000_000);
    let mut b = TranscodeBiller::new(JOB, "task".into());
    assert!(matches!(
        progress(&mut b, &chain, 1_003_599, 5, 50_000_000).await,
        PollOutcome::Stop(_)
    ));
}

#[tokio::test(start_paused = true)]
async fn a_final_claim_that_keeps_failing_withholds_the_result() {
    let chain = FakeChain::new();
    let mut b = TranscodeBiller::new(JOB, "task".into());
    chain.at(1_000_050);
    chain.with(|c| c.failing_proofs = 10);
    assert!(matches!(
        b.finish(&chain, 28_875).await,
        FinishOutcome::Unpaid(_)
    ));
}

#[tokio::test(start_paused = true)]
async fn one_transient_failure_of_the_final_claim_is_retried() {
    let chain = FakeChain::new();
    let mut b = TranscodeBiller::new(JOB, "task".into());
    chain.at(1_000_050);
    chain.with(|c| c.failing_proofs = 1);
    assert_eq!(
        b.finish(&chain, 28_875).await,
        FinishOutcome::Paid { proven: 28_875 }
    );
}

#[tokio::test(start_paused = true)]
async fn a_proof_that_mined_after_its_wait_is_never_claimed_twice() {
    let chain = FakeChain::new();
    let mut b = TranscodeBiller::new(JOB, "task".into());
    progress(&mut b, &chain, 1_000_005, 1, 28_875).await; // first read fixes the base
    chain.at(1_000_050);
    chain.with(|c| c.mined_but_unconfirmed = true);
    assert_eq!(
        b.finish(&chain, 28_875).await,
        FinishOutcome::Paid { proven: 28_875 }
    );
    chain.with(|c| assert_eq!(c.tokens_used, 28_875, "billed once: {:?}", c.proofs));
}

#[tokio::test]
async fn a_shrinking_estimate_never_over_claims() {
    let chain = FakeChain::new();
    let mut b = TranscodeBiller::new(JOB, "task".into());
    progress(&mut b, &chain, 1_000_010, 10, 20_000).await;
    progress(&mut b, &chain, 1_000_030, 50, 40_000).await; // a larger estimate later
    chain.with(|c| assert!(c.tokens_used <= 20_000 / 2, "{:?}", c.proofs));
}

#[tokio::test]
async fn a_small_job_bills_the_first_proof_minimum() {
    let chain = FakeChain::new();
    let mut b = TranscodeBiller::new(JOB, "task".into());
    chain.at(1_000_050);
    assert_eq!(
        b.finish(&chain, 300).await,
        FinishOutcome::Paid { proven: 1_000 }
    );
}

#[tokio::test]
async fn a_deposit_capped_job_bills_the_deposit_and_releases() {
    let chain = FakeChain::new();
    chain.with(|c| c.deposit = 50_000); // room for 10,000 tokens
    let mut b = TranscodeBiller::new(JOB, "task".into());
    chain.at(1_000_050);
    assert_eq!(
        b.finish(&chain, 28_875).await,
        FinishOutcome::Paid { proven: 10_000 }
    );
}

#[tokio::test(start_paused = true)]
async fn the_final_claim_waits_for_the_rate_cap() {
    let chain = FakeChain::new();
    let mut b = TranscodeBiller::new(JOB, "task".into());
    chain.at(1_000_001); // 1 s since start: room for 2,000 now
                         // Advance chain time as tokio time passes while the claim waits.
    let chain = std::sync::Arc::new(chain);
    let ticker = {
        let chain = chain.clone();
        tokio::spawn(async move {
            for s in 2..200u64 {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                chain.at(1_000_000 + s);
            }
        })
    };
    let outcome = b.finish(chain.as_ref(), 28_875).await;
    ticker.abort();
    assert_eq!(outcome, FinishOutcome::Paid { proven: 28_875 });
    chain.with(|c| assert!(c.proofs.len() >= 2, "split by the rate cap: {:?}", c.proofs));
}
