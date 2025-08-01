use anyhow::Result;
use ethers::types::{Address, H256, U256, TransactionReceipt};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

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
    Completed(H256), // tx hash
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

mod withdrawal_manager {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::{RwLock, Mutex};
    
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
        async fn get_available_balance(
            &self,
            node: Address,
            token: Address,
        ) -> Result<U256>;
        
        async fn request_withdrawal(
            &self,
            amount: U256,
            token: Address,
            destination: Address,
        ) -> Result<H256>;
        
        async fn execute_withdrawal(
            &self,
            request_id: H256,
        ) -> Result<TransactionReceipt>;
        
        async fn batch_withdraw(
            &self,
            requests: Vec<H256>,
        ) -> Result<Vec<TransactionReceipt>>;
    }
    
    impl WithdrawalManager {
        pub fn new(
            config: WithdrawalConfig,
            contract_client: Arc<dyn ContractClient>,
        ) -> Self {
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
            // Implementation should:
            // 1. Validate amount meets minimum
            // 2. Check cooldown period
            // 3. Verify available balance
            // 4. Create withdrawal request
            // 5. Add to pending queue
            unimplemented!()
        }
        
        pub async fn execute_withdrawal(
            &self,
            request_id: H256,
        ) -> Result<H256> {
            // Execute single withdrawal
            unimplemented!()
        }
        
        pub async fn process_batch_withdrawals(&self) -> Result<Vec<H256>> {
            // Process pending withdrawals in batches
            unimplemented!()
        }
        
        pub async fn update_available_balance(
            &self,
            token: Address,
            amount: U256,
        ) -> Result<()> {
            // Update cached balance
            unimplemented!()
        }
        
        pub async fn get_withdrawal_stats(&self) -> Result<WithdrawalStats> {
            // Calculate withdrawal statistics
            unimplemented!()
        }
        
        pub async fn cancel_withdrawal(
            &self,
            request_id: H256,
        ) -> Result<()> {
            // Cancel pending withdrawal
            unimplemented!()
        }
        
        pub async fn get_pending_withdrawals(&self) -> Result<Vec<WithdrawalRequest>> {
            // Get all pending withdrawal requests
            unimplemented!()
        }
        
        async fn check_cooldown(&self) -> Result<bool> {
            // Check if cooldown period has passed
            unimplemented!()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use withdrawal_manager::{WithdrawalManager, ContractClient};
    use std::sync::Arc;
    use tokio::sync::RwLock;
    
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
        
        async fn set_balance(&self, node: Address, token: Address, balance: U256) {
            self.balances.write().await.insert((node, token), balance);
        }
    }
    
    #[async_trait::async_trait]
    impl ContractClient for MockContractClient {
        async fn get_available_balance(
            &self,
            node: Address,
            token: Address,
        ) -> Result<U256> {
            Ok(self.balances.read().await
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
        
        async fn execute_withdrawal(
            &self,
            request_id: H256,
        ) -> Result<TransactionReceipt> {
            self.executed_withdrawals.write().await.push(request_id);
            Ok(TransactionReceipt {
                transaction_hash: H256::random(),
                block_number: Some(100u64.into()),
                ..Default::default()
            })
        }
        
        async fn batch_withdraw(
            &self,
            requests: Vec<H256>,
        ) -> Result<Vec<TransactionReceipt>> {
            let mut receipts = vec![];
            for request in requests {
                receipts.push(self.execute_withdrawal(request).await?);
            }
            Ok(receipts)
        }
    }
    
    #[tokio::test]
    async fn test_request_withdrawal_success() {
        let client = Arc::new(MockContractClient::new());
        let node_address = Address::random();
        let token = Address::zero(); // ETH
        let destination = Address::random();
        
        // Set balance
        client.set_balance(node_address, token, U256::from(1_000_000_000_000_000_000u64)).await;
        
        let manager = WithdrawalManager::new(
            WithdrawalConfig::default(),
            client,
        );
        
        // Update available balance
        manager.update_available_balance(token, U256::from(1_000_000_000_000_000_000u64)).await.unwrap();
        
        // Request withdrawal
        let amount = U256::from(100_000_000_000_000_000u64); // 0.1 ETH
        let request = manager.request_withdrawal(
            amount,
            token,
            destination,
        ).await.unwrap();
        
        assert_eq!(request.amount, amount);
        assert_eq!(request.token, token);
        assert_eq!(request.destination, destination);
        assert_eq!(request.status, WithdrawalStatus::Pending);
    }
    
    #[tokio::test]
    async fn test_minimum_withdrawal_enforcement() {
        let client = Arc::new(MockContractClient::new());
        let manager = WithdrawalManager::new(
            WithdrawalConfig::default(),
            client,
        );
        
        // Try to withdraw less than minimum
        let amount = U256::from(1_000_000_000_000_000u64); // 0.001 ETH (below minimum)
        let result = manager.request_withdrawal(
            amount,
            Address::zero(),
            Address::random(),
        ).await;
        
        assert!(result.is_err());
    }
    
    #[tokio::test]
    async fn test_cooldown_period_enforcement() {
        let client = Arc::new(MockContractClient::new());
        let token = Address::zero();
        
        client.set_balance(Address::random(), token, U256::from(10_000_000_000_000_000_000u64)).await;
        
        let mut config = WithdrawalConfig::default();
        config.cooldown_period_secs = 1; // 1 second for testing
        
        let manager = WithdrawalManager::new(config, client);
        
        // Update balance
        manager.update_available_balance(token, U256::from(10_000_000_000_000_000_000u64)).await.unwrap();
        
        // First withdrawal should succeed
        let request1 = manager.request_withdrawal(
            U256::from(100_000_000_000_000_000u64),
            token,
            Address::random(),
        ).await.unwrap();
        
        // Execute it
        manager.execute_withdrawal(request1.request_id).await.unwrap();
        
        // Immediate second withdrawal should fail (cooldown)
        let result = manager.request_withdrawal(
            U256::from(100_000_000_000_000_000u64),
            token,
            Address::random(),
        ).await;
        
        assert!(result.is_err());
        
        // Wait for cooldown
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        
        // Now should succeed
        let request2 = manager.request_withdrawal(
            U256::from(100_000_000_000_000_000u64),
            token,
            Address::random(),
        ).await;
        
        assert!(request2.is_ok());
    }
    
    #[tokio::test]
    async fn test_batch_withdrawal_processing() {
        let client = Arc::new(MockContractClient::new());
        let token = Address::zero();
        
        let mut config = WithdrawalConfig::default();
        config.batch_size = 3;
        config.cooldown_period_secs = 0; // No cooldown for testing
        
        let manager = WithdrawalManager::new(config, client);
        
        // Update balance
        manager.update_available_balance(token, U256::from(10_000_000_000_000_000_000u64)).await.unwrap();
        
        // Create 5 withdrawal requests
        let mut requests = vec![];
        for _ in 0..5 {
            let request = manager.request_withdrawal(
                U256::from(100_000_000_000_000_000u64),
                token,
                Address::random(),
            ).await.unwrap();
            requests.push(request);
        }
        
        // Process batch (should process 3)
        let tx_hashes = manager.process_batch_withdrawals().await.unwrap();
        
        assert_eq!(tx_hashes.len(), 3);
        
        // Should have 2 pending
        let pending = manager.get_pending_withdrawals().await.unwrap();
        assert_eq!(pending.len(), 2);
    }
    
    #[tokio::test]
    async fn test_withdrawal_cancellation() {
        let client = Arc::new(MockContractClient::new());
        let manager = WithdrawalManager::new(
            WithdrawalConfig::default(),
            client,
        );
        
        // Update balance
        manager.update_available_balance(
            Address::zero(),
            U256::from(1_000_000_000_000_000_000u64)
        ).await.unwrap();
        
        // Create withdrawal request
        let request = manager.request_withdrawal(
            U256::from(100_000_000_000_000_000u64),
            Address::zero(),
            Address::random(),
        ).await.unwrap();
        
        // Cancel it
        manager.cancel_withdrawal(request.request_id).await.unwrap();
        
        // Should not be in pending
        let pending = manager.get_pending_withdrawals().await.unwrap();
        assert_eq!(pending.len(), 0);
        
        // Try to execute cancelled withdrawal
        let result = manager.execute_withdrawal(request.request_id).await;
        assert!(result.is_err());
    }
    
    #[tokio::test]
    async fn test_withdrawal_statistics() {
        let client = Arc::new(MockContractClient::new());
        let manager = WithdrawalManager::new(
            WithdrawalConfig::default(),
            client,
        );
        
        // Update balance
        manager.update_available_balance(
            Address::zero(),
            U256::from(10_000_000_000_000_000_000u64)
        ).await.unwrap();
        
        // Execute multiple withdrawals
        let amounts = vec![
            U256::from(100_000_000_000_000_000u64),
            U256::from(200_000_000_000_000_000u64),
            U256::from(300_000_000_000_000_000u64),
        ];
        
        for amount in &amounts {
            let request = manager.request_withdrawal(
                *amount,
                Address::zero(),
                Address::random(),
            ).await.unwrap();
            
            manager.execute_withdrawal(request.request_id).await.unwrap();
        }
        
        let stats = manager.get_withdrawal_stats().await.unwrap();
        
        assert_eq!(stats.successful_withdrawals, 3);
        assert_eq!(stats.failed_withdrawals, 0);
        assert_eq!(stats.total_withdrawn, U256::from(600_000_000_000_000_000u64));
        assert_eq!(stats.average_withdrawal_amount, U256::from(200_000_000_000_000_000u64));
        assert!(stats.last_withdrawal.is_some());
    }
    
    #[tokio::test]
    async fn test_insufficient_balance_rejection() {
        let client = Arc::new(MockContractClient::new());
        let manager = WithdrawalManager::new(
            WithdrawalConfig::default(),
            client,
        );
        
        // Set low balance
        manager.update_available_balance(
            Address::zero(),
            U256::from(10_000_000_000_000_000u64) // 0.01 ETH
        ).await.unwrap();
        
        // Try to withdraw more than available
        let result = manager.request_withdrawal(
            U256::from(100_000_000_000_000_000u64), // 0.1 ETH
            Address::zero(),
            Address::random(),
        ).await;
        
        assert!(result.is_err());
    }
    
    #[tokio::test]
    async fn test_multi_token_withdrawals() {
        let client = Arc::new(MockContractClient::new());
        let manager = WithdrawalManager::new(
            WithdrawalConfig::default(),
            client,
        );
        
        let eth = Address::zero();
        let usdc = Address::random();
        
        // Set balances for different tokens
        manager.update_available_balance(eth, U256::from(1_000_000_000_000_000_000u64)).await.unwrap();
        manager.update_available_balance(usdc, U256::from(1000_000_000u64)).await.unwrap(); // 1000 USDC
        
        // Withdraw ETH
        let eth_request = manager.request_withdrawal(
            U256::from(100_000_000_000_000_000u64),
            eth,
            Address::random(),
        ).await.unwrap();
        
        // Withdraw USDC
        let usdc_request = manager.request_withdrawal(
            U256::from(100_000_000u64), // 100 USDC
            usdc,
            Address::random(),
        ).await.unwrap();
        
        assert_eq!(eth_request.token, eth);
        assert_eq!(usdc_request.token, usdc);
        
        // Both should be pending
        let pending = manager.get_pending_withdrawals().await.unwrap();
        assert_eq!(pending.len(), 2);
    }
}