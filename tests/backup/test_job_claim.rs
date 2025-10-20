// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// tests/test_job_claim.rs

use ethers::prelude::*;
use ethers::types::{Address, H256, U256};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};

use fabstir_llm_node::{
    JobClaimer,
    JobRequest,
    JobStatus,
    JobNodeConfig,
    ClaimError,
    ClaimResult,
    ClaimMarketplaceTrait,
};

#[derive(Debug, Clone)]
struct MockJobMarketplace {
    jobs: Arc<RwLock<Vec<JobRequest>>>,
    claimed_jobs: Arc<RwLock<Vec<(H256, Address)>>>,
    node_registry: Arc<RwLock<Vec<Address>>>,
}

impl MockJobMarketplace {
    fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(Vec::new())),
            claimed_jobs: Arc::new(RwLock::new(Vec::new())),
            node_registry: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    async fn add_job(&self, job: JobRequest) {
        self.jobs.write().await.push(job);
    }
    
    async fn register_node(&self, node_address: Address) {
        self.node_registry.write().await.push(node_address);
    }
    
    async fn is_node_registered(&self, node_address: Address) -> bool {
        self.node_registry.read().await.contains(&node_address)
    }
    
    async fn is_job_claimed(&self, job_id: H256) -> bool {
        self.claimed_jobs.read().await.iter().any(|(id, _)| *id == job_id)
    }
    
    async fn claim_job(&self, job_id: H256, node_address: Address) -> Result<(), ClaimError> {
        // Check if node is registered
        if !self.is_node_registered(node_address).await {
            return Err(ClaimError::NodeNotRegistered);
        }
        
        // Check if job exists
        let job_exists = self.jobs.read().await.iter().any(|j| j.job_id == job_id);
        if !job_exists {
            return Err(ClaimError::JobNotFound);
        }
        
        // Check if already claimed
        if self.is_job_claimed(job_id).await {
            return Err(ClaimError::JobAlreadyClaimed);
        }
        
        // Claim the job
        self.claimed_jobs.write().await.push((job_id, node_address));
        Ok(())
    }
    
    async fn unclaim_job(&self, job_id: H256) -> Result<(), ClaimError> {
        self.claimed_jobs.write().await.retain(|(id, _)| *id != job_id);
        Ok(())
    }
    
    async fn get_job(&self, job_id: H256) -> Option<JobRequest> {
        self.jobs.read().await
            .iter()
            .find(|job| job.job_id == job_id)
            .cloned()
    }
    
    async fn get_all_jobs(&self) -> Vec<JobRequest> {
        self.jobs.read().await.clone()
    }
    
    async fn estimate_gas(&self, _job_id: H256) -> Result<U256, anyhow::Error> {
        Ok(U256::from(100_000))
    }
    
    async fn get_gas_price(&self) -> Result<U256, anyhow::Error> {
        Ok(U256::from(20_000_000_000u64))
    }
}

#[async_trait::async_trait]
impl ClaimMarketplaceTrait for MockJobMarketplace {
    async fn is_node_registered(&self, node_address: Address) -> bool {
        self.is_node_registered(node_address).await
    }
    
    async fn is_job_claimed(&self, job_id: H256) -> bool {
        self.is_job_claimed(job_id).await
    }
    
    async fn claim_job(&self, job_id: H256, node_address: Address) -> Result<(), ClaimError> {
        self.claim_job(job_id, node_address).await
    }
    
    async fn unclaim_job(&self, job_id: H256) -> Result<(), ClaimError> {
        self.unclaim_job(job_id).await
    }
    
    async fn get_job(&self, job_id: H256) -> Option<JobRequest> {
        self.get_job(job_id).await
    }
    
    async fn get_all_jobs(&self) -> Vec<JobRequest> {
        self.get_all_jobs().await
    }
    
    async fn estimate_gas(&self, job_id: H256) -> Result<U256, anyhow::Error> {
        self.estimate_gas(job_id).await
    }
    
    async fn get_gas_price(&self) -> Result<U256, anyhow::Error> {
        self.get_gas_price().await
    }
}

#[tokio::test]
async fn test_job_claim_success() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    // Register node
    marketplace.register_node(node_address).await;
    
    // Add job
    let job = JobRequest {
        job_id,
        requester: Address::random(),
        model_id: "llama3-70b".to_string(),
        max_tokens: 1000,
        payment_amount: U256::from(1_000_000_000_000_000_000u64),
        ..Default::default()
    };
    marketplace.add_job(job.clone()).await;
    
    // Create claimer
    let config = JobNodeConfig {
        node_address,
        ..Default::default()
    };
    let claimer = JobClaimer::new(config, marketplace.clone());
    
    // Claim job
    let result = claimer.claim_job(job_id).await;
    assert!(result.is_ok());
    
    // Verify job is claimed
    assert!(marketplace.is_job_claimed(job_id).await);
}

#[tokio::test]
async fn test_job_claim_not_registered() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    // Add job but don't register node
    let job = JobRequest {
        job_id,
        max_tokens: 100,
        payment_amount: U256::from(1000),
        ..Default::default()
    };
    marketplace.add_job(job).await;
    
    let config = JobNodeConfig {
        node_address,
        ..Default::default()
    };
    let claimer = JobClaimer::new(config, marketplace.clone());
    
    // Should fail - node not registered
    let result = claimer.claim_job(job_id).await;
    assert!(matches!(result, Err(ClaimError::NodeNotRegistered)));
}

#[tokio::test]
async fn test_job_claim_already_claimed() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let node1 = Address::random();
    let node2 = Address::random();
    let job_id = H256::random();
    
    // Register both nodes
    marketplace.register_node(node1).await;
    marketplace.register_node(node2).await;
    
    // Add job
    let job = JobRequest {
        job_id,
        max_tokens: 100,
        payment_amount: U256::from(1_000_000_000_000_000_000u64), // 1 ETH
        ..Default::default()
    };
    marketplace.add_job(job).await;
    
    // First node claims
    let config1 = JobNodeConfig {
        node_address: node1,
        ..Default::default()
    };
    let claimer1 = JobClaimer::new(config1, marketplace.clone());
    let result = claimer1.claim_job(job_id).await;
    assert!(result.is_ok());
    
    // Second node tries to claim
    let config2 = JobNodeConfig {
        node_address: node2,
        ..Default::default()
    };
    let claimer2 = JobClaimer::new(config2, marketplace.clone());
    let result = claimer2.claim_job(job_id).await;
    assert!(matches!(result, Err(ClaimError::JobAlreadyClaimed)));
}

#[tokio::test]
async fn test_batch_job_claiming() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let node_address = Address::random();
    
    marketplace.register_node(node_address).await;
    
    // Add multiple jobs
    let job_ids: Vec<H256> = (0..5).map(|_| H256::random()).collect();
    for job_id in &job_ids {
        let job = JobRequest {
            job_id: *job_id,
            max_tokens: 100,
            payment_amount: U256::from(10_000_000_000_000_000u64), // 0.01 ETH
            ..Default::default()
        };
        marketplace.add_job(job).await;
    }
    
    let config = JobNodeConfig {
        node_address,
        max_concurrent_jobs: 3,
        min_payment_per_token: U256::from(100_000_000_000_000u64), // 0.0001 ETH per token
        ..Default::default()
    };
    let claimer = JobClaimer::new(config, marketplace.clone());
    
    // Try to claim all jobs
    let results = claimer.claim_batch(&job_ids).await;
    
    // Should only claim up to max_concurrent_jobs
    let successful_claims = results.iter().filter(|r| r.is_ok()).count();
    let failed_claims = results.iter().filter(|r| r.is_err()).count();
    println!("Results: {} successful, {} failed out of {} total", successful_claims, failed_claims, results.len());
    
    // Print all errors
    for (i, result) in results.iter().enumerate() {
        if let Err(e) = result {
            println!("Job {} failed: {:?}", i, e);
        }
    }
    
    assert_eq!(successful_claims, 3);
    
    // Verify exactly 3 jobs are claimed
    let mut claimed_count = 0;
    for job_id in &job_ids {
        if marketplace.is_job_claimed(*job_id).await {
            claimed_count += 1;
        }
    }
    assert_eq!(claimed_count, 3);
}

#[tokio::test]
async fn test_job_claim_with_retry() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    marketplace.register_node(node_address).await;
    
    let job = JobRequest {
        job_id,
        max_tokens: 100,
        payment_amount: U256::from(1_000_000_000_000_000_000u64), // 1 ETH
        ..Default::default()
    };
    marketplace.add_job(job).await;
    
    let config = JobNodeConfig {
        node_address,
        claim_retry_attempts: 3,
        claim_retry_delay: Duration::from_millis(100),
        ..Default::default()
    };
    let claimer = JobClaimer::new(config, marketplace.clone());
    
    // Simulate temporary failure then success
    let result = claimer.claim_job_with_retry(job_id).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_job_claim_gas_estimation() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    marketplace.register_node(node_address).await;
    
    let job = JobRequest {
        job_id,
        payment_amount: U256::from(1_000_000_000_000_000_000u64),
        max_tokens: 100,
        ..Default::default()
    };
    marketplace.add_job(job).await;
    
    let config = JobNodeConfig {
        node_address,
        max_gas_price: U256::from(50_000_000_000u64), // 50 gwei
        ..Default::default()
    };
    let claimer = JobClaimer::new(config, marketplace.clone());
    
    // Estimate gas before claiming
    let gas_estimate = claimer.estimate_claim_gas(job_id).await.unwrap();
    assert!(gas_estimate > U256::zero());
    assert!(gas_estimate < U256::from(500_000)); // Should be less than 500k gas
    
    // Verify profitability
    let is_profitable = claimer.is_claim_profitable(job_id).await.unwrap();
    assert!(is_profitable);
}

#[tokio::test]
async fn test_job_claim_filtering() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let node_address = Address::random();
    
    marketplace.register_node(node_address).await;
    
    // Add jobs with different characteristics
    let profitable_job = JobRequest {
        job_id: H256::from_low_u64_be(1),
        payment_amount: U256::from(5_000_000_000_000_000_000u64), // 5 ETH
        model_id: "llama3-70b".to_string(),
        max_tokens: 1000,
        ..Default::default()
    };
    
    let unprofitable_job = JobRequest {
        job_id: H256::from_low_u64_be(2),
        payment_amount: U256::from(1_000_000_000_000_000u64), // 0.001 ETH
        model_id: "llama3-70b".to_string(),
        max_tokens: 10000, // Too many tokens for low payment
        ..Default::default()
    };
    
    let unsupported_model_job = JobRequest {
        job_id: H256::from_low_u64_be(3),
        payment_amount: U256::from(1_000_000_000_000_000_000u64),
        model_id: "gpt-4".to_string(), // Not supported
        max_tokens: 1000,
        ..Default::default()
    };
    
    marketplace.add_job(profitable_job.clone()).await;
    marketplace.add_job(unprofitable_job).await;
    marketplace.add_job(unsupported_model_job).await;
    
    let config = JobNodeConfig {
        node_address,
        supported_models: vec!["llama3-70b".to_string()],
        min_payment_per_token: U256::from(1_000_000_000_000_000u64) / U256::from(1000), // 0.001 ETH per token
        ..Default::default()
    };
    let claimer = JobClaimer::new(config, marketplace.clone());
    
    // Get claimable jobs
    let claimable = claimer.get_claimable_jobs().await;
    
    // Should only include the profitable job
    assert_eq!(claimable.len(), 1);
    assert_eq!(claimable[0].job_id, profitable_job.job_id);
}

#[tokio::test]
async fn test_job_claim_event_emission() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    marketplace.register_node(node_address).await;
    
    let job = JobRequest {
        job_id,
        max_tokens: 100,
        payment_amount: U256::from(1_000_000_000_000_000_000u64), // 1 ETH
        ..Default::default()
    };
    marketplace.add_job(job).await;
    
    let config = JobNodeConfig {
        node_address,
        ..Default::default()
    };
    let claimer = JobClaimer::new(config, marketplace.clone());
    
    // Subscribe to claim events
    let mut event_receiver = claimer.subscribe_to_events().await;
    
    // Claim job
    let claim_task = tokio::spawn(async move {
        claimer.claim_job(job_id).await
    });
    
    // Wait for event
    let event = tokio::time::timeout(
        Duration::from_secs(1),
        event_receiver.recv()
    ).await.unwrap().unwrap();
    
    assert_eq!(event.job_id, job_id);
    assert_eq!(event.node_address, node_address);
    assert_eq!(event.event_type, "JobClaimed");
    
    claim_task.await.unwrap().unwrap();
}

#[tokio::test]
async fn test_job_claim_race_condition() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let job_id = H256::random();
    
    // Register multiple nodes
    let nodes: Vec<Address> = (0..5).map(|_| Address::random()).collect();
    for node in &nodes {
        marketplace.register_node(*node).await;
    }
    
    // Add single job
    let job = JobRequest {
        job_id,
        payment_amount: U256::from(10_000_000_000_000_000_000u64), // High value job
        max_tokens: 100,
        ..Default::default()
    };
    marketplace.add_job(job).await;
    
    // All nodes try to claim simultaneously
    let mut handles = vec![];
    for node in nodes {
        let mp = marketplace.clone();
        let handle = tokio::spawn(async move {
            let config = JobNodeConfig {
                node_address: node,
                ..Default::default()
            };
            let claimer = JobClaimer::new(config, mp);
            claimer.claim_job(job_id).await
        });
        handles.push(handle);
    }
    
    // Wait for all attempts
    let results: Vec<_> = futures::future::join_all(handles).await;
    
    // Exactly one should succeed
    let successful_claims = results.iter()
        .filter(|r| r.as_ref().unwrap().is_ok())
        .count();
    assert_eq!(successful_claims, 1);
    
    // Job should be claimed exactly once
    assert!(marketplace.is_job_claimed(job_id).await);
}

#[tokio::test]
async fn test_job_unclaim_on_timeout() {
    let marketplace = Arc::new(MockJobMarketplace::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    marketplace.register_node(node_address).await;
    
    let job = JobRequest {
        job_id,
        deadline: U256::from(1000), // Short deadline
        max_tokens: 100,
        payment_amount: U256::from(1_000_000_000_000_000_000u64), // 1 ETH
        ..Default::default()
    };
    marketplace.add_job(job).await;
    
    let config = JobNodeConfig {
        node_address,
        job_timeout: Duration::from_secs(60),
        ..Default::default()
    };
    let claimer = JobClaimer::new(config, marketplace.clone());
    
    // Claim job
    claimer.claim_job(job_id).await.unwrap();
    assert!(marketplace.is_job_claimed(job_id).await);
    
    // Simulate timeout
    sleep(Duration::from_secs(2)).await;
    
    // Check if job can be unclaimed
    let can_unclaim = claimer.check_timeout(job_id).await;
    assert!(can_unclaim);
    
    // Unclaim the job
    claimer.unclaim_job(job_id).await.unwrap();
    assert!(!marketplace.is_job_claimed(job_id).await);
}