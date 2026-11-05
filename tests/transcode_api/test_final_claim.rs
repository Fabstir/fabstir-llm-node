// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! 8.62.2: the final claim of a session transcode.
//!
//! Live evidence (host2, 2026-11-05, session 1467): a 2-minute 1080p job left
//! 68,000 tokens for the final claim at a model rate of 2,000 tokens/s. The
//! biller claimed as soon as anything was claimable, so each proof carried only
//! the ~1 s that passed while the previous one mined: 34 proofs over 84 s. The
//! same claim fits in ONE proof after 34 s. And the fixed 120 s wait meant any
//! job owing more than the rate could reach in 120 s ended BILLING_FAILED with
//! its result withheld (about a 3-minute 1080p video and up).
//! Session 1469: a session the depositor completed during the final claim told
//! the client BILLING_FAILED; API.md says SESSION_NOT_BILLABLE.
use anyhow::{anyhow, Result};
use ethers::types::H256;
use fabstir_llm_node::transcoder::biller::{FinishOutcome, TranscodeBiller, TranscodeChain};
use std::sync::{Arc, Mutex};

const JOB: u64 = 1467;
const RATE: u64 = 2000;
const T0: u64 = 1_000_000;

struct Contract {
    status: u8,
    deposit: u64,
    price: u64,
    tokens_used: u64,
    last_proof_time: u64,
    window: u64,
    now: u64,
    /// (chain time, tokens) of every proof that landed.
    proofs: Vec<(u64, u64)>,
}

/// The real `submitProofOfWork` rules that matter here: Not active, Min tokens,
/// Low first (interval 1,000), Too many (rate x time since the last proof),
/// Over dep.
struct FakeChain(Mutex<Contract>);

impl FakeChain {
    fn new(deposit: u64, window: u64) -> Arc<Self> {
        Arc::new(FakeChain(Mutex::new(Contract {
            status: 0,
            deposit,
            price: 5_000,
            tokens_used: 0,
            last_proof_time: T0,
            window,
            now: T0 + 1,
            proofs: Vec::new(),
        })))
    }
    fn with<R>(&self, f: impl FnOnce(&mut Contract) -> R) -> R {
        f(&mut self.0.lock().unwrap())
    }
    /// Chain time follows tokio's paused clock: one chain second per second.
    fn tick(self: &Arc<Self>, secs: u64) -> tokio::task::JoinHandle<()> {
        let chain = self.clone();
        tokio::spawn(async move {
            for s in 2..secs {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                chain.with(|c| c.now = T0 + s);
            }
        })
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
            3_600,
            T0,
            c.last_proof_time,
            1_000,
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
    async fn submit_proof(&self, _j: u64, tokens: u64, _h: [u8; 32], _c: String) -> Result<H256> {
        let mut c = self.0.lock().unwrap();
        if c.status != 0 {
            return Err(anyhow!("Not active"));
        }
        if tokens < 100 || (c.tokens_used == 0 && tokens < 1_000) {
            return Err(anyhow!("Min tokens"));
        }
        if tokens > (c.now - c.last_proof_time) * RATE {
            return Err(anyhow!("Too many"));
        }
        if c.tokens_used + tokens > c.deposit * 1000 / c.price {
            return Err(anyhow!("Over dep"));
        }
        c.tokens_used += tokens;
        c.last_proof_time = c.now;
        let now = c.now;
        c.proofs.push((now, tokens));
        Ok(H256::zero())
    }
}

/// The 1467 shape: a claim the rate covers within half the proof window lands
/// as ONE proof, not one per second of accrual.
#[tokio::test(start_paused = true)]
async fn a_claim_the_rate_covers_soon_lands_as_one_proof() {
    let chain = FakeChain::new(500_000, 300);
    let ticker = chain.tick(400);
    let mut b = TranscodeBiller::new(JOB, "task".into());
    let outcome = b.finish(chain.as_ref(), 68_000).await;
    ticker.abort();
    assert_eq!(outcome, FinishOutcome::Paid { proven: 68_000 });
    chain.with(|c| {
        assert_eq!(
            c.proofs.len(),
            1,
            "one proof, not a trickle: {:?}",
            c.proofs
        );
        assert_eq!(c.tokens_used, 68_000);
    })
}

/// A job owing more than the rate reaches in 120 s used to end Unpaid (result
/// withheld). Now the wait scales with what is owed.
#[tokio::test(start_paused = true)]
async fn a_long_jobs_final_claim_is_paid_in_full() {
    let chain = FakeChain::new(5_000_000, 300); // room for 1,000,000 tokens
    let ticker = chain.tick(900);
    let mut b = TranscodeBiller::new(JOB, "task".into());
    let owed = 600_000; // 300 s of rate: a ~9-minute 1080p video's tail
    let outcome = b.finish(chain.as_ref(), owed).await;
    ticker.abort();
    assert_eq!(outcome, FinishOutcome::Paid { proven: owed });
    chain.with(|c| assert!(c.proofs.len() <= 4, "few proofs: {:?}", c.proofs));
}

/// Batching must never let the proof window lapse, or a third party could
/// triggerSessionTimeout and cut the unproven tail: proofs stay less than half
/// a window apart (from the session's last proof too).
#[tokio::test(start_paused = true)]
async fn batching_never_lets_the_proof_window_lapse() {
    for window in [60u64, 300] {
        let chain = FakeChain::new(5_000_000, window);
        let ticker = chain.tick(900);
        let mut b = TranscodeBiller::new(JOB, "task".into());
        let outcome = b.finish(chain.as_ref(), 400_000).await;
        ticker.abort();
        assert_eq!(
            outcome,
            FinishOutcome::Paid { proven: 400_000 },
            "window {window}"
        );
        chain.with(|c| {
            let mut last = T0;
            for (t, _) in &c.proofs {
                assert!(
                    t - last < window / 2,
                    "window {window}: gap {} in {:?}",
                    t - last,
                    c.proofs
                );
                last = *t;
            }
        });
    }
}

/// Session 1469: completed by its depositor during the final claim. The steps
/// already proven stay paid; the outcome must say so, not "billing failed".
#[tokio::test(start_paused = true)]
async fn a_session_completed_by_someone_else_is_not_billable() {
    let chain = FakeChain::new(500_000, 300);
    chain.with(|c| c.status = 1);
    let mut b = TranscodeBiller::new(JOB, "task".into());
    assert!(
        matches!(
            b.finish(chain.as_ref(), 68_000).await,
            FinishOutcome::NotBillable(_)
        ),
        "a closed session is not billable, not a failed claim"
    );
}

/// A claim that keeps failing for another reason is still Unpaid (unchanged).
#[tokio::test(start_paused = true)]
async fn a_claim_on_a_live_session_that_cannot_land_is_still_unpaid() {
    let chain = FakeChain::new(500_000, 300);
    chain.with(|c| c.price = 0); // no deposit room: nothing claimable ever
    let mut b = TranscodeBiller::new(JOB, "task".into());
    let outcome = b.finish(chain.as_ref(), 68_000).await;
    assert!(
        !matches!(outcome, FinishOutcome::NotBillable(_)),
        "{outcome:?}"
    );
}
