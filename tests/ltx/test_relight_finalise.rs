// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// VFX Passes (VP1.1) D3 + D18: the finalising decision (deadline, session still Active), the delivery choice after the
// proof submit (`completion_outcome`) and the per-frame S5 upload retry. Each test names the mutation that turns it red.

use async_trait::async_trait;
use ethers::types::{Address, U256};
use fabstir_llm_node::api::websocket::handlers::ltx::{completion_outcome, CompletionOutcome};
use fabstir_llm_node::ltx::relight::{relight_finalise_decision, FinaliseDecision};
use fabstir_llm_node::ltx::submit::{upload_frame_with_retry, RELIGHT_UPLOAD_ATTEMPTS};
use fabstir_llm_node::ltx::types::{FrameManifest, Resolution};
use fabstir_llm_node::storage::s5_client::{
    MockS5Backend, S5Entry, S5ListResult, S5Storage, StorageError,
};
use fabstir_llm_node::training::accept::{SessionSnapshot, SessionStatus};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn finalise_decision_deadline_then_terms() {
    let d = Duration::from_secs(2700);
    assert_eq!(
        relight_finalise_decision(Duration::from_secs(10), d, Ok(())),
        FinaliseDecision::Proceed
    );
    // no deadline check -> Proceed -> red
    match relight_finalise_decision(Duration::from_secs(2701), d, Ok(())) {
        FinaliseDecision::Abandon(m) => assert!(m.starts_with("DEADLINE:"), "{m}"),
        p => panic!("{p:?}"),
    }
    // Err -> Proceed -> red
    match relight_finalise_decision(
        Duration::from_secs(10),
        d,
        Err("the session is not Active (TimedOut)".into()),
    ) {
        FinaliseDecision::Abandon(m) => assert!(m.starts_with("SESSION_CLOSED:"), "{m}"),
        p => panic!("{p:?}"),
    }
}

fn snap(status: SessionStatus, used: u64) -> SessionSnapshot {
    SessionSnapshot {
        depositor: Address::zero(),
        attempt_address: Address::zero(),
        host: Address::repeat_byte(1),
        payment_token: Address::zero(),
        deposit: U256::from(1_000_000u64),
        price_per_token: U256::from(1500u64),
        tokens_used: U256::from(used),
        max_duration: U256::zero(),
        start_time: U256::zero(),
        proof_timeout_window: U256::from(300u64),
        status,
    }
}

fn manifest() -> FrameManifest {
    FrameManifest {
        frame_count: 1,
        fps: 25,
        resolution: Resolution { w: 1920, h: 1088 },
        colour_encoding: "vfx-passes-v1".into(),
        frame_hashes: vec!["0x01".into()],
        merkle_root: "0x01".into(),
    }
}

const BEFORE: u64 = 5000;
const TOKENS: u64 = 302_900;

fn outcome(
    sidecar: Option<&str>,
    finalize: Result<(String, bool), String>,
    reread: Option<Result<SessionSnapshot, String>>,
) -> CompletionOutcome {
    let caps = vec!["cap-1".to_string()];
    completion_outcome(
        sidecar,
        finalize,
        reread,
        U256::from(BEFORE),
        TOKENS,
        "out",
        &caps,
        &manifest(),
        "1500",
        Some("r"),
    )
}

fn is_complete(o: &CompletionOutcome) -> bool {
    match o {
        CompletionOutcome::Complete(v) => {
            assert_eq!(v["type"], "ltx_complete");
            assert_eq!(v["frames"][0], "cap-1");
            assert_eq!(v["proofCID"], "proof");
            true
        }
        CompletionOutcome::Fail(..) => false,
    }
}

fn closed_fail(o: &CompletionOutcome) {
    match o {
        CompletionOutcome::Fail(code, msg) => {
            assert_eq!(*code, "GENERATION_FAILED");
            assert!(msg.starts_with("SESSION_CLOSED:"), "{msg}");
            assert!(!msg.contains("cap-1"), "no capabilities leave the node");
        }
        c => panic!("withheld expected: {c:?}"),
    }
}

const R: Option<&str> = Some("relight");
fn ok(submitted: bool) -> Result<(String, bool), String> {
    Ok(("proof".into(), submitted))
}

#[test]
fn relight_submitted_delivers() {
    assert!(is_complete(&outcome(R, ok(true), None)));
}

#[test]
fn relight_unsubmitted_closed_and_unpaid_is_withheld() {
    // deliver on every false -> red
    closed_fail(&outcome(
        R,
        ok(false),
        Some(Ok(snap(SessionStatus::TimedOut, BEFORE))),
    ));
}

#[test]
fn relight_unsubmitted_but_active_delivers() {
    assert!(is_complete(&outcome(
        R,
        ok(false),
        Some(Ok(snap(SessionStatus::Active, BEFORE)))
    )));
}

#[test]
fn relight_unsubmitted_closed_but_paid_delivers() {
    // withhold on every false -> red
    assert!(is_complete(&outcome(
        R,
        ok(false),
        Some(Ok(snap(SessionStatus::Completed, BEFORE + TOKENS + 7)))
    )));
}

#[test]
fn relight_unsubmitted_reread_error_delivers() {
    assert!(is_complete(&outcome(
        R,
        ok(false),
        Some(Err("rpc down".into()))
    )));
}

#[test]
fn relight_unsubmitted_never_reread_is_withheld() {
    closed_fail(&outcome(R, ok(false), None));
}

#[test]
fn growth_by_exactly_the_job_counts_and_one_less_does_not() {
    // `>` instead of `>=` -> red; any growth counts -> red
    assert!(is_complete(&outcome(
        R,
        ok(false),
        Some(Ok(snap(SessionStatus::TimedOut, BEFORE + TOKENS)))
    )));
    closed_fail(&outcome(
        R,
        ok(false),
        Some(Ok(snap(SessionStatus::TimedOut, BEFORE + TOKENS - 1))),
    ));
}

#[test]
fn ltx_unsubmitted_still_delivers() {
    // withholding for all -> red
    assert!(is_complete(&outcome(
        None,
        ok(false),
        Some(Ok(snap(SessionStatus::TimedOut, 0)))
    )));
    assert!(is_complete(&outcome(None, ok(false), None)));
}

#[test]
fn upload_failure_fails_for_both_families() {
    // Err mapped to Complete -> red
    for sc in [None, R] {
        match outcome(sc, Err("s5 down".into()), None) {
            CompletionOutcome::Fail(code, msg) => {
                assert_eq!(code, "GENERATION_FAILED");
                assert_eq!(msg, "proof upload failed: s5 down");
            }
            c => panic!("{c:?}"),
        }
    }
}

/// Delegates to `MockS5Backend`, counts `put`, fails the first `fail_first`.
struct CountingS5 {
    inner: MockS5Backend,
    puts: Arc<AtomicU32>,
    fail_first: u32,
}

#[async_trait]
impl S5Storage for CountingS5 {
    async fn put(&self, path: &str, data: Vec<u8>) -> Result<String, StorageError> {
        let n = self.puts.fetch_add(1, Ordering::SeqCst);
        if n < self.fail_first {
            return Err(StorageError::NetworkError("stub hiccup".into()));
        }
        self.inner.put(path, data).await
    }
    async fn put_with_metadata(
        &self,
        p: &str,
        d: Vec<u8>,
        m: HashMap<String, String>,
    ) -> Result<String, StorageError> {
        self.inner.put_with_metadata(p, d, m).await
    }
    async fn get(&self, p: &str) -> Result<Vec<u8>, StorageError> {
        self.inner.get(p).await
    }
    async fn get_metadata(&self, p: &str) -> Result<HashMap<String, String>, StorageError> {
        self.inner.get_metadata(p).await
    }
    async fn get_by_cid(&self, c: &str) -> Result<Vec<u8>, StorageError> {
        self.inner.get_by_cid(c).await
    }
    async fn list(&self, p: &str) -> Result<Vec<S5Entry>, StorageError> {
        self.inner.list(p).await
    }
    async fn list_with_options(
        &self,
        p: &str,
        l: Option<usize>,
        c: Option<String>,
    ) -> Result<S5ListResult, StorageError> {
        self.inner.list_with_options(p, l, c).await
    }
    async fn delete(&self, p: &str) -> Result<(), StorageError> {
        self.inner.delete(p).await
    }
    async fn exists(&self, p: &str) -> Result<bool, StorageError> {
        self.inner.exists(p).await
    }
    fn clone(&self) -> Box<dyn S5Storage> {
        Box::new(CountingS5 {
            inner: MockS5Backend::new(),
            puts: self.puts.clone(),
            fail_first: self.fail_first,
        })
    }
}

#[tokio::test]
async fn upload_retry_is_bounded_at_three() {
    assert_eq!(RELIGHT_UPLOAD_ATTEMPTS, 3);
    // one hiccup -> Ok after two puts (no retry -> red)
    let puts = Arc::new(AtomicU32::new(0));
    let s5 = CountingS5 {
        inner: MockS5Backend::new(),
        puts: puts.clone(),
        fail_first: 1,
    };
    assert!(upload_frame_with_retry(
        &s5,
        "home/ltx/x/frame_00000.bin",
        vec![1, 2, 3],
        RELIGHT_UPLOAD_ATTEMPTS
    )
    .await
    .is_ok());
    assert_eq!(puts.load(Ordering::SeqCst), 2);
    // always failing -> Err after exactly three puts (unbounded -> red)
    let puts = Arc::new(AtomicU32::new(0));
    let s5 = CountingS5 {
        inner: MockS5Backend::new(),
        puts: puts.clone(),
        fail_first: u32::MAX,
    };
    assert!(upload_frame_with_retry(
        &s5,
        "home/ltx/x/frame_00000.bin",
        vec![1, 2, 3],
        RELIGHT_UPLOAD_ATTEMPTS
    )
    .await
    .is_err());
    assert_eq!(puts.load(Ordering::SeqCst), 3);
}
