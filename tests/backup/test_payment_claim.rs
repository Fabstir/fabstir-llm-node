// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// tests/test_payment_claim.rs

use ethers::prelude::*;
use ethers::types::{Address, H256, U256};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};

use fabstir_llm_node::{
    PaymentClaimer,
    PaymentStatus,
    PaymentError,
    NodeConfig,
    PaymentSplitter,
    EscrowManager,
    PaymentSystemTrait,
};

#[derive(Debug, Clone)]
struct MockPaymentSystem {
    escrow_balances: Arc<RwLock<std::collections::HashMap<H256, U256>>>,
    completed_jobs: Arc<RwLock<Vec<H256>>>,
    paid_jobs: Arc<RwLock<Vec<H256>>>,
    node_balances: Arc<RwLock<std::collections::HashMap<Address, U256>>>,
    payment_splitter: PaymentSplitter,
}

impl MockPaymentSystem {
    fn new() -> Self {
        Self {
            escrow_balances: Arc::new(RwLock::new(std::collections::HashMap::new())),
            completed_jobs: Arc::new(RwLock::new(Vec::new())),
            paid_jobs: Arc::new(RwLock::new(Vec::new())),
            node_balances: Arc::new(RwLock::new(std::collections::HashMap::new())),
            payment_splitter: PaymentSplitter::new(8500, 1000, 500), // 85%, 10%, 5%
        }
    }
    
    async fn add_job_payment(&self, job_id: H256, amount: U256) {
        self.escrow_balances.write().await.insert(job_id, amount);
    }
    
    async fn mark_job_completed(&self, job_id: H256) {
        self.completed_jobs.write().await.push(job_id);
    }
    
    async fn is_job_payable(&self, job_id: H256) -> bool {
        let is_completed = self.completed_jobs.read().await.contains(&job_id);
        let is_not_paid = !self.paid_jobs.read().await.contains(&job_id);
        let has_balance = self.escrow_balances.read().await.contains_key(&job_id);
        
        is_completed && is_not_paid && has_balance
    }
    
    async fn claim_payment(
        &self, 
        job_id: H256, 
        node_address: Address
    ) -> Result<(U256, H256), PaymentError> {
        if !self.is_job_payable(job_id).await {
            return Err(PaymentError::JobNotPayable);
        }
        
        let amount = self.escrow_balances.read().await
            .get(&job_id)
            .copied()
            .ok_or(PaymentError::NoEscrowBalance)?;
        
        // Apply payment split
        let (host_share, treasury_share, stakers_share) = 
            self.payment_splitter.calculate_splits(amount);
        
        // Update node balance (host share)
        let mut balances = self.node_balances.write().await;
        let current = balances.get(&node_address).copied().unwrap_or_default();
        balances.insert(node_address, current + host_share);
        
        // Mark as paid
        self.paid_jobs.write().await.push(job_id);
        
        // Return host share and tx hash
        Ok((host_share, H256::random()))
    }
    
    async fn get_node_balance(&self, node: Address) -> U256 {
        self.node_balances.read().await
            .get(&node)
            .copied()
            .unwrap_or_default()
    }
    
    async fn get_escrow_balance(&self, job_id: H256) -> Option<U256> {
        self.escrow_balances.read().await.get(&job_id).copied()
    }
    
    async fn estimate_gas(&self, _job_id: H256) -> Result<U256, anyhow::Error> {
        Ok(U256::from(100_000))
    }
    
    async fn get_gas_price(&self) -> Result<U256, anyhow::Error> {
        Ok(U256::from(20_000_000_000u64))
    }
    
    async fn withdraw(&self, node: Address, to: Address, amount: U256) -> Result<H256, PaymentError> {
        let mut balances = self.node_balances.write().await;
        let current = balances.get(&node).copied().unwrap_or_default();
        
        if current < amount {
            return Err(PaymentError::InsufficientBalance);
        }
        
        balances.insert(node, current - amount);
        
        // In real implementation, this would transfer to 'to' address
        Ok(H256::random())
    }
}

#[async_trait::async_trait]
impl PaymentSystemTrait for MockPaymentSystem {
    async fn is_job_payable(&self, job_id: H256) -> bool {
        self.is_job_payable(job_id).await
    }
    
    async fn get_escrow_balance(&self, job_id: H256) -> Option<U256> {
        self.get_escrow_balance(job_id).await
    }
    
    async fn claim_payment(&self, job_id: H256, node_address: Address) -> Result<(U256, H256), PaymentError> {
        self.claim_payment(job_id, node_address).await
    }
    
    async fn get_node_balance(&self, node: Address) -> U256 {
        self.get_node_balance(node).await
    }
    
    async fn estimate_gas(&self, job_id: H256) -> Result<U256, anyhow::Error> {
        self.estimate_gas(job_id).await
    }
    
    async fn get_gas_price(&self) -> Result<U256, anyhow::Error> {
        self.get_gas_price().await
    }
    
    async fn withdraw(&self, node: Address, to: Address, amount: U256) -> Result<H256, PaymentError> {
        self.withdraw(node, to, amount).await
    }
}

#[tokio::test]
async fn test_payment_claim_success() {
    let payment_system = Arc::new(MockPaymentSystem::new());
    let node_address = Address::random();
    let job_id = H256::random();
    let payment_amount = U256::from(1_000_000_000_000_000_000u64); // 1 ETH
    
    // Setup job payment
    payment_system.add_job_payment(job_id, payment_amount).await;
    payment_system.mark_job_completed(job_id).await;
    
    let config = NodeConfig {
        node_address,
        ..Default::default()
    };
    
    let claimer = PaymentClaimer::new(config, payment_system.clone());
    
    // Claim payment
    let (amount_received, tx_hash) = claimer.claim_payment(job_id).await.unwrap();
    
    // Verify host received 85%
    let expected_amount = payment_amount * U256::from(8500) / U256::from(10000);
    assert_eq!(amount_received, expected_amount);
    
    // Verify balance updated
    let balance = payment_system.get_node_balance(node_address).await;
    assert_eq!(balance, expected_amount);
    
    // Verify job marked as paid
    assert!(payment_system.paid_jobs.read().await.contains(&job_id));
}

#[tokio::test]
async fn test_payment_claim_job_not_completed() {
    let payment_system = Arc::new(MockPaymentSystem::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    // Add payment but don't mark completed
    payment_system.add_job_payment(job_id, U256::from(1_000_000_000_000_000_000u64)).await;
    
    let config = NodeConfig {
        node_address,
        ..Default::default()
    };
    
    let claimer = PaymentClaimer::new(config, payment_system.clone());
    
    // Should fail
    let result = claimer.claim_payment(job_id).await;
    assert!(matches!(result, Err(PaymentError::JobNotPayable)));
}

#[tokio::test]
async fn test_payment_claim_already_paid() {
    let payment_system = Arc::new(MockPaymentSystem::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    // Setup and claim once
    payment_system.add_job_payment(job_id, U256::from(1_000_000_000_000_000_000u64)).await;
    payment_system.mark_job_completed(job_id).await;
    
    let config = NodeConfig {
        node_address,
        ..Default::default()
    };
    
    let claimer = PaymentClaimer::new(config, payment_system.clone());
    
    // First claim succeeds
    let result1 = claimer.claim_payment(job_id).await;
    assert!(result1.is_ok());
    
    // Second claim fails
    let result2 = claimer.claim_payment(job_id).await;
    assert!(matches!(result2, Err(PaymentError::JobNotPayable)));
}

#[tokio::test]
async fn test_batch_payment_claims() {
    let payment_system = Arc::new(MockPaymentSystem::new());
    let node_address = Address::random();
    
    // Setup multiple completed jobs
    let job_ids: Vec<H256> = (0..5).map(|_| H256::random()).collect();
    let payment_amount = U256::from(500_000_000_000_000_000u64); // 0.5 ETH each
    
    for job_id in &job_ids {
        payment_system.add_job_payment(*job_id, payment_amount).await;
        payment_system.mark_job_completed(*job_id).await;
    }
    
    let config = NodeConfig {
        node_address,
        ..Default::default()
    };
    
    let claimer = PaymentClaimer::new(config, payment_system.clone());
    
    // Claim all in batch
    let results = claimer.claim_batch(&job_ids).await;
    
    // All should succeed
    assert_eq!(results.len(), 5);
    assert!(results.iter().all(|r| r.is_ok()));
    
    // Verify total balance
    let total_expected = payment_amount * U256::from(5) * U256::from(8500) / U256::from(10000);
    let balance = payment_system.get_node_balance(node_address).await;
    assert_eq!(balance, total_expected);
}

#[tokio::test]
async fn test_payment_with_fab_token() {
    let payment_system = Arc::new(MockPaymentSystem::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    // Payment in FAB tokens (with 20% user discount applied)
    let fab_payment = U256::from(800_000_000_000_000_000u64); // 0.8 FAB (after discount)
    
    payment_system.add_job_payment(job_id, fab_payment).await;
    payment_system.mark_job_completed(job_id).await;
    
    let config = NodeConfig {
        node_address,
        ..Default::default()
    };
    
    let claimer = PaymentClaimer::new(config, payment_system.clone());
    
    // Claim FAB payment
    let (amount_received, _) = claimer.claim_payment(job_id).await.unwrap();
    
    // Host still gets 85% of actual payment
    let expected = fab_payment * U256::from(8500) / U256::from(10000);
    assert_eq!(amount_received, expected);
}

#[tokio::test]
async fn test_payment_gas_estimation() {
    let payment_system = Arc::new(MockPaymentSystem::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    payment_system.add_job_payment(job_id, U256::from(1_000_000_000_000_000_000u64)).await;
    payment_system.mark_job_completed(job_id).await;
    
    let config = NodeConfig {
        node_address,
        max_gas_price: U256::from(50_000_000_000u64), // 50 gwei
        ..Default::default()
    };
    
    let claimer = PaymentClaimer::new(config, payment_system.clone());
    
    // Estimate gas for claim
    let gas_estimate = claimer.estimate_claim_gas(job_id).await.unwrap();
    assert!(gas_estimate > U256::zero());
    assert!(gas_estimate < U256::from(200_000)); // Should be less than 200k gas
    
    // Check if profitable to claim
    let is_profitable = claimer.is_claim_profitable(job_id).await.unwrap();
    assert!(is_profitable);
}

#[tokio::test]
async fn test_payment_minimum_threshold() {
    let payment_system = Arc::new(MockPaymentSystem::new());
    let node_address = Address::random();
    
    // Small payment job
    let small_job = H256::random();
    let small_payment = U256::from(1_000_000_000_000_000u64); // 0.001 ETH
    
    // Large payment job  
    let large_job = H256::random();
    let large_payment = U256::from(100_000_000_000_000_000u64); // 0.1 ETH
    
    payment_system.add_job_payment(small_job, small_payment).await;
    payment_system.add_job_payment(large_job, large_payment).await;
    payment_system.mark_job_completed(small_job).await;
    payment_system.mark_job_completed(large_job).await;
    
    let config = NodeConfig {
        node_address,
        min_claim_amount: U256::from(10_000_000_000_000_000u64), // 0.01 ETH minimum
        ..Default::default()
    };
    
    let claimer = PaymentClaimer::new(config, payment_system.clone());
    
    // Small payment should be skipped
    let small_result = claimer.claim_payment(small_job).await;
    assert!(matches!(small_result, Err(PaymentError::BelowMinimumThreshold)));
    
    // Large payment should succeed
    let large_result = claimer.claim_payment(large_job).await;
    assert!(large_result.is_ok());
}

#[tokio::test]
async fn test_accumulated_payments() {
    let payment_system = Arc::new(MockPaymentSystem::new());
    let node_address = Address::random();
    
    // Multiple small payments
    let small_amount = U256::from(5_000_000_000_000_000u64); // 0.005 ETH each
    let job_ids: Vec<H256> = (0..10).map(|_| H256::random()).collect();
    
    for job_id in &job_ids {
        payment_system.add_job_payment(*job_id, small_amount).await;
        payment_system.mark_job_completed(*job_id).await;
    }
    
    let config = NodeConfig {
        node_address,
        min_claim_amount: U256::zero(), // No minimum for accumulated payments test
        enable_payment_accumulation: true,
        accumulation_threshold: U256::from(40_000_000_000_000_000u64), // 0.04 ETH
        ..Default::default()
    };
    
    let claimer = PaymentClaimer::new(config, payment_system.clone());
    
    // Accumulate payments
    for job_id in &job_ids {
        claimer.add_to_accumulator(*job_id).await;
    }
    
    // Check if accumulated amount meets threshold
    let accumulated = claimer.get_accumulated_amount().await;
    let expected_accumulated = small_amount * U256::from(10) * U256::from(8500) / U256::from(10000);
    println!("Accumulated: {}, Expected: {}, Threshold: {}", accumulated, expected_accumulated, U256::from(40_000_000_000_000_000u64));
    assert_eq!(accumulated, expected_accumulated);
    assert!(accumulated >= U256::from(40_000_000_000_000_000u64));
    
    // Claim all accumulated
    let (total_claimed, _tx_hash) = claimer.claim_accumulated().await.unwrap();
    assert_eq!(total_claimed, expected_accumulated);
}

#[tokio::test]
async fn test_payment_retry_mechanism() {
    let payment_system = Arc::new(MockPaymentSystem::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    payment_system.add_job_payment(job_id, U256::from(1_000_000_000_000_000_000u64)).await;
    payment_system.mark_job_completed(job_id).await;
    
    let config = NodeConfig {
        node_address,
        payment_retry_attempts: 3,
        payment_retry_delay: Duration::from_millis(100),
        ..Default::default()
    };
    
    let claimer = PaymentClaimer::new(config, payment_system.clone());
    
    // Claim with retry
    let result = claimer.claim_with_retry(job_id).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_payment_events() {
    let payment_system = Arc::new(MockPaymentSystem::new());
    let node_address = Address::random();
    let job_id = H256::random();
    
    payment_system.add_job_payment(job_id, U256::from(1_000_000_000_000_000_000u64)).await;
    payment_system.mark_job_completed(job_id).await;
    
    let config = NodeConfig {
        node_address,
        ..Default::default()
    };
    
    let claimer = PaymentClaimer::new(config, payment_system.clone());
    
    // Subscribe to payment events
    let mut event_receiver = claimer.subscribe_to_events().await;
    
    // Claim payment
    let claim_task = tokio::spawn(async move {
        claimer.claim_payment(job_id).await
    });
    
    // Wait for event
    let event = tokio::time::timeout(
        Duration::from_secs(1),
        event_receiver.recv()
    ).await.unwrap().unwrap();
    
    assert_eq!(event.job_id, job_id);
    assert_eq!(event.node_address, node_address);
    assert_eq!(event.event_type, "PaymentClaimed");
    assert!(event.amount > U256::zero());
    
    claim_task.await.unwrap().unwrap();
}

#[tokio::test]
async fn test_payment_withdrawal() {
    let payment_system = Arc::new(MockPaymentSystem::new());
    let node_address = Address::random();
    let withdrawal_address = Address::random();
    
    // Claim multiple payments to build balance
    for i in 0..3 {
        let job_id = H256::from_low_u64_be(i);
        payment_system.add_job_payment(job_id, U256::from(1_000_000_000_000_000_000u64)).await;
        payment_system.mark_job_completed(job_id).await;
    }
    
    let config = NodeConfig {
        node_address,
        withdrawal_address,
        min_withdrawal_amount: U256::from(2_000_000_000_000_000_000u64), // 2 ETH
        ..Default::default()
    };
    
    let claimer = PaymentClaimer::new(config, payment_system.clone());
    
    // Claim all payments
    for i in 0..3 {
        claimer.claim_payment(H256::from_low_u64_be(i)).await.unwrap();
    }
    
    // Check withdrawable balance
    let withdrawable = claimer.get_withdrawable_balance().await;
    assert!(withdrawable >= U256::from(2_000_000_000_000_000_000u64));
    
    // Initiate withdrawal
    let (withdrawn_amount, tx_hash) = claimer.withdraw_earnings().await.unwrap();
    assert_eq!(withdrawn_amount, withdrawable);
}

#[tokio::test]
async fn test_payment_statistics() {
    let payment_system = Arc::new(MockPaymentSystem::new());
    let node_address = Address::random();
    
    let config = NodeConfig {
        node_address,
        ..Default::default()
    };
    
    let claimer = PaymentClaimer::new(config, payment_system.clone());
    
    // Process multiple payments
    let amounts = vec![
        U256::from(1_000_000_000_000_000_000u64), // 1 ETH
        U256::from(500_000_000_000_000_000u64),   // 0.5 ETH
        U256::from(2_000_000_000_000_000_000u64), // 2 ETH
    ];
    
    for (i, amount) in amounts.iter().enumerate() {
        let job_id = H256::from_low_u64_be(i as u64);
        payment_system.add_job_payment(job_id, *amount).await;
        payment_system.mark_job_completed(job_id).await;
        claimer.claim_payment(job_id).await.unwrap();
    }
    
    // Get statistics
    let stats = claimer.get_payment_statistics().await;
    
    assert_eq!(stats.total_jobs_paid, 3);
    assert_eq!(stats.total_earned, 
        amounts.iter().fold(U256::zero(), |acc, x| acc + x) * U256::from(8500) / U256::from(10000)
    );
    assert_eq!(stats.average_payment,
        stats.total_earned / U256::from(3)
    );
    assert!(stats.largest_payment > U256::zero());
    assert!(stats.smallest_payment > U256::zero());
}

#[tokio::test]
async fn test_concurrent_payment_claims() {
    let payment_system = Arc::new(MockPaymentSystem::new());
    let node_address = Address::random();
    
    // Setup multiple jobs
    let job_count = 20;
    let job_ids: Vec<H256> = (0..job_count).map(|i| H256::from_low_u64_be(i)).collect();
    
    for job_id in &job_ids {
        payment_system.add_job_payment(*job_id, U256::from(100_000_000_000_000_000u64)).await;
        payment_system.mark_job_completed(*job_id).await;
    }
    
    let config = NodeConfig {
        node_address,
        max_concurrent_jobs: 5,
        ..Default::default()
    };
    
    // Claim all concurrently
    let mut handles = vec![];
    for job_id in job_ids {
        let claimer = PaymentClaimer::new(config.clone(), payment_system.clone());
        let handle = tokio::spawn(async move {
            claimer.claim_payment(job_id).await
        });
        handles.push(handle);
    }
    
    // Wait for all
    let results: Vec<_> = futures::future::join_all(handles).await;
    
    // All should succeed
    assert!(results.iter().all(|r| r.as_ref().unwrap().is_ok()));
    
    // Verify total balance
    let balance = payment_system.get_node_balance(node_address).await;
    let expected_total = U256::from(100_000_000_000_000_000u64) * U256::from(job_count)
        * U256::from(8500) / U256::from(10000);
    assert_eq!(balance, expected_total);
}