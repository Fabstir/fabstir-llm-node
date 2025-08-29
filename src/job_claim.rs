use ethers::prelude::*;
use ethers::types::{Address, H256, U256};
use std::sync::Arc;
use std::collections::{HashSet, HashMap};
use tokio::sync::{RwLock, mpsc};
use tokio::time::{sleep, Duration};
use anyhow::{Result, anyhow};
use serde::{Serialize, Deserialize};
use tracing::{info, warn, error, debug};

use crate::contracts::Web3Client;
use crate::job_processor::{JobRequest, JobStatus, NodeConfig};
use crate::job_assignment_types::{JobClaimConfig, AssignmentRecord, AssignmentStatus};
use crate::host::registry::HostRegistry;
use crate::host::selection::{HostSelector, JobRequirements};

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
    assignments: Arc<RwLock<HashMap<String, AssignmentRecord>>>,
    host_registry: Option<Arc<HostRegistry>>,
    host_selector: Option<Arc<HostSelector>>,
}

impl JobClaimer {
    pub fn new_with_marketplace<C: Into<ClaimConfig>>(config: C, marketplace: Arc<dyn JobMarketplaceTrait>) -> Self {
        Self {
            config: config.into(),
            marketplace,
            claimed_jobs: Arc::new(RwLock::new(HashSet::new())),
            event_subscribers: Arc::new(RwLock::new(Vec::new())),
            active_claims: Arc::new(RwLock::new(0)),
            assignments: Arc::new(RwLock::new(HashMap::new())),
            host_registry: None,
            host_selector: None,
        }
    }

    pub async fn new(config: JobClaimConfig) -> Result<Self> {
        let claim_config = ClaimConfig {
            node_address: Address::zero(),
            max_concurrent_jobs: config.max_concurrent_jobs,
            claim_retry_attempts: 3,
            claim_retry_delay: Duration::from_millis(config.claim_timeout_ms),
            max_gas_price: U256::from(100_000_000_000u64),
            supported_models: vec![],
            min_payment_per_token: U256::zero(),
        };
        let marketplace = Arc::new(MockMarketplace {
            registered_nodes: Arc::new(RwLock::new(HashSet::new())),
            jobs: Arc::new(RwLock::new(HashMap::new())),
            claimed_jobs: Arc::new(RwLock::new(HashSet::new())),
        }) as Arc<dyn JobMarketplaceTrait>;
        
        Ok(Self::new_with_marketplace(claim_config, marketplace))
    }
    
    pub fn with_host_management(mut self, registry: Arc<HostRegistry>, selector: Arc<HostSelector>) -> Self {
        self.host_registry = Some(registry);
        self.host_selector = Some(selector);
        self
    }

    pub async fn claim_job(&self, job_id: H256) -> ClaimResult {
        let mut active = self.active_claims.write().await;
        if *active >= self.config.max_concurrent_jobs {
            return Err(ClaimError::Other("Max concurrent jobs reached".to_string()));
        }
        *active += 1;
        drop(active);
        let result = self.try_claim_job(job_id).await;
        if result.is_err() {
            *self.active_claims.write().await -= 1;
        }
        
        result
    }
    
    async fn try_claim_job(&self, job_id: H256) -> ClaimResult {
        if !self.marketplace.is_node_registered(self.config.node_address).await {
            return Err(ClaimError::NodeNotRegistered);
        }

        let job = self.marketplace.get_job(job_id).await
            .ok_or(ClaimError::JobNotFound)?;

        if self.marketplace.is_job_claimed(job_id).await {
            return Err(ClaimError::JobAlreadyClaimed);
        }

        self.validate_job(&job)?;
        let gas_cost = self.estimate_claim_gas(job_id).await?;
        if !self.is_profitable(&job, gas_cost).await? {
            return Err(ClaimError::Other("Job not profitable".to_string()));
        }

        self.marketplace.claim_job(job_id, self.config.node_address).await?;
        self.claimed_jobs.write().await.insert(job_id);
        self.emit_event(ClaimEvent {
            job_id,
            node_address: self.config.node_address,
            event_type: "JobClaimed".to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }).await;

        Ok(H256::random())
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

    pub async fn unclaim_job(&self, job_id: H256) -> Result<(), ClaimError> {
        self.marketplace.unclaim_job(job_id).await?;
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
    // Assignment methods
    pub async fn assign_job_to_host(&self, job_id: &str, host_address: Address, _registry: &Arc<HostRegistry>) -> Result<()> {
        let mut assignments = self.assignments.write().await;
        let record = AssignmentRecord {
            job_id: job_id.to_string(),
            host_address,
            assigned_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            status: AssignmentStatus::Confirmed,
        };
        assignments.insert(job_id.to_string(), record);
        info!("Assigned job to host: {}", host_address);
        Ok(())
    }

    pub async fn batch_assign_jobs(&self, job_assignments: Vec<(&str, Address)>, registry: &Arc<HostRegistry>) -> Result<Vec<Result<()>>> {
        let mut results = Vec::new();
        for (job_id, host) in job_assignments {
            results.push(self.assign_job_to_host(job_id, host, registry).await);
        }
        Ok(results)
    }

    pub async fn reassign_job(&self, job_id: &str, new_host: Address, _registry: &Arc<HostRegistry>) -> Result<()> {
        let mut assignments = self.assignments.write().await;
        if let Some(record) = assignments.get_mut(job_id) {
            record.host_address = new_host;
            record.status = AssignmentStatus::Reassigned;
            record.assigned_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            info!("Reassigned job {} to host: {}", job_id, new_host);
            Ok(())
        } else {
            Err(anyhow!("Job {} not found in assignments", job_id))
        }
    }

    pub async fn auto_assign_job(&self, job_id: &str, registry: &Arc<HostRegistry>, selector: &Arc<HostSelector>, requirements: &JobRequirements) -> Result<Address> {
        let hosts = registry.get_available_hosts(&requirements.model_id).await;
        if hosts.is_empty() {
            return Err(anyhow!("No available hosts found"));
        }

        let mut host_infos = Vec::new();
        for host in hosts {
            if let Some(info) = registry.get_host_metadata(host).await {
                host_infos.push(info);
            }
        }

        if host_infos.is_empty() {
            return Err(anyhow!("No host info available"));
        }

        let selected_host = selector.select_best_host(host_infos.clone(), &requirements).await
            .ok_or_else(|| anyhow!("Failed to select best host"))?;

        self.assign_job_to_host(job_id, selected_host, registry).await?;
        Ok(selected_host)
    }

    pub async fn get_assignment_record(&self, job_id: &str) -> Option<AssignmentRecord> {
        self.assignments.read().await.get(job_id).cloned()
    }

    pub async fn get_host_assignments(&self, host: Address) -> Vec<String> {
        let assignments = self.assignments.read().await;
        assignments.iter()
            .filter_map(|(job_id, record)| {
                if record.host_address == host {
                    Some(job_id.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub async fn update_assignment_status(&self, job_id: &str, status: AssignmentStatus) -> Result<()> {
        let mut assignments = self.assignments.write().await;
        if let Some(record) = assignments.get_mut(job_id) {
            record.status = status;
            Ok(())
        } else {
            Err(anyhow!("Assignment not found for job {}", job_id))
        }
    }

    pub async fn add_priority_job(&self, job_id: &str, priority: u32) {
        let mut assignments = self.assignments.write().await;
        let record = AssignmentRecord {
            job_id: job_id.to_string(),
            host_address: Address::zero(),
            assigned_at: priority as u64,
            status: AssignmentStatus::Pending,
        };
        assignments.insert(job_id.to_string(), record);
    }

    pub async fn process_priority_assignments(&self, host: Address, _registry: &Arc<HostRegistry>, limit: usize) -> Vec<String> {
        let mut assignments = self.assignments.write().await;
        let mut processed = Vec::new();
        
        let mut pending: Vec<_> = assignments.iter_mut()
            .filter(|(_, record)| record.status == AssignmentStatus::Pending)
            .collect();
        pending.sort_by(|a, b| b.1.assigned_at.cmp(&a.1.assigned_at));
        
        for (job_id, record) in pending.into_iter().take(limit) {
            record.host_address = host;
            record.status = AssignmentStatus::Confirmed;
            processed.push(job_id.clone());
        }
        
        processed
    }
}

// Mock marketplace for testing
pub struct MockMarketplace {
    registered_nodes: Arc<RwLock<HashSet<Address>>>,
    jobs: Arc<RwLock<HashMap<H256, JobRequest>>>,
    claimed_jobs: Arc<RwLock<HashSet<H256>>>,
}

#[async_trait::async_trait]
impl JobMarketplaceTrait for MockMarketplace {
    async fn is_node_registered(&self, node_address: Address) -> bool {
        self.registered_nodes.read().await.contains(&node_address)
    }
    
    async fn is_job_claimed(&self, job_id: H256) -> bool {
        self.claimed_jobs.read().await.contains(&job_id)
    }
    
    async fn claim_job(&self, job_id: H256, _node_address: Address) -> Result<(), ClaimError> {
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
        Ok(U256::from(20_000_000_000u64))
    }
}