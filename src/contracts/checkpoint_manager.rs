// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::{anyhow, Result};
use ethers::abi::Token;
use ethers::prelude::*;
use ethers::types::Bytes;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use super::client::Web3Client;

// S5 decentralized storage for off-chain proof storage (Phase 2.1)
use crate::storage::s5_client::{S5Client, S5Storage};

#[cfg(feature = "real-ezkl")]
use crate::crypto::ezkl::{EzklProver, WitnessBuilder};

const CHECKPOINT_THRESHOLD: u64 = 1000; // Submit checkpoint every 1000 tokens (production value to minimize streaming pauses)
                                       // Minimum tokens required for checkpoint submission (contract requirement)
const MIN_PROVEN_TOKENS: u64 = 100;

#[derive(Debug, Clone)]
pub struct JobTokenTracker {
    pub job_id: u64,
    pub tokens_generated: u64,
    pub last_checkpoint: u64,
    pub session_id: Option<String>,
    pub submission_in_progress: bool,
    pub last_proof_timestamp: Option<std::time::Instant>, // Track when last proof was submitted
}

pub struct CheckpointManager {
    web3_client: Arc<Web3Client>,
    job_trackers: Arc<RwLock<HashMap<u64, JobTokenTracker>>>,
    proof_system_address: Address,
    host_address: Address,
    s5_storage: Box<dyn S5Storage>, // S5 storage for off-chain proof storage
}

impl CheckpointManager {
    pub async fn new(web3_client: Arc<Web3Client>) -> Result<Self> {
        // Read JobMarketplace address from environment variable - REQUIRED, NO FALLBACK
        let job_marketplace_address = std::env::var("CONTRACT_JOB_MARKETPLACE")
            .expect("❌ FATAL: CONTRACT_JOB_MARKETPLACE environment variable MUST be set. No fallback addresses allowed.");

        let proof_system_address = job_marketplace_address
            .parse()
            .map_err(|e| anyhow!("Invalid JobMarketplace address: {}", e))?;

        // Get host address from web3 client
        let host_address = web3_client.address();

        // Initialize S5 storage for off-chain proof storage (Phase 2.1)
        let s5_storage = S5Client::create_from_env()
            .await
            .map_err(|e| anyhow!("Failed to initialize S5 storage: {}", e))?;

        info!("✅ S5 storage initialized for off-chain proof storage");

        eprintln!(
            "🏠 CheckpointManager initialized with host address: {:?}",
            host_address
        );
        eprintln!(
            "📝 CONTRACT VERSION: Using JobMarketplace at {}",
            job_marketplace_address
        );
        eprintln!("🔖 BUILD VERSION: {}", crate::version::VERSION);

        Ok(Self {
            web3_client,
            job_trackers: Arc::new(RwLock::new(HashMap::new())),
            proof_system_address,
            host_address,
            s5_storage,
        })
    }

    /// Track tokens generated for a specific job
    pub async fn track_tokens(
        &self,
        job_id: u64,
        tokens: u64,
        session_id: Option<String>,
    ) -> Result<()> {
        println!(
            "🔔 CHECKPOINT MANAGER: track_tokens called for job {} with {} tokens",
            job_id, tokens
        );

        let mut trackers = self.job_trackers.write().await;

        let tracker = trackers.entry(job_id).or_insert_with(|| {
            info!("Starting token tracking for job {}", job_id);
            JobTokenTracker {
                job_id,
                tokens_generated: 0,
                last_checkpoint: 0,
                session_id,
                submission_in_progress: false,
                last_proof_timestamp: None,
            }
        });

        tracker.tokens_generated += tokens;
        info!(
            "Generated {} tokens for job {} (total: {}, last checkpoint: {})",
            tokens, job_id, tracker.tokens_generated, tracker.last_checkpoint
        );

        // Check if we need to submit a checkpoint
        let tokens_since_checkpoint = tracker.tokens_generated - tracker.last_checkpoint;
        println!("🔍 Checkpoint check: job {} has {} total tokens, {} since last checkpoint (threshold: {})",
                 job_id, tracker.tokens_generated, tokens_since_checkpoint, CHECKPOINT_THRESHOLD);

        if tokens_since_checkpoint >= CHECKPOINT_THRESHOLD && !tracker.submission_in_progress {
            println!(
                "🚨 TRIGGERING CHECKPOINT for job {} with {} tokens!",
                job_id, tracker.tokens_generated
            );
            // CRITICAL FIX: Submit only the DELTA (tokens since last checkpoint), not cumulative total
            let mut tokens_to_submit = tokens_since_checkpoint;
            let previous_checkpoint = tracker.last_checkpoint; // Save for rollback
            let is_first_checkpoint = previous_checkpoint == 0;

            // ONLY pad the first checkpoint if it's less than MIN_PROVEN_TOKENS
            if is_first_checkpoint && tokens_to_submit < MIN_PROVEN_TOKENS {
                info!(
                    "📝 Padding FIRST checkpoint from {} to {} tokens (contract minimum) for job {}",
                    tokens_to_submit, MIN_PROVEN_TOKENS, job_id
                );
                tokens_to_submit = MIN_PROVEN_TOKENS;
            }

            // Mark submission as in progress to prevent race conditions
            tracker.submission_in_progress = true;
            // Optimistically update the checkpoint to reflect cumulative tokens proven
            tracker.last_checkpoint = tracker.tokens_generated;

            drop(trackers); // Release lock before async operation

            info!(
                "Threshold reached for job {} - submitting checkpoint with {} tokens (delta since last checkpoint)",
                job_id, tokens_to_submit
            );

            // ASYNC CHECKPOINT SUBMISSION: Spawn background task to avoid blocking streaming
            // Clone the necessary data for the spawned task
            let web3_client = self.web3_client.clone();
            let job_trackers = self.job_trackers.clone();
            let proof_system_address = self.proof_system_address;
            let host_address = self.host_address;
            let s5_storage = self.s5_storage.clone();

            tokio::spawn(async move {
                info!("🚀 [ASYNC] Starting background checkpoint submission for job {}", job_id);

                // Create a temporary checkpoint submitter with cloned data
                let submission_result = Self::submit_checkpoint_async(
                    web3_client,
                    s5_storage,
                    proof_system_address,
                    host_address,
                    job_id,
                    tokens_to_submit,
                ).await;

                // Update tracker based on result
                let mut trackers = job_trackers.write().await;
                if let Some(tracker) = trackers.get_mut(&job_id) {
                    tracker.submission_in_progress = false; // Clear the flag

                    if let Err(e) = submission_result {
                        error!("Failed to submit checkpoint for job {}: {}", job_id, e);
                        // Rollback to previous checkpoint value so we can retry
                        tracker.last_checkpoint = previous_checkpoint;
                        warn!(
                            "Rolled back checkpoint for job {} to {} tokens (will retry on next token)",
                            job_id, previous_checkpoint
                        );
                    } else {
                        // Update timestamp to track when this proof was submitted (for dispute window)
                        tracker.last_proof_timestamp = Some(std::time::Instant::now());
                        info!(
                            "✅ [ASYNC] Checkpoint submitted successfully for job {} - dispute window starts now",
                            job_id
                        );
                    }
                }
            });
        } else if tracker.submission_in_progress {
            info!(
                "Checkpoint submission already in progress for job {} - skipping",
                job_id
            );
        }

        Ok(())
    }

    /// Upload proof to S5 decentralized storage and return CID (Phase 2.2)
    async fn upload_proof_to_s5(&self, job_id: u64, proof_bytes: &[u8]) -> Result<String> {
        info!(
            "📤 Uploading proof to S5 for job {} ({} bytes, {:.2} KB)",
            job_id,
            proof_bytes.len(),
            proof_bytes.len() as f64 / 1024.0
        );

        // Use a path that works with both Mock and EnhancedS5 backends
        let proof_path = format!("home/proofs/job_{}_proof.bin", job_id);

        // Upload to S5 - this will return a CID
        let cid = self
            .s5_storage
            .put(&proof_path, proof_bytes.to_vec())
            .await
            .map_err(|e| anyhow!("S5 upload failed: {}", e))?;

        info!("✅ Proof uploaded to S5 successfully");
        info!("   Path: {}", proof_path);
        info!("   CID: {}", cid);
        info!(
            "   Size: {} bytes ({:.2} KB)",
            proof_bytes.len(),
            proof_bytes.len() as f64 / 1024.0
        );

        Ok(cid)
    }

    /// Generate cryptographic proof of work
    /// Creates witness from available data and generates Risc0 STARK proof
    fn generate_proof(&self, job_id: u64, tokens_generated: u64) -> Result<Vec<u8>> {
        #[cfg(feature = "real-ezkl")]
        {
            info!(
                "🔐 Generating real Risc0 STARK proof for job {} ({} tokens)",
                job_id, tokens_generated
            );

            // Create witness from available data
            // job_id: Convert u64 to [u8; 32] by creating SHA256 hash
            let mut job_id_bytes = [0u8; 32];
            let job_id_hash = Sha256::digest(job_id.to_le_bytes());
            job_id_bytes.copy_from_slice(&job_id_hash);

            // model_hash: Get from MODEL_PATH environment variable
            let model_path =
                std::env::var("MODEL_PATH").unwrap_or_else(|_| "./models/default.gguf".to_string());
            let model_hash = Sha256::digest(model_path.as_bytes());
            let mut model_hash_bytes = [0u8; 32];
            model_hash_bytes.copy_from_slice(&model_hash);

            // input_hash: Deterministic hash from job_id + "input" marker
            let input_data = format!("job_{}:input", job_id);
            let input_hash = Sha256::digest(input_data.as_bytes());
            let mut input_hash_bytes = [0u8; 32];
            input_hash_bytes.copy_from_slice(&input_hash);

            // output_hash: Deterministic hash from job_id + tokens + "output" marker
            let output_data = format!("job_{}:output:tokens_{}", job_id, tokens_generated);
            let output_hash = Sha256::digest(output_data.as_bytes());
            let mut output_hash_bytes = [0u8; 32];
            output_hash_bytes.copy_from_slice(&output_hash);

            // Build witness
            let witness = WitnessBuilder::new()
                .with_job_id(job_id_bytes)
                .with_model_hash(model_hash_bytes)
                .with_input_hash(input_hash_bytes)
                .with_output_hash(output_hash_bytes)
                .build()
                .map_err(|e| anyhow!("Failed to build witness: {}", e))?;

            // Generate proof
            let mut prover = EzklProver::new();
            let proof_data = prover
                .generate_proof(&witness)
                .map_err(|e| anyhow!("Failed to generate proof: {}", e))?;

            info!(
                "✅ STARK proof generated: {} bytes ({:.2} KB)",
                proof_data.proof_bytes.len(),
                proof_data.proof_bytes.len() as f64 / 1024.0
            );

            Ok(proof_data.proof_bytes)
        }

        #[cfg(not(feature = "real-ezkl"))]
        {
            // Mock proof generation (for testing without real-ezkl feature)
            warn!("🎭 Generating mock proof (real-ezkl feature not enabled)");
            let proof_json = serde_json::json!({
                "timestamp": chrono::Utc::now().timestamp_millis(),
                "tokensUsed": tokens_generated,
                "hostAddress": format!("{:?}", self.host_address),
                "jobId": job_id,
                "mock": true
            });
            Ok(proof_json.to_string().into_bytes())
        }
    }

    /// Submit checkpoint to the blockchain
    /// IMPORTANT: tokens_generated should be the INCREMENTAL tokens since last checkpoint
    async fn submit_checkpoint(&self, job_id: u64, tokens_generated: u64) -> Result<()> {
        // CRITICAL: For incremental checkpoints, submit the actual amount
        // Do NOT pad incremental checkpoints - only the first checkpoint should be padded if needed
        // The padding logic should be handled by the caller based on whether this is the first checkpoint
        let tokens_to_submit = tokens_generated;

        info!(
            "Submitting proof of work for job {} with {} tokens (actual: {})...",
            job_id, tokens_to_submit, tokens_generated
        );

        // Generate STARK proof using Risc0 zkVM
        let proof_bytes = self.generate_proof(job_id, tokens_generated)?;

        info!(
            "📊 Proof generated: {} bytes ({:.2} KB)",
            proof_bytes.len(),
            proof_bytes.len() as f64 / 1024.0
        );

        // Calculate SHA256 hash of proof (v8.1.2)
        let mut hasher = Sha256::new();
        hasher.update(&proof_bytes);
        let proof_hash = hasher.finalize();
        let proof_hash_bytes: [u8; 32] = proof_hash.into();

        info!("📊 Proof hash: 0x{}", hex::encode(&proof_hash_bytes));

        // Upload proof to S5 decentralized storage and get CID (Phase 2.2)
        let proof_cid = self.upload_proof_to_s5(job_id, &proof_bytes).await?;

        // Encode contract call with hash + CID (NEW v8.1.2 signature)
        let data = encode_checkpoint_call(job_id, tokens_to_submit, proof_hash_bytes, proof_cid);

        info!(
            "📦 Transaction size: {} bytes (was {}KB proof - 737x reduction!)",
            data.len(),
            proof_bytes.len() / 1024
        );

        // Send transaction with the correct method signature
        match self
            .web3_client
            .send_transaction(
                self.proof_system_address,
                U256::zero(), // No ETH value sent
                Some(data.into()),
            )
            .await
        {
            Ok(tx_hash) => {
                info!(
                    "Transaction sent for job {} - tx_hash: {:?}",
                    job_id, tx_hash
                );
                info!(
                    "⏳ Waiting for confirmation (this can take 15-30 seconds on Base Sepolia)..."
                );

                // CRITICAL: Wait for confirmation with proper timeout
                // Base Sepolia can take 15-30 seconds for confirmation
                let start_time = std::time::Instant::now();

                // Use tokio timeout to wait up to 60 seconds for confirmation
                match tokio::time::timeout(
                    Duration::from_secs(60),
                    self.web3_client.wait_for_confirmation(tx_hash),
                )
                .await
                {
                    Ok(Ok(receipt)) => {
                        let elapsed = start_time.elapsed();
                        info!(
                            "✅ Transaction confirmed after {:.1}s for job {}",
                            elapsed.as_secs_f32(),
                            job_id
                        );

                        if receipt.status == Some(U64::from(1)) {
                            let host_pct = std::env::var("HOST_EARNINGS_PERCENTAGE")
                                .unwrap_or_else(|_| "90".to_string());
                            let treasury_pct = std::env::var("TREASURY_FEE_PERCENTAGE")
                                .unwrap_or_else(|_| "10".to_string());
                            info!(
                                "✅ Checkpoint SUCCESS for job {} - payment distributed ({}% host, {}% treasury)",
                                job_id, host_pct, treasury_pct
                            );
                            info!("Transaction receipt: {:?}", receipt);
                        } else {
                            error!(
                                "❌ Checkpoint transaction FAILED for job {} - status: {:?}",
                                job_id, receipt.status
                            );
                            return Err(anyhow!(
                                "Transaction failed with status: {:?}",
                                receipt.status
                            ));
                        }
                    }
                    Ok(Err(e)) => {
                        error!("❌ Failed to get receipt for job {}: {}", job_id, e);
                        return Err(anyhow!("Failed to get transaction receipt: {}", e));
                    }
                    Err(_) => {
                        error!(
                            "❌ TIMEOUT waiting for confirmation after 60 seconds for job {} - tx_hash: {:?}",
                            job_id, tx_hash
                        );
                        // Don't fail here - transaction might still succeed
                        warn!(
                            "Transaction might still be pending. Check tx_hash: {:?}",
                            tx_hash
                        );
                    }
                }

                Ok(())
            }
            Err(e) => {
                error!("❌ Failed to send transaction for job {}: {}", job_id, e);
                Err(anyhow!("Transaction send failed: {}", e))
            }
        }
    }

    /// Async checkpoint submission for background tasks (static method)
    /// This is used by tokio::spawn to submit checkpoints without blocking streaming
    async fn submit_checkpoint_async(
        web3_client: Arc<Web3Client>,
        s5_storage: Box<dyn S5Storage>,
        proof_system_address: Address,
        host_address: Address,
        job_id: u64,
        tokens_generated: u64,
    ) -> Result<()> {
        let tokens_to_submit = tokens_generated;

        info!(
            "🚀 [ASYNC] Submitting proof of work for job {} with {} tokens...",
            job_id, tokens_to_submit
        );

        // Generate STARK proof using Risc0 zkVM (static version)
        // CRITICAL: Use spawn_blocking for CPU-intensive proof generation
        // to avoid blocking the Tokio async runtime and freezing streaming
        let proof_bytes = tokio::task::spawn_blocking(move || {
            Self::generate_proof_static(job_id, tokens_generated, host_address)
        })
        .await
        .map_err(|e| anyhow!("Proof generation task failed: {}", e))??;

        info!(
            "📊 [ASYNC] Proof generated: {} bytes ({:.2} KB)",
            proof_bytes.len(),
            proof_bytes.len() as f64 / 1024.0
        );

        // Calculate SHA256 hash of proof (v8.1.2)
        let mut hasher = Sha256::new();
        hasher.update(&proof_bytes);
        let proof_hash = hasher.finalize();
        let proof_hash_bytes: [u8; 32] = proof_hash.into();

        info!("📊 [ASYNC] Proof hash: 0x{}", hex::encode(&proof_hash_bytes));

        // Upload proof to S5 decentralized storage and get CID (Phase 2.2)
        let proof_cid = Self::upload_proof_to_s5_static(&s5_storage, job_id, &proof_bytes).await?;

        // Encode contract call with hash + CID (NEW v8.1.2 signature)
        let data = encode_checkpoint_call(job_id, tokens_to_submit, proof_hash_bytes, proof_cid);

        info!(
            "📦 [ASYNC] Transaction size: {} bytes (was {}KB proof - 737x reduction!)",
            data.len(),
            proof_bytes.len() / 1024
        );

        // Send transaction - FIRE AND FORGET for non-blocking streaming
        match web3_client
            .send_transaction(
                proof_system_address,
                U256::zero(), // No ETH value sent
                Some(data.into()),
            )
            .await
        {
            Ok(tx_hash) => {
                info!(
                    "📤 [ASYNC] Transaction sent for job {} - tx_hash: {:?}",
                    job_id, tx_hash
                );

                // FIRE AND FORGET: Don't wait for confirmation to avoid blocking
                // The transaction is on-chain and will be confirmed eventually
                // We spawn a background task to log confirmation status
                let web3_client_clone = web3_client.clone();
                tokio::spawn(async move {
                    // Wait for 1 confirmation with short timeout (15s)
                    match tokio::time::timeout(
                        Duration::from_secs(15),
                        web3_client_clone.wait_for_confirmation(tx_hash),
                    )
                    .await
                    {
                        Ok(Ok(receipt)) => {
                            if receipt.status == Some(U64::from(1)) {
                                info!(
                                    "✅ [ASYNC-BG] Checkpoint confirmed for job {} - tx: {:?}",
                                    job_id, tx_hash
                                );
                            } else {
                                error!(
                                    "❌ [ASYNC-BG] Checkpoint FAILED for job {} - tx: {:?}",
                                    job_id, tx_hash
                                );
                            }
                        }
                        Ok(Err(e)) => {
                            warn!("[ASYNC-BG] Receipt error for job {}: {} - tx may still succeed", job_id, e);
                        }
                        Err(_) => {
                            info!("[ASYNC-BG] Confirmation pending for job {} - tx: {:?}", job_id, tx_hash);
                        }
                    }
                });

                Ok(())
            }
            Err(e) => {
                error!("❌ [ASYNC] Failed to send transaction for job {}: {}", job_id, e);
                Err(anyhow!("Transaction send failed: {}", e))
            }
        }
    }

    /// Static version of generate_proof for async tasks
    fn generate_proof_static(job_id: u64, tokens_generated: u64, host_address: Address) -> Result<Vec<u8>> {
        #[cfg(feature = "real-ezkl")]
        {
            info!(
                "🔐 [ASYNC] Generating real Risc0 STARK proof for job {} ({} tokens)",
                job_id, tokens_generated
            );

            // Create witness from available data
            let mut job_id_bytes = [0u8; 32];
            let job_id_hash = Sha256::digest(job_id.to_le_bytes());
            job_id_bytes.copy_from_slice(&job_id_hash);

            let model_path =
                std::env::var("MODEL_PATH").unwrap_or_else(|_| "./models/default.gguf".to_string());
            let model_hash = Sha256::digest(model_path.as_bytes());
            let mut model_hash_bytes = [0u8; 32];
            model_hash_bytes.copy_from_slice(&model_hash);

            let input_data = format!("job_{}:input", job_id);
            let input_hash = Sha256::digest(input_data.as_bytes());
            let mut input_hash_bytes = [0u8; 32];
            input_hash_bytes.copy_from_slice(&input_hash);

            let output_data = format!("job_{}:output:tokens_{}", job_id, tokens_generated);
            let output_hash = Sha256::digest(output_data.as_bytes());
            let mut output_hash_bytes = [0u8; 32];
            output_hash_bytes.copy_from_slice(&output_hash);

            let witness = WitnessBuilder::new()
                .with_job_id(job_id_bytes)
                .with_model_hash(model_hash_bytes)
                .with_input_hash(input_hash_bytes)
                .with_output_hash(output_hash_bytes)
                .build()
                .map_err(|e| anyhow!("Failed to build witness: {}", e))?;

            let mut prover = EzklProver::new();
            let proof_data = prover
                .generate_proof(&witness)
                .map_err(|e| anyhow!("Failed to generate proof: {}", e))?;

            info!(
                "✅ [ASYNC] STARK proof generated: {} bytes ({:.2} KB)",
                proof_data.proof_bytes.len(),
                proof_data.proof_bytes.len() as f64 / 1024.0
            );

            Ok(proof_data.proof_bytes)
        }

        #[cfg(not(feature = "real-ezkl"))]
        {
            warn!("🎭 [ASYNC] Generating mock proof (real-ezkl feature not enabled)");
            let proof_json = serde_json::json!({
                "timestamp": chrono::Utc::now().timestamp_millis(),
                "tokensUsed": tokens_generated,
                "hostAddress": format!("{:?}", host_address),
                "jobId": job_id,
                "mock": true
            });
            Ok(proof_json.to_string().into_bytes())
        }
    }

    /// Static version of upload_proof_to_s5 for async tasks
    async fn upload_proof_to_s5_static(
        s5_storage: &Box<dyn S5Storage>,
        job_id: u64,
        proof_bytes: &[u8],
    ) -> Result<String> {
        info!(
            "📤 [ASYNC] Uploading proof to S5 for job {} ({} bytes, {:.2} KB)",
            job_id,
            proof_bytes.len(),
            proof_bytes.len() as f64 / 1024.0
        );

        let proof_path = format!("home/proofs/job_{}_proof.bin", job_id);

        let cid = s5_storage
            .put(&proof_path, proof_bytes.to_vec())
            .await
            .map_err(|e| anyhow!("S5 upload failed: {}", e))?;

        info!("✅ [ASYNC] Proof uploaded to S5 successfully");
        info!("   Path: {}", proof_path);
        info!("   CID: {}", cid);
        info!(
            "   Size: {} bytes ({:.2} KB)",
            proof_bytes.len(),
            proof_bytes.len() as f64 / 1024.0
        );

        Ok(cid)
    }

    /// Force submit checkpoint for a job when completing session (ignores MIN_PROVEN_TOKENS)
    pub async fn force_checkpoint_on_completion(&self, job_id: u64) -> Result<()> {
        // Use write lock to ensure consistency
        let mut trackers = self.job_trackers.write().await;

        if let Some(tracker) = trackers.get_mut(&job_id) {
            let tokens_since_checkpoint = tracker.tokens_generated - tracker.last_checkpoint;

            // Check if submission is already in progress
            if tracker.submission_in_progress {
                info!(
                    "⏸️ Skipping force checkpoint for job {} - submission already in progress",
                    job_id
                );
                return Ok(());
            }

            // For session completion, submit ANY tokens we have (even if < MIN_PROVEN_TOKENS)
            if tokens_since_checkpoint > 0 {
                let mut tokens_to_submit = tokens_since_checkpoint; // Submit ONLY the delta, not total
                let previous_checkpoint = tracker.last_checkpoint;
                let is_first_checkpoint = previous_checkpoint == 0;

                // ONLY pad the first checkpoint if it's less than MIN_PROVEN_TOKENS
                if is_first_checkpoint && tokens_to_submit < MIN_PROVEN_TOKENS {
                    info!(
                        "📝 Padding FIRST checkpoint from {} to {} tokens (contract minimum) for job {}",
                        tokens_to_submit, MIN_PROVEN_TOKENS, job_id
                    );
                    tokens_to_submit = MIN_PROVEN_TOKENS;
                } else if !is_first_checkpoint && tokens_to_submit < MIN_PROVEN_TOKENS {
                    // Not first checkpoint and below minimum - contract will reject this
                    warn!(
                        "⚠️ Skipping final checkpoint for job {} - only {} tokens remaining (below minimum {})",
                        job_id, tokens_to_submit, MIN_PROVEN_TOKENS
                    );
                    warn!(
                        "   Total tracked: {} tokens, Last proven: {} tokens, Lost: {} tokens",
                        tracker.tokens_generated, previous_checkpoint, tokens_to_submit
                    );
                    warn!(
                        "   These {} tokens will NOT be charged to the user",
                        tokens_to_submit
                    );
                    drop(trackers);
                    return Ok(());
                }

                info!(
                    "💪 Force submitting checkpoint on completion for job {} with {} new tokens (total: {})",
                    job_id, tokens_to_submit, tracker.tokens_generated
                );

                // Mark as in progress and update checkpoint to reflect total tokens proven
                tracker.submission_in_progress = true;
                tracker.last_checkpoint = tracker.tokens_generated; // Update to total after submission

                drop(trackers); // Release lock for async operation

                // SYNCHRONOUS CHECKPOINT SUBMISSION: Must wait for proof to be on-chain
                // before calling completeSessionJob, otherwise contract thinks 0 tokens used!
                info!("🔒 [SYNC-FINAL] Submitting final checkpoint for job {} (waiting for confirmation)...", job_id);

                let submission_result = Self::submit_checkpoint_async(
                    self.web3_client.clone(),
                    self.s5_storage.clone(),
                    self.proof_system_address,
                    self.host_address,
                    job_id,
                    tokens_to_submit,
                ).await;

                // Update tracker based on result
                let mut trackers = self.job_trackers.write().await;
                if let Some(tracker) = trackers.get_mut(&job_id) {
                    tracker.submission_in_progress = false;

                    if let Err(ref e) = submission_result {
                        error!("❌ [SYNC-FINAL] Failed to submit final checkpoint for job {}: {}", job_id, e);
                        tracker.last_checkpoint = previous_checkpoint;
                    } else {
                        tracker.last_proof_timestamp = Some(std::time::Instant::now());
                        info!(
                            "✅ [SYNC-FINAL] Successfully submitted proof of {} tokens for job {}",
                            tokens_to_submit, job_id
                        );
                    }
                }
                drop(trackers);

                // Propagate the error if submission failed
                if let Err(e) = submission_result {
                    return Err(e);
                }

                info!("✅ Final checkpoint confirmed for job {} - safe to complete session", job_id);
            } else {
                info!(
                    "⚠️ No new tokens to submit for job {} (0 tokens since last checkpoint)",
                    job_id
                );
            }
        } else {
            error!(
                "❌ No tracker found for job {} - tokens were never tracked!",
                job_id
            );
            error!("   This means HTTP inference didn't track tokens for this job ID");
            error!("   Check if job_id/session_id is correctly passed in inference requests");
        }

        Ok(())
    }

    /// Force submit checkpoint for a job (e.g., when session ends)
    /// FULLY ASYNC: Spawns background task immediately without blocking for locks
    pub async fn force_checkpoint(&self, job_id: u64) -> Result<()> {
        // Clone all necessary data BEFORE spawning to avoid blocking
        let job_trackers = self.job_trackers.clone();
        let web3_client = self.web3_client.clone();
        let proof_system_address = self.proof_system_address;
        let host_address = self.host_address;
        let s5_storage = self.s5_storage.clone();

        info!("🚀 [FORCE-CHECKPOINT] Spawning background task for job {} (returns immediately)", job_id);

        // Spawn the entire force_checkpoint logic in background
        // This ensures the caller NEVER blocks waiting for locks
        tokio::spawn(async move {
            info!("🔄 [FORCE-CHECKPOINT-BG] Starting background force checkpoint for job {}", job_id);

            // Acquire lock inside the spawned task (not blocking caller)
            let mut trackers = job_trackers.write().await;

            let (tokens_to_submit, previous_checkpoint) = if let Some(tracker) = trackers.get_mut(&job_id) {
                let tokens_since_checkpoint = tracker.tokens_generated - tracker.last_checkpoint;

                // Check if submission is already in progress
                if tracker.submission_in_progress {
                    info!(
                        "⏸️ [FORCE-CHECKPOINT-BG] Skipping - submission already in progress for job {}",
                        job_id
                    );
                    return;
                }

                // Only submit if we have at least MIN_PROVEN_TOKENS since last checkpoint
                if tokens_since_checkpoint < MIN_PROVEN_TOKENS {
                    if tokens_since_checkpoint > 0 {
                        info!(
                            "⏸️ [FORCE-CHECKPOINT-BG] Skipping job {} - only {} tokens (minimum: {})",
                            job_id, tokens_since_checkpoint, MIN_PROVEN_TOKENS
                        );
                    }
                    return;
                }

                // We have enough tokens to submit
                let mut tokens_to_submit = tokens_since_checkpoint;
                let previous_checkpoint = tracker.last_checkpoint;
                let is_first_checkpoint = previous_checkpoint == 0;

                // ONLY pad the first checkpoint if it's less than MIN_PROVEN_TOKENS
                if is_first_checkpoint && tokens_to_submit < MIN_PROVEN_TOKENS {
                    info!(
                        "📝 [FORCE-CHECKPOINT-BG] Padding FIRST checkpoint from {} to {} tokens for job {}",
                        tokens_to_submit, MIN_PROVEN_TOKENS, job_id
                    );
                    tokens_to_submit = MIN_PROVEN_TOKENS;
                }

                info!(
                    "📤 [FORCE-CHECKPOINT-BG] Submitting {} new tokens for job {} (total: {})",
                    tokens_to_submit, job_id, tracker.tokens_generated
                );

                // Mark as in progress and update checkpoint
                tracker.submission_in_progress = true;
                tracker.last_checkpoint = tracker.tokens_generated;

                (tokens_to_submit, previous_checkpoint)
            } else {
                info!("⚠️ [FORCE-CHECKPOINT-BG] No tracker found for job {}", job_id);
                return;
            };

            // Release lock before async operation
            drop(trackers);

            // Submit checkpoint (this is the slow part)
            let submission_result = Self::submit_checkpoint_async(
                web3_client,
                s5_storage,
                proof_system_address,
                host_address,
                job_id,
                tokens_to_submit,
            ).await;

            // Update tracker based on result
            let mut trackers = job_trackers.write().await;
            if let Some(tracker) = trackers.get_mut(&job_id) {
                tracker.submission_in_progress = false;

                if let Err(e) = submission_result {
                    error!("❌ [FORCE-CHECKPOINT-BG] Failed for job {}: {}", job_id, e);
                    tracker.last_checkpoint = previous_checkpoint;
                } else {
                    tracker.last_proof_timestamp = Some(std::time::Instant::now());
                    info!("✅ [FORCE-CHECKPOINT-BG] Success for job {}", job_id);
                }
            }
        });

        Ok(())
    }

    /// Get current token count for a job
    pub async fn get_token_count(&self, job_id: u64) -> u64 {
        let trackers = self.job_trackers.read().await;
        trackers
            .get(&job_id)
            .map(|t| t.tokens_generated)
            .unwrap_or(0)
    }

    /// Complete a session job and trigger payment settlement
    pub async fn complete_session_job(
        &self,
        job_id: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("[CHECKPOINT-MGR] 🎯 === STARTING PAYMENT SETTLEMENT PROCESS ===");
        info!("[CHECKPOINT-MGR] Job ID: {}", job_id);
        info!("[CHECKPOINT-MGR] This will trigger on-chain payment distribution");

        // Debug: Check what trackers we have
        {
            let trackers = self.job_trackers.read().await;
            info!(
                "[CHECKPOINT-MGR] 📊 Current tracked jobs: {:?}",
                trackers.keys().collect::<Vec<_>>()
            );
            if let Some(tracker) = trackers.get(&job_id) {
                info!("[CHECKPOINT-MGR]   ✓ Job {} token tracking:", job_id);
                info!(
                    "[CHECKPOINT-MGR]     - Tokens generated: {}",
                    tracker.tokens_generated
                );
                info!(
                    "[CHECKPOINT-MGR]     - Last checkpoint at: {} tokens",
                    tracker.last_checkpoint
                );
                info!(
                    "[CHECKPOINT-MGR]     - Session ID: {:?}",
                    tracker.session_id
                );
            } else {
                error!("[CHECKPOINT-MGR]   ❌ Job {} has NO TRACKER - payment calculation may be affected!", job_id);
            }
        }

        // First submit any pending checkpoint - FORCE submission even if < MIN_PROVEN_TOKENS
        // When completing a session, we must submit whatever tokens we have
        let checkpoint_result = self.force_checkpoint_on_completion(job_id).await;

        // If checkpoint submission failed, we should still try to complete the session
        // but log the error
        if let Err(e) = checkpoint_result {
            error!("⚠️ Checkpoint submission failed for job {}: {}", job_id, e);
            // Continue with session completion anyway
        }

        // IMPORTANT: Check if we need to wait for dispute window before attempting completion
        // Get dispute window duration from environment
        let dispute_window_secs = std::env::var("DISPUTE_WINDOW_SECONDS")
            .unwrap_or_else(|_| "30".to_string())
            .parse::<u64>()
            .unwrap_or(30);

        // Check if we have a recent proof submission
        let trackers = self.job_trackers.read().await;
        if let Some(tracker) = trackers.get(&job_id) {
            if let Some(last_proof_time) = tracker.last_proof_timestamp {
                let elapsed = last_proof_time.elapsed().as_secs();
                if elapsed < dispute_window_secs {
                    let wait_time = dispute_window_secs - elapsed;
                    info!(
                        "⏳ Job {} in dispute window. Last proof submitted {}s ago. Waiting {}s before completion...",
                        job_id, elapsed, wait_time
                    );
                    info!(
                        "   Dispute window: {}s, Elapsed: {}s, Remaining: {}s",
                        dispute_window_secs, elapsed, wait_time
                    );
                    drop(trackers); // Release lock before sleeping
                    tokio::time::sleep(Duration::from_secs(wait_time)).await;
                    info!(
                        "✅ Dispute window elapsed for job {}. Proceeding with completion.",
                        job_id
                    );
                } else {
                    info!(
                        "✅ Job {} dispute window already elapsed ({}s > {}s). Proceeding immediately.",
                        job_id, elapsed, dispute_window_secs
                    );
                    drop(trackers);
                }
            } else {
                info!(
                    "ℹ️ No recent proof timestamp for job {}. Proceeding with completion.",
                    job_id
                );
                drop(trackers);
            }
        } else {
            drop(trackers);
        }

        // Now call completeSessionJob on the contract to trigger payment settlement
        info!(
            "💰 Completing session job {} to trigger payment settlement...",
            job_id
        );

        // TODO: Get actual conversation CID from S5 storage
        // For now, use a placeholder CID to complete the session
        let conversation_cid = format!("session_job_{}_completed", job_id);
        let data = encode_complete_session_call(job_id, conversation_cid);

        // Use the Web3Client's send_transaction which properly signs the transaction
        match self
            .web3_client
            .send_transaction(
                self.proof_system_address,
                U256::zero(), // No ETH value, just calling a function
                Some(data.clone().into()),
            )
            .await
        {
            Ok(tx_hash) => {
                info!(
                    "Transaction sent for completing job {} - tx_hash: {:?}",
                    job_id, tx_hash
                );

                // Wait for confirmation
                let start = std::time::Instant::now();
                match self.web3_client.wait_for_confirmation(tx_hash).await {
                    Ok(receipt) => {
                        let elapsed = start.elapsed().as_secs_f32();
                        info!(
                            "✅ Transaction confirmed after {:.1}s for job {}",
                            elapsed, job_id
                        );

                        if receipt.status == Some(U64::from(1)) {
                            let host_pct = std::env::var("HOST_EARNINGS_PERCENTAGE")
                                .unwrap_or_else(|_| "90".to_string());
                            let treasury_pct = std::env::var("TREASURY_FEE_PERCENTAGE")
                                .unwrap_or_else(|_| "10".to_string());
                            info!(
                                "💰 Session completed and payments distributed for job {}",
                                job_id
                            );
                            info!(
                                "  - Host earnings ({}%) sent to HostEarnings contract",
                                host_pct
                            );
                            info!("  - Treasury fee ({}%) collected", treasury_pct);
                            info!("  - Unused deposit refunded to user");
                        } else {
                            error!("❌ Transaction failed for job {}", job_id);
                        }
                    }
                    Err(e) => {
                        error!("❌ Transaction error for job {}: {:?}", job_id, e);
                    }
                }
            }
            Err(e) => {
                // Check for nonce-related errors and retry with delay
                if e.to_string()
                    .contains("replacement transaction underpriced")
                    || e.to_string().contains("nonce too low")
                {
                    error!("❌ Nonce conflict detected for job {}: {}", job_id, e);
                    info!("⏳ Retrying with 5 second delay to resolve nonce conflict...");

                    // Wait for previous transaction to clear
                    tokio::time::sleep(Duration::from_secs(5)).await;

                    // Retry the transaction once
                    match self
                        .web3_client
                        .send_transaction(
                            self.proof_system_address,
                            U256::zero(),
                            Some(data.clone().into()),
                        )
                        .await
                    {
                        Ok(tx_hash) => {
                            info!(
                                "🔄 Retry successful for job {} - tx_hash: {:?}",
                                job_id, tx_hash
                            );

                            // Wait for confirmation
                            match self.web3_client.wait_for_confirmation(tx_hash).await {
                                Ok(receipt) => {
                                    if receipt.status == Some(U64::from(1)) {
                                        info!("✅ Session completed on retry for job {}", job_id);
                                    } else {
                                        error!("❌ Retry transaction failed for job {}", job_id);
                                    }
                                }
                                Err(e) => {
                                    error!(
                                        "❌ Retry transaction error for job {}: {:?}",
                                        job_id, e
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            error!(
                                "❌ Failed to send complete session transaction for job {}: {}",
                                job_id, e
                            );
                            // Clean up tracker since we failed
                            let mut trackers = self.job_trackers.write().await;
                            if trackers.remove(&job_id).is_some() {
                                info!("Cleaned up tracker for job {} after retry failure", job_id);
                            }
                            return Err(
                                format!("Failed to complete session after retry: {}", e).into()
                            );
                        }
                    }
                }
                // Check if the error is due to dispute window
                else if e.to_string().contains("Must wait dispute window") {
                    // Get dispute window duration from environment or use defaults
                    let dispute_window = std::env::var("DISPUTE_WINDOW_SECONDS")
                        .unwrap_or_else(|_| "30".to_string())
                        .parse::<u64>()
                        .unwrap_or(30);

                    warn!(
                        "⏳ Job {} is in dispute window ({} seconds). Scheduling retry...",
                        job_id, dispute_window
                    );

                    // Schedule a delayed retry with exponential backoff
                    let web3_client = self.web3_client.clone();
                    let proof_system_address = self.proof_system_address;
                    let job_trackers = self.job_trackers.clone();

                    tokio::spawn(async move {
                        let mut retry_count = 0u32;
                        let max_retries = 5; // Max 5 retries for shorter windows

                        // For testing (30s window): 10s, 20s, 40s delays
                        // For production (5min window): 2min, 4min, 8min delays
                        let base_delay = if dispute_window <= 60 {
                            10 // 10 seconds for test environments
                        } else {
                            120 // 2 minutes for production
                        };

                        loop {
                            // Calculate delay with exponential backoff
                            let delay_secs = base_delay * (1 << retry_count);

                            info!(
                                "⏳ Waiting {} seconds before retry {} for job {} (dispute window: {}s)",
                                delay_secs, retry_count + 1, job_id, dispute_window
                            );

                            tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs as u64))
                                .await;

                            info!(
                                "🔄 Retry {} - Attempting to complete session for job {}",
                                retry_count + 1,
                                job_id
                            );

                            // Retry the completion
                            let conversation_cid = format!("session_job_{}_completed", job_id);
                            let data = encode_complete_session_call(job_id, conversation_cid);

                            match web3_client
                                .send_transaction(
                                    proof_system_address,
                                    U256::zero(),
                                    Some(data.clone().into()),
                                )
                                .await
                            {
                                Ok(tx_hash) => {
                                    info!(
                                        "📤 Retry {} transaction sent for job {} - tx_hash: {:?}",
                                        retry_count + 1,
                                        job_id,
                                        tx_hash
                                    );

                                    // Wait for confirmation
                                    match web3_client.wait_for_confirmation(tx_hash).await {
                                        Ok(receipt) => {
                                            if receipt.status == Some(U64::from(1)) {
                                                info!(
                                                    "✅ Session completed for job {} after {} retries",
                                                    job_id, retry_count + 1
                                                );
                                                info!("💰 Payments distributed successfully");

                                                // Clean up tracker after successful completion
                                                let mut trackers = job_trackers.write().await;
                                                if trackers.remove(&job_id).is_some() {
                                                    info!("Cleaned up tracker for job {} after successful retry", job_id);
                                                }
                                                break; // Success! Exit retry loop
                                            } else {
                                                error!(
                                                    "❌ Retry {} transaction failed for job {}",
                                                    retry_count + 1,
                                                    job_id
                                                );
                                                break; // Transaction failed for non-dispute reasons
                                            }
                                        }
                                        Err(e) => {
                                            error!(
                                                "❌ Retry {} transaction error for job {}: {:?}",
                                                retry_count + 1,
                                                job_id,
                                                e
                                            );
                                            break; // Transaction error for non-dispute reasons
                                        }
                                    }
                                }
                                Err(e) => {
                                    let error_msg = e.to_string();

                                    // Check if it's still the dispute window error
                                    if error_msg.contains("Must wait dispute window") {
                                        warn!(
                                            "⏳ Retry {} failed - dispute window still active for job {}",
                                            retry_count + 1, job_id
                                        );

                                        retry_count += 1;
                                        if retry_count >= max_retries {
                                            error!(
                                                "❌ Max retries ({}) reached for job {}. Giving up.",
                                                max_retries, job_id
                                            );

                                            // Clean up tracker since we're giving up
                                            let mut trackers = job_trackers.write().await;
                                            if trackers.remove(&job_id).is_some() {
                                                warn!("Cleaned up tracker for job {} after max retries", job_id);
                                            }
                                            break;
                                        }
                                        // Continue to next retry iteration
                                    } else {
                                        // Different error - stop retrying
                                        error!(
                                            "❌ Failed to send retry {} transaction for job {}: {:?}",
                                            retry_count + 1, job_id, e
                                        );
                                        break;
                                    }
                                }
                            }
                        }
                    });

                    // Don't clean up the tracker yet - we'll need it for the retry
                    return Ok(());
                } else {
                    error!(
                        "❌ Failed to send complete session transaction for job {}: {:?}",
                        job_id, e
                    );
                    // Clean up tracker and return error
                    let mut trackers = self.job_trackers.write().await;
                    if trackers.remove(&job_id).is_some() {
                        info!("Cleaned up tracker for job {} after failure", job_id);
                    }
                    return Err(
                        format!("Failed to send complete session transaction: {}", e).into(),
                    );
                }
            }
        }

        // Clean up tracker (only if not retrying due to dispute window)
        let mut trackers = self.job_trackers.write().await;
        if trackers.remove(&job_id).is_some() {
            info!("Cleaned up tracker for job {}", job_id);
        }

        Ok(())
    }

    /// Clean up job tracker (when job completes)
    pub async fn cleanup_job(&self, job_id: u64) {
        // Force checkpoint if needed
        let _ = self.force_checkpoint(job_id).await;

        // Remove tracker
        let mut trackers = self.job_trackers.write().await;
        if trackers.remove(&job_id).is_some() {
            info!("Cleaned up tracker for job {}", job_id);
        }
    }
}

// ABI encoding helper for completeSessionJob
fn encode_complete_session_call(job_id: u64, conversation_cid: String) -> Vec<u8> {
    use ethers::abi::Function;

    // Define the function signature for completeSessionJob
    let function = Function {
        name: "completeSessionJob".to_string(),
        inputs: vec![
            ethers::abi::Param {
                name: "jobId".to_string(),
                kind: ethers::abi::ParamType::Uint(256),
                internal_type: None,
            },
            ethers::abi::Param {
                name: "conversationCID".to_string(),
                kind: ethers::abi::ParamType::String,
                internal_type: None,
            },
        ],
        outputs: vec![],
        constant: None,
        state_mutability: ethers::abi::StateMutability::NonPayable,
    };

    // Encode the call data with both parameters
    let tokens = vec![
        ethers::abi::Token::Uint(U256::from(job_id)),
        ethers::abi::Token::String(conversation_cid),
    ];
    function.encode_input(&tokens).unwrap()
}

// ABI encoding helper for submitProofOfWork (v8.1.2 - S5 off-chain storage)
// NEW: Accepts proof hash + CID instead of full proof bytes
fn encode_checkpoint_call(
    job_id: u64,
    tokens_generated: u64,
    proof_hash: [u8; 32],
    proof_cid: String,
) -> Vec<u8> {
    use ethers::abi::Function;

    // Define the NEW function signature for submitProofOfWork
    // Contract now accepts: (uint256 jobId, uint256 tokensClaimed, bytes32 proofHash, string proofCID)
    let function = Function {
        name: "submitProofOfWork".to_string(),
        inputs: vec![
            ethers::abi::Param {
                name: "jobId".to_string(),
                kind: ethers::abi::ParamType::Uint(256),
                internal_type: None,
            },
            ethers::abi::Param {
                name: "tokensClaimed".to_string(),
                kind: ethers::abi::ParamType::Uint(256),
                internal_type: None,
            },
            ethers::abi::Param {
                name: "proofHash".to_string(),
                kind: ethers::abi::ParamType::FixedBytes(32), // NEW: bytes32 instead of bytes
                internal_type: None,
            },
            ethers::abi::Param {
                name: "proofCID".to_string(),
                kind: ethers::abi::ParamType::String, // NEW: S5 CID
                internal_type: None,
            },
        ],
        outputs: vec![],
        constant: None,
        state_mutability: ethers::abi::StateMutability::NonPayable,
    };

    // Encode the function call with hash + CID
    let tokens = vec![
        Token::Uint(U256::from(job_id)),
        Token::Uint(U256::from(tokens_generated)),
        Token::FixedBytes(proof_hash.to_vec()), // NEW: 32-byte hash
        Token::String(proof_cid),               // NEW: S5 CID string
    ];

    function
        .encode_input(&tokens)
        .expect("Failed to encode submitProofOfWork call")
}
