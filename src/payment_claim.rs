use ethers::prelude::*;
use ethers::types::{Address, H256, U256};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::{RwLock, mpsc, Semaphore};
use tokio::time::{sleep, Duration};
use anyhow::{Result, anyhow};
use serde::{Serialize, Deserialize};
use tracing::{info, warn, error, debug};

use crate::contracts::Web3Client;
use crate::job_processor::NodeConfig;

#[derive(Debug, Clone)]
pub enum PaymentError {
    JobNotPayable,
    NoEscrowBalance,
    BelowMinimumThreshold,
    WithdrawalFailed,
    InsufficientBalance,
    ContractError(String),
    Other(String),
}

impl std::fmt::Display for PaymentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PaymentError::JobNotPayable => write!(f, "Job not payable"),
            PaymentError::NoEscrowBalance => write!(f, "No escrow balance"),
            PaymentError::BelowMinimumThreshold => write!(f, "Below minimum threshold"),
            PaymentError::WithdrawalFailed => write!(f, "Withdrawal failed"),
            PaymentError::InsufficientBalance => write!(f, "Insufficient balance"),
            PaymentError::ContractError(e) => write!(f, "Contract error: {}", e),
            PaymentError::Other(e) => write!(f, "Other error: {}", e),
        }
    }
}

impl std::error::Error for PaymentError {}

impl From<anyhow::Error> for PaymentError {
    fn from(err: anyhow::Error) -> Self {
        PaymentError::Other(err.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentStatus {
    Pending,
    Claimed,
    Failed,
}

#[derive(Debug, Clone)]
pub struct PaymentEvent {
    pub job_id: H256,
    pub node_address: Address,
    pub event_type: String,
    pub amount: U256,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub struct PaymentConfig {
    pub node_address: Address,
    pub batch_claim_size: usize,
    pub accept_fab_payments: bool,
    pub max_gas_price: U256,
    pub min_claim_amount: U256,
    pub enable_payment_accumulation: bool,
    pub accumulation_threshold: U256,
    pub payment_retry_attempts: usize,
    pub payment_retry_delay: Duration,
    pub withdrawal_address: Option<Address>,
    pub min_withdrawal_amount: U256,
    pub track_payment_stats: bool,
    pub max_concurrent_claims: usize,
}

impl From<NodeConfig> for PaymentConfig {
    fn from(config: NodeConfig) -> Self {
        Self {
            node_address: config.node_address,
            batch_claim_size: 10,
            accept_fab_payments: true,
            max_gas_price: config.max_gas_price,
            min_claim_amount: config.min_claim_amount,
            enable_payment_accumulation: config.enable_payment_accumulation,
            accumulation_threshold: config.accumulation_threshold,
            payment_retry_attempts: config.payment_retry_attempts,
            payment_retry_delay: config.payment_retry_delay,
            withdrawal_address: Some(config.withdrawal_address),
            min_withdrawal_amount: config.min_withdrawal_amount,
            track_payment_stats: true,
            max_concurrent_claims: config.max_concurrent_jobs,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PaymentSplitter {
    pub host_percentage: u16,      // 8500 = 85%
    pub treasury_percentage: u16,  // 1000 = 10%
    pub stakers_percentage: u16,   // 500 = 5%
}

impl PaymentSplitter {
    pub fn new(host: u16, treasury: u16, stakers: u16) -> Self {
        assert_eq!(host + treasury + stakers, 10000, "Percentages must sum to 10000");
        Self {
            host_percentage: host,
            treasury_percentage: treasury,
            stakers_percentage: stakers,
        }
    }

    pub fn calculate_splits(&self, amount: U256) -> (U256, U256, U256) {
        let host_share = amount * U256::from(self.host_percentage) / U256::from(10000);
        let treasury_share = amount * U256::from(self.treasury_percentage) / U256::from(10000);
        let stakers_share = amount * U256::from(self.stakers_percentage) / U256::from(10000);
        
        (host_share, treasury_share, stakers_share)
    }
}

impl Default for PaymentSplitter {
    fn default() -> Self {
        Self::new(8500, 1000, 500)
    }
}

// Contract interface traits
#[async_trait::async_trait]
pub trait PaymentSystemTrait: Send + Sync {
    async fn is_job_payable(&self, job_id: H256) -> bool;
    async fn get_escrow_balance(&self, job_id: H256) -> Option<U256>;
    async fn claim_payment(&self, job_id: H256, node_address: Address) -> Result<(U256, H256), PaymentError>;
    async fn get_node_balance(&self, node: Address) -> U256;
    async fn estimate_gas(&self, job_id: H256) -> Result<U256>;
    async fn get_gas_price(&self) -> Result<U256>;
    async fn withdraw(&self, node: Address, to: Address, amount: U256) -> Result<H256, PaymentError>;
}

pub struct EscrowManager;

#[derive(Debug, Clone)]
pub struct PaymentStatistics {
    pub total_jobs_paid: u64,
    pub total_earned: U256,
    pub average_payment: U256,
    pub largest_payment: U256,
    pub smallest_payment: U256,
}

#[derive(Clone)]
pub struct PaymentClaimer {
    config: PaymentConfig,
    payment_system: Arc<dyn PaymentSystemTrait>,
    payment_splitter: PaymentSplitter,
    accumulated_jobs: Arc<RwLock<Vec<H256>>>,
    accumulated_amount: Arc<RwLock<U256>>,
    payment_stats: Arc<RwLock<PaymentStatistics>>,
    event_subscribers: Arc<RwLock<Vec<mpsc::Sender<PaymentEvent>>>>,
    claim_semaphore: Arc<Semaphore>,
}

impl PaymentClaimer {
    pub fn new<C: Into<PaymentConfig>>(config: C, payment_system: Arc<dyn PaymentSystemTrait>) -> Self {
        let config = config.into();
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_claims));
        
        Self {
            config,
            payment_system,
            payment_splitter: PaymentSplitter::default(),
            accumulated_jobs: Arc::new(RwLock::new(Vec::new())),
            accumulated_amount: Arc::new(RwLock::new(U256::zero())),
            payment_stats: Arc::new(RwLock::new(PaymentStatistics {
                total_jobs_paid: 0,
                total_earned: U256::zero(),
                average_payment: U256::zero(),
                largest_payment: U256::zero(),
                smallest_payment: U256::max_value(),
            })),
            event_subscribers: Arc::new(RwLock::new(Vec::new())),
            claim_semaphore: semaphore,
        }
    }

    pub async fn claim_payment(&self, job_id: H256) -> Result<(U256, H256), PaymentError> {
        // Check if job is payable
        if !self.payment_system.is_job_payable(job_id).await {
            return Err(PaymentError::JobNotPayable);
        }

        // Get escrow balance
        let escrow_balance = self.payment_system.get_escrow_balance(job_id).await
            .ok_or(PaymentError::NoEscrowBalance)?;

        // Calculate host share
        let (host_share, _, _) = self.payment_splitter.calculate_splits(escrow_balance);

        // Check minimum threshold
        if host_share < self.config.min_claim_amount {
            return Err(PaymentError::BelowMinimumThreshold);
        }

        // Check if profitable after gas
        if !self.is_claim_profitable_internal(job_id, host_share).await? {
            return Err(PaymentError::Other("Not profitable after gas costs".to_string()));
        }

        // Claim payment
        let (amount_received, tx_hash) = self.payment_system
            .claim_payment(job_id, self.config.node_address).await?;

        // Update statistics
        if self.config.track_payment_stats {
            self.update_statistics(amount_received).await;
        }

        // Emit event
        self.emit_event(PaymentEvent {
            job_id,
            node_address: self.config.node_address,
            event_type: "PaymentClaimed".to_string(),
            amount: amount_received,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }).await;

        Ok((amount_received, tx_hash))
    }

    pub async fn claim_batch(&self, job_ids: &[H256]) -> Vec<Result<(U256, H256), PaymentError>> {
        let mut handles = Vec::new();

        for &job_id in job_ids.iter().take(self.config.batch_claim_size) {
            let claimer = self.clone();
            let permit = self.claim_semaphore.clone().acquire_owned().await.unwrap();
            
            let handle = tokio::spawn(async move {
                let res = claimer.claim_payment(job_id).await;
                drop(permit);
                res
            });
            
            handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(res) => results.push(res),
                Err(e) => results.push(Err(PaymentError::Other(e.to_string()))),
            }
        }

        results
    }

    pub async fn claim_with_retry(&self, job_id: H256) -> Result<(U256, H256), PaymentError> {
        let mut attempts = 0;
        let mut last_error = None;

        while attempts < self.config.payment_retry_attempts {
            match self.claim_payment(job_id).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error = Some(e.clone());
                    
                    // Don't retry on certain errors
                    match &e {
                        PaymentError::JobNotPayable |
                        PaymentError::NoEscrowBalance |
                        PaymentError::BelowMinimumThreshold => return Err(e),
                        _ => {}
                    }

                    attempts += 1;
                    if attempts < self.config.payment_retry_attempts {
                        sleep(self.config.payment_retry_delay).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or(PaymentError::Other("Unknown error".to_string())))
    }

    pub async fn estimate_claim_gas(&self, job_id: H256) -> Result<U256> {
        self.payment_system.estimate_gas(job_id).await
    }

    pub async fn is_claim_profitable(&self, job_id: H256) -> Result<bool> {
        let escrow_balance = self.payment_system.get_escrow_balance(job_id).await
            .ok_or_else(|| anyhow!("No escrow balance"))?;
        
        let (host_share, _, _) = self.payment_splitter.calculate_splits(escrow_balance);
        self.is_claim_profitable_internal(job_id, host_share).await
    }

    async fn is_claim_profitable_internal(&self, job_id: H256, payment: U256) -> Result<bool> {
        let gas_estimate = self.estimate_claim_gas(job_id).await?;
        let gas_price = self.payment_system.get_gas_price().await?;
        
        if gas_price > self.config.max_gas_price {
            return Ok(false);
        }
        
        let gas_cost = gas_estimate * gas_price;
        Ok(payment > gas_cost)
    }

    pub async fn add_to_accumulator(&self, job_id: H256) {
        if !self.config.enable_payment_accumulation {
            return;
        }

        if let Some(balance) = self.payment_system.get_escrow_balance(job_id).await {
            let (host_share, _, _) = self.payment_splitter.calculate_splits(balance);
            
            self.accumulated_jobs.write().await.push(job_id);
            *self.accumulated_amount.write().await += host_share;
        }
    }

    pub async fn get_accumulated_amount(&self) -> U256 {
        *self.accumulated_amount.read().await
    }

    pub async fn claim_accumulated(&self) -> Result<(U256, H256), PaymentError> {
        let jobs = self.accumulated_jobs.read().await.clone();
        if jobs.is_empty() {
            return Err(PaymentError::Other("No accumulated payments".to_string()));
        }

        let accumulated = *self.accumulated_amount.read().await;
        if accumulated < self.config.accumulation_threshold {
            return Err(PaymentError::BelowMinimumThreshold);
        }

        // Claim all accumulated jobs
        let mut total_claimed = U256::zero();
        let mut last_tx_hash = H256::zero();

        for job_id in jobs {
            match self.claim_payment(job_id).await {
                Ok((amount, tx_hash)) => {
                    total_claimed += amount;
                    last_tx_hash = tx_hash;
                }
                Err(e) => warn!("Failed to claim payment for job {}: {}", job_id, e),
            }
        }

        // Clear accumulator
        self.accumulated_jobs.write().await.clear();
        *self.accumulated_amount.write().await = U256::zero();

        Ok((total_claimed, last_tx_hash))
    }

    pub async fn get_withdrawable_balance(&self) -> U256 {
        self.payment_system.get_node_balance(self.config.node_address).await
    }

    pub async fn withdraw_earnings(&self) -> Result<(U256, H256), PaymentError> {
        let balance = self.get_withdrawable_balance().await;
        
        if balance < self.config.min_withdrawal_amount {
            return Err(PaymentError::BelowMinimumThreshold);
        }

        let to_address = self.config.withdrawal_address
            .unwrap_or(self.config.node_address);

        let tx_hash = self.payment_system
            .withdraw(self.config.node_address, to_address, balance).await?;

        Ok((balance, tx_hash))
    }

    pub async fn get_payment_statistics(&self) -> PaymentStatistics {
        self.payment_stats.read().await.clone()
    }

    async fn update_statistics(&self, amount: U256) {
        let mut stats = self.payment_stats.write().await;
        
        stats.total_jobs_paid += 1;
        stats.total_earned += amount;
        stats.average_payment = stats.total_earned / U256::from(stats.total_jobs_paid);
        
        if amount > stats.largest_payment {
            stats.largest_payment = amount;
        }
        if amount < stats.smallest_payment {
            stats.smallest_payment = amount;
        }
    }

    pub async fn subscribe_to_events(&self) -> mpsc::Receiver<PaymentEvent> {
        let (tx, rx) = mpsc::channel(100);
        self.event_subscribers.write().await.push(tx);
        rx
    }

    async fn emit_event(&self, event: PaymentEvent) {
        let subscribers = self.event_subscribers.read().await;
        for subscriber in subscribers.iter() {
            let _ = subscriber.send(event.clone()).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockPaymentSystem {
        escrow_balances: Arc<RwLock<HashMap<H256, U256>>>,
        completed_jobs: Arc<RwLock<Vec<H256>>>,
        paid_jobs: Arc<RwLock<Vec<H256>>>,
        node_balances: Arc<RwLock<HashMap<Address, U256>>>,
    }

    #[async_trait::async_trait]
    impl PaymentSystemTrait for MockPaymentSystem {
        async fn is_job_payable(&self, job_id: H256) -> bool {
            let is_completed = self.completed_jobs.read().await.contains(&job_id);
            let is_not_paid = !self.paid_jobs.read().await.contains(&job_id);
            let has_balance = self.escrow_balances.read().await.contains_key(&job_id);
            
            is_completed && is_not_paid && has_balance
        }

        async fn get_escrow_balance(&self, job_id: H256) -> Option<U256> {
            self.escrow_balances.read().await.get(&job_id).copied()
        }

        async fn claim_payment(&self, job_id: H256, node_address: Address) -> Result<(U256, H256), PaymentError> {
            let amount = self.get_escrow_balance(job_id).await
                .ok_or(PaymentError::NoEscrowBalance)?;
            
            let splitter = PaymentSplitter::default();
            let (host_share, _, _) = splitter.calculate_splits(amount);
            
            // Update balances
            let mut balances = self.node_balances.write().await;
            let current = balances.get(&node_address).copied().unwrap_or_default();
            balances.insert(node_address, current + host_share);
            
            self.paid_jobs.write().await.push(job_id);
            
            Ok((host_share, H256::random()))
        }

        async fn get_node_balance(&self, node: Address) -> U256 {
            self.node_balances.read().await
                .get(&node)
                .copied()
                .unwrap_or_default()
        }

        async fn estimate_gas(&self, _job_id: H256) -> Result<U256> {
            Ok(U256::from(100_000))
        }

        async fn get_gas_price(&self) -> Result<U256> {
            Ok(U256::from(20_000_000_000u64)) // 20 gwei
        }

        async fn withdraw(&self, from: Address, _to: Address, amount: U256) -> Result<H256, PaymentError> {
            let mut balances = self.node_balances.write().await;
            let current = balances.get(&from).copied().unwrap_or_default();
            
            if current < amount {
                return Err(PaymentError::WithdrawalFailed);
            }
            
            balances.insert(from, current - amount);
            Ok(H256::random())
        }
    }
}