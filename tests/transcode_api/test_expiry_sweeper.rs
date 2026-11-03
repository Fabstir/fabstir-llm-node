// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! SCI-C D17: the expiry sweeper completes a session nobody else will, once the
//! CHAIN's clock is past expiry plus margin and the dispute window, with no
//! work in flight; never on the node's clock, never on proof silence alone.

use anyhow::{anyhow, Result};
use ethers::types::Address;
use fabstir_llm_node::contracts::expiry_sweeper::{
    sweep_decision, ExpirySweeper, SweepConfig, SweepDecision, SweepFacts, SweepTarget,
};
use fabstir_llm_node::training::accept::SessionStatus;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const START: u64 = 1_000_000;
const MAX: u64 = 3_600;
const EXPIRY: u64 = START + MAX;
const MARGIN: u64 = 300;
const WINDOW: u64 = 600;
const CAP: u64 = 1_800;

fn facts(chain_now: u64) -> SweepFacts {
    SweepFacts {
        status: SessionStatus::Active,
        chain_now,
        start_time: START,
        max_duration: MAX,
        last_proof_time: START + 100,
        dispute_window: WINDOW,
        margin: MARGIN,
        work_in_flight: false,
        work_cap: CAP,
        owned_elsewhere: false,
        settled: false,
        foreign: false,
    }
}

#[test]
fn completes_only_past_expiry_plus_margin() {
    assert!(matches!(
        sweep_decision(&facts(EXPIRY + MARGIN)),
        SweepDecision::Skip(_)
    ));
    assert_eq!(
        sweep_decision(&facts(EXPIRY + MARGIN + 1)),
        SweepDecision::Complete
    );
    // Just past expiry, inside the margin: not yet.
    assert!(matches!(
        sweep_decision(&facts(EXPIRY + 1)),
        SweepDecision::Skip(_)
    ));
}

#[test]
fn a_proof_just_before_expiry_restarts_the_wait() {
    let mut f = facts(EXPIRY + MARGIN + 1);
    f.last_proof_time = EXPIRY - 10; // dispute window runs to EXPIRY + 590
    assert!(matches!(sweep_decision(&f), SweepDecision::Skip(_)));
    f.chain_now = EXPIRY - 10 + WINDOW + 1;
    assert_eq!(sweep_decision(&f), SweepDecision::Complete);
}

#[test]
fn proof_silence_alone_is_not_a_reason() {
    // Long past the proof-timeout window (what triggerSessionTimeout keys on),
    // well before expiry: the sweeper leaves it alone.
    let mut f = facts(START + 2_000);
    f.last_proof_time = START + 10;
    assert!(matches!(sweep_decision(&f), SweepDecision::Skip(_)));
}

#[test]
fn settled_and_ended_sessions_are_dropped() {
    let mut f = facts(EXPIRY + 10_000);
    f.settled = true;
    assert_eq!(sweep_decision(&f), SweepDecision::Drop);
    for status in [SessionStatus::Completed, SessionStatus::TimedOut] {
        let mut f = facts(START);
        f.status = status;
        assert_eq!(sweep_decision(&f), SweepDecision::Drop);
    }
}

#[test]
fn running_work_is_waited_on_up_to_its_timeout() {
    let mut f = facts(EXPIRY + MARGIN + 1);
    f.work_in_flight = true;
    assert!(matches!(sweep_decision(&f), SweepDecision::Skip(_)));
    f.chain_now = EXPIRY + MARGIN + CAP;
    assert!(matches!(sweep_decision(&f), SweepDecision::Skip(_)));
    f.chain_now = EXPIRY + MARGIN + CAP + 1;
    assert_eq!(sweep_decision(&f), SweepDecision::Complete);
}

#[test]
fn another_hosts_session_is_never_ours() {
    let mut f = facts(EXPIRY + 100_000);
    f.foreign = true;
    assert_eq!(sweep_decision(&f), SweepDecision::Drop);
}

#[test]
fn a_proof_in_flight_elsewhere_owns_completion() {
    let mut f = facts(EXPIRY + 100_000);
    f.owned_elsewhere = true;
    assert!(matches!(sweep_decision(&f), SweepDecision::Skip(_)));
}

// ── a tick against a fake chain ──────────────────────────────────────────

const OUR_HOST: u64 = 3;

struct Session {
    status: u64,
    start: u64,
    last_proof: u64,
    host: u64,
}

struct Fake {
    now: u64,
    sessions: HashMap<u64, Session>,
    settled: HashSet<u64>,
    work: Mutex<HashSet<u64>>,
    elsewhere: Mutex<HashSet<u64>>,
    claim_refused: Mutex<HashSet<u64>>,
    completed: Mutex<Vec<u64>>,
    reads: Mutex<Vec<u64>>,
    fail_complete: AtomicBool,
    hang_complete: AtomicBool,
    hang_chain: AtomicBool,
}

fn word(v: u64) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&v.to_be_bytes());
    w
}

#[async_trait::async_trait]
impl SweepTarget for Fake {
    async fn registry(&self) -> Vec<u64> {
        let mut jobs: Vec<u64> = self.sessions.keys().copied().collect();
        jobs.push(9_999); // a job the chain does not know
        jobs
    }
    async fn session_raw(&self, job_id: u64) -> Result<Vec<u8>> {
        self.reads.lock().unwrap().push(job_id);
        let Some(s) = self.sessions.get(&job_id) else {
            return Ok(vec![0u8; 18 * 32]);
        };
        let words = [
            job_id,
            1,
            s.host,
            3,
            500_000,
            5_000,
            100,
            MAX,
            s.start,
            s.last_proof,
            1_000,
            300,
            s.status,
            0,
            0,
            0,
            0,
            0,
        ];
        Ok(words.iter().flat_map(|w| word(*w)).collect())
    }
    async fn chain_now(&self) -> Result<u64> {
        if self.hang_chain.load(Ordering::SeqCst) {
            futures::future::pending::<()>().await;
        }
        Ok(self.now)
    }
    async fn dispute_window_secs(&self) -> u64 {
        WINDOW
    }
    fn host_address(&self) -> Address {
        Address::from_low_u64_be(OUR_HOST)
    }
    async fn is_settled(&self, job_id: u64) -> bool {
        self.settled.contains(&job_id)
    }
    fn work_in_flight(&self, job_id: u64) -> bool {
        self.work.lock().unwrap().contains(&job_id)
    }
    async fn owned_elsewhere(&self, job_id: u64) -> bool {
        self.elsewhere.lock().unwrap().contains(&job_id)
    }
    async fn claim(&self, job_id: u64) -> bool {
        !self.claim_refused.lock().unwrap().contains(&job_id)
    }
    async fn complete(&self, job_id: u64) -> Result<()> {
        if self.hang_complete.load(Ordering::SeqCst) {
            futures::future::pending::<()>().await;
        }
        if self.fail_complete.load(Ordering::SeqCst) {
            return Err(anyhow!("send failed"));
        }
        self.completed.lock().unwrap().push(job_id);
        Ok(())
    }
}

fn config() -> SweepConfig {
    SweepConfig {
        interval: Duration::from_secs(300),
        margin: MARGIN,
        work_cap: CAP,
    }
}

fn fake(now: u64) -> Fake {
    let mut sessions = HashMap::new();
    let ours = |status, start, last_proof| Session {
        status,
        start,
        last_proof,
        host: OUR_HOST,
    };
    // 1433: expired, proven work, nobody completing it.
    sessions.insert(1433, ours(0, START, START + 100));
    // 1500: still within its lifetime.
    sessions.insert(1500, ours(0, now - 100, now - 50));
    // 1501: already completed on chain.
    sessions.insert(1501, ours(1, START, START));
    // 1502: expired, but settled by this node already.
    sessions.insert(1502, ours(0, START, START));
    // 1503: expired, but another host's session (or another chain's id).
    sessions.insert(
        1503,
        Session {
            status: 0,
            start: START,
            last_proof: START,
            host: 77,
        },
    );
    Fake {
        now,
        sessions,
        settled: [1502].into_iter().collect(),
        work: Mutex::new(HashSet::new()),
        elsewhere: Mutex::new(HashSet::new()),
        claim_refused: Mutex::new(HashSet::new()),
        completed: Mutex::new(Vec::new()),
        reads: Mutex::new(Vec::new()),
        fail_complete: AtomicBool::new(false),
        hang_complete: AtomicBool::new(false),
        hang_chain: AtomicBool::new(false),
    }
}

/// Let spawned completions run.
async fn settle() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn a_tick_completes_the_stranded_session_on_chain_time_only() {
    // Chain time is past 1433's expiry; the node's own clock (years later)
    // would make 1500 look expired too, and must not be used.
    let fake = Arc::new(fake(EXPIRY + MARGIN + WINDOW));
    let target: Arc<dyn SweepTarget> = fake.clone();
    let mut sweeper = ExpirySweeper::new(config());
    let report = sweeper.tick(&target).await;
    settle().await;
    assert_eq!(report.dispatched, vec![1433]);
    assert_eq!(*fake.completed.lock().unwrap(), vec![1433]);
    for j in [1501, 1502, 1503] {
        assert!(report.dropped.contains(&j), "{j} dropped: {report:?}");
    }
    assert_eq!(report.unreadable, vec![9_999], "drift canary: unknown job");
    assert!(
        !fake.reads.lock().unwrap().contains(&1502),
        "settled: no read"
    );
    // Dropped jobs are not read again on the next tick.
    fake.reads.lock().unwrap().clear();
    sweeper.tick(&target).await;
    let reads = fake.reads.lock().unwrap().clone();
    for j in [1501, 1502, 1503] {
        assert!(!reads.contains(&j));
    }
}

#[tokio::test]
async fn before_the_chain_says_so_nothing_is_completed() {
    let fake = Arc::new(fake(EXPIRY + MARGIN));
    let target: Arc<dyn SweepTarget> = fake.clone();
    let report = ExpirySweeper::new(config()).tick(&target).await;
    settle().await;
    assert!(report.dispatched.is_empty());
    assert!(fake.completed.lock().unwrap().is_empty());
}

#[tokio::test]
async fn work_in_flight_proofs_elsewhere_and_a_refused_claim_hold_it_off() {
    let fake = Arc::new(fake(EXPIRY + MARGIN + WINDOW));
    let target: Arc<dyn SweepTarget> = fake.clone();
    fake.work.lock().unwrap().insert(1433);
    assert!(ExpirySweeper::new(config())
        .tick(&target)
        .await
        .dispatched
        .is_empty());
    fake.work.lock().unwrap().clear();
    fake.elsewhere.lock().unwrap().insert(1433);
    assert!(ExpirySweeper::new(config())
        .tick(&target)
        .await
        .dispatched
        .is_empty());
    fake.elsewhere.lock().unwrap().clear();
    fake.claim_refused.lock().unwrap().insert(1433);
    assert!(ExpirySweeper::new(config())
        .tick(&target)
        .await
        .dispatched
        .is_empty());
    settle().await;
    assert!(fake.completed.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_failed_completion_backs_off_then_retries() {
    let fake = Arc::new(fake(EXPIRY + MARGIN + WINDOW));
    let target: Arc<dyn SweepTarget> = fake.clone();
    fake.fail_complete.store(true, Ordering::SeqCst);
    let mut sweeper = ExpirySweeper::new(config());
    assert_eq!(sweeper.tick(&target).await.dispatched, vec![1433]);
    settle().await;
    // One failure: wait one interval.
    assert_eq!(sweeper.tick(&target).await.backing_off, vec![1433]);
    tokio::time::advance(Duration::from_secs(300)).await;
    assert_eq!(sweeper.tick(&target).await.dispatched, vec![1433]);
    settle().await;
    // Two failures: two intervals.
    tokio::time::advance(Duration::from_secs(300)).await;
    assert_eq!(sweeper.tick(&target).await.backing_off, vec![1433]);
    fake.fail_complete.store(false, Ordering::SeqCst);
    tokio::time::advance(Duration::from_secs(300)).await;
    assert_eq!(sweeper.tick(&target).await.dispatched, vec![1433]);
    settle().await;
    assert_eq!(*fake.completed.lock().unwrap(), vec![1433]);
}

#[tokio::test]
async fn a_long_completion_never_holds_up_the_tick_or_repeats() {
    let mut f = fake(EXPIRY + MARGIN + WINDOW);
    f.sessions.insert(
        1434,
        Session {
            status: 0,
            start: START,
            last_proof: START,
            host: OUR_HOST,
        },
    );
    let fake = Arc::new(f);
    let target: Arc<dyn SweepTarget> = fake.clone();
    fake.hang_complete.store(true, Ordering::SeqCst);
    let mut sweeper = ExpirySweeper::new(config());
    let first = tokio::time::timeout(Duration::from_secs(5), sweeper.tick(&target))
        .await
        .expect("the tick returns while completions hang");
    assert_eq!(first.dispatched, vec![1433, 1434]);
    let second = sweeper.tick(&target).await;
    assert!(second.dispatched.is_empty());
    assert_eq!(second.in_flight, vec![1433, 1434]);
}

#[tokio::test(start_paused = true)]
async fn a_hung_chain_read_skips_the_tick() {
    let fake = Arc::new(fake(EXPIRY + MARGIN + WINDOW));
    let target: Arc<dyn SweepTarget> = fake.clone();
    fake.hang_chain.store(true, Ordering::SeqCst);
    let report = ExpirySweeper::new(config()).tick(&target).await;
    assert!(report.no_chain_time);
    assert!(report.dispatched.is_empty());
}
