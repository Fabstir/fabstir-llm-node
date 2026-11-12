// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Content checks (N10) test support for the RESULT stage, which sits after the storage and chain lookups in
//! `run()`: a stub JSON-RPC chain good enough to build a real `CheckpointManager` offline (session reads, and a
//! proof transaction that is captured so its claimed tokens can be read back), and a provenance signer that really
//! signs. With them one job runs from accept to `ltx_complete` in-process, so "nothing is uploaded, signed or
//! billed" and "a pass bills exactly its estimate" are observed, not inferred.
#![allow(dead_code)]

use axum::extract::State;
use axum::{Json, Router};
use ethers::signers::{LocalWallet, Signer};
use ethers::types::U256;
use fabstir_llm_node::api::server::ApiServer;
use fabstir_llm_node::contracts::checkpoint_manager::CheckpointManager;
use fabstir_llm_node::contracts::client::{Web3Client, Web3Config};
use fabstir_llm_node::ltx::provenance::signer_client::{ProvenanceSigner, SignerError};
use fabstir_provenance::contract::{
    Limits, ReserveRequest, ReserveResponse, SignRequest, SignSequenceRequest,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const HOST_KEY: &str = "0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318";

pub struct Chain {
    /// Every raw transaction the node sent.
    pub raw_txs: Mutex<Vec<Vec<u8>>>,
    model: [u8; 32],
    session: Vec<u8>,
}

impl Chain {
    /// `tokensClaimed` of each `submitProofOfWork(jobId, tokensClaimed, ...)` the node sent.
    pub fn claimed_tokens(&self) -> Vec<u64> {
        self.raw_txs
            .lock()
            .unwrap()
            .iter()
            .map(|raw| {
                let rlp = ethers::utils::rlp::Rlp::new(raw);
                let (tx, _sig) =
                    ethers::types::transaction::eip2718::TypedTransaction::decode_signed(&rlp)
                        .expect("a signed transaction");
                let data = tx.data().expect("calldata").to_vec();
                U256::from_big_endian(&data[4 + 32..4 + 64]).as_u64()
            })
            .collect()
    }
}

fn selector(sig: &str) -> String {
    hex::encode(&ethers::utils::id(sig)[..])
}

async fn rpc(State(c): State<Arc<Chain>>, Json(req): Json<Value>) -> Json<Value> {
    let hex0x = |b: &[u8]| format!("0x{}", hex::encode(b));
    let result = match req["method"].as_str().unwrap_or_default() {
        "eth_chainId" => json!("0x14a34"),
        "eth_blockNumber" => json!("0x100"),
        "eth_gasPrice" => json!("0x3b9aca00"),
        "eth_estimateGas" => json!("0x493e0"),
        "eth_getTransactionCount" => json!("0x0"),
        "eth_getBalance" => json!("0xde0b6b3a7640000"),
        "eth_call" => {
            let p = &req["params"][0];
            let data = p["data"].as_str().or(p["input"].as_str()).unwrap_or("0x");
            let sel = data.trim_start_matches("0x").get(..8).unwrap_or_default();
            if sel == selector("sessionModel(uint256)") {
                json!(hex0x(&c.model))
            } else if sel == selector("sessionJobs(uint256)") {
                json!(hex0x(&c.session))
            } else {
                json!(hex0x(&[0u8; 32]))
            }
        }
        "eth_sendRawTransaction" => {
            let raw =
                hex::decode(req["params"][0].as_str().unwrap().trim_start_matches("0x")).unwrap();
            let hash = ethers::utils::keccak256(&raw);
            c.raw_txs.lock().unwrap().push(raw);
            json!(hex0x(&hash))
        }
        "eth_getTransactionReceipt" => {
            let zero32 = hex0x(&[0u8; 32]);
            json!({
                "transactionHash": req["params"][0], "transactionIndex": "0x0", "blockHash": zero32,
                "blockNumber": "0x100", "from": hex0x(&[0u8; 20]), "to": hex0x(&[0u8; 20]),
                "cumulativeGasUsed": "0x5208", "gasUsed": "0x5208", "contractAddress": null, "logs": [],
                "status": "0x1", "logsBloom": hex0x(&[0u8; 256]), "type": "0x0",
                "effectiveGasPrice": "0x3b9aca00"
            })
        }
        _ => Value::Null,
    };
    Json(json!({"jsonrpc": "2.0", "id": req["id"], "result": result}))
}

/// Start the stub chain: one Active session of this host, opened for `model`, with a deposit that covers any job.
pub async fn spawn_chain(model: [u8; 32]) -> (Arc<Chain>, String) {
    let host = HOST_KEY.parse::<LocalWallet>().unwrap().address();
    let mut session = vec![0u8; 20 * 32];
    session[2 * 32 + 12..3 * 32].copy_from_slice(host.as_bytes());
    U256::exp10(24).to_big_endian(&mut session[4 * 32..5 * 32]); // deposit
    U256::one().to_big_endian(&mut session[5 * 32..6 * 32]); // price per token
    U256::from(3600).to_big_endian(&mut session[11 * 32..12 * 32]); // proof window
    let chain = Arc::new(Chain {
        raw_txs: Mutex::new(vec![]),
        model,
        session,
    });
    let app = Router::new().fallback(rpc).with_state(chain.clone());
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    (chain, url)
}

/// A real `CheckpointManager` over the stub chain and the in-memory S5 backend. The caller holds `S5_ENV_LOCK`
/// (this clears `ENHANCED_S5_URL` so storage is the in-memory mock, and sets two process settings it needs).
pub async fn install_checkpoint_manager(
    server: &ApiServer,
    rpc_url: &str,
) -> Arc<CheckpointManager> {
    std::env::remove_var("ENHANCED_S5_URL");
    std::env::set_var(
        "CONTRACT_JOB_MARKETPLACE",
        "0xD067719Ee4c514B5735d1aC0FfB46FECf2A9adA4",
    );
    std::env::set_var("DISPUTE_WINDOW_SECONDS", "30");
    let web3 = Web3Client::new(Web3Config {
        rpc_url: rpc_url.to_string(),
        chain_id: 84532,
        private_key: Some(HOST_KEY.to_string()),
        ..Web3Config::default()
    })
    .await
    .expect("the stub chain answers eth_chainId");
    let cm = Arc::new(CheckpointManager::new(Arc::new(web3)).await.unwrap());
    server.set_checkpoint_manager(cm.clone()).await;
    cm
}

/// The real signer service in-process, signing for real and counting what it signed.
pub struct Signing {
    ts: provenance_signer::testkit::TestSigner,
    host: String,
    pub signed: AtomicUsize,
    /// `deadline_secs` of the last reservation the node asked for.
    pub reserved_secs: std::sync::atomic::AtomicU64,
}

fn perr(f: provenance_signer::service::Fail) -> SignerError {
    SignerError {
        code: Some(f.0),
        message: f.1,
        retryable: f.0.retryable(),
        limits: None,
    }
}

#[async_trait::async_trait]
impl ProvenanceSigner for Signing {
    async fn limits(&self) -> Result<Limits, SignerError> {
        Ok(self.ts.svc.limits())
    }
    async fn reserve(&self, req: &ReserveRequest) -> Result<ReserveResponse, SignerError> {
        self.reserved_secs
            .store(req.deadline_secs, Ordering::SeqCst);
        self.ts.svc.reserve(&self.host, req).map_err(perr)
    }
    async fn sign(&self, req: &SignRequest) -> Result<Vec<u8>, SignerError> {
        self.signed.fetch_add(1, Ordering::SeqCst);
        let body = serde_json::to_vec(req).unwrap();
        self.ts
            .svc
            .sign_mp4(&self.host, &body, req)
            .map(|s| s.manifest)
            .map_err(perr)
    }
    async fn sign_sequence(&self, req: &SignSequenceRequest) -> Result<Vec<u8>, SignerError> {
        self.signed.fetch_add(1, Ordering::SeqCst);
        let body = serde_json::to_vec(req).unwrap();
        self.ts
            .svc
            .sign_sequence(&self.host, &body, req)
            .map(|s| s.manifest)
            .map_err(perr)
    }
}

/// Install the signing provenance runtime (needs `PROVENANCE_FFMPEG`, the pinned ffmpeg: tests never skip).
pub async fn install_signing_provenance(server: &ApiServer) -> Arc<Signing> {
    let host = [0x42u8; 20];
    let signer = Arc::new(Signing {
        ts: provenance_signer::testkit::TestSigner::new_with_min_bundle(
            host,
            &[fabstir_llm_node::version::VERSION],
            0,
        ),
        host: hex::encode(host),
        signed: AtomicUsize::new(0),
        reserved_secs: std::sync::atomic::AtomicU64::new(0),
    });
    let ffmpeg = fabstir_llm_node::ltx::provenance::ffmpeg::Ffmpeg::from_env()
        .await
        .expect("PROVENANCE_FFMPEG must point at the pinned ffmpeg 7.0.2 (tests never skip)");
    let rt = fabstir_llm_node::ltx::provenance::Runtime {
        signer: signer.clone(),
        ffmpeg,
        reserve: 12_000,
    };
    server.set_provenance(Arc::new(rt)).await;
    signer
}
