use ethers::prelude::*;
use ethers::types::{Address, H256, U256};
use std::sync::Arc;
use std::collections::HashSet;
use tokio::sync::{RwLock, mpsc};
use tokio::time::{sleep, Duration};
use anyhow::{Result, anyhow};
use serde::{Serialize, Deserialize};
use tracing::{info, warn, error, debug};

use crate::contracts::Web3Client;
use crate::job_processor::{JobRequest, JobStatus, NodeConfig};

#[derive(Debug, Clone)]
pub enum ClaimError {
    NodeNotRegistered,
    JobNotFound,
    JobAlreadyClaimed,
    GasPriceTooHigh,
    BelowMinimumThreshold,
    UnsupportedModel,
    InvalidJob,
    ContractError(String),
    Other(String),
}

impl std::fmt::Display for ClaimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClaimError::NodeNotRegistered => write!(f, "Node not registered"),
            ClaimError::JobNotFound => write!(f, "Job not found"),
            ClaimError::JobAlreadyClaimed => write!(f, "Job already claimed"),
            ClaimError::GasPriceTooHigh => write!(f, "Gas price too high"),
            ClaimError::BelowMinimumThreshold => write!(f, "Below minimum threshold"),
            ClaimError::UnsupportedModel => write!(f, "Unsupported model"),
            ClaimError::InvalidJob => write!(f, "Invalid job parameters"),
            ClaimError::ContractError(e) => write!(f, "Contract error: {}", e),
            ClaimError::Other(e) => write!(f, "Other error: {}", e),
        }
    }
}

impl std::error::Error for ClaimError {}

impl From<anyhow::Error> for ClaimError {
    fn from(err: anyhow::Error) -> Self {
        ClaimError::Other(err.to_string())
    }
}

pub type ClaimResult = Result<H256, ClaimError>;

#[derive(Debug, Clone)]
pub struct ClaimEvent {
    pub job_id: H256,
    pub node_address: Address,
    pub event_type: String,
    pub timestamp: u64,
}

// Extended NodeConfig with claim-specific fields
#[derive(Debug, Clone)]
pub struct ClaimConfig {
    pub node_address: Address,
    pub max_concurrent_jobs: usize,
    pub claim_retry_attempts: usize,
    pub claim_retry_delay: Duration,
    pub max_gas_price: U256,
    pub supported_models: Vec<String>,
    pub min_payment_per_token: U256,
}

impl From<NodeConfig> for ClaimConfig {
    fn from(config: NodeConfig) -> Self {
        Self {
            node_address: config.node_address,
            max_concurrent_jobs: config.max_concurrent_jobs,
            claim_retry_attempts: config.claim_retry_attempts,
            claim_retry_delay: config.claim_retry_delay,
            max_gas_price: config.max_gas_price,
            supported_models: config.supported_models,
            min_payment_per_token: config.min_payment_per_token,
        }
    }
}

// Contract interface trait
#[async_trait::async_trait]
pub trait JobMarketplaceTrait: Send + Sync {
    async fn is_node_registered(&self, node_address: Address) -> bool;
    async fn is_job_claimed(&self, job_id: H256) -> bool;
    async fn claim_job(&self, job_id: H256, node_address: Address) -> Result<(), ClaimError>;
    async fn unclaim_job(&self, job_id: H256) -> Result<(), ClaimError>;
    async fn get_job(&self, job_id: H256) -> Option<JobRequest>;
    async fn get_all_jobs(&self) -> Vec<JobRequest>;
    async fn estimate_gas(&self, job_id: H256) -> Result<U256, anyhow::Error>;
    async fn get_gas_price(&self) -> Result<U256, anyhow::Error>;
}

// Alias for the trait to match test expectations
pub use JobMarketplaceTrait as ClaimMarketplaceTrait;

#[derive(Clone)]
pub struct JobClaimer {
    config: ClaimConfig,
    marketplace: Arc<dyn JobMarketplaceTrait>,
    claimed_jobs: Arc<RwLock<HashSet<H256>>>,
    event_subscribers: Arc<RwLock<Vec<mpsc::Sender<ClaimEvent>>>>,
    active_claims: Arc<RwLock<usize>>,
}

impl JobClaimer {
    pub fn new<C: Into<ClaimConfig>>(config: C, marketplace: Arc<dyn JobMarketplaceTrait>) -> Self {
        Self {
            config: config.into(),
            marketplace,
            claimed_jobs: Arc::new(RwLock::new(HashSet::new())),
            event_subscribers: Arc::new(RwLock::new(Vec::new())),
            active_claims: Arc::new(RwLock::new(0)),
        }
    }

    pub async fn claim_job(&self, job_id: H256) -> ClaimResult {
        // Reserve slot first by incrementing active claims
        let mut active = self.active_claims.write().await;
        if *active >= self.config.max_concurrent_jobs {
            return Err(ClaimError::Other("Max concurrent jobs reached".to_string()));
        }
        *active += 1;
        drop(active); // Release write lock early

        // Now try to claim the job
        let result = self.try_claim_job(job_id).await;
        
        // If claim failed, decrement active claims
        if result.is_err() {
            *self.active_claims.write().await -= 1;
        }
        
        result
    }
    
    async fn try_claim_job(&self, job_id: H256) -> ClaimResult {
        // Verify node is registered
        if !self.marketplace.is_node_registered(self.config.node_address).await {
            return Err(ClaimError::NodeNotRegistered);
        }

        // Check if job exists
        let job = self.marketplace.get_job(job_id).await
            .ok_or(ClaimError::JobNotFound)?;

        // Check if already claimed
        if self.marketplace.is_job_claimed(job_id).await {
            return Err(ClaimError::JobAlreadyClaimed);
        }

        // Validate job parameters
        self.validate_job(&job)?;

        // Estimate gas and check profitability
        let gas_cost = self.estimate_claim_gas(job_id).await?;
        if !self.is_profitable(&job, gas_cost).await? {
            return Err(ClaimError::Other("Job not profitable".to_string()));
        }

        // Attempt to claim
        self.marketplace.claim_job(job_id, self.config.node_address).await?;

        // Update internal state
        self.claimed_jobs.write().await.insert(job_id);

        // Emit event
        self.emit_event(ClaimEvent {
            job_id,
            node_address: self.config.node_address,
            event_type: "JobClaimed".to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }).await;

        Ok(H256::random()) // Return mock transaction hash
    }

    pub async fn claim_batch(&self, job_ids: &[H256]) -> Vec<ClaimResult> {
        let mut results = Vec::new();

        for &job_id in job_ids {
            results.push(self.claim_job(job_id).await);
        }

        results
    }

    pub async fn claim_job_with_retry(&self, job_id: H256) -> ClaimResult {
        let mut attempts = 0;
        let mut last_error = None;

        while attempts < self.config.claim_retry_attempts {
            match self.claim_job(job_id).await {
                Ok(tx_hash) => return Ok(tx_hash),
                Err(e) => {
                    last_error = Some(e.clone());
                    
                    // Don't retry on certain errors
                    match &e {
                        ClaimError::NodeNotRegistered |
                        ClaimError::JobNotFound |
                        ClaimError::JobAlreadyClaimed |
                        ClaimError::UnsupportedModel => return Err(e),
                        _ => {}
                    }

                    attempts += 1;
                    if attempts < self.config.claim_retry_attempts {
                        sleep(self.config.claim_retry_delay).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or(ClaimError::Other("Unknown error".to_string())))
    }

    pub async fn estimate_claim_gas(&self, job_id: H256) -> Result<U256, ClaimError> {
        self.marketplace.estimate_gas(job_id).await
            .map_err(|e| ClaimError::Other(e.to_string()))
    }

    pub async fn is_claim_profitable(&self, job_id: H256) -> Result<bool, ClaimError> {
        let job = self.marketplace.get_job(job_id).await
            .ok_or(ClaimError::JobNotFound)?;
        
        let gas_cost = self.estimate_claim_gas(job_id).await?;
        Ok(self.is_profitable(&job, gas_cost).await?)
    }

    async fn is_profitable(&self, job: &JobRequest, gas_cost: U256) -> Result<bool> {
        let gas_price = self.marketplace.get_gas_price().await?;
        let total_gas_cost = gas_cost * gas_price;
        
        // Check if payment covers gas + minimum profit
        let min_profit = job.payment_amount / U256::from(10); // 10% minimum profit
        Ok(job.payment_amount > total_gas_cost + min_profit)
    }

    fn validate_job(&self, job: &JobRequest) -> Result<(), ClaimError> {
        // Check supported models
        if !self.config.supported_models.is_empty() 
            && !self.config.supported_models.contains(&job.model_id) {
            return Err(ClaimError::UnsupportedModel);
        }

        // Check for valid max_tokens
        if job.max_tokens == 0 {
            return Err(ClaimError::InvalidJob);
        }

        // Check minimum payment per token
        let payment_per_token = job.payment_amount / U256::from(job.max_tokens);
        if payment_per_token < self.config.min_payment_per_token {
            return Err(ClaimError::BelowMinimumThreshold);
        }

        Ok(())
    }

    pub async fn get_claimable_jobs(&self) -> Vec<JobRequest> {
        // Get all jobs from the marketplace
        let all_jobs = self.marketplace.get_all_jobs().await;
        
        // Filter jobs that meet our criteria
        let mut claimable_jobs = Vec::new();
        for job in all_jobs {
            // Skip already claimed jobs
            if self.marketplace.is_job_claimed(job.job_id).await {
                continue;
            }
            
            // Validate job against our criteria
            if self.validate_job(&job).is_ok() {
                // Check profitability
                if let Ok(gas_cost) = self.estimate_claim_gas(job.job_id).await {
                    if let Ok(is_profitable) = self.is_profitable(&job, gas_cost).await {
                        if is_profitable {
                            claimable_jobs.push(job);
                        }
                    }
                }
            }
        }
        
        claimable_jobs
    }

    pub async fn add_to_accumulator(&self, _job_id: H256) {
        // For payment accumulation in payment_claim module
    }

    pub async fn check_timeout(&self, job_id: H256) -> bool {
        // Check if job has timed out
        if let Some(job) = self.marketplace.get_job(job_id).await {
            // Get current timestamp (seconds since UNIX epoch)
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as u64;
            
            // Check if deadline has passed
            // Note: deadline is stored as U256, convert to u64 for comparison
            let deadline = job.deadline.as_u64();
            current_time > deadline
        } else {
            false
        }
    }

    pub async fn unclaim_job(&self, job_id: H256) -> Result<(), ClaimError> {
        // First unclaim from the marketplace
        self.marketplace.unclaim_job(job_id).await?;
        
        // Then update internal state
        self.claimed_jobs.write().await.remove(&job_id);
        *self.active_claims.write().await -= 1;
        Ok(())
    }

    pub async fn subscribe_to_events(&self) -> mpsc::Receiver<ClaimEvent> {
        let (tx, rx) = mpsc::channel(100);
        self.event_subscribers.write().await.push(tx);
        rx
    }

    async fn emit_event(&self, event: ClaimEvent) {
        let subscribers = self.event_subscribers.read().await;
        for subscriber in subscribers.iter() {
            let _ = subscriber.send(event.clone()).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MockJobMarketplace {
        registered_nodes: Arc<RwLock<HashSet<Address>>>,
        jobs: Arc<RwLock<HashMap<H256, JobRequest>>>,
        claimed_jobs: Arc<RwLock<HashSet<H256>>>,
    }

    #[async_trait::async_trait]
    impl JobMarketplaceTrait for MockJobMarketplace {
        async fn is_node_registered(&self, node_address: Address) -> bool {
            self.registered_nodes.read().await.contains(&node_address)
        }

        async fn is_job_claimed(&self, job_id: H256) -> bool {
            self.claimed_jobs.read().await.contains(&job_id)
        }

        async fn claim_job(&self, job_id: H256, _node_address: Address) -> Result<(), ClaimError> {
            if !self.jobs.read().await.contains_key(&job_id) {
                return Err(ClaimError::JobNotFound);
            }
            if self.claimed_jobs.read().await.contains(&job_id) {
                return Err(ClaimError::JobAlreadyClaimed);
            }
            self.claimed_jobs.write().await.insert(job_id);
            Ok(())
        }

        async fn unclaim_job(&self, job_id: H256) -> Result<(), ClaimError> {
            self.claimed_jobs.write().await.remove(&job_id);
            Ok(())
        }

        async fn get_job(&self, job_id: H256) -> Option<JobRequest> {
            self.jobs.read().await.get(&job_id).cloned()
        }
        
        async fn get_all_jobs(&self) -> Vec<JobRequest> {
            self.jobs.read().await.values().cloned().collect()
        }

        async fn estimate_gas(&self, _job_id: H256) -> Result<U256> {
            Ok(U256::from(100_000))
        }

        async fn get_gas_price(&self) -> Result<U256> {
            Ok(U256::from(20_000_000_000u64)) // 20 gwei
        }
    }
}