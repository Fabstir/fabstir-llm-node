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
        eprintln!("🔖 BUILD VERSION: v6-env-contract-addresses-2024-09-22");

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
        info!(
            "Submitting proof of work for job {} with {} tokens...",
            job_id, tokens_generated
        );

        // Create proof data with timestamp and token info
        let proof_json = serde_json::json!({
            "timestamp": chrono::Utc::now().timestamp_millis(),
            "tokensUsed": tokens_generated,
            "hostAddress": format!("{:?}", self.host_address),
            "jobId": job_id
        });
        let proof_data = proof_json.to_string().into_bytes();

        // Properly encode the function call with selector
        let data = encode_checkpoint_call(job_id, tokens_generated, proof_data);

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
        // First submit any pending checkpoint
        let _ = self.force_checkpoint(job_id).await;

        // Now call completeSessionJob on the contract to trigger payment settlement
        info!(
            "💰 Completing session job {} to trigger payment settlement...",
            job_id
        );

        let data = encode_complete_session_call(job_id);
        let tx_request = TransactionRequest::new()
            .to(self.proof_system_address)
            .data(data)
            .gas(U256::from(300_000));

        match self
            .web3_client
            .provider
            .send_transaction(tx_request, None)
            .await
        {
            Ok(pending_tx) => {
                info!(
                    "Transaction sent for completing job {} - tx_hash: {:?}",
                    job_id,
                    pending_tx.tx_hash()
                );

                // Wait for confirmation
                let start = std::time::Instant::now();
                match pending_tx.await {
                    Ok(Some(receipt)) => {
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
                    Ok(None) => {
                        error!("❌ Transaction dropped for job {}", job_id);
                    }
                    Err(e) => {
                        error!("❌ Transaction error for job {}: {:?}", job_id, e);
                    }
                }
            }
            Err(e) => {
                error!(
                    "❌ Failed to send complete session transaction for job {}: {:?}",
                    job_id, e
                );
            }
        }

        // Clean up tracker
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
fn encode_complete_session_call(job_id: u64) -> Vec<u8> {
    use ethers::abi::Function;

    // Define the function signature for completeSessionJob
    let function = Function {
        name: "completeSessionJob".to_string(),
        inputs: vec![ethers::abi::Param {
            name: "jobId".to_string(),
            kind: ethers::abi::ParamType::Uint(256),
            internal_type: None,
        }],
        outputs: vec![],
        constant: None,
        state_mutability: ethers::abi::StateMutability::NonPayable,
    };

    // Encode the call data
    let tokens = vec![ethers::abi::Token::Uint(U256::from(job_id))];
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
