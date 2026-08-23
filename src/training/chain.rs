// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Production impls of the training chain seams over `CheckpointManager`
//! (T3.5). Reads use the drift-proof raw-word pattern (NEVER the 17-field
//! typed decode — the deployed struct has 18 fields); completion rides
//! `complete_session_job` (Band A/tests use the trait mocks instead; these
//! impls are exercised at T6 on the GPU host against the live chain).

use std::sync::Arc;

use crate::contracts::checkpoint_manager::CheckpointManager;
use crate::training::accept::{decode_session_snapshot, SessionSnapshot};
use crate::training::core::SessionReader;
use crate::training::submit::SessionComplete;

/// Production session reads for the A.3 gates.
pub struct ChainSessionReader {
    pub manager: Arc<CheckpointManager>,
}

#[async_trait::async_trait]
impl SessionReader for ChainSessionReader {
    async fn session_snapshot(&self, job_id: u64) -> Result<SessionSnapshot, String> {
        let raw = self
            .manager
            .query_session_jobs_raw(job_id)
            .await
            .map_err(|e| e.to_string())?;
        decode_session_snapshot(&raw)
    }

    async fn session_model(&self, job_id: u64) -> Result<[u8; 32], String> {
        self.manager
            .query_session_model(job_id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn dispute_window_secs(&self) -> u64 {
        self.manager.dispute_window_secs()
    }
}

/// Production completion (`completeSessionJob`) — the C.3 zero-settle and
/// the end-of-run settle both land here; scheduling is the CALLER's duty.
pub struct ChainSessionComplete {
    pub manager: Arc<CheckpointManager>,
}

#[async_trait::async_trait]
impl SessionComplete for ChainSessionComplete {
    async fn complete_session(&self, job_id: u64) -> Result<(), String> {
        self.manager
            .complete_session_job(job_id)
            .await
            .map_err(|e| e.to_string())
    }
}

/// T4.5 — wire the training feature from env at startup. Gated on
/// `TRAIN_ENABLED=true`; silently absent (capacity route 404s, `train`
/// rejects `SIDECAR_UNAVAILABLE`) otherwise. Requires the checkpoint manager
/// (chain reads + settles) and the S5 backend (artifact uploads); runs the
/// TD15 boot sweeps on both roots.
pub async fn wire_training_from_env(server: &crate::api::server::ApiServer) {
    if std::env::var("TRAIN_ENABLED").map(|v| v == "true" || v == "1") != Ok(true) {
        println!("ℹ️  TRAIN_ENABLED not set — training disabled");
        return;
    }
    // TRAIN_MOCK_CHAIN is AUTHORITATIVE (T5 converge round 1, F1/F8). The
    // first cut selected mocks only when the checkpoint manager was absent —
    // but the wiring hard-requires HOST_PRIVATE_KEY, which is exactly what
    // CREATES the manager, so mock mode was unreachable in every
    // configuration where training could wire at all. Worse, the banner
    // printed on the FLAG while the seams were chosen on the manager, so a
    // chain-capable host announced "IN-MEMORY" and ran on the real chain —
    // the T6 sheet's "no testnet money" promise was false exactly where it
    // was aimed. An explicit flag now wins, and says so loudly.
    let mock_chain = std::env::var("TRAIN_MOCK_CHAIN").map(|v| v == "true" || v == "1") == Ok(true);
    let manager = if mock_chain {
        println!(
            "🧪 TRAIN_MOCK_CHAIN=true — training chain seams are IN-MEMORY: no session is \
             read, NO PROOF IS SUBMITTED and no completion reaches a chain. Unpaid T6 gate \
             only; unset this for any real or paid run."
        );
        None
    } else {
        // Round-2 R2-6: the real branch printed NOTHING about the chain, so
        // "no mock banner" was the only evidence that money was in play —
        // and since the flag parse accepts only exactly `true` or `1`, a
        // `TRAIN_MOCK_CHAIN=TRUE` typo selected the real chain in silence,
        // defeating the T6 sheet's "no testnet money" promise. Absence of a
        // line is not a signal; state the real case too.
        println!(
            "⛓️  TRAIN_MOCK_CHAIN not set to exactly `true` or `1` — training rides the REAL \
             chain: sessions are read on-chain, proofs are submitted and completions settle."
        );
        match server.get_checkpoint_manager().await {
            Some(manager) => Some(manager),
            None => {
                println!("⚠️  TRAIN_ENABLED but no checkpoint manager — training disabled");
                return;
            }
        }
    };

    let required = |name: &str| std::env::var(name).map_err(|_| name.to_string());
    let wired: Result<(), String> = async {
        let socket = required("TRAINER_SOCKET")?;
        let staging_root = std::path::PathBuf::from(required("TRAINING_STAGING_ROOT")?);
        let work_root = std::path::PathBuf::from(required("TRAINING_WORK_ROOT")?);
        let template_path = std::path::PathBuf::from(required("TRAINING_TEMPLATE_PATH")?);
        let model_id_hex = required("TRAINING_MODEL_ID")?;
        let model_id_bytes = hex::decode(model_id_hex.trim_start_matches("0x"))
            .map_err(|e| format!("TRAINING_MODEL_ID: {e}"))?;
        let model_id: [u8; 32] = model_id_bytes
            .try_into()
            .map_err(|_| "TRAINING_MODEL_ID must be 32 bytes".to_string())?;
        // The REGISTERED price (must equal the on-chain registration; A.3
        // rejects sessions priced differently either way — env is the
        // node-side mirror until a registry read lands here).
        let price: u64 = required("TRAINING_PRICE_PER_TOKEN")?
            .parse()
            .map_err(|e| format!("TRAINING_PRICE_PER_TOKEN: {e}"))?;
        let usdc = required("USDC_TOKEN")?
            .parse::<ethers::types::Address>()
            .map_err(|e| format!("USDC_TOKEN: {e}"))?;
        let node_key_hex = required("HOST_PRIVATE_KEY")?;
        let node_key_bytes = hex::decode(node_key_hex.trim_start_matches("0x"))
            .map_err(|e| format!("HOST_PRIVATE_KEY: {e}"))?;
        let node_key: [u8; 32] = node_key_bytes
            .try_into()
            .map_err(|_| "HOST_PRIVATE_KEY must be 32 bytes".to_string())?;
        let host_address = match &manager {
            Some(manager) => manager
                .get_host_address()
                .parse::<ethers::types::Address>()
                .map_err(|e| format!("host address: {e}"))?,
            None => required("TRAINING_MOCK_HOST_ADDRESS")?
                .parse::<ethers::types::Address>()
                .map_err(|e| format!("TRAINING_MOCK_HOST_ADDRESS: {e}"))?,
        };

        let template = crate::training::core::load_training_template(&template_path)?;

        // TD15 boot sweeps: at startup nothing is legitimately in flight.
        let swept_staging = crate::training::staging::sweep_orphan_job_dirs(&staging_root);
        let swept_work = crate::training::staging::sweep_orphan_job_dirs(&work_root);
        let swept_adapters = crate::training::serve::sweep_orphan_adapter_dirs(&staging_root);
        if swept_staging + swept_work + swept_adapters > 0 {
            println!(
                "🧹 Training boot sweep: {swept_staging} staging + {swept_work} work + \
                 {swept_adapters} adapter orphan dirs removed"
            );
        }

        let s5_base = std::env::var("ENHANCED_S5_URL")
            .unwrap_or_else(|_| "http://localhost:5522".to_string());
        let artifact_store: std::sync::Arc<dyn crate::storage::s5_client::S5Storage> = {
            let client =
                crate::storage::enhanced_s5_client::EnhancedS5Client::new_legacy(s5_base.clone())
                    .map_err(|e| format!("S5 client: {e}"))?;
            std::sync::Arc::new(crate::storage::s5_client::EnhancedS5Backend::new(client))
        };

        let env_or = |name: &str| std::env::var(name).unwrap_or_else(|_| "unknown".to_string());
        let env_hash = crate::ltx::attestation::env_hash(&crate::ltx::attestation::EnvMeta {
            weights_hash: env_or("TRAINING_WEIGHTS_HASH"),
            lora_hash: env_or("TRAINING_LORA_HASH"),
            comfy_commit: env_or("TRAINING_STACK_COMMIT"),
            node_commit: env_or("LTX_NODE_COMMIT"),
            cuda_version: env_or("LTX_CUDA_VERSION"),
            gpu_class: env_or("LTX_GPU_CLASS"),
        });

        let env_u64 = |name: &str, default: u64| {
            std::env::var(name)
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(default)
        };
        let mock_seams = std::sync::Arc::new(MockChainSeams::new(
            host_address,
            usdc,
            price,
            model_id,
        ));
        let deps = crate::training::core::TrainingDeps {
            sessions: match &manager {
                Some(manager) => std::sync::Arc::new(ChainSessionReader {
                    manager: manager.clone(),
                }),
                None => mock_seams.clone(),
            },
            completer: match &manager {
                Some(manager) => std::sync::Arc::new(ChainSessionComplete {
                    manager: manager.clone(),
                }),
                None => mock_seams.clone(),
            },
            trainer: std::sync::Arc::new(crate::training::trainer_client::TrainerClient::new(
                std::path::PathBuf::from(socket),
                std::time::Duration::from_secs(env_u64("TRAINER_CLIENT_TIMEOUT_SECS", 600)),
            )),
            attempts: std::sync::Arc::new(crate::training::accept::AttemptRegistry::new()),
            staging_root,
            s5_base,
            host_address,
            model_id,
            expected_price: ethers::types::U256::from(price),
            priced_tokens: vec![usdc],
            template,
            adapters: std::sync::Arc::new(
                crate::training::serve::AdapterRegistry::new(),
            ),
            accept_cfg: crate::training::accept::AcceptConfig {
                train_job_timeout_secs: env_u64("TRAIN_JOB_TIMEOUT_SECS", 12_600),
                settle_margin_secs: 600,
                min_proof_timeout_window_secs: 3_600,
            },
            cooldown_secs: env_u64("TRAIN_ACCEPT_COOLDOWN_SECS", 60),
            settle_buffer_secs: env_u64("DISPUTE_WINDOW_BUFFER_SECS", 45),
            capacity_cache: std::sync::Mutex::new(None),
            work_root,
            artifact_store,
            proof: match &manager {
                Some(manager) => manager.clone(),
                None => mock_seams.clone(),
            },
            tracker: std::sync::Arc::new(crate::training::tracker::TrainTracker::new()),
            node_key,
            env_hash,
            rate_limit_tokens_per_sec: env_u64("TRAINING_RATE_LIMIT_TOKENS_PER_SEC", 10_000),
            completing_latch: std::time::Duration::from_secs(30),
            allow_list_version: env_u64("TRAINING_ALLOWLIST_VERSION", 1),
        };
        server.set_training_deps(std::sync::Arc::new(deps)).await;
        println!("🎓 Training M0 wired: capacity hint live at /v1/training/capacity");
        Ok(())
    }
    .await;
    if let Err(missing) = wired {
        println!("⚠️  Training wiring failed ({missing}) — training disabled");
    }
}

// ---------------------------------------------------------------------------
// T6 support: the mock-chain mode. The plan specifies the T6 GPU gate as
// "TRAIN_ENABLED=1, mock chain", but the production wiring requires a real
// CheckpointManager (it is the source of both chain seams AND the proof
// submitter). Rather than have an UNPAID gate need a funded Base Sepolia
// session, `TRAIN_MOCK_CHAIN=true` swaps the three seams for in-memory ones
// that satisfy A.3 by construction. Paid behaviour is unchanged and remains
// entirely T7's business.
// ---------------------------------------------------------------------------

/// A synthetic Active session that passes every A.3 gate by construction,
/// built from the same env values the node advertises.
pub struct MockChainSeams {
    snapshot: crate::training::accept::SessionSnapshot,
    model_id: [u8; 32],
}

impl MockChainSeams {
    pub fn new(
        host: ethers::types::Address,
        payment_token: ethers::types::Address,
        price: u64,
        model_id: [u8; 32],
    ) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        MockChainSeams {
            snapshot: crate::training::accept::SessionSnapshot {
                depositor: host,
                host,
                payment_token,
                // Generous headroom: an unpaid gate must never reject on
                // deposit maths.
                deposit: ethers::types::U256::from(u64::MAX / 2),
                price_per_token: ethers::types::U256::from(price),
                tokens_used: ethers::types::U256::zero(),
                max_duration: ethers::types::U256::from(14_400u64),
                start_time: ethers::types::U256::from(now),
                proof_timeout_window: ethers::types::U256::from(3_600u64),
                status: crate::training::accept::SessionStatus::Active,
            },
            model_id,
        }
    }
}

#[async_trait::async_trait]
impl SessionReader for MockChainSeams {
    async fn session_snapshot(
        &self,
        _job_id: u64,
    ) -> Result<crate::training::accept::SessionSnapshot, String> {
        // start_time tracks the real clock so the C.3 completion floor
        // behaves as it will in production.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        Ok(crate::training::accept::SessionSnapshot {
            start_time: ethers::types::U256::from(now),
            ..self.snapshot.clone()
        })
    }
    async fn session_model(&self, _job_id: u64) -> Result<[u8; 32], String> {
        Ok(self.model_id)
    }
    async fn dispute_window_secs(&self) -> u64 {
        0 // no chain to wait for
    }
}

#[async_trait::async_trait]
impl SessionComplete for MockChainSeams {
    async fn complete_session(&self, job_id: u64) -> Result<(), String> {
        println!("🧪 [mock chain] completeSessionJob({job_id})");
        Ok(())
    }
}

#[async_trait::async_trait]
impl crate::ltx::submit::ProofSubmit for MockChainSeams {
    async fn submit_ltx_proof(
        &self,
        job_id: u64,
        tokens: u64,
        _proof_hash: [u8; 32],
        proof_cid: String,
    ) -> anyhow::Result<ethers::types::H256> {
        println!("🧪 [mock chain] submitProofOfWork(job {job_id}, {tokens} tokens, {proof_cid})");
        Ok(ethers::types::H256::zero())
    }
    async fn session_proof_interval(&self, _job_id: u64) -> u64 {
        1000
    }
}
