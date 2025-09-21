use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

const CHECKPOINT_THRESHOLD: u64 = 100; // Submit checkpoint every 100 tokens

#[derive(Debug, Clone)]
pub struct JobTokenInfo {
    pub job_id: u64,
    pub session_id: Option<String>,
    pub tokens_generated: u64,
    pub last_checkpoint: u64,
}

/// Simple token tracker that logs when checkpoints should be submitted
/// In production, this would integrate with Web3Client to submit actual transactions
pub struct TokenTracker {
    jobs: Arc<RwLock<HashMap<u64, JobTokenInfo>>>,
}

impl TokenTracker {
    pub fn new() -> Self {
        info!("Initializing token tracker for checkpoint management");
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Track tokens generated for a job
    pub async fn track_tokens(&self, job_id: Option<u64>, tokens: usize, session_id: Option<String>) {
        // Only track if we have a job_id
        let job_id = match job_id {
            Some(id) => id,
            None => return, // No job_id, nothing to track
        };

        let mut jobs = self.jobs.write().await;

        let job_info = jobs.entry(job_id).or_insert_with(|| {
            info!("Starting token tracking for job {} (session: {:?})", job_id, session_id);
            JobTokenInfo {
                job_id,
                session_id: session_id.clone(),
                tokens_generated: 0,
                last_checkpoint: 0,
            }
        });

        job_info.tokens_generated += tokens as u64;

        info!(
            "Generated {} tokens for job {} (total: {}, last checkpoint: {})",
            tokens, job_id, job_info.tokens_generated, job_info.last_checkpoint
        );

        // Check if we need to submit a checkpoint
        let tokens_since_checkpoint = job_info.tokens_generated - job_info.last_checkpoint;

        if tokens_since_checkpoint >= CHECKPOINT_THRESHOLD {
            info!(
                "🔔 CHECKPOINT NEEDED for job {} with {} tokens!",
                job_id, job_info.tokens_generated
            );

            // Log what should be submitted
            warn!(
                "CHECKPOINT SUBMISSION REQUIRED:\n\
                 - Job ID: {}\n\
                 - Tokens to submit: {}\n\
                 - Session ID: {:?}\n\
                 - Contract: ProofSystem at 0x2ACcc60893872A499700908889B38C5420CBcFD1\n\
                 - Function: submitCheckpoint(jobId: {}, tokensGenerated: {}, proof: 0x...)\n\
                 - Expected payment split: 90% to host, 10% to treasury\n\
                 \n\
                 NOTE: Actual blockchain submission not implemented yet!\n\
                 To enable payments:\n\
                 1. Configure HOST_PRIVATE_KEY environment variable\n\
                 2. Initialize Web3Client with Base Sepolia RPC\n\
                 3. Call ProofSystem.submitCheckpoint() with signature",
                job_id, job_info.tokens_generated, job_info.session_id,
                job_id, job_info.tokens_generated
            );

            // Mark as checkpointed (in real implementation, only after successful submission)
            job_info.last_checkpoint = job_info.tokens_generated;
        }
    }

    /// Get token count for a job
    pub async fn get_token_count(&self, job_id: u64) -> u64 {
        let jobs = self.jobs.read().await;
        jobs.get(&job_id).map(|j| j.tokens_generated).unwrap_or(0)
    }

    /// Force checkpoint for a job (e.g., when session ends)
    pub async fn force_checkpoint(&self, job_id: u64) {
        let jobs = self.jobs.read().await;

        if let Some(job_info) = jobs.get(&job_id) {
            let tokens_since_checkpoint = job_info.tokens_generated - job_info.last_checkpoint;

            if tokens_since_checkpoint > 0 {
                warn!(
                    "🔔 FORCE CHECKPOINT for job {} with {} total tokens ({} since last checkpoint)",
                    job_id, job_info.tokens_generated, tokens_since_checkpoint
                );
            }
        }
    }

    /// Clean up job tracking
    pub async fn cleanup_job(&self, job_id: u64) {
        // Force checkpoint before cleanup
        self.force_checkpoint(job_id).await;

        let mut jobs = self.jobs.write().await;
        if jobs.remove(&job_id).is_some() {
            info!("Cleaned up token tracking for job {}", job_id);
        }
    }

    /// Get summary of all tracked jobs
    pub async fn get_summary(&self) -> Vec<JobTokenInfo> {
        let jobs = self.jobs.read().await;
        jobs.values().cloned().collect()
    }
}