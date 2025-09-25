use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use ethers::types::{Address, TransactionReceipt, H256, U256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawalRequest {
    pub request_id: H256,
    pub amount: U256,
    pub token: Address,
    pub destination: Address,
    pub requested_at: DateTime<Utc>,
    pub status: WithdrawalStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WithdrawalStatus {
    Pending,
    Processing,
    Completed(H256),
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct WithdrawalConfig {
    pub minimum_withdrawal: U256,
    pub withdrawal_fee: U256,
    pub batch_size: usize,
    pub cooldown_period_secs: u64,
    pub max_pending_withdrawals: usize,
}

impl Default for WithdrawalConfig {
    fn default() -> Self {
        Self {
            minimum_withdrawal: U256::from(10_000_000_000_000_000u64), // 0.01 ETH
            withdrawal_fee: U256::from(1_000_000_000_000_000u64),      // 0.001 ETH
            batch_size: 10,
            cooldown_period_secs: 3600, // 1 hour
            max_pending_withdrawals: 5,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WithdrawalStats {
    pub total_withdrawn: U256,
    pub total_fees_paid: U256,
    pub successful_withdrawals: u64,
    pub failed_withdrawals: u64,
    pub average_withdrawal_amount: U256,
    pub last_withdrawal: Option<DateTime<Utc>>,
}

pub struct WithdrawalManager {
    config: WithdrawalConfig,
    contract_client: Arc<dyn ContractClient>,
    pending_withdrawals: Arc<RwLock<Vec<WithdrawalRequest>>>,
    withdrawal_history: Arc<RwLock<Vec<WithdrawalRequest>>>,
    available_balance: Arc<RwLock<HashMap<Address, U256>>>,
    last_withdrawal_time: Arc<Mutex<Option<DateTime<Utc>>>>,
}

#[async_trait::async_trait]
pub trait ContractClient: Send + Sync {
    async fn get_available_balance(&self, node: Address, token: Address) -> Result<U256>;

    async fn request_withdrawal(
        &self,
        amount: U256,
        token: Address,
        destination: Address,
    ) -> Result<H256>;

    async fn execute_withdrawal(&self, request_id: H256) -> Result<TransactionReceipt>;

    async fn batch_withdraw(&self, requests: Vec<H256>) -> Result<Vec<TransactionReceipt>>;
}

impl WithdrawalManager {
    pub fn new(config: WithdrawalConfig, contract_client: Arc<dyn ContractClient>) -> Self {
        Self {
            config,
            contract_client,
            pending_withdrawals: Arc::new(RwLock::new(Vec::new())),
            withdrawal_history: Arc::new(RwLock::new(Vec::new())),
            available_balance: Arc::new(RwLock::new(HashMap::new())),
            last_withdrawal_time: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn request_withdrawal(
        &self,
        amount: U256,
        token: Address,
        destination: Address,
    ) -> Result<WithdrawalRequest> {
        // Validate minimum amount
        if amount < self.config.minimum_withdrawal {
            anyhow::bail!("Amount below minimum withdrawal threshold");
        }

        // Check cooldown period
        if !self.check_cooldown().await? {
            anyhow::bail!("Cooldown period not met");
        }

        // Verify available balance
        let available = self
            .available_balance
            .read()
            .await
            .get(&token)
            .cloned()
            .unwrap_or_default();

        if available < amount {
            anyhow::bail!("Insufficient balance");
        }

        // Check max pending withdrawals
        let pending_count = self.pending_withdrawals.read().await.len();
        if pending_count >= self.config.max_pending_withdrawals {
            anyhow::bail!("Too many pending withdrawals");
        }

        // Create withdrawal request
        let request_id = self
            .contract_client
            .request_withdrawal(amount, token, destination)
            .await?;
        let request = WithdrawalRequest {
            request_id,
            amount,
            token,
            destination,
            requested_at: Utc::now(),
            status: WithdrawalStatus::Pending,
        };

        // Add to pending queue
        self.pending_withdrawals.write().await.push(request.clone());

        Ok(request)
    }

    pub async fn execute_withdrawal(&self, request_id: H256) -> Result<H256> {
        let mut pending = self.pending_withdrawals.write().await;
        let index = pending
            .iter()
            .position(|r| r.request_id == request_id)
            .ok_or_else(|| anyhow::anyhow!("Request not found"))?;

        let mut request = pending[index].clone();

        // Check if already cancelled
        if request.status == WithdrawalStatus::Cancelled {
            anyhow::bail!("Withdrawal already cancelled");
        }

        // Update status to processing
        request.status = WithdrawalStatus::Processing;
        pending[index] = request.clone();
        drop(pending);

        // Execute withdrawal
        match self.contract_client.execute_withdrawal(request_id).await {
            Ok(receipt) => {
                let tx_hash = receipt.transaction_hash;
                request.status = WithdrawalStatus::Completed(tx_hash);

                // Update last withdrawal time
                *self.last_withdrawal_time.lock().await = Some(Utc::now());

                // Remove from pending and add to history
                let mut pending = self.pending_withdrawals.write().await;
                pending.retain(|r| r.request_id != request_id);
                self.withdrawal_history.write().await.push(request);

                Ok(tx_hash)
            }
            Err(e) => {
                request.status = WithdrawalStatus::Failed(e.to_string());
                let mut pending = self.pending_withdrawals.write().await;
                if let Some(idx) = pending.iter().position(|r| r.request_id == request_id) {
                    pending[idx] = request.clone();
                }
                self.withdrawal_history.write().await.push(request);
                Err(e)
            }
        }
    }

    pub async fn process_batch_withdrawals(&self) -> Result<Vec<H256>> {
        let pending = self.pending_withdrawals.read().await;
        let batch: Vec<_> = pending
            .iter()
            .filter(|r| matches!(r.status, WithdrawalStatus::Pending))
            .take(self.config.batch_size)
            .map(|r| r.request_id)
            .collect();
        drop(pending);

        let mut tx_hashes = Vec::new();

        for request_id in batch {
            match self.execute_withdrawal(request_id).await {
                Ok(tx_hash) => tx_hashes.push(tx_hash),
                Err(_) => continue,
            }
        }

        Ok(tx_hashes)
    }

    pub async fn update_available_balance(&self, token: Address, amount: U256) -> Result<()> {
        self.available_balance.write().await.insert(token, amount);
        Ok(())
    }

    pub async fn get_withdrawal_stats(&self) -> Result<WithdrawalStats> {
        let history = self.withdrawal_history.read().await;

        let successful: Vec<_> = history
            .iter()
            .filter(|r| matches!(r.status, WithdrawalStatus::Completed(_)))
            .collect();

        let failed_count = history
            .iter()
            .filter(|r| matches!(r.status, WithdrawalStatus::Failed(_)))
            .count() as u64;

        let total_withdrawn = successful
            .iter()
            .map(|r| r.amount)
            .fold(U256::zero(), |acc, amt| acc + amt);

        let total_fees_paid = successful
            .iter()
            .map(|_| self.config.withdrawal_fee)
            .fold(U256::zero(), |acc, fee| acc + fee);

        let successful_count = successful.len() as u64;
        let average_withdrawal_amount = if successful_count > 0 {
            total_withdrawn / U256::from(successful_count)
        } else {
            U256::zero()
        };

        let last_withdrawal = successful.iter().map(|r| r.requested_at).max();

        Ok(WithdrawalStats {
            total_withdrawn,
            total_fees_paid,
            successful_withdrawals: successful_count,
            failed_withdrawals: failed_count,
            average_withdrawal_amount,
            last_withdrawal,
        })
    }

    pub async fn cancel_withdrawal(&self, request_id: H256) -> Result<()> {
        let mut pending = self.pending_withdrawals.write().await;

        if let Some(request) = pending.iter_mut().find(|r| r.request_id == request_id) {
            if !matches!(request.status, WithdrawalStatus::Pending) {
                anyhow::bail!("Can only cancel pending withdrawals");
            }
            request.status = WithdrawalStatus::Cancelled;
            let cancelled = request.clone();
            pending.retain(|r| r.request_id != request_id);
            self.withdrawal_history.write().await.push(cancelled);
            Ok(())
        } else {
            anyhow::bail!("Withdrawal request not found");
        }
    }

    pub async fn get_pending_withdrawals(&self) -> Result<Vec<WithdrawalRequest>> {
        Ok(self.pending_withdrawals.read().await.clone())
    }

    async fn check_cooldown(&self) -> Result<bool> {
        let last_time = self.last_withdrawal_time.lock().await;

        match *last_time {
            None => Ok(true),
            Some(last) => {
                let elapsed = Utc::now().signed_duration_since(last);
                Ok(elapsed >= Duration::seconds(self.config.cooldown_period_secs as i64))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockContractClient {
        balances: Arc<RwLock<HashMap<(Address, Address), U256>>>,
        executed_withdrawals: Arc<RwLock<Vec<H256>>>,
    }

    impl MockContractClient {
        fn new() -> Self {
            Self {
                balances: Arc::new(RwLock::new(HashMap::new())),
                executed_withdrawals: Arc::new(RwLock::new(Vec::new())),
            }
        }
    }

    #[async_trait::async_trait]
    impl ContractClient for MockContractClient {
        async fn get_available_balance(&self, node: Address, token: Address) -> Result<U256> {
            Ok(self
                .balances
                .read()
                .await
                .get(&(node, token))
                .cloned()
                .unwrap_or_default())
        }

        async fn request_withdrawal(
            &self,
            _amount: U256,
            _token: Address,
            _destination: Address,
        ) -> Result<H256> {
            Ok(H256::random())
        }

        async fn execute_withdrawal(&self, request_id: H256) -> Result<TransactionReceipt> {
            self.executed_withdrawals.write().await.push(request_id);
            Ok(TransactionReceipt {
                transaction_hash: H256::random(),
                block_number: Some(100u64.into()),
                ..Default::default()
            })
        }

        async fn batch_withdraw(&self, requests: Vec<H256>) -> Result<Vec<TransactionReceipt>> {
            let mut receipts = vec![];
            for request in requests {
                receipts.push(self.execute_withdrawal(request).await?);
            }
            Ok(receipts)
        }
    }

    #[tokio::test]
    async fn test_withdrawal_manager_creation() {
        let client = Arc::new(MockContractClient::new());
        let manager = WithdrawalManager::new(WithdrawalConfig::default(), client);

        assert_eq!(manager.config.batch_size, 10);
        assert_eq!(manager.config.cooldown_period_secs, 3600);
    }
}
