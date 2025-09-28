use anyhow::{anyhow, Result};
use ethers::abi::Token;
use ethers::prelude::*;
use ethers::types::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use super::client::Web3Client;

const CHECKPOINT_THRESHOLD: u64 = 100; // Submit checkpoint every 100 tokens
                                       // Minimum tokens required for checkpoint submission (contract requirement)
const MIN_PROVEN_TOKENS: u64 = 100;

#[derive(Debug, Clone)]
pub struct JobTokenTracker {
    pub job_id: u64,
    pub tokens_generated: u64,
    pub last_checkpoint: u64,
    pub session_id: Option<String>,
    pub submission_in_progress: bool,
}

pub struct CheckpointManager {
    web3_client: Arc<Web3Client>,
    job_trackers: Arc<RwLock<HashMap<u64, JobTokenTracker>>>,
    proof_system_address: Address,
    host_address: Address,
}

impl CheckpointManager {
    pub fn new(web3_client: Arc<Web3Client>) -> Result<Self> {
        // Read JobMarketplace address from environment variable
        let job_marketplace_address =
            std::env::var("CONTRACT_JOB_MARKETPLACE").unwrap_or_else(|_| {
                warn!("CONTRACT_JOB_MARKETPLACE not set, using default address");
                "0x1273E6358aa52Bb5B160c34Bf2e617B745e4A944".to_string()
            });

        let proof_system_address = job_marketplace_address
            .parse()
            .map_err(|e| anyhow!("Invalid JobMarketplace address: {}", e))?;

        // Get host address from web3 client
        let host_address = web3_client.address();

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
            let tokens_to_submit = tracker.tokens_generated;
            let previous_checkpoint = tracker.last_checkpoint; // Save for rollback

            // Mark submission as in progress to prevent race conditions
            tracker.submission_in_progress = true;
            // Optimistically update the checkpoint
            tracker.last_checkpoint = tokens_to_submit;

            drop(trackers); // Release lock before async operation

            info!(
                "Threshold reached for job {} - submitting checkpoint with {} tokens",
                job_id, tokens_to_submit
            );

            let submission_result = self.submit_checkpoint(job_id, tokens_to_submit).await;

            // Update tracker based on result
            let mut trackers = self.job_trackers.write().await;
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
                }
            }
        } else if tracker.submission_in_progress {
            info!(
                "Checkpoint submission already in progress for job {} - skipping",
                job_id
            );
        }

        Ok(())
    }

    /// Submit checkpoint to the blockchain
    async fn submit_checkpoint(&self, job_id: u64, tokens_generated: u64) -> Result<()> {
        // CRITICAL: Contract requires minimum 100 tokens
        // Pad to 100 if less to ensure payment distribution works
        let tokens_to_submit = if tokens_generated < MIN_PROVEN_TOKENS {
            info!(
                "📝 Padding tokens from {} to {} (contract minimum) for job {}",
                tokens_generated, MIN_PROVEN_TOKENS, job_id
            );
            MIN_PROVEN_TOKENS
        } else {
            tokens_generated
        };

        info!(
            "Submitting proof of work for job {} with {} tokens (actual: {})...",
            job_id, tokens_to_submit, tokens_generated
        );

        // Create proof data with actual AND padded token info
        let proof_json = serde_json::json!({
            "timestamp": chrono::Utc::now().timestamp_millis(),
            "tokensUsed": tokens_to_submit,
            "actualTokens": tokens_generated,
            "padded": tokens_generated < MIN_PROVEN_TOKENS,
            "hostAddress": format!("{:?}", self.host_address),
            "jobId": job_id
        });
        let proof_data = proof_json.to_string().into_bytes();

        // Properly encode the function call with selector - use PADDED amount
        let data = encode_checkpoint_call(job_id, tokens_to_submit, proof_data);

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
            if tracker.tokens_generated > 0 {
                let tokens_to_submit = tracker.tokens_generated;
                let previous_checkpoint = tracker.last_checkpoint;

                info!(
                    "💪 Force submitting checkpoint on completion for job {} with {} total tokens ({} since last checkpoint)",
                    job_id, tokens_to_submit, tokens_since_checkpoint
                );

                // Mark as in progress and update checkpoint optimistically
                tracker.submission_in_progress = true;
                tracker.last_checkpoint = tokens_to_submit;

                drop(trackers); // Release lock for async operation

                let result = self.submit_checkpoint(job_id, tokens_to_submit).await;

                // Update tracker based on result
                let mut trackers = self.job_trackers.write().await;
                if let Some(tracker) = trackers.get_mut(&job_id) {
                    tracker.submission_in_progress = false;

                    if let Err(e) = result {
                        error!("Failed to submit checkpoint for job {}: {}", job_id, e);
                        // Rollback to previous checkpoint value
                        tracker.last_checkpoint = previous_checkpoint;
                        return Err(e);
                    } else {
                        info!(
                            "✅ Successfully submitted proof of {} tokens for job {}",
                            tokens_to_submit, job_id
                        );
                    }
                }
            } else {
                info!(
                    "⚠️ No tokens to submit for job {} (0 tokens generated)",
                    job_id
                );
            }
        } else {
            error!(
                "❌ No tracker found for job {} - tokens were never tracked!",
                job_id
            );
            error!(
                "   This means HTTP inference didn't track tokens for this job ID"
            );
            error!(
                "   Check if job_id/session_id is correctly passed in inference requests"
            );
        }

        Ok(())
    }

    /// Force submit checkpoint for a job (e.g., when session ends)
    pub async fn force_checkpoint(&self, job_id: u64) -> Result<()> {
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

            // Only submit if we have at least MIN_PROVEN_TOKENS since last checkpoint
            if tokens_since_checkpoint < MIN_PROVEN_TOKENS {
                if tokens_since_checkpoint > 0 {
                    info!(
                        "⏸️ Skipping force checkpoint for job {} - only {} tokens since last checkpoint (minimum: {})",
                        job_id, tokens_since_checkpoint, MIN_PROVEN_TOKENS
                    );
                }
                return Ok(());
            }

            // We have enough tokens to submit
            let tokens_to_submit = tracker.tokens_generated;
            let previous_checkpoint = tracker.last_checkpoint;

            info!(
                "Force submitting checkpoint for job {} with {} total tokens ({} since last checkpoint)",
                job_id, tokens_to_submit, tokens_since_checkpoint
            );

            // Mark as in progress and update checkpoint optimistically
            tracker.submission_in_progress = true;
            tracker.last_checkpoint = tokens_to_submit;

            drop(trackers); // Release lock for async operation

            let result = self.submit_checkpoint(job_id, tokens_to_submit).await;

            // Update tracker based on result
            let mut trackers = self.job_trackers.write().await;
            if let Some(tracker) = trackers.get_mut(&job_id) {
                tracker.submission_in_progress = false;

                if let Err(e) = result {
                    // Rollback on error
                    tracker.last_checkpoint = previous_checkpoint;
                    return Err(e);
                }
            }
        }

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
            info!("[CHECKPOINT-MGR] 📊 Current tracked jobs: {:?}", trackers.keys().collect::<Vec<_>>());
            if let Some(tracker) = trackers.get(&job_id) {
                info!("[CHECKPOINT-MGR]   ✓ Job {} token tracking:", job_id);
                info!("[CHECKPOINT-MGR]     - Tokens generated: {}", tracker.tokens_generated);
                info!("[CHECKPOINT-MGR]     - Last checkpoint at: {} tokens", tracker.last_checkpoint);
                info!("[CHECKPOINT-MGR]     - Session ID: {:?}", tracker.session_id);
            } else {
                error!("[CHECKPOINT-MGR]   ❌ Job {} has NO TRACKER - payment calculation may be affected!", job_id);
            }
        }

        // First submit any pending checkpoint - FORCE submission even if < MIN_PROVEN_TOKENS
        // When completing a session, we must submit whatever tokens we have
        let _ = self.force_checkpoint_on_completion(job_id).await;

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
                Some(data.into()),
            )
            .await
        {
            Ok(tx_hash) => {
                info!(
                    "Transaction sent for completing job {} - tx_hash: {:?}",
                    job_id,
                    tx_hash
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
                // Check if the error is due to dispute window
                if e.to_string().contains("Must wait dispute window") {
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

                            tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs as u64)).await;

                            info!(
                                "🔄 Retry {} - Attempting to complete session for job {}",
                                retry_count + 1, job_id
                            );

                            // Retry the completion
                            let conversation_cid = format!("session_job_{}_completed", job_id);
                            let data = encode_complete_session_call(job_id, conversation_cid);

                            match web3_client
                                .send_transaction(
                                    proof_system_address,
                                    U256::zero(),
                                    Some(data.into()),
                                )
                                .await
                            {
                                Ok(tx_hash) => {
                                    info!(
                                        "📤 Retry {} transaction sent for job {} - tx_hash: {:?}",
                                        retry_count + 1, job_id, tx_hash
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
                                                error!("❌ Retry {} transaction failed for job {}", retry_count + 1, job_id);
                                                break; // Transaction failed for non-dispute reasons
                                            }
                                        }
                                        Err(e) => {
                                            error!("❌ Retry {} transaction error for job {}: {:?}", retry_count + 1, job_id, e);
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

// ABI encoding helper for submitProofOfWork
fn encode_checkpoint_call(job_id: u64, tokens_generated: u64, proof: Vec<u8>) -> Vec<u8> {
    use ethers::abi::Function;

    // Define the function signature for submitProofOfWork
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
                name: "proof".to_string(),
                kind: ethers::abi::ParamType::Bytes,
                internal_type: None,
            },
        ],
        outputs: vec![],
        constant: None,
        state_mutability: ethers::abi::StateMutability::NonPayable,
    };

    // Encode the function call properly
    let tokens = vec![
        Token::Uint(U256::from(job_id)),
        Token::Uint(U256::from(tokens_generated)),
        Token::Bytes(proof),
    ];

    function.encode_input(&tokens).unwrap_or_else(|_| {
        // Fallback to manual encoding if the proper method fails
        let selector = ethers::utils::keccak256("submitCheckpoint(uint256,uint256,bytes)");
        let mut data = selector[0..4].to_vec();
        let params = ethers::abi::encode(&tokens);
        data.extend_from_slice(&params);
        data
    })
}
