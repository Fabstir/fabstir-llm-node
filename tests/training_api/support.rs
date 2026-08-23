// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Shared harness pieces for the T3 matrix (the `ltx_task_support`
//! convention): real capability-CID encryption, an in-memory S5 blob mock,
//! and the two-shard dataset fixture both the staging and pipeline suites
//! stage. Fixtures are SYNTHETIC ONLY.

use std::collections::HashMap;
use std::sync::Arc;

use fabstir_llm_node::ltx::exr::{capability_cid, encrypt_frame, padding_for};
use fabstir_llm_node::ltx::input_image::blob_download_cid;
use fabstir_llm_node::training::attestation::{
    canonical_manifest_bytes, canonical_manifest_sha256,
};
use fabstir_llm_node::training::staging::DatasetManifest;
use fabstir_llm_node::training::types::TrainingJob;
use sha2::{Digest, Sha256};

pub const KEY: [u8; 32] = [7u8; 32];
pub const TOK_SHA: &str = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

pub fn sha256_hex(data: &[u8]) -> String {
    format!("0x{}", hex::encode(Sha256::digest(data)))
}

/// Encrypt `plaintext` as a capability blob; returns (capability CID,
/// download CID for the mock store, ciphertext).
pub fn encrypt_blob(plaintext: &[u8]) -> (String, String, Vec<u8>) {
    let ciphertext = encrypt_frame(plaintext, &KEY).expect("encrypt");
    let cap = capability_cid(
        plaintext,
        &ciphertext,
        &KEY,
        padding_for(plaintext.len()) as u32,
    );
    let ct_hash: [u8; 32] = *blake3::hash(&ciphertext).as_bytes();
    (cap, blob_download_cid(&ct_hash), ciphertext)
}

/// Mock S5 bridge: `GET /s5/blob/{cid}` from an in-memory store.
pub async fn spawn_s5(store: HashMap<String, Vec<u8>>) -> String {
    use axum::extract::Path as AxPath;
    use axum::routing::get;
    let store = Arc::new(store);
    let app = axum::Router::new().route(
        "/s5/blob/:cid",
        get(move |AxPath(cid): AxPath<String>| {
            let store = store.clone();
            async move {
                match store.get(&cid) {
                    Some(bytes) => (axum::http::StatusCode::OK, bytes.clone()),
                    None => (axum::http::StatusCode::NOT_FOUND, Vec::new()),
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://127.0.0.1:{port}")
}

/// Two-shard dataset + canonical manifest + matching wire job, served by a
/// fresh mock S5. `tamper` mutates ONE claim (see the match arms).
pub struct Fixture {
    pub base_url: String,
    pub manifest_cid: String,
    pub manifest_sha256: String,
    pub manifest: DatasetManifest,
    pub job: TrainingJob,
    pub shard_plaintexts: Vec<Vec<u8>>,
}

pub async fn fixture(tamper: Option<&str>) -> Fixture {
    let pt1 = b"{\"text\": \"hello world\"}\n".to_vec();
    let pt2 = b"{\"text\": \"the fox jumps\"}\n{\"text\": \"hello\"}\n".to_vec();
    let mut store = HashMap::new();

    let (cap1, dl1, ct1) = encrypt_blob(&pt1);
    let (cap2, dl2, ct2) = encrypt_blob(&pt2);
    store.insert(dl1, ct1);
    store.insert(dl2, ct2);

    let mut shard1_sha = sha256_hex(&pt1);
    if tamper == Some("shard-sha") {
        shard1_sha = sha256_hex(b"something else entirely");
    }
    let mut total = (pt1.len() + pt2.len()) as u64;
    if tamper == Some("total-bytes") {
        total += 1;
    }
    let declared: u64 = if tamper == Some("implausible-bytes") {
        5
    } else {
        9
    };
    let manifest_value = serde_json::json!({
        "schema": "dataset-manifest-v1",
        "format": "jsonl-text-v1",
        "countingRecipe": "count-v1",
        "tokenizerSha256": TOK_SHA,
        "samples": 3u64,
        "declaredTokens": declared,
        "totalBytes": total,
        "shards": [
            { "cid": cap1, "sha256": shard1_sha, "sizeBytes": pt1.len() as u64 },
            { "cid": cap2, "sha256": sha256_hex(&pt2), "sizeBytes": pt2.len() as u64 },
        ]
    });
    let stored_bytes = canonical_manifest_bytes(&manifest_value).into_bytes();
    let mut manifest_sha256 = canonical_manifest_sha256(&manifest_value);
    if tamper == Some("manifest-sha") {
        manifest_sha256 = sha256_hex(b"not the manifest");
    }
    let (manifest_cap, manifest_dl, manifest_ct) = encrypt_blob(&stored_bytes);
    store.insert(manifest_dl, manifest_ct);

    let base_url = spawn_s5(store).await;
    let manifest: DatasetManifest = serde_json::from_slice(&stored_bytes).unwrap();
    let job: TrainingJob = serde_json::from_value(serde_json::json!({
        "templateId": "train-qlora-synthetic-test-v1",
        "templateHash": "0xabababababababababababababababababababababababababababababababab",
        "dataset": {
            "manifestCID": manifest_cap.clone(),
            "manifestSha256": manifest_sha256.clone(),
            "declaredTokens": declared,
            "samples": 3u64
        },
        "epochs": 1,
        "hyper": { "rank": 16, "alpha": 32, "lr": "0.000200", "seed": "13", "seqLen": 2048 },
        "output": "adapter-v1"
    }))
    .unwrap();
    Fixture {
        base_url,
        manifest_cid: manifest_cap,
        manifest_sha256,
        manifest,
        job,
        shard_plaintexts: vec![pt1, pt2],
    }
}

// --- pipeline-level mocks (wave B) ---

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use ethers::types::{Address, U256};
use fabstir_llm_node::training::accept::{
    AcceptConfig, AttemptRegistry, SessionSnapshot, SessionStatus,
};
use fabstir_llm_node::training::core::{SessionReader, TrainingDeps, TrainingTemplate};
use fabstir_llm_node::training::submit::SessionComplete;
use fabstir_llm_node::training::trainer_client::TrainerClient;

pub const NOW: u64 = 1_756_000_000;
pub const PRICE: u64 = 904;

pub fn addr(byte: u8) -> Address {
    Address::from_slice(&[byte; 20])
}

pub fn model_id(byte: u8) -> [u8; 32] {
    [byte; 32]
}

/// A passing snapshot whose `start_time` is derived from the REAL clock —
/// required by any row that pins the completion CREATION floor (the floor
/// compares `start_time + window` against `SystemTime::now()`; the constant
/// `NOW` fixture sits ~a year in the past, which made that pin vacuous —
/// round-2 F1).
pub fn snapshot_started_secs_ago(secs: u64) -> SessionSnapshot {
    let real_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    SessionSnapshot {
        start_time: U256::from(real_now.saturating_sub(secs)),
        ..passing_snapshot()
    }
}

/// A passing chain snapshot for the 9-token fixture job.
pub fn passing_snapshot() -> SessionSnapshot {
    SessionSnapshot {
        depositor: addr(0xD1),
        host: addr(0xB0),
        payment_token: addr(0xEC),
        deposit: U256::from(1_000_000u64),
        price_per_token: U256::from(PRICE),
        tokens_used: U256::zero(),
        max_duration: U256::from(14_400u64),
        start_time: U256::from(NOW - 100),
        proof_timeout_window: U256::from(3_600u64),
        status: SessionStatus::Active,
    }
}

pub struct MockSessions {
    pub snapshot: Result<SessionSnapshot, String>,
    pub model: [u8; 32],
    pub dispute: u64,
}

#[async_trait::async_trait]
impl SessionReader for MockSessions {
    async fn session_snapshot(&self, _job_id: u64) -> Result<SessionSnapshot, String> {
        self.snapshot.clone()
    }
    async fn session_model(&self, _job_id: u64) -> Result<[u8; 32], String> {
        Ok(self.model)
    }
    async fn dispute_window_secs(&self) -> u64 {
        self.dispute
    }
}

#[derive(Default)]
pub struct MockCompleter {
    pub calls: Mutex<Vec<u64>>,
    /// Fail this many complete_session attempts before succeeding.
    pub fail_times: std::sync::atomic::AtomicU32,
}

#[async_trait::async_trait]
impl SessionComplete for MockCompleter {
    async fn complete_session(&self, job_id: u64) -> Result<(), String> {
        use std::sync::atomic::Ordering;
        let remaining = self.fail_times.load(Ordering::SeqCst);
        if remaining > 0 {
            self.fail_times.store(remaining - 1, Ordering::SeqCst);
            return Err("synthetic completer failure".to_string());
        }
        self.calls.lock().unwrap().push(job_id);
        Ok(())
    }
}

impl MockCompleter {
    pub fn count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum ScanBehaviour {
    Cleared,
    Blocked,
    Flagged,
    FailEnvelope,
    Drop,
    /// A verdict string the node does not recognise (version skew).
    UnknownVerdict,
}

#[derive(Clone, Copy, PartialEq)]
pub enum CountBehaviour {
    Tokens(u64),
    Malformed,
    /// A 200 SOURCE_MUTATED envelope (the §3.7 catch-all class).
    MutatedEnvelope,
}

#[derive(Clone, Copy, PartialEq)]
pub enum SidecarHealth {
    /// Pins echo the fixture template (the happy accept-time consult).
    Ok,
    /// Pins echo a DIFFERENT template hash (B.6 skew — deployment fault).
    PinSkew,
    /// Health ok but the run slot reports busy.
    SlotBusy,
}

/// Mock trainer sidecar over a real UDS for the pipeline's scan+count legs.
pub fn spawn_sidecar(
    dir: &std::path::Path,
    scan: ScanBehaviour,
    count: CountBehaviour,
    sidecar: SidecarHealth,
) -> PathBuf {
    use http_body_util::Full;
    use hyper::body::Bytes;
    use hyper::service::service_fn;
    use hyper::{Response, StatusCode};
    use hyper_util::rt::TokioIo;

    let sock = dir.join("trainer.sock");
    let listener = tokio::net::UnixListener::bind(&sock).expect("bind mock UDS");
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            if scan == ScanBehaviour::Drop {
                drop(stream);
                continue;
            }
            tokio::spawn(async move {
                let service = service_fn(
                    move |req: hyper::Request<hyper::body::Incoming>| async move {
                        let (status, body) = match req.uri().path() {
                        "/v1/health" => {
                            let template_hash = if sidecar == SidecarHealth::PinSkew {
                                "0xDIFFERENT"
                            } else {
                                "0xabababababababababababababababababababababababababababababababab"
                            };
                            (
                                StatusCode::OK,
                                format!(
                                    r#"{{"status":"ok","pins":{{"templateHash":"{template_hash}","tokenizerSha256":"{TOK_SHA}","stackDigest":"sha256:s"}}}}"#
                                ),
                            )
                        }
                        "/v1/status" => {
                            let slot = if sidecar == SidecarHealth::SlotBusy { "held" } else { "free" };
                            (StatusCode::OK, format!(r#"{{"slot":"{slot}"}}"#))
                        }
                        "/v1/scan" => match scan {
                            ScanBehaviour::Cleared => (StatusCode::OK, r#"{"verdict":"cleared","policyVersion":"structural-v0"}"#.to_string()),
                            ScanBehaviour::Blocked => (StatusCode::OK, r#"{"verdict":"blocked","policyVersion":"structural-v0"}"#.to_string()),
                            ScanBehaviour::Flagged => (StatusCode::OK, r#"{"verdict":"flagged","policyVersion":"structural-v0"}"#.to_string()),
                            ScanBehaviour::FailEnvelope => (StatusCode::OK, r#"{"error":{"kind":"SCAN_FAILURE","detail":"synthetic"}}"#.to_string()),
                            ScanBehaviour::UnknownVerdict => (StatusCode::OK, r#"{"verdict":"maybe","policyVersion":"structural-v0"}"#.to_string()),
                            ScanBehaviour::Drop => unreachable!(),
                        },
                        "/v1/count" => match count {
                            CountBehaviour::Tokens(n) => (StatusCode::OK, format!(r#"{{"tokens":{n},"samples":3}}"#)),
                            CountBehaviour::Malformed => (StatusCode::BAD_REQUEST, r#"{"error":{"kind":"DATASET_MALFORMED","detail":"synthetic"}}"#.to_string()),
                            CountBehaviour::MutatedEnvelope => (StatusCode::OK, r#"{"error":{"kind":"SOURCE_MUTATED","detail":"synthetic"}}"#.to_string()),
                        },
                        _ => (StatusCode::NOT_FOUND, "{}".to_string()),
                    };
                        Ok::<_, std::convert::Infallible>(
                            Response::builder()
                                .status(status)
                                .header("content-type", "application/json")
                                .body(Full::new(Bytes::from(body)))
                                .unwrap(),
                        )
                    },
                );
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    sock
}

pub fn fixture_template() -> TrainingTemplate {
    TrainingTemplate {
        base_serving_model_id: "0x00000000000000000000000000000000000000000000000000000000000000ba"
            .to_string(),
        template_id: "train-qlora-synthetic-test-v1".to_string(),
        template_hash: "0xabababababababababababababababababababababababababababababababab"
            .to_string(),
        tokenizer_sha256: TOK_SHA.to_string(),
        ranks: vec![8, 16, 32],
        alphas: vec![16, 32, 64],
        seq_lens: vec![1024, 2048, 4096],
        lrs: None,
        max_epochs: 5,
        max_total_tokens: 15_000_000,
        slice_tokens: 1_000_000,
    }
}

pub struct Harness {
    pub deps: TrainingDeps,
    pub completer: Arc<MockCompleter>,
    pub proof: Arc<MockProof>,
    pub artifact_store: Arc<fabstir_llm_node::storage::s5_client::MockS5Backend>,
    pub staging_dir: tempfile::TempDir,
    pub work_dir: tempfile::TempDir,
    _sidecar_dir: tempfile::TempDir,
}

/// hardhat #0 — SYNTHETIC test key (the vectors' signer), never production.
pub const NODE_KEY: [u8; 32] = [
    0xac, 0x09, 0x74, 0xbe, 0xc3, 0x9a, 0x17, 0xe3, 0x6b, 0xa4, 0xa6, 0xb4, 0xd2, 0x38, 0xff, 0x94,
    0x4b, 0xac, 0xb4, 0x78, 0xcb, 0xed, 0x5e, 0xfc, 0xae, 0x78, 0x4d, 0x7b, 0xf4, 0xf2, 0xff, 0x80,
];

/// Recording ProofSubmit mock: scripted per-call results; logs
/// `(job_id, tokens, proof_cid)` and appends "proof:<slice-cid>" to the
/// shared interleave log (the delivery-before-settlement order check).
pub struct MockProof {
    pub calls: Mutex<Vec<(u64, u64, String)>>,
    pub script: Mutex<std::collections::VecDeque<Result<(), String>>>,
    pub interleave: Arc<Mutex<Vec<String>>>,
}

impl MockProof {
    pub fn ok() -> Arc<Self> {
        Arc::new(MockProof {
            calls: Mutex::new(Vec::new()),
            script: Mutex::new(std::collections::VecDeque::new()),
            interleave: Arc::new(Mutex::new(Vec::new())),
        })
    }
    pub fn count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

#[async_trait::async_trait]
impl fabstir_llm_node::ltx::submit::ProofSubmit for MockProof {
    async fn submit_ltx_proof(
        &self,
        job_id: u64,
        tokens: u64,
        _proof_hash: [u8; 32],
        proof_cid: String,
    ) -> anyhow::Result<ethers::types::H256> {
        let scripted = self.script.lock().unwrap().pop_front();
        match scripted {
            Some(Err(e)) => {
                self.interleave
                    .lock()
                    .unwrap()
                    .push(format!("proof-err:{tokens}"));
                Err(anyhow::anyhow!(e))
            }
            _ => {
                self.calls.lock().unwrap().push((job_id, tokens, proof_cid));
                self.interleave
                    .lock()
                    .unwrap()
                    .push(format!("proof:{tokens}"));
                Ok(ethers::types::H256::zero())
            }
        }
    }

    async fn session_proof_interval(&self, _job_id: u64) -> u64 {
        1000
    }
}

/// Full pipeline deps over the fixture: mock chain, mock completer, REAL
/// TrainerClient → mock UDS sidecar, REAL S5 mock from `fixture()`.
pub fn make_deps(
    fx: &Fixture,
    sessions: MockSessions,
    scan: ScanBehaviour,
    count: CountBehaviour,
) -> Harness {
    make_deps_with_sidecar(fx, sessions, scan, count, SidecarHealth::Ok)
}

pub fn make_deps_with_sidecar(
    fx: &Fixture,
    sessions: MockSessions,
    scan: ScanBehaviour,
    count: CountBehaviour,
    sidecar: SidecarHealth,
) -> Harness {
    let staging_dir = tempfile::tempdir().unwrap();
    let work_dir = tempfile::tempdir().unwrap();
    let sidecar_dir = tempfile::tempdir().unwrap();
    let sock = spawn_sidecar(sidecar_dir.path(), scan, count, sidecar);
    let completer = Arc::new(MockCompleter::default());
    let proof = MockProof::ok();
    let tracker = Arc::new(fabstir_llm_node::training::tracker::TrainTracker::new());
    let artifact_store = Arc::new(fabstir_llm_node::storage::s5_client::MockS5Backend::new());
    let deps = TrainingDeps {
        adapters: std::sync::Arc::new(
            fabstir_llm_node::training::serve::AdapterRegistry::new(),
        ),
        sessions: Arc::new(sessions),
        completer: completer.clone(),
        trainer: Arc::new(TrainerClient::new(sock, Duration::from_secs(5))),
        attempts: Arc::new(AttemptRegistry::new()),
        staging_root: staging_dir.path().to_path_buf(),
        s5_base: fx.base_url.clone(),
        host_address: addr(0xB0),
        model_id: model_id(0xAA),
        expected_price: U256::from(PRICE),
        priced_tokens: vec![addr(0xEC)],
        template: fixture_template(),
        accept_cfg: AcceptConfig::default(),
        cooldown_secs: 60,
        settle_buffer_secs: 45,
        capacity_cache: std::sync::Mutex::new(None),
        work_root: work_dir.path().to_path_buf(),
        artifact_store: artifact_store.clone(),
        proof: proof.clone(),
        tracker: tracker.clone(),
        node_key: NODE_KEY,
        env_hash: format!("0x{}", "e0".repeat(32)),
        rate_limit_tokens_per_sec: 10_000,
        completing_latch: Duration::from_secs(30),
        allow_list_version: 1,
    };
    Harness {
        deps,
        completer,
        proof,
        artifact_store,
        staging_dir,
        work_dir,
        _sidecar_dir: sidecar_dir,
    }
}

// --- the §3.5 train-stream mock (T4) ---

/// One scripted NDJSON line with a pre-send delay.
#[derive(Clone)]
pub struct ScriptLine {
    pub delay_ms: u64,
    pub line: String,
}

pub fn line(delay_ms: u64, json: &str) -> ScriptLine {
    ScriptLine {
        delay_ms,
        line: format!("{json}\n"),
    }
}

#[derive(Clone)]
pub enum TrainBehaviour {
    /// Stream the script then END the body cleanly (a well-terminated stream
    /// ends after `done`/terminal; ending WITHOUT one = the died-stream case).
    Script(Vec<ScriptLine>),
    /// Pre-stream 409 SLOT_BUSY envelope.
    Busy409,
    /// Pre-stream 400 TEMPLATE_BOUNDS envelope.
    Bounds400,
}

/// Mock sidecar serving ONLY `/v1/train` with a scripted held stream.
pub fn spawn_train_sidecar(dir: &std::path::Path, behaviour: TrainBehaviour) -> PathBuf {
    use futures::StreamExt;
    use http_body_util::{combinators::BoxBody, BodyExt, Full, StreamBody};
    use hyper::body::{Bytes, Frame};
    use hyper::service::service_fn;
    use hyper::{Response, StatusCode};
    use hyper_util::rt::TokioIo;

    let sock = dir.join("trainer.sock");
    let listener = tokio::net::UnixListener::bind(&sock).expect("bind mock UDS");
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let behaviour = behaviour.clone();
            tokio::spawn(async move {
                let service = service_fn(move |_req: hyper::Request<hyper::body::Incoming>| {
                    let behaviour = behaviour.clone();
                    async move {
                        let response: Response<BoxBody<Bytes, std::convert::Infallible>> =
                            match behaviour {
                                TrainBehaviour::Busy409 => Response::builder()
                                    .status(StatusCode::CONFLICT)
                                    .header("content-type", "application/json")
                                    .body(
                                        Full::new(Bytes::from(
                                            r#"{"error":{"kind":"SLOT_BUSY","detail":"synthetic"}}"#,
                                        ))
                                        .boxed(),
                                    )
                                    .unwrap(),
                                TrainBehaviour::Bounds400 => Response::builder()
                                    .status(StatusCode::BAD_REQUEST)
                                    .header("content-type", "application/json")
                                    .body(
                                        Full::new(Bytes::from(
                                            r#"{"error":{"kind":"TEMPLATE_BOUNDS","detail":"synthetic"}}"#,
                                        ))
                                        .boxed(),
                                    )
                                    .unwrap(),
                                TrainBehaviour::Script(script) => {
                                    let stream =
                                        futures::stream::iter(script.into_iter()).then(|s| async move {
                                            tokio::time::sleep(std::time::Duration::from_millis(
                                                s.delay_ms,
                                            ))
                                            .await;
                                            Ok::<_, std::convert::Infallible>(Frame::data(
                                                Bytes::from(s.line),
                                            ))
                                        });
                                    Response::builder()
                                        .status(StatusCode::OK)
                                        .header("content-type", "application/x-ndjson")
                                        .body(BodyExt::boxed(StreamBody::new(stream)))
                                        .unwrap()
                                }
                            };
                        Ok::<_, std::convert::Infallible>(response)
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    sock
}
