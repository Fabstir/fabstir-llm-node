use ethers::prelude::*;
use ethers::abi::Token;
use ethers::types::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use anyhow::{Result, anyhow};
use tracing::{info, warn, error};

use super::client::Web3Client;

const CHECKPOINT_THRESHOLD: u64 = 100; // Submit checkpoint every 100 tokens
const PROOF_SYSTEM_ADDRESS: &str = "0x2ACcc60893872A499700908889B38C5420CBcFD1";

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
        let proof_system_address = PROOF_SYSTEM_ADDRESS.parse()
            .map_err(|e| anyhow!("Invalid ProofSystem address: {}", e))?;

        // Get host address from web3 client
        let host_address = web3_client.address();

        Ok(Self {
            web3_client,
            job_trackers: Arc::new(RwLock::new(HashMap::new())),
            proof_system_address,
            host_address,
        })
    }

    /// Track tokens generated for a specific job
    pub async fn track_tokens(&self, job_id: u64, tokens: u64, session_id: Option<String>) -> Result<()> {
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
        if tokens_since_checkpoint >= CHECKPOINT_THRESHOLD && !tracker.submission_in_progress {
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
            "Submitting checkpoint for job {} with {} tokens...",
            job_id, tokens_generated
        );

        // Create minimal proof data (32 bytes of zeros for now)
        let proof_data = vec![0u8; 32];

        // Properly encode the function call with selector
        let data = encode_checkpoint_call(job_id, tokens_generated, proof_data);

        // Send transaction with the correct method signature
        match self.web3_client.send_transaction(
            self.proof_system_address,
            U256::zero(), // No ETH value sent
            Some(data.into())
        ).await {
            Ok(tx_hash) => {
                info!(
                    "Checkpoint submitted successfully for job {} - tx_hash: {:?}",
                    job_id, tx_hash
                );

                // Wait for confirmation
                if let Ok(receipt) = self.web3_client.wait_for_confirmation(tx_hash).await {
                    if receipt.status == Some(U64::from(1)) {
                        info!(
                            "Checkpoint confirmed for job {} - payment distributed (90% host, 10% treasury)",
                            job_id
                        );
                    } else {
                        warn!("Checkpoint transaction failed for job {}", job_id);
                    }
                }

                Ok(())
            }
            Err(e) => {
                error!("Failed to submit checkpoint for job {}: {}", job_id, e);
                Err(anyhow!("Checkpoint submission failed: {}", e))
            }
        }
    }

    /// Force submit checkpoint for a job (e.g., when session ends)
    pub async fn force_checkpoint(&self, job_id: u64) -> Result<()> {
        // Use write lock to ensure consistency
        let mut trackers = self.job_trackers.write().await;

        if let Some(tracker) = trackers.get_mut(&job_id) {
            let tokens_since_checkpoint = tracker.tokens_generated - tracker.last_checkpoint;

            if tokens_since_checkpoint > 0 && !tracker.submission_in_progress {
                let tokens_to_submit = tracker.tokens_generated;
                let previous_checkpoint = tracker.last_checkpoint;

                info!(
                    "Force submitting checkpoint for job {} with {} tokens",
                    job_id, tokens_to_submit
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
        }

        Ok(())
    }

    /// Get current token count for a job
    pub async fn get_token_count(&self, job_id: u64) -> u64 {
        let trackers = self.job_trackers.read().await;
        trackers.get(&job_id).map(|t| t.tokens_generated).unwrap_or(0)
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

// ABI encoding helper
fn encode_checkpoint_call(job_id: u64, tokens_generated: u64, proof: Vec<u8>) -> Vec<u8> {
    use ethers::abi::Function;

    // Define the function signature
    let function = Function {
        name: "submitCheckpoint".to_string(),
        inputs: vec![
            ethers::abi::Param {
                name: "jobId".to_string(),
                kind: ethers::abi::ParamType::Uint(256),
                internal_type: None,
            },
            ethers::abi::Param {
                name: "tokensGenerated".to_string(),
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