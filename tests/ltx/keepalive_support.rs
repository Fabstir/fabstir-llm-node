// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Shared mocks for the Batch D21 keep-alive tests (`test_keepalive.rs`,
//! `test_keepalive_bounds.rs`): a scripted interim submitter (optionally never
//! resolving its first call), a unix clock on tokio's clock, a `KeepAliveCfg`
//! builder and a gated S5 store (its first `put` can block the worker thread or
//! never resolve).

#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ethers::types::H256;
use fabstir_llm_node::ltx::billing::LtxTracker;
use fabstir_llm_node::ltx::keepalive::{start_keepalive, KeepAliveCfg, KeepAliveGuard, UnixClock};
use fabstir_llm_node::ltx::submit::{ProofReceipt, ProofSubmit, ProofSubmitError};
use fabstir_llm_node::storage::s5_client::{
    MockS5Backend, S5Entry, S5ListResult, S5Storage, StorageError,
};
use serde_json::Value;
use tokio::sync::mpsc;

/// Scripted interim submitter: pops one outcome per call (exhausted ⇒ Ok).
/// `hang_first` makes the FIRST call never resolve.
#[derive(Default)]
pub struct Interims {
    pub script: Mutex<VecDeque<Result<(), ProofSubmitError>>>,
    pub calls: Mutex<Vec<(u64, [u8; 32], String)>>,
    pub hang_first: AtomicBool,
}

impl Interims {
    pub fn scripted(s: Vec<Result<(), ProofSubmitError>>) -> Arc<Self> {
        Arc::new(Interims {
            script: Mutex::new(s.into()),
            ..Default::default()
        })
    }
    pub fn claims(&self) -> Vec<u64> {
        self.calls.lock().unwrap().iter().map(|c| c.0).collect()
    }
}

#[async_trait::async_trait]
impl ProofSubmit for Interims {
    async fn submit_ltx_proof(
        &self,
        _job_id: u64,
        tokens: u64,
        proof_hash: [u8; 32],
        proof_cid: String,
    ) -> Result<H256, ProofSubmitError> {
        self.calls
            .lock()
            .unwrap()
            .push((tokens, proof_hash, proof_cid));
        if self.hang_first.swap(false, Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        match self.script.lock().unwrap().pop_front() {
            Some(Err(e)) => Err(e),
            _ => Ok(H256::repeat_byte(1)),
        }
    }
    async fn session_proof_interval(&self, _job_id: u64) -> u64 {
        1000
    }
    async fn receipt_of(&self, _tx: H256) -> Result<Option<ProofReceipt>, String> {
        Ok(None)
    }
}

pub struct TokioClock(pub tokio::time::Instant);
impl UnixClock for TokioClock {
    fn now_unix(&self) -> u64 {
        1_790_000_000 + self.0.elapsed().as_secs()
    }
}

/// A keep-alive config: first tick one interval in, tokens large (no cap hit).
pub fn cfg(interval: Duration, deadline: Duration) -> KeepAliveCfg {
    KeepAliveCfg {
        job_id: 42,
        job_tag: "42-p1".to_string(),
        session_id: "0xabc".to_string(),
        host: "0xb0".to_string(),
        model_id: "0x01".to_string(),
        interval,
        first_tick: interval,
        deadline_at: tokio::time::Instant::now() + deadline,
        proof_interval: 1000,
        interim_cap: 1_000_000,
    }
}

pub struct Run {
    pub sub: Arc<Interims>,
    pub s5: Arc<MockS5Backend>,
    pub tracker: LtxTracker,
    pub guard: KeepAliveGuard,
    pub rx: mpsc::Receiver<Value>,
}

pub fn start(script: Vec<Result<(), ProofSubmitError>>, deadline: Duration) -> Run {
    start_cfg(script, cfg(Duration::from_secs(900), deadline))
}

pub fn start_cfg(script: Vec<Result<(), ProofSubmitError>>, cfg: KeepAliveCfg) -> Run {
    let sub = Interims::scripted(script);
    let s5 = Arc::new(MockS5Backend::new());
    let tracker = LtxTracker::new();
    let (tx, rx) = mpsc::channel(64);
    let guard = start_keepalive(
        cfg,
        s5.clone(),
        sub.clone(),
        tracker.clone(),
        tx,
        None,
        Arc::new(TokioClock(tokio::time::Instant::now())),
    );
    Run {
        sub,
        s5,
        tracker,
        guard,
        rx,
    }
}

pub async fn at(secs: u64, origin: tokio::time::Instant) {
    tokio::time::sleep_until(origin + Duration::from_secs(secs)).await;
}

/// An S5 store whose FIRST `put` is gated: `blocking` blocks the worker thread
/// on a std channel until released (the tick is INSIDE a poll), `hanging`
/// never resolves.
pub struct GatedS5 {
    pub inner: MockS5Backend,
    pub entered: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    pub release: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    pub hang_first: AtomicBool,
}

impl GatedS5 {
    pub fn blocking(
        entered: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
    ) -> Self {
        GatedS5 {
            inner: MockS5Backend::new(),
            entered: Mutex::new(Some(entered)),
            release: Mutex::new(Some(release)),
            hang_first: AtomicBool::new(false),
        }
    }
    pub fn hanging() -> Self {
        GatedS5 {
            inner: MockS5Backend::new(),
            entered: Mutex::new(None),
            release: Mutex::new(None),
            hang_first: AtomicBool::new(true),
        }
    }
}

#[async_trait::async_trait]
impl S5Storage for GatedS5 {
    async fn put(&self, path: &str, data: Vec<u8>) -> Result<String, StorageError> {
        if let Some(tx) = self.entered.lock().unwrap().take() {
            let _ = tx.send(());
            if let Some(rx) = self.release.lock().unwrap().take() {
                let _ = rx.recv(); // blocks the worker
            }
        }
        if self.hang_first.swap(false, Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        self.inner.put(path, data).await
    }
    async fn put_with_metadata(
        &self,
        path: &str,
        data: Vec<u8>,
        m: HashMap<String, String>,
    ) -> Result<String, StorageError> {
        self.inner.put_with_metadata(path, data, m).await
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
        S5Storage::clone(&self.inner)
    }
}
