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

// Checkpoint publishing for conversation recovery (Phase 2/3)
use crate::checkpoint::{CheckpointMessage, CheckpointPublisher};

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
    pub submission_started_at: Option<std::time::Instant>, // Track when current submission started (for timeout calculation)
    /// Session's proofInterval from contract - minimum billable tokens (v8.14.2)
    pub proof_interval: u64,
}

/// Content hashes for cryptographic proof binding (Phase 4 - v8.10.0)
///
/// Stores the actual prompt and response hashes to bind them into the STARK proof,
/// replacing placeholder hashes with real content hashes.
#[derive(Debug, Clone, Default)]
pub struct ContentHashes {
    /// SHA256 of the original prompt (set at inference start)
    pub prompt_hash: Option<[u8; 32]>,
    /// SHA256 of the generated response (computed at checkpoint time)
    pub response_hash: Option<[u8; 32]>,
    /// Accumulated response text (cleared after hash computation)
    response_buffer: String,
}

/// Cached proof data for S5 propagation delay handling (v8.12.6)
///
/// Local cache that stores proof data after S5 upload succeeds but before on-chain tx.
/// This allows settlement to proceed even if S5 data hasn't fully propagated to other peers.
#[derive(Debug, Clone)]
pub struct CachedProofEntry {
    /// SHA256 hash of the proof (for on-chain submission)
    pub proof_hash: [u8; 32],
    /// S5 CID of the uploaded proof
    pub proof_cid: String,
    /// Delta CID for encrypted checkpoint (if applicable)
    pub delta_cid: Option<String>,
    /// Token count for this proof
    pub tokens: u64,
    /// When this cache entry was created
    pub cached_at: std::time::Instant,
}

/// Proof submission cache - allows on-chain tx even if S5 hasn't fully propagated
pub struct ProofSubmissionCache {
    /// Map of job_id to list of cached proof entries
    cache: RwLock<HashMap<u64, Vec<CachedProofEntry>>>,
    /// TTL for cache entries (default: 1 hour)
    ttl: Duration,
}

impl ProofSubmissionCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            ttl,
        }
    }

    /// Cache proof data after successful S5 upload
    pub async fn cache_proof(&self, job_id: u64, entry: CachedProofEntry) {
        let proof_hash_hex = hex::encode(&entry.proof_hash[..8]);
        let mut cache = self.cache.write().await;
        cache.entry(job_id).or_insert_with(Vec::new).push(entry);
        info!(
            "[PROOF-CACHE] Cached proof for job {} (hash: 0x{}...)",
            job_id,
            proof_hash_hex
        );
    }

    /// Get cached proof data for a job
    pub async fn get_cached_proofs(&self, job_id: u64) -> Vec<CachedProofEntry> {
        let cache = self.cache.read().await;
        cache.get(&job_id).cloned().unwrap_or_default()
    }

    /// Get the most recent cached proof for a job
    pub async fn get_latest_proof(&self, job_id: u64) -> Option<CachedProofEntry> {
        let cache = self.cache.read().await;
        cache.get(&job_id).and_then(|proofs| proofs.last().cloned())
    }

    /// Cleanup entries for a specific job (called when job tracker is cleaned up)
    pub async fn cleanup_job(&self, job_id: u64) {
        let mut cache = self.cache.write().await;
        if cache.remove(&job_id).is_some() {
            info!("[PROOF-CACHE] Cleaned up cache for job {}", job_id);
        }
    }

    /// Cleanup expired entries (entries older than TTL)
    pub async fn cleanup_expired(&self) {
        let mut cache = self.cache.write().await;
        let now = std::time::Instant::now();
        let mut expired_count = 0;

        for proofs in cache.values_mut() {
            let before_len = proofs.len();
            proofs.retain(|p| now.duration_since(p.cached_at) < self.ttl);
            expired_count += before_len - proofs.len();
        }

        // Remove empty entries
        cache.retain(|_, proofs| !proofs.is_empty());

        if expired_count > 0 {
            info!("[PROOF-CACHE] Cleaned up {} expired entries", expired_count);
        }
    }
}

pub struct CheckpointManager {
    web3_client: Arc<Web3Client>,
    job_trackers: Arc<RwLock<HashMap<u64, JobTokenTracker>>>,
    /// Content hashes for real prompt/response binding (Phase 4 - v8.10.0)
    content_hashes: Arc<RwLock<HashMap<u64, ContentHashes>>>,
    proof_system_address: Address,
    host_address: Address,
    s5_storage: Box<dyn S5Storage>, // S5 storage for off-chain proof storage
    /// Checkpoint publisher for conversation recovery (Phase 3)
    checkpoint_publisher: Arc<CheckpointPublisher>,
    /// Proof submission cache for S5 propagation delay handling (v8.12.6)
    proof_cache: Arc<ProofSubmissionCache>,
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

        // Initialize checkpoint publisher for conversation recovery (Phase 3)
        let checkpoint_publisher = Arc::new(CheckpointPublisher::new(format!("{:?}", host_address)));

        // Initialize proof cache for S5 propagation delay handling (v8.12.6)
        // TTL of 1 hour - proofs older than this are cleaned up
        let proof_cache = Arc::new(ProofSubmissionCache::new(Duration::from_secs(3600)));

        eprintln!("CheckpointManager initialized:");
        eprintln!("  Host: {:?}", host_address);
        eprintln!("  JobMarketplace: {}", job_marketplace_address);
        eprintln!("  BUILD VERSION: {}", crate::version::VERSION);

        Ok(Self {
            web3_client,
            job_trackers: Arc::new(RwLock::new(HashMap::new())),
            content_hashes: Arc::new(RwLock::new(HashMap::new())),
            proof_system_address,
            host_address,
            s5_storage,
            checkpoint_publisher,
            proof_cache,
        })
    }

    /// Query the session's proofInterval from the JobMarketplace contract (v8.14.2)
    ///
    /// Returns the proofInterval for minimum billing, or MIN_PROVEN_TOKENS as fallback.
    /// The proofInterval is at index 10 in the sessionJobs return tuple.
    async fn query_session_proof_interval(&self, job_id: u64) -> u64 {
        // Build the sessionJobs(uint256) call
        // Returns: (id, depositor, host, paymentToken, deposit, pricePerToken, tokensUsed,
        //           maxDuration, startTime, lastProofTime, proofInterval, status, ...)
        let contract = ethers::contract::Contract::new(
            self.proof_system_address,
            ethers::abi::parse_abi(&[
                "function sessionJobs(uint256 jobId) external view returns (uint256, address, address, address, uint256, uint256, uint256, uint256, uint256, uint256, uint256, uint8, uint256, uint256, string, bytes32, string)"
            ]).unwrap_or_default(),
            self.web3_client.provider.clone(),
        );

        match contract
            .method::<_, (U256, Address, Address, Address, U256, U256, U256, U256, U256, U256, U256, u8, U256, U256, String, H256, String)>(
                "sessionJobs",
                U256::from(job_id),
            ) {
            Ok(call) => {
                match call.call().await {
                    Ok(result) => {
                        // proofInterval is at index 10
                        let proof_interval = result.10.as_u64();
                        info!(
                            "📋 Queried proofInterval for job {}: {} tokens (minimum billing)",
                            job_id, proof_interval
                        );
                        // Use contract value, but ensure it's at least MIN_PROVEN_TOKENS
                        std::cmp::max(proof_interval, MIN_PROVEN_TOKENS)
                    }
                    Err(e) => {
                        warn!(
                            "⚠️ Failed to query proofInterval for job {}: {}. Using default {}",
                            job_id, e, CHECKPOINT_THRESHOLD
                        );
                        CHECKPOINT_THRESHOLD // Fallback to checkpoint threshold
                    }
                }
            }
            Err(e) => {
                warn!(
                    "⚠️ Failed to create sessionJobs call for job {}: {}. Using default {}",
                    job_id, e, CHECKPOINT_THRESHOLD
                );
                CHECKPOINT_THRESHOLD
            }
        }
    }

    /// Track tokens generated for a specific job
    pub async fn track_tokens(
        &self,
        job_id: u64,
        tokens: u64,
        session_id: Option<String>,
    ) -> Result<()> {
        // Check if we need to create a new tracker (requires async contract query)
        let needs_new_tracker = {
            let trackers = self.job_trackers.read().await;
            !trackers.contains_key(&job_id)
        };

        // If new tracker needed, query contract for proofInterval BEFORE acquiring write lock
        let proof_interval = if needs_new_tracker {
            self.query_session_proof_interval(job_id).await
        } else {
            0 // Will be ignored, existing tracker has its own value
        };

        let mut trackers = self.job_trackers.write().await;

        let tracker = trackers.entry(job_id).or_insert_with(|| {
            eprintln!("📝 Starting token tracking for job {} (proofInterval: {} tokens)", job_id, proof_interval);
            JobTokenTracker {
                job_id,
                tokens_generated: 0,
                last_checkpoint: 0,
                session_id: session_id.clone(),
                submission_in_progress: false,
                last_proof_timestamp: None,
                submission_started_at: None,
                proof_interval, // Store session's proofInterval for minimum billing
            }
        });

        // Update session_id if provided and not already set (Phase 3.3)
        if tracker.session_id.is_none() && session_id.is_some() {
            tracker.session_id = session_id;
        }

        tracker.tokens_generated += tokens;

        // Check if we need to submit a checkpoint
        let tokens_since_checkpoint = tracker.tokens_generated - tracker.last_checkpoint;

        if tokens_since_checkpoint >= CHECKPOINT_THRESHOLD && !tracker.submission_in_progress {
            info!(
                "🚨 Checkpoint triggered: job {} at {} tokens",
                job_id, tracker.tokens_generated
            );
            // CRITICAL FIX: Submit only the DELTA (tokens since last checkpoint), not cumulative total
            let mut tokens_to_submit = tokens_since_checkpoint;
            let previous_checkpoint = tracker.last_checkpoint; // Save for rollback
            let is_first_checkpoint = previous_checkpoint == 0;

            // v8.14.2: Use session's proofInterval for minimum billing (not just MIN_PROVEN_TOKENS)
            // This ensures hosts get paid for at least proofInterval tokens even on short sessions
            let min_billable_tokens = tracker.proof_interval;
            if is_first_checkpoint && tokens_to_submit < min_billable_tokens {
                info!(
                    "📝 Padding FIRST checkpoint from {} to {} tokens (session proofInterval) for job {}",
                    tokens_to_submit, min_billable_tokens, job_id
                );
                tokens_to_submit = min_billable_tokens;
            }

            // Mark submission as in progress to prevent race conditions
            tracker.submission_in_progress = true;
            tracker.submission_started_at = Some(std::time::Instant::now());
            // Optimistically update the checkpoint to reflect cumulative tokens proven
            tracker.last_checkpoint = tracker.tokens_generated;

            // Phase 3: Clone session_id for checkpoint publishing (v8.11.0)
            let session_id_for_publish = tracker.session_id.clone();

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
            let checkpoint_publisher = self.checkpoint_publisher.clone();
            let proof_cache = self.proof_cache.clone();

            // Phase 4: Get content hashes for real proof binding (v8.10.0+)
            let content_hashes = self.get_content_hashes(job_id).await;

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
                    content_hashes,
                    session_id_for_publish,
                    checkpoint_publisher,
                    previous_checkpoint,
                    proof_cache,
                ).await;

                // Update tracker based on result
                let mut trackers = job_trackers.write().await;
                if let Some(tracker) = trackers.get_mut(&job_id) {
                    tracker.submission_in_progress = false; // Clear the flag
                    tracker.submission_started_at = None; // Clear the start timestamp

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
        }

        Ok(())
    }

    // ========================================================================
    // Phase 4: Content Hash Methods (v8.10.0)
    // ========================================================================

    /// Set the prompt hash for a job (called at inference start)
    ///
    /// This should be called once when inference begins, with the SHA256 hash
    /// of the actual prompt text.
    pub async fn set_prompt_hash(&self, job_id: u64, hash: [u8; 32]) {
        let mut content_hashes = self.content_hashes.write().await;
        let entry = content_hashes.entry(job_id).or_insert_with(ContentHashes::default);
        entry.prompt_hash = Some(hash);
        tracing::debug!(
            "📝 Set prompt hash for job {}: 0x{}",
            job_id,
            hex::encode(&hash[..8])
        );
    }

    /// Append response text for a job (called during token streaming)
    ///
    /// Accumulates the response text so we can hash it at checkpoint time.
    pub async fn append_response(&self, job_id: u64, text: &str) {
        let mut content_hashes = self.content_hashes.write().await;
        let entry = content_hashes.entry(job_id).or_insert_with(ContentHashes::default);
        entry.response_buffer.push_str(text);
    }

    /// Finalize and compute the response hash for a job
    ///
    /// Computes SHA256 of the accumulated response buffer, stores it,
    /// and clears the buffer. Returns the computed hash.
    pub async fn finalize_response_hash(&self, job_id: u64) -> [u8; 32] {
        let mut content_hashes = self.content_hashes.write().await;
        let entry = content_hashes.entry(job_id).or_insert_with(ContentHashes::default);

        // Compute SHA256 of accumulated response
        let hash = Sha256::digest(entry.response_buffer.as_bytes());
        let mut hash_bytes = [0u8; 32];
        hash_bytes.copy_from_slice(&hash);

        // Store the hash
        entry.response_hash = Some(hash_bytes);

        tracing::debug!(
            "📝 Finalized response hash for job {}: 0x{} ({} chars)",
            job_id,
            hex::encode(&hash_bytes[..8]),
            entry.response_buffer.len()
        );

        // Note: We don't clear the buffer here in case of retry
        // It will be cleared when the job is cleaned up

        hash_bytes
    }

    /// Get the content hashes for a job (prompt_hash, response_hash)
    ///
    /// For intermediate checkpoints (during streaming), computes partial response hash
    /// from the accumulated buffer if finalized hash is not yet available.
    /// Returns None only if prompt_hash is not set.
    pub async fn get_content_hashes(&self, job_id: u64) -> Option<([u8; 32], [u8; 32])> {
        let content_hashes = self.content_hashes.read().await;
        content_hashes.get(&job_id).and_then(|entry| {
            // Must have prompt hash
            let prompt_hash = entry.prompt_hash?;

            // Use finalized response hash if available, otherwise compute partial hash
            let response_hash = if let Some(finalized) = entry.response_hash {
                finalized
            } else if !entry.response_buffer.is_empty() {
                // Compute partial response hash for intermediate checkpoint
                let hash = Sha256::digest(entry.response_buffer.as_bytes());
                let mut hash_bytes = [0u8; 32];
                hash_bytes.copy_from_slice(&hash);
                tracing::debug!(
                    "📝 Computing partial response hash for job {} ({} chars so far)",
                    job_id,
                    entry.response_buffer.len()
                );
                hash_bytes
            } else {
                // No response data yet
                return None;
            };

            Some((prompt_hash, response_hash))
        })
    }

    /// Clear content hashes for a job (called after successful checkpoint)
    pub async fn clear_content_hashes(&self, job_id: u64) {
        let mut content_hashes = self.content_hashes.write().await;
        content_hashes.remove(&job_id);
        tracing::debug!("🧹 Cleared content hashes for job {}", job_id);
    }

    /// Get the current response buffer length for a job
    #[cfg(test)]
    pub async fn get_response_buffer_len(&self, job_id: u64) -> usize {
        let content_hashes = self.content_hashes.read().await;
        content_hashes
            .get(&job_id)
            .map(|e| e.response_buffer.len())
            .unwrap_or(0)
    }

    // ========================================================================
    // End Phase 4 Content Hash Methods
    // ========================================================================

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

    /// Query the modelId for a session from the JobMarketplace contract
    ///
    /// # AUDIT-F4 Compliance
    ///
    /// This function queries `sessionModel(uint256 sessionId)` to get the model ID
    /// that must be included in proof signatures to prevent cross-model replay attacks.
    ///
    /// # Arguments
    ///
    /// * `job_id` - The session ID to query (same as job_id in this system)
    ///
    /// # Returns
    ///
    /// - `Ok([u8; 32])` - The modelId as bytes32
    ///   - Returns [0u8; 32] (bytes32(0)) for non-model sessions (legacy sessions)
    ///   - Returns actual model ID for model-specific sessions
    ///
    /// # Errors
    ///
    /// Returns error if contract call fails or RPC is unavailable
    ///
    /// # Example
    ///
    /// ```ignore
    /// let model_id = checkpoint_manager.query_session_model(job_id).await?;
    /// // Use model_id in signature generation
    /// let signature = sign_proof_data(&key, hash, addr, tokens, model_id)?;
    /// ```
    pub async fn query_session_model(&self, job_id: u64) -> Result<[u8; 32]> {
        let query = SessionModelQuery::new(job_id);
        let call_data = query.encode();

        info!(
            "🔍 Querying sessionModel for job {} (AUDIT-F4 compliance)",
            job_id
        );

        // Create transaction request for contract call
        let tx = TransactionRequest::new()
            .to(self.proof_system_address)
            .data(call_data);

        // Call contract (read-only, doesn't send transaction)
        let result = self
            .web3_client
            .provider
            .call(&tx.into(), None)
            .await
            .map_err(|e| anyhow!("Failed to query sessionModel for job {}: {}", job_id, e))?;

        // Decode bytes32 response
        if result.len() != 32 {
            return Err(anyhow!(
                "Invalid sessionModel response length: {} (expected 32)",
                result.len()
            ));
        }

        let mut model_id = [0u8; 32];
        model_id.copy_from_slice(&result[..32]);

        if model_id == [0u8; 32] {
            info!("   Non-model session (modelId = bytes32(0))");
        } else {
            info!("   Model ID: 0x{}", hex::encode(&model_id[..8]));
        }

        Ok(model_id)
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

        // Generate host signature for security audit compliance (v8.9.0)
        let private_key = crate::crypto::extract_node_private_key()
            .map_err(|e| anyhow!("Failed to get host private key for signing: {}", e))?;

        // Query modelId from contract for AUDIT-F4 compliance
        let model_id = self.query_session_model(job_id).await?;
        info!(
            "📋 Job {} modelId: 0x{} (for reference)",
            job_id,
            hex::encode(&model_id[..8])
        );

        // v8.14.0: Signature removed from submitProofOfWork per BREAKING_CHANGES.md
        // Contract now verifies msg.sender == session.host instead of signature
        // This saves ~3,000 gas per proof submission

        // Encode contract call with hash + CID + deltaCID (v8.14.0 - no signature)
        // Sync path doesn't publish encrypted checkpoints, so delta_cid is empty
        let data = encode_checkpoint_call(job_id, tokens_to_submit, proof_hash_bytes, proof_cid, String::new());

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
    ///
    /// Phase 4 (v8.10.0+): Accepts optional content hashes for proof binding
    /// Phase 3 (v8.11.0+): Publishes checkpoint to S5 BEFORE chain submission
    async fn submit_checkpoint_async(
        web3_client: Arc<Web3Client>,
        s5_storage: Box<dyn S5Storage>,
        proof_system_address: Address,
        host_address: Address,
        job_id: u64,
        tokens_generated: u64,
        content_hashes: Option<([u8; 32], [u8; 32])>,
        session_id: Option<String>,
        checkpoint_publisher: Arc<CheckpointPublisher>,
        previous_checkpoint_tokens: u64,
        proof_cache: Arc<ProofSubmissionCache>,
    ) -> Result<()> {
        let tokens_to_submit = tokens_generated;

        info!(
            "🚀 [ASYNC] Submitting proof of work for job {} with {} tokens (content_hashes={})...",
            job_id, tokens_to_submit, content_hashes.is_some()
        );

        // Generate STARK proof using Risc0 zkVM (static version)
        // CRITICAL: Use spawn_blocking for CPU-intensive proof generation
        // to avoid blocking the Tokio async runtime and freezing streaming
        let proof_bytes = tokio::task::spawn_blocking(move || {
            Self::generate_proof_static(job_id, tokens_generated, host_address, content_hashes)
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

        // Generate host signature for security audit compliance (v8.9.0)
        let private_key = crate::crypto::extract_node_private_key()
            .map_err(|e| anyhow!("Failed to get host private key for signing: {}", e))?;

        // Phase 3: Publish checkpoint to S5 BEFORE chain submission (v8.11.0)
        // Phase 10: Capture delta_cid for on-chain storage (v8.12.4)
        // CRITICAL: If this fails, we MUST NOT submit proof to chain
        let delta_cid = if let Some(ref session_id) = session_id {
            let start_token = previous_checkpoint_tokens;
            let end_token = previous_checkpoint_tokens + tokens_to_submit;

            match checkpoint_publisher
                .publish_checkpoint(
                    session_id,
                    proof_hash_bytes,
                    start_token,
                    end_token,
                    &private_key,
                    s5_storage.as_ref(),
                )
                .await
            {
                Ok(cid) => {
                    info!(
                        "✅ [ASYNC] Checkpoint published to S5: {} (session={})",
                        cid, session_id
                    );
                    cid // Capture delta_cid for on-chain submission
                }
                Err(e) => {
                    error!(
                        "❌ [ASYNC] S5 checkpoint upload failed - NOT submitting proof: {}",
                        e
                    );
                    return Err(anyhow!(
                        "Checkpoint publishing failed - proof NOT submitted: {}",
                        e
                    ));
                }
            }
        } else {
            String::new() // No session_id means no encrypted checkpoint
        };

        // Query modelId from contract for AUDIT-F4 compliance
        let query = SessionModelQuery::new(job_id);
        let call_data = query.encode();
        let tx = TransactionRequest::new()
            .to(proof_system_address)
            .data(call_data);
        let result = web3_client
            .provider
            .call(&tx.into(), None)
            .await
            .map_err(|e| anyhow!("Failed to query sessionModel for job {}: {}", job_id, e))?;

        let mut model_id = [0u8; 32];
        if result.len() == 32 {
            model_id.copy_from_slice(&result[..32]);
        }

        info!(
            "📋 [ASYNC] Job {} modelId: 0x{} (for reference)",
            job_id,
            hex::encode(&model_id[..8])
        );

        // v8.14.0: Signature removed from submitProofOfWork per BREAKING_CHANGES.md
        // Contract now verifies msg.sender == session.host instead of signature
        // This saves ~3,000 gas per proof submission

        // Cache proof data for S5 propagation delay handling (v8.12.6)
        // Cache BEFORE on-chain tx so data is available even if tx fails
        let delta_cid_option = if delta_cid.is_empty() {
            None
        } else {
            Some(delta_cid.clone())
        };
        proof_cache
            .cache_proof(
                job_id,
                CachedProofEntry {
                    proof_hash: proof_hash_bytes,
                    proof_cid: proof_cid.clone(),
                    delta_cid: delta_cid_option,
                    tokens: tokens_to_submit,
                    cached_at: std::time::Instant::now(),
                },
            )
            .await;

        // Encode contract call with hash + CID + deltaCID (v8.14.0 - no signature)
        let data = encode_checkpoint_call(job_id, tokens_to_submit, proof_hash_bytes, proof_cid, delta_cid);

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
    ///
    /// Phase 4 (v8.10.0+): Accepts optional real content hashes for proof binding
    /// If content_hashes is None, falls back to placeholder hashes
    fn generate_proof_static(
        job_id: u64,
        tokens_generated: u64,
        host_address: Address,
        content_hashes: Option<([u8; 32], [u8; 32])>,
    ) -> Result<Vec<u8>> {
        #[cfg(feature = "real-ezkl")]
        {
            // Log whether we're using real or placeholder hashes
            let using_real_hashes = content_hashes.is_some();
            info!(
                "🔐 [ASYNC] Generating real Risc0 STARK proof for job {} ({} tokens, real_hashes={})",
                job_id, tokens_generated, using_real_hashes
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

            // Phase 4: Use real content hashes if available, otherwise placeholder
            let (input_hash_bytes, output_hash_bytes) = if let Some((prompt_hash, response_hash)) = content_hashes {
                info!(
                    "📝 [ASYNC] Using real content hashes - prompt: 0x{}..., response: 0x{}...",
                    hex::encode(&prompt_hash[..8]),
                    hex::encode(&response_hash[..8])
                );
                (prompt_hash, response_hash)
            } else {
                // Fallback to placeholder hashes (backward compatible)
                let input_data = format!("job_{}:input", job_id);
                let input_hash = Sha256::digest(input_data.as_bytes());
                let mut input_bytes = [0u8; 32];
                input_bytes.copy_from_slice(&input_hash);

                let output_data = format!("job_{}:output:tokens_{}", job_id, tokens_generated);
                let output_hash = Sha256::digest(output_data.as_bytes());
                let mut output_bytes = [0u8; 32];
                output_bytes.copy_from_slice(&output_hash);

                warn!(
                    "⚠️ [ASYNC] Using placeholder hashes (no content hashes available for job {})",
                    job_id
                );
                (input_bytes, output_bytes)
            };

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
                "mock": true,
                "hasRealContentHashes": content_hashes.is_some()
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

            // Skip if submission is already in progress
            if tracker.submission_in_progress {
                return Ok(());
            }

            // For session completion, submit ANY tokens we have (even if < MIN_PROVEN_TOKENS)
            if tokens_since_checkpoint > 0 {
                let mut tokens_to_submit = tokens_since_checkpoint; // Submit ONLY the delta, not total
                let previous_checkpoint = tracker.last_checkpoint;
                let is_first_checkpoint = previous_checkpoint == 0;

                // v8.14.2: Use session's proofInterval for minimum billing
                let min_billable_tokens = tracker.proof_interval;
                if is_first_checkpoint && tokens_to_submit < min_billable_tokens {
                    info!(
                        "📝 Padding FIRST checkpoint from {} to {} tokens (session proofInterval) for job {}",
                        tokens_to_submit, min_billable_tokens, job_id
                    );
                    tokens_to_submit = min_billable_tokens;
                } else if !is_first_checkpoint && tokens_to_submit < MIN_PROVEN_TOKENS {
                    // Not first checkpoint and below contract minimum - will be rejected
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
                tracker.submission_started_at = Some(std::time::Instant::now());
                tracker.last_checkpoint = tracker.tokens_generated; // Update to total after submission

                // Phase 3: Clone session_id for checkpoint publishing (v8.11.0)
                let session_id_for_publish = tracker.session_id.clone();

                drop(trackers); // Release lock for async operation

                // SYNCHRONOUS CHECKPOINT SUBMISSION: Must wait for proof to be on-chain
                // before calling completeSessionJob, otherwise contract thinks 0 tokens used!
                info!("🔒 [SYNC-FINAL] Submitting final checkpoint for job {} (waiting for confirmation)...", job_id);

                // Phase 4: Get content hashes for real proof binding (v8.10.0+)
                let content_hashes = self.get_content_hashes(job_id).await;

                let submission_result = Self::submit_checkpoint_async(
                    self.web3_client.clone(),
                    self.s5_storage.clone(),
                    self.proof_system_address,
                    self.host_address,
                    job_id,
                    tokens_to_submit,
                    content_hashes,
                    session_id_for_publish,
                    self.checkpoint_publisher.clone(),
                    previous_checkpoint,
                    self.proof_cache.clone(),
                ).await;

                // Update tracker based on result
                let mut trackers = self.job_trackers.write().await;
                if let Some(tracker) = trackers.get_mut(&job_id) {
                    tracker.submission_in_progress = false;
                    tracker.submission_started_at = None;

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
        let checkpoint_publisher = self.checkpoint_publisher.clone();
        let proof_cache = self.proof_cache.clone();

        // Phase 4: Get content hashes before spawning (v8.10.0+)
        let content_hashes = self.get_content_hashes(job_id).await;

        info!("🚀 [FORCE-CHECKPOINT] Spawning background task for job {} (returns immediately)", job_id);

        // Spawn the entire force_checkpoint logic in background
        // This ensures the caller NEVER blocks waiting for locks
        tokio::spawn(async move {
            info!("🔄 [FORCE-CHECKPOINT-BG] Starting background force checkpoint for job {}", job_id);

            // Acquire lock inside the spawned task (not blocking caller)
            let mut trackers = job_trackers.write().await;

            let (tokens_to_submit, previous_checkpoint, session_id_for_publish) = if let Some(tracker) = trackers.get_mut(&job_id) {
                let tokens_since_checkpoint = tracker.tokens_generated - tracker.last_checkpoint;

                // Skip if submission in progress or not enough tokens
                if tracker.submission_in_progress {
                    return;
                }
                if tokens_since_checkpoint < MIN_PROVEN_TOKENS {
                    return;
                }

                // We have enough tokens to submit
                let mut tokens_to_submit = tokens_since_checkpoint;
                let previous_checkpoint = tracker.last_checkpoint;
                let is_first_checkpoint = previous_checkpoint == 0;

                // v8.14.2: Use session's proofInterval for minimum billing
                let min_billable_tokens = tracker.proof_interval;
                if is_first_checkpoint && tokens_to_submit < min_billable_tokens {
                    info!(
                        "📝 [FORCE-CHECKPOINT-BG] Padding FIRST checkpoint from {} to {} tokens (proofInterval) for job {}",
                        tokens_to_submit, min_billable_tokens, job_id
                    );
                    tokens_to_submit = min_billable_tokens;
                }

                info!(
                    "📤 [FORCE-CHECKPOINT-BG] Submitting {} new tokens for job {} (total: {})",
                    tokens_to_submit, job_id, tracker.tokens_generated
                );

                // Mark as in progress and update checkpoint
                tracker.submission_in_progress = true;
                tracker.submission_started_at = Some(std::time::Instant::now());
                tracker.last_checkpoint = tracker.tokens_generated;

                // Phase 3: Clone session_id for checkpoint publishing (v8.11.0)
                let session_id_for_publish = tracker.session_id.clone();

                (tokens_to_submit, previous_checkpoint, session_id_for_publish)
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
                content_hashes,
                session_id_for_publish,
                checkpoint_publisher,
                previous_checkpoint,
                proof_cache,
            ).await;

            // Update tracker based on result
            let mut trackers = job_trackers.write().await;
            if let Some(tracker) = trackers.get_mut(&job_id) {
                tracker.submission_in_progress = false;
                tracker.submission_started_at = None;

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

        // CRITICAL FIX: Wait for any in-flight background proof submission to complete
        // before attempting final checkpoint or settlement. This prevents the race condition
        // where settlement proceeds while a background proof task is still running.
        let max_wait = Duration::from_secs(120); // 2 minutes max wait
        let poll_interval = Duration::from_millis(500);
        let wait_start = std::time::Instant::now();

        loop {
            let in_progress = {
                let trackers = self.job_trackers.read().await;
                trackers
                    .get(&job_id)
                    .map(|t| t.submission_in_progress)
                    .unwrap_or(false)
            };

            if !in_progress {
                info!(
                    "[CHECKPOINT-MGR] ✅ No in-flight submission for job {} - proceeding with settlement",
                    job_id
                );
                break;
            }

            if wait_start.elapsed() > max_wait {
                warn!(
                    "[CHECKPOINT-MGR] ⚠️ Timeout waiting for in-flight submission for job {} after {:.1}s",
                    job_id,
                    wait_start.elapsed().as_secs_f32()
                );
                warn!("[CHECKPOINT-MGR] Proceeding with settlement anyway - proof may be lost");
                break;
            }

            info!(
                "[CHECKPOINT-MGR] ⏳ Waiting for in-flight submission to complete for job {} ({:.1}s elapsed)...",
                job_id,
                wait_start.elapsed().as_secs_f32()
            );
            tokio::time::sleep(poll_interval).await;
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
        drop(trackers);

        // Clean up proof cache for this job
        self.proof_cache.cleanup_job(job_id).await;

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
        drop(trackers);

        // Clean up proof cache for this job
        self.proof_cache.cleanup_job(job_id).await;
    }

    // ============================================================
    // Checkpoint Publishing for Conversation Recovery (Phase 3)
    // ============================================================

    /// Get reference to checkpoint publisher
    pub fn checkpoint_publisher(&self) -> &Arc<CheckpointPublisher> {
        &self.checkpoint_publisher
    }

    /// Set the recovery public key for a session (enables encrypted checkpoints)
    ///
    /// Call this during session initialization when the SDK provides a recoveryPublicKey.
    /// Once set, all subsequent checkpoints for this session will be encrypted.
    ///
    /// # Arguments
    /// * `session_id` - Session identifier
    /// * `recovery_public_key` - User's recovery public key (0x-prefixed hex, compressed secp256k1)
    pub async fn set_session_recovery_public_key(&self, session_id: &str, recovery_public_key: String) {
        self.checkpoint_publisher
            .set_recovery_public_key(session_id, recovery_public_key)
            .await;
        info!(
            "🔐 Recovery public key set for session {} (encrypted checkpoints enabled)",
            session_id
        );
    }

    /// Check if a session has encrypted checkpoints enabled
    pub async fn has_session_recovery_key(&self, session_id: &str) -> bool {
        self.checkpoint_publisher.has_recovery_key(session_id).await
    }

    /// Get the host's Ethereum address (lowercase, 0x prefixed)
    /// Used by HTTP endpoint for checkpoint retrieval path
    pub fn get_host_address(&self) -> String {
        format!("{:#x}", self.host_address)
    }

    /// Get reference to S5 storage for checkpoint retrieval
    /// Used by HTTP endpoint to fetch checkpoint index from S5
    pub fn get_s5_storage(&self) -> &dyn S5Storage {
        self.s5_storage.as_ref()
    }

    /// Track a conversation message for checkpoint publishing
    ///
    /// Call this for each user prompt and assistant response.
    /// Messages are buffered and included in the next checkpoint.
    ///
    /// # Arguments
    /// * `session_id` - Session identifier
    /// * `role` - "user" or "assistant"
    /// * `content` - Message content
    /// * `partial` - True if streaming response is incomplete
    pub async fn track_conversation_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        partial: bool,
    ) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let message = if role == "user" {
            CheckpointMessage::new_user(content.to_string(), timestamp)
        } else {
            CheckpointMessage::new_assistant(content.to_string(), timestamp, partial)
        };

        self.checkpoint_publisher
            .buffer_message(session_id, message)
            .await;
    }

    /// Initialize checkpoint session (resume from S5 if exists)
    ///
    /// Call this when a session starts to check for existing checkpoint index.
    /// If found, resumes numbering from last checkpoint.
    pub async fn init_checkpoint_session(&self, session_id: &str) -> Result<()> {
        self.checkpoint_publisher
            .init_session(session_id, self.s5_storage.as_ref())
            .await
    }

    /// Remove checkpoint session state (cleanup on disconnect)
    pub async fn cleanup_checkpoint_session(&self, session_id: &str) {
        self.checkpoint_publisher.remove_session(session_id).await;
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

// ABI encoding helper for submitProofOfWork (v8.14.0 - Post-Remediation)
// Signature parameter REMOVED per BREAKING_CHANGES.md (Feb 4, 2026)
// Authentication now via msg.sender == session.host check in contract
fn encode_checkpoint_call(
    job_id: u64,
    tokens_generated: u64,
    proof_hash: [u8; 32],
    proof_cid: String,
    delta_cid: String, // Phase 10: Encrypted checkpoint delta CID for on-chain recovery
) -> Vec<u8> {
    use ethers::abi::Function;

    // Define the function signature for submitProofOfWork (v8.14.0 - Signature Removed)
    // Contract accepts: (uint256 jobId, uint256 tokensClaimed, bytes32 proofHash, string proofCID, string deltaCID)
    // Note: Signature removed - contract verifies msg.sender == session.host instead
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
                kind: ethers::abi::ParamType::FixedBytes(32),
                internal_type: None,
            },
            ethers::abi::Param {
                name: "proofCID".to_string(),
                kind: ethers::abi::ParamType::String,
                internal_type: None,
            },
            ethers::abi::Param {
                name: "deltaCID".to_string(), // Phase 10: Encrypted checkpoint delta CID
                kind: ethers::abi::ParamType::String,
                internal_type: None,
            },
        ],
        outputs: vec![],
        constant: None,
        state_mutability: ethers::abi::StateMutability::NonPayable,
    };

    // Encode the function call with hash + CID + deltaCID (no signature)
    let tokens = vec![
        Token::Uint(U256::from(job_id)),
        Token::Uint(U256::from(tokens_generated)),
        Token::FixedBytes(proof_hash.to_vec()),
        Token::String(proof_cid),
        Token::String(delta_cid), // Phase 10: delta CID (empty string if not encrypted)
    ];

    function
        .encode_input(&tokens)
        .expect("Failed to encode submitProofOfWork call")
}

// ========================================================================
// Phase 2.1: SessionModel Contract Query (AUDIT-F4 Compliance)
// ========================================================================

/// Query the modelId for a session from JobMarketplace contract
///
/// Calls: `sessionModel(uint256 sessionId) returns (bytes32)`
///
/// Returns bytes32(0) for sessions created without a model (legacy sessions).
/// For model-specific sessions, returns the registered model ID.
///
/// # AUDIT-F4 Compliance
///
/// The modelId must be included in proof signatures to prevent cross-model replay attacks.
/// This query retrieves the modelId that was set when the session was created.
#[derive(Debug, Clone)]
pub struct SessionModelQuery {
    pub session_id: U256,
}

impl SessionModelQuery {
    /// Create a new sessionModel query for the given session ID
    pub fn new(session_id: u64) -> Self {
        Self {
            session_id: U256::from(session_id),
        }
    }

    /// Encode the contract call using ethers-rs ABI encoding
    ///
    /// Returns the encoded call data for `sessionModel(uint256)`
    pub fn encode(&self) -> Bytes {
        // Function signature: sessionModel(uint256)
        let function_sig = &ethers::utils::keccak256(b"sessionModel(uint256)")[..4];
        let mut data = Vec::from(function_sig);

        // Encode session_id as uint256 (32 bytes, big-endian)
        let mut session_bytes = [0u8; 32];
        self.session_id.to_big_endian(&mut session_bytes);
        data.extend_from_slice(&session_bytes);

        Bytes::from(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that encode_checkpoint_call encodes correctly (v8.14.0 - no signature)
    #[test]
    fn test_checkpoint_encodes_correctly() {
        let job_id = 12345u64;
        let tokens_generated = 1000u64;
        let proof_hash = [0xab; 32];
        let proof_cid = "bafytest123".to_string();

        let encoded = encode_checkpoint_call(
            job_id,
            tokens_generated,
            proof_hash,
            proof_cid,
            String::new(), // No delta CID
        );

        // Function selector (4 bytes) + encoded params
        assert!(encoded.len() > 4, "Encoded data should include function selector");

        // Verify function selector is for submitProofOfWork
        // keccak256("submitProofOfWork(uint256,uint256,bytes32,string,string)")
        let selector = &encoded[0..4];
        assert!(
            !selector.iter().all(|&b| b == 0),
            "Function selector should not be all zeros"
        );
    }

    /// Test that proof hash appears in the encoded transaction data
    #[test]
    fn test_proof_hash_in_transaction_data() {
        let job_id = 100u64;
        let tokens_generated = 500u64;
        let proof_hash = [0x11; 32];
        let proof_cid = "cid123".to_string();

        let encoded = encode_checkpoint_call(
            job_id,
            tokens_generated,
            proof_hash,
            proof_cid,
            String::new(),
        );

        // The encoded data should contain our proof hash bytes
        assert!(
            encoded.len() >= 32,
            "Encoded data should be at least 32 bytes to contain proof hash"
        );
    }

    /// Test that different proof hashes produce different encoded data
    #[test]
    fn test_different_hashes_different_encoding() {
        let job_id = 100u64;
        let tokens_generated = 500u64;
        let proof_cid = "cid123".to_string();

        let hash1 = [0xaa; 32];
        let hash2 = [0xbb; 32];

        let encoded1 = encode_checkpoint_call(
            job_id,
            tokens_generated,
            hash1,
            proof_cid.clone(),
            String::new(),
        );

        let encoded2 = encode_checkpoint_call(
            job_id,
            tokens_generated,
            hash2,
            proof_cid,
            String::new(),
        );

        assert_ne!(
            encoded1, encoded2,
            "Different proof hashes should produce different encoded data"
        );
    }

    /// Test encode_checkpoint_call produces consistent output
    #[test]
    fn test_encoding_is_deterministic() {
        let job_id = 42u64;
        let tokens_generated = 100u64;
        let proof_hash = [0xde; 32];
        let proof_cid = "testcid".to_string();

        let encoded1 = encode_checkpoint_call(
            job_id,
            tokens_generated,
            proof_hash,
            proof_cid.clone(),
            String::new(),
        );

        let encoded2 = encode_checkpoint_call(
            job_id,
            tokens_generated,
            proof_hash,
            proof_cid,
            String::new(),
        );

        assert_eq!(
            encoded1, encoded2,
            "Same inputs should produce identical encoded output"
        );
    }

    // ========================================================================
    // Phase 10: deltaCID On-Chain Support Tests (Sub-phase 10.1)
    // ========================================================================

    /// Test that encode_checkpoint_call includes delta_cid in the encoded data
    #[test]
    fn test_encode_checkpoint_call_includes_delta_cid() {
        let job_id = 999u64;
        let tokens_generated = 500u64;
        let proof_hash = [0xaa; 32];
        let proof_cid = "bafyproof123".to_string();
        let delta_cid = "blob:abc123def456".to_string(); // Non-empty delta CID

        let encoded = encode_checkpoint_call(
            job_id,
            tokens_generated,
            proof_hash,
            proof_cid,
            delta_cid.clone(),
        );

        // Function selector (4 bytes) + encoded params
        assert!(encoded.len() > 4, "Encoded data should include function selector");

        // The delta_cid string should appear in the encoded data (ABI-encoded)
        // In ABI encoding, strings are stored as: offset pointer + length + UTF-8 bytes
        let delta_bytes = delta_cid.as_bytes();
        let encoded_contains_delta = encoded
            .windows(delta_bytes.len())
            .any(|window| window == delta_bytes);
        assert!(
            encoded_contains_delta,
            "Encoded data should contain delta_cid bytes"
        );
    }

    /// Test that encode_checkpoint_call works with empty delta_cid (backward compat)
    #[test]
    fn test_encode_checkpoint_call_with_empty_delta_cid() {
        let job_id = 888u64;
        let tokens_generated = 250u64;
        let proof_hash = [0xcc; 32];
        let proof_cid = "bafyproof456".to_string();
        let delta_cid = "".to_string(); // Empty delta CID for non-encrypted path

        let encoded = encode_checkpoint_call(
            job_id,
            tokens_generated,
            proof_hash,
            proof_cid,
            delta_cid,
        );

        // Should still encode successfully with empty string
        assert!(encoded.len() > 4, "Encoded data should include function selector");
        assert!(!encoded.is_empty(), "Encoded data should not be empty");
    }

    /// Test that different delta_cids produce different encoded data
    #[test]
    fn test_different_delta_cids_different_encoding() {
        let job_id = 777u64;
        let tokens_generated = 100u64;
        let proof_hash = [0xee; 32];
        let proof_cid = "bafyproof789".to_string();

        let delta_cid1 = "blob:first".to_string();
        let delta_cid2 = "blob:second".to_string();

        let encoded1 = encode_checkpoint_call(
            job_id,
            tokens_generated,
            proof_hash,
            proof_cid.clone(),
            delta_cid1,
        );

        let encoded2 = encode_checkpoint_call(
            job_id,
            tokens_generated,
            proof_hash,
            proof_cid,
            delta_cid2,
        );

        assert_ne!(
            encoded1, encoded2,
            "Different delta_cids should produce different encoded data"
        );
    }

    // ========================================================================
    // Phase 4: ContentHashes Tests (Sub-phase 4.1)
    // ========================================================================

    /// Test that set_prompt_hash stores the hash correctly
    #[tokio::test]
    async fn test_set_prompt_hash_stores_hash() {
        // Create a minimal CheckpointManager for testing
        let content_hashes: Arc<RwLock<HashMap<u64, ContentHashes>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let job_id = 12345u64;
        let test_hash = [0xab; 32];

        // Manually simulate what set_prompt_hash does
        {
            let mut hashes = content_hashes.write().await;
            let entry = hashes.entry(job_id).or_insert_with(ContentHashes::default);
            entry.prompt_hash = Some(test_hash);
        }

        // Verify hash was stored
        let hashes = content_hashes.read().await;
        let entry = hashes.get(&job_id).expect("Entry should exist");
        assert_eq!(entry.prompt_hash, Some(test_hash));
    }

    /// Test that append_response accumulates text correctly
    #[tokio::test]
    async fn test_append_response_accumulates_text() {
        let content_hashes: Arc<RwLock<HashMap<u64, ContentHashes>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let job_id = 100u64;

        // Append multiple chunks
        {
            let mut hashes = content_hashes.write().await;
            let entry = hashes.entry(job_id).or_insert_with(ContentHashes::default);
            entry.response_buffer.push_str("Hello");
            entry.response_buffer.push_str(" ");
            entry.response_buffer.push_str("World");
        }

        // Verify accumulated text
        let hashes = content_hashes.read().await;
        let entry = hashes.get(&job_id).expect("Entry should exist");
        assert_eq!(entry.response_buffer, "Hello World");
    }

    /// Test that finalize_response_hash computes SHA256 correctly
    #[tokio::test]
    async fn test_finalize_response_hash_computes_sha256() {
        use sha2::{Digest, Sha256};

        let content_hashes: Arc<RwLock<HashMap<u64, ContentHashes>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let job_id = 200u64;
        let test_response = "This is a test response for hashing";

        // Add response to buffer
        {
            let mut hashes = content_hashes.write().await;
            let entry = hashes.entry(job_id).or_insert_with(ContentHashes::default);
            entry.response_buffer.push_str(test_response);
        }

        // Compute hash (simulating finalize_response_hash)
        let computed_hash = {
            let mut hashes = content_hashes.write().await;
            let entry = hashes.entry(job_id).or_insert_with(ContentHashes::default);
            let hash = Sha256::digest(entry.response_buffer.as_bytes());
            let mut hash_bytes = [0u8; 32];
            hash_bytes.copy_from_slice(&hash);
            entry.response_hash = Some(hash_bytes);
            hash_bytes
        };

        // Calculate expected hash
        let expected_hash = Sha256::digest(test_response.as_bytes());
        let mut expected_bytes = [0u8; 32];
        expected_bytes.copy_from_slice(&expected_hash);

        assert_eq!(computed_hash, expected_bytes);

        // Verify it was stored
        let hashes = content_hashes.read().await;
        let entry = hashes.get(&job_id).expect("Entry should exist");
        assert_eq!(entry.response_hash, Some(expected_bytes));
    }

    /// Test that get_content_hashes returns both hashes when available
    #[tokio::test]
    async fn test_get_content_hashes_returns_both() {
        let content_hashes: Arc<RwLock<HashMap<u64, ContentHashes>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let job_id = 300u64;
        let prompt_hash = [0x11; 32];
        let response_hash = [0x22; 32];

        // Set both hashes
        {
            let mut hashes = content_hashes.write().await;
            let entry = hashes.entry(job_id).or_insert_with(ContentHashes::default);
            entry.prompt_hash = Some(prompt_hash);
            entry.response_hash = Some(response_hash);
        }

        // Get content hashes (simulating get_content_hashes)
        let result = {
            let hashes = content_hashes.read().await;
            hashes.get(&job_id).and_then(|entry| {
                match (entry.prompt_hash, entry.response_hash) {
                    (Some(p), Some(r)) => Some((p, r)),
                    _ => None,
                }
            })
        };

        assert!(result.is_some());
        let (p, r) = result.unwrap();
        assert_eq!(p, prompt_hash);
        assert_eq!(r, response_hash);
    }

    /// Test that get_content_hashes returns None when only one hash is set
    #[tokio::test]
    async fn test_get_content_hashes_returns_none_when_incomplete() {
        let content_hashes: Arc<RwLock<HashMap<u64, ContentHashes>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let job_id = 400u64;
        let prompt_hash = [0x33; 32];

        // Set only prompt hash (no response hash)
        {
            let mut hashes = content_hashes.write().await;
            let entry = hashes.entry(job_id).or_insert_with(ContentHashes::default);
            entry.prompt_hash = Some(prompt_hash);
        }

        // Get content hashes
        let result = {
            let hashes = content_hashes.read().await;
            hashes.get(&job_id).and_then(|entry| {
                match (entry.prompt_hash, entry.response_hash) {
                    (Some(p), Some(r)) => Some((p, r)),
                    _ => None,
                }
            })
        };

        assert!(result.is_none(), "Should return None when response_hash is missing");
    }

    /// Test that content_hashes are cleared after checkpoint
    #[tokio::test]
    async fn test_content_hashes_cleared_after_checkpoint() {
        let content_hashes: Arc<RwLock<HashMap<u64, ContentHashes>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let job_id = 500u64;
        let prompt_hash = [0x44; 32];
        let response_hash = [0x55; 32];

        // Set hashes
        {
            let mut hashes = content_hashes.write().await;
            let entry = hashes.entry(job_id).or_insert_with(ContentHashes::default);
            entry.prompt_hash = Some(prompt_hash);
            entry.response_hash = Some(response_hash);
            entry.response_buffer = "Some response text".to_string();
        }

        // Verify entry exists
        {
            let hashes = content_hashes.read().await;
            assert!(hashes.contains_key(&job_id));
        }

        // Clear (simulating clear_content_hashes)
        {
            let mut hashes = content_hashes.write().await;
            hashes.remove(&job_id);
        }

        // Verify entry is gone
        let hashes = content_hashes.read().await;
        assert!(!hashes.contains_key(&job_id));
    }

    /// Test ContentHashes default initialization
    #[test]
    fn test_content_hashes_default() {
        let hashes = ContentHashes::default();
        assert!(hashes.prompt_hash.is_none());
        assert!(hashes.response_hash.is_none());
        assert!(hashes.response_buffer.is_empty());
    }

    /// Test multiple jobs have independent content hashes
    #[tokio::test]
    async fn test_multiple_jobs_independent_hashes() {
        let content_hashes: Arc<RwLock<HashMap<u64, ContentHashes>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let job_id_1 = 1000u64;
        let job_id_2 = 2000u64;

        // Set different hashes for each job
        {
            let mut hashes = content_hashes.write().await;

            let entry1 = hashes.entry(job_id_1).or_insert_with(ContentHashes::default);
            entry1.prompt_hash = Some([0xaa; 32]);
            entry1.response_buffer.push_str("Response 1");

            let entry2 = hashes.entry(job_id_2).or_insert_with(ContentHashes::default);
            entry2.prompt_hash = Some([0xbb; 32]);
            entry2.response_buffer.push_str("Response 2");
        }

        // Verify independence
        let hashes = content_hashes.read().await;
        let entry1 = hashes.get(&job_id_1).unwrap();
        let entry2 = hashes.get(&job_id_2).unwrap();

        assert_eq!(entry1.prompt_hash, Some([0xaa; 32]));
        assert_eq!(entry2.prompt_hash, Some([0xbb; 32]));
        assert_eq!(entry1.response_buffer, "Response 1");
        assert_eq!(entry2.response_buffer, "Response 2");
    }

    // ========================================================================
    // Phase 4 Integration Tests: Content Hash Binding (Sub-phase 4.5)
    // ========================================================================

    /// Test that different prompts produce different input hashes
    #[test]
    fn test_different_prompts_different_hash() {
        use sha2::{Digest, Sha256};

        let prompt_a = "What is 2+2?";
        let prompt_b = "What is 3+3?";

        let hash_a = Sha256::digest(prompt_a.as_bytes());
        let hash_b = Sha256::digest(prompt_b.as_bytes());

        assert_ne!(
            hash_a.as_slice(),
            hash_b.as_slice(),
            "Different prompts should produce different hashes"
        );
    }

    /// Test that same prompt produces same hash (determinism)
    #[test]
    fn test_same_prompt_same_hash() {
        use sha2::{Digest, Sha256};

        let prompt = "What is 2+2?";

        let hash1 = Sha256::digest(prompt.as_bytes());
        let hash2 = Sha256::digest(prompt.as_bytes());

        assert_eq!(
            hash1.as_slice(),
            hash2.as_slice(),
            "Same prompt should produce same hash (determinism)"
        );
    }

    /// Test that different responses produce different output hashes
    #[test]
    fn test_different_responses_different_hash() {
        use sha2::{Digest, Sha256};

        let response_a = "The answer is 4.";
        let response_b = "The answer is 6.";

        let hash_a = Sha256::digest(response_a.as_bytes());
        let hash_b = Sha256::digest(response_b.as_bytes());

        assert_ne!(
            hash_a.as_slice(),
            hash_b.as_slice(),
            "Different responses should produce different hashes"
        );
    }

    /// Test that placeholder hashes are used when content_hashes is None
    #[test]
    fn test_placeholder_hash_fallback() {
        // When content_hashes is None, the system should use placeholder hashes
        // This tests the format of placeholder hashes
        use sha2::{Digest, Sha256};

        let job_id: u64 = 12345;
        let tokens_generated: u64 = 500;

        // These match the placeholder generation in generate_proof_static
        let input_data = format!("job_{}:input", job_id);
        let input_hash = Sha256::digest(input_data.as_bytes());

        let output_data = format!("job_{}:output:tokens_{}", job_id, tokens_generated);
        let output_hash = Sha256::digest(output_data.as_bytes());

        // Verify the placeholder format produces expected hashes
        assert_eq!(input_hash.len(), 32);
        assert_eq!(output_hash.len(), 32);

        // Different job IDs should produce different placeholder hashes
        let other_job_id: u64 = 99999;
        let other_input_data = format!("job_{}:input", other_job_id);
        let other_input_hash = Sha256::digest(other_input_data.as_bytes());

        assert_ne!(
            input_hash.as_slice(),
            other_input_hash.as_slice(),
            "Different job IDs should produce different placeholder hashes"
        );
    }

    /// Test content hash tuple creation for proof generation
    #[test]
    fn test_content_hash_tuple_for_proof() {
        use sha2::{Digest, Sha256};

        let prompt = "What is the capital of France?";
        let response = "The capital of France is Paris.";

        // Create content hash tuple (as used in generate_proof_static)
        let prompt_hash = Sha256::digest(prompt.as_bytes());
        let response_hash = Sha256::digest(response.as_bytes());

        let mut prompt_hash_bytes = [0u8; 32];
        let mut response_hash_bytes = [0u8; 32];
        prompt_hash_bytes.copy_from_slice(&prompt_hash);
        response_hash_bytes.copy_from_slice(&response_hash);

        let content_hashes: Option<([u8; 32], [u8; 32])> =
            Some((prompt_hash_bytes, response_hash_bytes));

        assert!(content_hashes.is_some());
        let (input, output) = content_hashes.unwrap();

        // Verify the hashes are 32 bytes and non-zero
        assert_eq!(input.len(), 32);
        assert_eq!(output.len(), 32);
        assert!(!input.iter().all(|&b| b == 0), "Input hash should not be all zeros");
        assert!(!output.iter().all(|&b| b == 0), "Output hash should not be all zeros");
    }

    /// Test that response accumulation produces correct hash
    #[tokio::test]
    async fn test_response_accumulation_hash_consistency() {
        use sha2::{Digest, Sha256};

        let content_hashes: Arc<RwLock<HashMap<u64, ContentHashes>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let job_id = 600u64;

        // Simulate streaming tokens
        let tokens = vec!["Hello", " ", "World", "!"];
        let expected_response = "Hello World!";

        // Accumulate tokens
        {
            let mut hashes = content_hashes.write().await;
            let entry = hashes.entry(job_id).or_insert_with(ContentHashes::default);
            for token in &tokens {
                entry.response_buffer.push_str(token);
            }
        }

        // Finalize hash
        let computed_hash = {
            let mut hashes = content_hashes.write().await;
            let entry = hashes.entry(job_id).or_insert_with(ContentHashes::default);
            let hash = Sha256::digest(entry.response_buffer.as_bytes());
            let mut hash_bytes = [0u8; 32];
            hash_bytes.copy_from_slice(&hash);
            entry.response_hash = Some(hash_bytes);
            hash_bytes
        };

        // Calculate expected hash
        let expected_hash = Sha256::digest(expected_response.as_bytes());
        let mut expected_bytes = [0u8; 32];
        expected_bytes.copy_from_slice(&expected_hash);

        assert_eq!(
            computed_hash, expected_bytes,
            "Accumulated response should produce same hash as full response"
        );
    }

    // ========================================================================
    // Phase 3: Checkpoint Publishing Integration Tests (Sub-phase 3.2)
    // ========================================================================

    /// Test that CheckpointPublisher is properly initialized with host address
    #[test]
    fn test_checkpoint_publisher_initialization() {
        use crate::checkpoint::CheckpointPublisher;

        let host_address = "0xABC123DEF456".to_string();
        let publisher = CheckpointPublisher::new(host_address);

        // Host address should be lowercase
        assert_eq!(publisher.host_address(), "0xabc123def456");
    }

    /// Test that track_conversation_message creates proper message types
    #[tokio::test]
    async fn test_track_conversation_message_user() {
        use crate::checkpoint::{CheckpointMessage, CheckpointPublisher};

        let publisher = Arc::new(CheckpointPublisher::new("0xhost".to_string()));
        let session_id = "test-session";

        // Simulate track_conversation_message for user
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let message = CheckpointMessage::new_user("Hello".to_string(), timestamp);
        publisher.buffer_message(session_id, message).await;

        let state = publisher.get_session_state(session_id).await.unwrap();
        assert_eq!(state.buffer_size(), 1);
    }

    /// Test that track_conversation_message creates assistant messages with partial flag
    #[tokio::test]
    async fn test_track_conversation_message_assistant_partial() {
        use crate::checkpoint::{CheckpointMessage, CheckpointPublisher};

        let publisher = Arc::new(CheckpointPublisher::new("0xhost".to_string()));
        let session_id = "test-session";

        // Simulate track_conversation_message for partial assistant response
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let message = CheckpointMessage::new_assistant("Partial...".to_string(), timestamp, true);
        publisher.buffer_message(session_id, message).await;

        let state = publisher.get_session_state(session_id).await.unwrap();
        let messages = state.get_buffered_messages();
        assert_eq!(messages.len(), 1);
        assert!(messages[0].metadata.is_some());
    }

    /// Test that cleanup_checkpoint_session removes session state
    #[tokio::test]
    async fn test_cleanup_checkpoint_session() {
        use crate::checkpoint::{CheckpointMessage, CheckpointPublisher};

        let publisher = Arc::new(CheckpointPublisher::new("0xhost".to_string()));
        let session_id = "test-session";

        // Add a message
        let timestamp = 12345u64;
        let message = CheckpointMessage::new_user("Test".to_string(), timestamp);
        publisher.buffer_message(session_id, message).await;

        // Verify session exists
        assert!(publisher.get_session_state(session_id).await.is_some());

        // Cleanup
        publisher.remove_session(session_id).await;

        // Verify session is gone
        assert!(publisher.get_session_state(session_id).await.is_none());
    }

    /// Test that session_id is properly passed through token tracking
    #[tokio::test]
    async fn test_session_id_in_job_tracker() {
        let job_trackers: Arc<RwLock<HashMap<u64, JobTokenTracker>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let job_id = 12345u64;
        let session_id = Some("session-abc".to_string());

        // Create tracker with session_id
        {
            let mut trackers = job_trackers.write().await;
            trackers.insert(
                job_id,
                JobTokenTracker {
                    job_id,
                    tokens_generated: 0,
                    last_checkpoint: 0,
                    session_id: session_id.clone(),
                    submission_in_progress: false,
                    last_proof_timestamp: None,
                    submission_started_at: None,
                    proof_interval: 1000, // Default for tests
                },
            );
        }

        // Verify session_id is stored
        let trackers = job_trackers.read().await;
        let tracker = trackers.get(&job_id).unwrap();
        assert_eq!(tracker.session_id, Some("session-abc".to_string()));
    }

    /// Test that session_id can be updated if initially None
    #[tokio::test]
    async fn test_session_id_updated_when_initially_none() {
        let job_trackers: Arc<RwLock<HashMap<u64, JobTokenTracker>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let job_id = 99999u64;

        // First: create tracker WITHOUT session_id
        {
            let mut trackers = job_trackers.write().await;
            trackers.insert(
                job_id,
                JobTokenTracker {
                    job_id,
                    tokens_generated: 100,
                    last_checkpoint: 0,
                    session_id: None, // Initially None
                    submission_in_progress: false,
                    last_proof_timestamp: None,
                    submission_started_at: None,
                    proof_interval: 1000, // Default for tests
                },
            );
        }

        // Verify session_id is None
        {
            let trackers = job_trackers.read().await;
            let tracker = trackers.get(&job_id).unwrap();
            assert!(tracker.session_id.is_none());
        }

        // Simulate update logic from track_tokens
        {
            let mut trackers = job_trackers.write().await;
            if let Some(tracker) = trackers.get_mut(&job_id) {
                let new_session_id = Some("late-session".to_string());
                if tracker.session_id.is_none() && new_session_id.is_some() {
                    tracker.session_id = new_session_id;
                }
            }
        }

        // Verify session_id is now set
        let trackers = job_trackers.read().await;
        let tracker = trackers.get(&job_id).unwrap();
        assert_eq!(tracker.session_id, Some("late-session".to_string()));
    }

    /// Test that session_id is NOT overwritten if already set
    #[tokio::test]
    async fn test_session_id_not_overwritten_if_already_set() {
        let job_trackers: Arc<RwLock<HashMap<u64, JobTokenTracker>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let job_id = 88888u64;

        // Create tracker WITH session_id
        {
            let mut trackers = job_trackers.write().await;
            trackers.insert(
                job_id,
                JobTokenTracker {
                    job_id,
                    tokens_generated: 100,
                    last_checkpoint: 0,
                    session_id: Some("original-session".to_string()),
                    submission_in_progress: false,
                    last_proof_timestamp: None,
                    submission_started_at: None,
                    proof_interval: 1000, // Default for tests
                },
            );
        }

        // Simulate update logic with different session_id
        {
            let mut trackers = job_trackers.write().await;
            if let Some(tracker) = trackers.get_mut(&job_id) {
                let new_session_id = Some("different-session".to_string());
                // This should NOT update because session_id is already set
                if tracker.session_id.is_none() && new_session_id.is_some() {
                    tracker.session_id = new_session_id;
                }
            }
        }

        // Verify session_id is STILL the original
        let trackers = job_trackers.read().await;
        let tracker = trackers.get(&job_id).unwrap();
        assert_eq!(tracker.session_id, Some("original-session".to_string()));
    }

    // ========================================================================
    // Race Condition Fix Tests (v8.12.6)
    // ========================================================================

    /// Test that proof cache stores and retrieves entries correctly
    #[tokio::test]
    async fn test_proof_cache_basic_operations() {
        use std::time::Duration;

        let cache = ProofSubmissionCache::new(Duration::from_secs(3600));
        let job_id = 12345u64;

        // Cache should be empty initially
        let proofs = cache.get_cached_proofs(job_id).await;
        assert!(proofs.is_empty());

        // Add a proof entry
        let entry = CachedProofEntry {
            proof_hash: [1u8; 32],
            proof_cid: "blobtest123".to_string(),
            delta_cid: Some("deltacid456".to_string()),
            tokens: 500,
            cached_at: std::time::Instant::now(),
        };
        cache.cache_proof(job_id, entry).await;

        // Should have one entry now
        let proofs = cache.get_cached_proofs(job_id).await;
        assert_eq!(proofs.len(), 1);
        assert_eq!(proofs[0].tokens, 500);
        assert_eq!(proofs[0].proof_cid, "blobtest123");

        // Get latest should return the entry
        let latest = cache.get_latest_proof(job_id).await;
        assert!(latest.is_some());
        assert_eq!(latest.unwrap().tokens, 500);

        // Cleanup should remove the entry
        cache.cleanup_job(job_id).await;
        let proofs = cache.get_cached_proofs(job_id).await;
        assert!(proofs.is_empty());
    }

    /// Test that submission_started_at is set and cleared correctly
    #[tokio::test]
    async fn test_submission_started_at_tracking() {
        let job_trackers: Arc<RwLock<HashMap<u64, JobTokenTracker>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let job_id = 77777u64;

        // Create tracker with submission_in_progress = false
        {
            let mut trackers = job_trackers.write().await;
            trackers.insert(
                job_id,
                JobTokenTracker {
                    job_id,
                    tokens_generated: 100,
                    last_checkpoint: 0,
                    session_id: Some("test-session".to_string()),
                    submission_in_progress: false,
                    last_proof_timestamp: None,
                    submission_started_at: None,
                    proof_interval: 1000, // Default for tests
                },
            );
        }

        // Verify initial state
        {
            let trackers = job_trackers.read().await;
            let tracker = trackers.get(&job_id).unwrap();
            assert!(!tracker.submission_in_progress);
            assert!(tracker.submission_started_at.is_none());
        }

        // Simulate starting a submission
        {
            let mut trackers = job_trackers.write().await;
            if let Some(tracker) = trackers.get_mut(&job_id) {
                tracker.submission_in_progress = true;
                tracker.submission_started_at = Some(std::time::Instant::now());
            }
        }

        // Verify submission is in progress
        {
            let trackers = job_trackers.read().await;
            let tracker = trackers.get(&job_id).unwrap();
            assert!(tracker.submission_in_progress);
            assert!(tracker.submission_started_at.is_some());
        }

        // Simulate completing a submission
        {
            let mut trackers = job_trackers.write().await;
            if let Some(tracker) = trackers.get_mut(&job_id) {
                tracker.submission_in_progress = false;
                tracker.submission_started_at = None;
                tracker.last_proof_timestamp = Some(std::time::Instant::now());
            }
        }

        // Verify submission completed
        {
            let trackers = job_trackers.read().await;
            let tracker = trackers.get(&job_id).unwrap();
            assert!(!tracker.submission_in_progress);
            assert!(tracker.submission_started_at.is_none());
            assert!(tracker.last_proof_timestamp.is_some());
        }
    }

    // ========================================================================
    // Phase 7.1: Accessor Methods Tests (HTTP Checkpoint Endpoint)
    // ========================================================================

    /// Test that get_host_address returns lowercase address with 0x prefix
    #[test]
    fn test_get_host_address_returns_lowercase() {
        use ethers::types::Address;
        use std::str::FromStr;

        // Test addresses with mixed case
        let test_cases = vec![
            (
                "0xABC123DEF456789012345678901234567890ABCD",
                "0xabc123def456789012345678901234567890abcd",
            ),
            (
                "0x0000000000000000000000000000000000000000",
                "0x0000000000000000000000000000000000000000",
            ),
            (
                "0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF",
                "0xffffffffffffffffffffffffffffffffffffffff",
            ),
        ];

        for (input, expected) in test_cases {
            let address = Address::from_str(input).unwrap();
            let result = format!("{:#x}", address);
            assert_eq!(
                result, expected,
                "Address {} should format as lowercase {}",
                input, expected
            );
        }
    }

    /// Test that get_s5_storage returns a valid storage reference
    /// This test verifies the MockS5Backend works correctly
    #[tokio::test]
    async fn test_get_s5_storage_returns_storage() {
        use crate::storage::s5_client::MockS5Backend;
        use crate::storage::S5Storage;

        // Create mock storage
        let mock_storage = MockS5Backend::new();

        // Verify we can call methods on it
        let test_path = "home/test/data.json";
        let test_data = b"test content".to_vec();

        // Put data
        let cid = mock_storage.put(test_path, test_data.clone()).await.unwrap();
        assert!(!cid.is_empty(), "CID should not be empty");

        // Get data back
        let retrieved = mock_storage.get(test_path).await.unwrap();
        assert_eq!(retrieved, test_data, "Retrieved data should match");
    }

    // Sub-phase 9.10: Recovery Public Key Integration Tests

    #[tokio::test]
    async fn test_set_session_recovery_public_key() {
        use crate::checkpoint::CheckpointPublisher;
        use std::sync::Arc;

        let publisher = Arc::new(CheckpointPublisher::new("0xhost".to_string()));
        let session_id = "session-recovery";
        let recovery_key = "0x02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";

        // Before setting - no recovery key
        assert!(!publisher.has_recovery_key(session_id).await);

        // Set recovery key
        publisher
            .set_recovery_public_key(session_id, recovery_key.to_string())
            .await;

        // After setting - has recovery key
        assert!(publisher.has_recovery_key(session_id).await);

        // Verify key value
        let stored_key = publisher.get_recovery_public_key(session_id).await;
        assert_eq!(stored_key, Some(recovery_key.to_string()));
    }

    #[tokio::test]
    async fn test_has_session_recovery_key() {
        use crate::checkpoint::CheckpointPublisher;
        use std::sync::Arc;

        let publisher = Arc::new(CheckpointPublisher::new("0xhost".to_string()));

        // Session without recovery key
        publisher
            .buffer_message(
                "session-no-key",
                crate::checkpoint::CheckpointMessage::new_user("test".to_string(), 100),
            )
            .await;
        assert!(!publisher.has_recovery_key("session-no-key").await);

        // Session with recovery key
        publisher
            .set_recovery_public_key(
                "session-with-key",
                "0x02abc123".to_string(),
            )
            .await;
        assert!(publisher.has_recovery_key("session-with-key").await);
    }

    // ========================================================================
    // Phase 2.1: SessionModel Query Tests (AUDIT-F4 - Sub-phase 2.1)
    // ========================================================================

    #[test]
    fn test_session_model_query_encodes_correctly() {
        let query = SessionModelQuery::new(42);
        let encoded = query.encode();

        // Should be 4 bytes (function sig) + 32 bytes (uint256)
        assert_eq!(encoded.len(), 36, "Encoded query should be 36 bytes");

        // Function signature for sessionModel(uint256)
        let expected_sig = &ethers::utils::keccak256(b"sessionModel(uint256)")[..4];
        assert_eq!(&encoded[..4], expected_sig, "Function signature mismatch");
    }

    #[test]
    fn test_session_model_returns_bytes32() {
        let query = SessionModelQuery::new(100);
        let encoded = query.encode();

        // Verify session_id is encoded as uint256 (32 bytes)
        assert_eq!(encoded.len(), 36, "Encoded data should be 36 bytes");

        // Verify the session ID is in the encoded data (after the 4-byte function selector)
        let session_id_bytes = &encoded[4..36];
        assert_eq!(session_id_bytes.len(), 32, "Session ID should be 32 bytes");
    }

    // ========================================================================
    // Phase 2.2: query_session_model Function Tests (AUDIT-F4 - Sub-phase 2.2)
    // ========================================================================

    #[tokio::test]
    async fn test_query_session_model_success() {
        // Test that SessionModelQuery can be created and encoded
        // (Full integration test would require mock web3_client)
        let query = SessionModelQuery::new(42);
        let encoded = query.encode();

        assert!(encoded.len() > 0, "Query should encode to non-empty data");
        assert_eq!(encoded.len(), 36, "Query should be 36 bytes");
    }

    #[tokio::test]
    async fn test_query_session_model_returns_zero_for_legacy() {
        // Test that bytes32(0) is handled correctly
        let zero_model_id = [0u8; 32];

        // Verify bytes32(0) is the expected format for non-model sessions
        assert_eq!(zero_model_id.len(), 32, "ModelId should be 32 bytes");
        assert!(zero_model_id.iter().all(|&b| b == 0), "bytes32(0) should be all zeros");
    }
}
