use anyhow::{anyhow, Result};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use ethers::types::{Address, U256};
use serde::{Deserialize, Serialize};
use tracing::{info, debug, error, warn};

// Use the existing contracts module
use crate::contracts::client::Web3Client;

// Define Job types since they're not in contracts::types yet
#[derive(Debug, Clone)]
pub struct Job {
    pub id: U256,
    pub client: Address,
    pub max_price_per_token: U256,
    pub model_id: String,
    pub input_url: String,
    pub output_url: String,
    pub state: JobState,
    pub selected_host: Address,
    pub result_commitment: [u8; 32],
    pub created_at: U256,
    pub deadline: U256,
    pub max_tokens: U256,
    pub chain_id: u64, // Add chain ID to job
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobState {
    Open,
    Assigned,
    Completed,
    Cancelled,
    Disputed,
}

/// Job verification configuration
#[derive(Debug, Clone)]
pub struct JobVerificationConfig {
    pub enabled: bool,
    pub blockchain_verification: bool,
    pub cache_duration: Duration,
    pub marketplace_addresses: HashMap<u64, String>, // Per-chain marketplace addresses
    pub supported_chains: Vec<u64>,
}

impl Default for JobVerificationConfig {
    fn default() -> Self {
        let mut marketplace_addresses = HashMap::new();
        marketplace_addresses.insert(84532, "0x7ce861CC0188c260f3Ba58eb9a4d33e17Eb62304".to_string()); // Base Sepolia
        marketplace_addresses.insert(5611, "0x0000000000000000000000000000000000000000".to_string()); // opBNB placeholder

        Self {
            enabled: true,
            blockchain_verification: true,
            cache_duration: Duration::from_secs(300), // 5 minutes
            marketplace_addresses,
            supported_chains: vec![84532, 5611],
        }
    }
}

/// Job status enum
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Claimed,
    Completed,
    Failed,
    Expired,
}

impl From<JobState> for JobStatus {
    fn from(state: JobState) -> Self {
        match state {
            JobState::Open => JobStatus::Pending,
            JobState::Assigned => JobStatus::Claimed,
            JobState::Completed => JobStatus::Completed,
            JobState::Cancelled => JobStatus::Failed,
            JobState::Disputed => JobStatus::Failed,
        }
    }
}

/// Job details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobDetails {
    pub job_id: u64,
    pub chain_id: u64,
    pub client_address: String,
    pub payment_amount: u128,
    pub model_id: String,
    pub input_url: String,
    pub output_url: Option<String>,
    pub status: JobStatus,
    pub created_at: u64,
    pub deadline: u64,
}

/// Verification result
#[derive(Debug, Clone)]
pub struct VerificationResult {
    pub is_valid: bool,
    pub job_details: Option<JobDetails>,
    pub error: Option<String>,
}

/// Cache entry with chain ID
#[derive(Debug, Clone)]
struct CacheEntry {
    details: JobDetails,
    timestamp: Instant,
    chain_id: u64,
}

/// Job verifier with blockchain integration
pub struct JobVerifier {
    config: JobVerificationConfig,
    web3_clients: HashMap<u64, Arc<Web3Client>>, // Per-chain Web3 clients
    cache: Arc<RwLock<HashMap<(u64, u64), CacheEntry>>>, // (chain_id, job_id) -> entry
}

impl JobVerifier {
    /// Create new job verifier with multi-chain support
    pub async fn new(config: JobVerificationConfig) -> Result<Self> {
        let mut web3_clients = HashMap::new();

        if config.blockchain_verification {
            use crate::contracts::client::Web3Config;

            // Initialize Web3 client for each supported chain
            for chain_id in &config.supported_chains {
                let rpc_url = match chain_id {
                    84532 => "https://sepolia.base.org".to_string(),
                    5611 => "https://opbnb-testnet-rpc.bnbchain.org".to_string(),
                    _ => continue,
                };

                let web3_config = Web3Config {
                    rpc_url,
                    chain_id: *chain_id,
                    confirmations: if *chain_id == 84532 { 3 } else { 15 },
                    polling_interval: Duration::from_secs(5),
                    private_key: None,
                    max_reconnection_attempts: 3,
                    reconnection_delay: Duration::from_secs(1),
                };

                match Web3Client::new(web3_config).await {
                    Ok(client) => {
                        web3_clients.insert(*chain_id, Arc::new(client));
                    }
                    Err(e) => {
                        warn!("Failed to create Web3 client for chain {}: {}", chain_id, e);
                    }
                }
            }
        }

        Ok(Self {
            config,
            web3_clients,
            cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }
    
    /// Verify a job by ID on a specific chain
    pub async fn verify_job(&self, job_id: u64, chain_id: u64) -> Result<JobDetails> {
        // Validate chain is supported
        if !self.config.supported_chains.contains(&chain_id) {
            return Err(anyhow!("Chain {} is not supported", chain_id));
        }

        // If disabled, return mock job
        if !self.config.enabled {
            return Ok(self.create_mock_job(job_id, chain_id));
        }

        // Check cache first
        if let Some(details) = self.get_cached_job(job_id, chain_id).await {
            debug!("Job {} found in cache for chain {}", job_id, chain_id);
            return Ok(details);
        }

        // Verify from blockchain
        if let Some(client) = self.web3_clients.get(&chain_id) {
            let job = self.fetch_job_from_blockchain(client, job_id).await?;
            let details = self.convert_job_to_details(job_id, chain_id, job)?;

            // Cache the result
            self.cache_job(job_id, chain_id, details.clone()).await;

            Ok(details)
        } else {
            // Fallback to mock if no blockchain client for this chain
            warn!("No Web3 client for chain {}, using mock", chain_id);
            Ok(self.create_mock_job(job_id, chain_id))
        }
    }
    
    /// Check if job can be claimed
    pub async fn can_claim_job(&self, job: &JobDetails) -> bool {
        if !self.config.enabled {
            return true;
        }
        
        // Check status
        if job.status != JobStatus::Pending {
            return false;
        }
        
        // Check if expired
        if self.is_job_expired(job).await {
            return false;
        }
        
        true
    }
    
    /// Check if job is expired
    pub async fn is_job_expired(&self, job: &JobDetails) -> bool {
        let now = chrono::Utc::now().timestamp() as u64;
        job.deadline < now
    }
    
    /// Verify payment is escrowed on specific chain
    pub async fn verify_payment_escrowed(&self, job: &JobDetails) -> Result<bool> {
        if !self.config.enabled {
            return Ok(true);
        }

        if let Some(client) = self.web3_clients.get(&job.chain_id) {
            // Check escrow contract for payment on the job's chain
            debug!("Verifying payment for job {} on chain {}: {} wei",
                   job.job_id, job.chain_id, job.payment_amount);

            // For now, assume payment is verified if amount > 0
            Ok(job.payment_amount > 0)
        } else {
            warn!("No Web3 client for chain {}", job.chain_id);
            Ok(true) // Mock verification passes
        }
    }
    
    /// Create claim message for signing with chain context
    pub async fn create_claim_message(&self, job_id: u64, chain_id: u64, host_address: &str) -> String {
        format!("Claim job {} on chain {} as host {}", job_id, chain_id, host_address)
    }
    
    /// Verify claim signature on specific chain
    pub async fn verify_claim_signature(
        &self,
        job_id: u64,
        chain_id: u64,
        host_address: &str,
        signature: &str,
    ) -> Result<bool> {
        if !self.config.enabled {
            return Ok(true);
        }

        if let Some(client) = self.web3_clients.get(&chain_id) {
            let message = self.create_claim_message(job_id, chain_id, host_address).await;
            
            // Verify signature using ethers
            // This would use actual signature verification
            debug!("Verifying signature for job {} from {}", job_id, host_address);
            
            // Mock verification for now
            Ok(!signature.is_empty())
        } else {
            Ok(true)
        }
    }
    
    /// Get job metadata
    pub async fn get_job_metadata(&self, job_id: u64, chain_id: u64) -> Result<HashMap<String, String>> {
        let job = self.verify_job(job_id, chain_id).await?;
        
        let mut metadata = HashMap::new();
        metadata.insert("job_id".to_string(), job.job_id.to_string());
        metadata.insert("model_id".to_string(), job.model_id.clone());
        metadata.insert("input_url".to_string(), job.input_url.clone());
        metadata.insert("status".to_string(), format!("{:?}", job.status));
        metadata.insert("payment_amount".to_string(), job.payment_amount.to_string());
        metadata.insert("max_tokens".to_string(), "1000".to_string()); // Default
        metadata.insert("chain_id".to_string(), job.chain_id.to_string());

        Ok(metadata)
    }
    
    /// Batch verify jobs on a specific chain
    pub async fn batch_verify_jobs(&self, job_ids: Vec<u64>, chain_id: u64) -> Result<Vec<JobDetails>> {
        let mut results = Vec::new();

        for job_id in job_ids {
            match self.verify_job(job_id, chain_id).await {
                Ok(details) => results.push(details),
                Err(e) => {
                    warn!("Failed to verify job {}: {}", job_id, e);
                    // Continue with other jobs
                }
            }
        }
        
        Ok(results)
    }
    
    /// Clone for concurrent use
    pub fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            web3_client: self.web3_client.clone(),
            cache: self.cache.clone(),
        }
    }
    
    // Private helper methods
    
    async fn get_cached_job(&self, job_id: u64) -> Option<JobDetails> {
        let cache = self.cache.read().await;
        
        if let Some(entry) = cache.get(&job_id) {
            if entry.timestamp.elapsed() < self.config.cache_duration {
                return Some(entry.details.clone());
            }
        }
        
        None
    }
    
    async fn cache_job(&self, job_id: u64, details: JobDetails) {
        let mut cache = self.cache.write().await;
        
        cache.insert(job_id, CacheEntry {
            details,
            timestamp: Instant::now(),
        });
        
        // Clean old entries
        cache.retain(|_, entry| {
            entry.timestamp.elapsed() < self.config.cache_duration * 2
        });
    }
    
    async fn fetch_job_from_blockchain(
        &self,
        client: &Web3Client,
        job_id: u64,
    ) -> Result<Job> {
        // This would call the actual marketplace contract
        // For now, we'll create a mock job based on the client
        
        // In real implementation:
        // let marketplace = client.get_marketplace_contract(self.config.marketplace_address)?;
        // let job = marketplace.get_job(job_id).await?;
        
        // Mock implementation for testing
        Ok(Job {
            id: U256::from(job_id),
            client: Address::from_low_u64_be(12345),
            max_price_per_token: U256::from(1000000000000000u64), // 0.001 ETH
            model_id: "tinyllama-1.1b".to_string(),
            input_url: format!("https://s5.garden/input/{}", job_id),
            output_url: "".to_string(),
            state: JobState::Open,
            selected_host: Address::zero(),
            result_commitment: [0u8; 32],
            created_at: U256::from(chrono::Utc::now().timestamp() - 3600),
            deadline: U256::from(chrono::Utc::now().timestamp() + 3600),
            max_tokens: U256::from(1000),
            chain_id: 84532, // Default to Base Sepolia for mock
        })
    }
    
    fn convert_job_to_details(&self, job_id: u64, chain_id: u64, job: Job) -> Result<JobDetails> {
        Ok(JobDetails {
            job_id,
            chain_id,
            client_address: format!("{:?}", job.client),
            payment_amount: job.max_price_per_token.as_u128() * job.max_tokens.as_u128(),
            model_id: job.model_id,
            input_url: job.input_url,
            output_url: if job.output_url.is_empty() {
                None
            } else {
                Some(job.output_url)
            },
            status: JobStatus::from(job.state),
            created_at: job.created_at.as_u64(),
            deadline: job.deadline.as_u64(),
        })
    }
    
    fn create_mock_job(&self, job_id: u64, chain_id: u64) -> JobDetails {
        JobDetails {
            job_id,
            chain_id,
            client_address: "0x1234567890123456789012345678901234567890".to_string(),
            payment_amount: 1000000000000000000u128, // 1 ETH
            model_id: "tinyllama-1.1b".to_string(),
            input_url: format!("https://s5.garden/input/{}", job_id),
            output_url: None,
            status: JobStatus::Pending,
            created_at: chrono::Utc::now().timestamp() as u64 - 3600,
            deadline: chrono::Utc::now().timestamp() as u64 + 3600,
        }
    }
}

/// Blockchain verifier helper
pub struct BlockchainVerifier;

impl BlockchainVerifier {
    pub async fn verify_job_exists(
        client: &Web3Client,
        marketplace_address: &str,
        job_id: u64,
    ) -> Result<bool> {
        // This would check if job exists on blockchain
        debug!("Verifying job {} exists at {}", job_id, marketplace_address);
        
        // Mock for now
        Ok(true)
    }
    
    pub async fn verify_payment(
        client: &Web3Client,
        escrow_address: &str,
        job_id: u64,
        expected_amount: u128,
    ) -> Result<bool> {
        // This would verify payment in escrow contract
        debug!("Verifying payment for job {}: {} wei", job_id, expected_amount);
        
        // Mock for now
        Ok(expected_amount > 0)
    }
}