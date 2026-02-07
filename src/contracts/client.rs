// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::{anyhow, Result};
use ethers::prelude::*;
use ethers::providers::{Http, Provider};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tracing::info;

use super::types::*;

#[derive(Debug, Clone)]
pub struct Web3Config {
    pub rpc_url: String,
    pub chain_id: u64,
    pub confirmations: usize,
    pub polling_interval: Duration,
    pub private_key: Option<String>,
    pub max_reconnection_attempts: usize,
    pub reconnection_delay: Duration,
}

impl Default for Web3Config {
    fn default() -> Self {
        Self {
            rpc_url: "http://localhost:8545".to_string(),
            chain_id: 31337,
            confirmations: 1,
            polling_interval: Duration::from_millis(100),
            private_key: None,
            max_reconnection_attempts: 3,
            reconnection_delay: Duration::from_millis(100),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChainConfig {
    pub name: String,
    pub chain_id: u64,
    pub rpc_url: String,
}

impl ChainConfig {
    pub fn base_mainnet() -> Self {
        Self {
            name: "Base Mainnet".to_string(),
            chain_id: 8453,
            rpc_url: "https://mainnet.base.org".to_string(),
        }
    }

    pub fn base_sepolia() -> Self {
        Self {
            name: "Base Sepolia".to_string(),
            chain_id: 84532,
            rpc_url: "https://sepolia.base.org".to_string(),
        }
    }
}

pub struct Web3Client {
    pub provider: Arc<Provider<Http>>,
    wallet: Arc<RwLock<Option<SignerMiddleware<Arc<Provider<Http>>, LocalWallet>>>>,
    config: Web3Config,
    contract_addresses: Arc<RwLock<HashMap<String, Address>>>,
    multicall: Arc<RwLock<Option<Multicall3<Provider<Http>>>>>,
    block_stream_sender: Arc<RwLock<Option<mpsc::Sender<Block<H256>>>>>,
}

impl Web3Client {
    pub async fn new(config: Web3Config) -> Result<Self> {
        let provider = Provider::<Http>::try_from(&config.rpc_url)
            .map_err(|e| anyhow!("Failed to create provider: {}", e))?
            .interval(config.polling_interval);

        // Verify connection
        let chain_id = provider
            .get_chainid()
            .await
            .map_err(|e| anyhow!("Failed to connect to RPC: {}", e))?;

        if chain_id.as_u64() != config.chain_id {
            return Err(anyhow!(
                "Chain ID mismatch: expected {}, got {}",
                config.chain_id,
                chain_id
            ));
        }

        let provider = Arc::new(provider);

        let wallet = if let Some(private_key) = &config.private_key {
            let wallet = private_key
                .parse::<LocalWallet>()
                .map_err(|e| anyhow!("Invalid private key: {}", e))?
                .with_chain_id(config.chain_id);

            Some(SignerMiddleware::new(provider.clone(), wallet))
        } else {
            None
        };

        Ok(Self {
            provider: provider.clone(),
            wallet: Arc::new(RwLock::new(wallet)),
            config,
            contract_addresses: Arc::new(RwLock::new(HashMap::new())),
            multicall: Arc::new(RwLock::new(None)),
            block_stream_sender: Arc::new(RwLock::new(None)),
        })
    }

    pub async fn is_connected(&self) -> bool {
        self.provider.get_block_number().await.is_ok()
    }

    pub async fn chain_id(&self) -> Result<u64> {
        let chain_id = self.provider.get_chainid().await?;
        Ok(chain_id.as_u64())
    }

    pub async fn get_block_number(&self) -> Result<u64> {
        let block_number = self.provider.get_block_number().await?;
        Ok(block_number.as_u64())
    }

    pub fn address(&self) -> Address {
        // This is a blocking operation, should be refactored in production
        futures::executor::block_on(async {
            if let Some(wallet) = self.wallet.read().await.as_ref() {
                wallet.address()
            } else {
                Address::zero()
            }
        })
    }

    pub async fn get_balance(&self) -> Result<U256> {
        let address = self.address();
        if address.is_zero() {
            return Err(anyhow!("No wallet configured"));
        }

        let balance = self.provider.get_balance(address, None).await?;
        Ok(balance)
    }

    pub async fn load_contract_addresses(&self, _path: &str) -> Result<HashMap<String, Address>> {
        // Load from environment variables or .env.contracts file
        let mut addresses = HashMap::new();

        // Try to load from environment variables first
        // First try CONTRACT_NODE_REGISTRY, then fallback to NODE_REGISTRY
        if let Ok(addr) =
            std::env::var("CONTRACT_NODE_REGISTRY").or_else(|_| std::env::var("NODE_REGISTRY"))
        {
            if !addr.is_empty() {
                addresses.insert("NodeRegistry".to_string(), addr.parse()?);
            }
        }

        // First try CONTRACT_JOB_MARKETPLACE, then fallback to JOB_MARKETPLACE
        if let Ok(addr) =
            std::env::var("CONTRACT_JOB_MARKETPLACE").or_else(|_| std::env::var("JOB_MARKETPLACE"))
        {
            if !addr.is_empty() {
                addresses.insert("JobMarketplace".to_string(), addr.parse()?);
            }
        }

        if let Ok(addr) = std::env::var("PAYMENT_ESCROW_WITH_EARNINGS_ADDRESS") {
            if !addr.is_empty() {
                addresses.insert("PaymentEscrow".to_string(), addr.parse()?);
            }
        }

        if let Ok(addr) = std::env::var("HOST_EARNINGS_ADDRESS") {
            if !addr.is_empty() {
                addresses.insert("HostEarnings".to_string(), addr.parse()?);
            }
        }

        if let Ok(addr) = std::env::var("REPUTATION_SYSTEM_ADDRESS") {
            if !addr.is_empty() {
                addresses.insert("ReputationSystem".to_string(), addr.parse()?);
            }
        }

        if let Ok(addr) = std::env::var("PROOF_SYSTEM_ADDRESS") {
            if !addr.is_empty() {
                addresses.insert("ProofSystem".to_string(), addr.parse()?);
            }
        }

        if let Ok(addr) = std::env::var("EZKL_VERIFIER_ADDRESS") {
            if !addr.is_empty() {
                addresses.insert("EzklVerifier".to_string(), addr.parse()?);
            }
        }

        // If no environment variables found, fall back to default addresses
        if addresses.is_empty() {
            // Default addresses for Base Sepolia from .env.local.test
            addresses.insert(
                "NodeRegistry".to_string(),
                std::env::var("CONTRACT_NODE_REGISTRY")
                    .expect("CONTRACT_NODE_REGISTRY must be set")
                    .parse()?,
            );
            addresses.insert(
                "JobMarketplace".to_string(),
                std::env::var("CONTRACT_JOB_MARKETPLACE")
                    .expect("CONTRACT_JOB_MARKETPLACE must be set")
                    .parse()?,
            );
            addresses.insert(
                "ProofSystem".to_string(),
                std::env::var("CONTRACT_PROOF_SYSTEM")
                    .expect("CONTRACT_PROOF_SYSTEM must be set")
                    .parse()?,
            );
            addresses.insert(
                "HostEarnings".to_string(),
                std::env::var("CONTRACT_HOST_EARNINGS")
                    .expect("CONTRACT_HOST_EARNINGS must be set")
                    .parse()?,
            );
            addresses.insert(
                "ModelRegistry".to_string(),
                std::env::var("CONTRACT_MODEL_REGISTRY")
                    .expect("❌ FATAL: CONTRACT_MODEL_REGISTRY must be set")
                    .parse()?,
            );
        }

        *self.contract_addresses.write().await = addresses.clone();
        Ok(addresses)
    }

    pub fn set_wallet(&mut self, private_key: &str) -> Result<()> {
        let wallet = private_key
            .parse::<LocalWallet>()
            .map_err(|e| anyhow!("Invalid private key: {}", e))?
            .with_chain_id(self.config.chain_id);

        let signer = SignerMiddleware::new(self.provider.clone(), wallet);

        // This is a blocking operation, should be refactored in production
        futures::executor::block_on(async {
            *self.wallet.write().await = Some(signer);
        });

        Ok(())
    }

    pub async fn estimate_gas(
        &self,
        to: Address,
        value: U256,
        data: Option<Bytes>,
    ) -> Result<U256> {
        let from = self.address();
        if from.is_zero() {
            return Err(anyhow!("No wallet configured"));
        }

        let mut tx = TransactionRequest::new().from(from).to(to).value(value);

        if let Some(data) = data {
            tx = tx.data(data);
        }

        // ethers 2.0 uses estimate_gas directly on the transaction request
        let gas = self.provider.estimate_gas(&tx.into(), None).await?;
        Ok(gas)
    }

    pub async fn send_transaction(
        &self,
        to: Address,
        value: U256,
        data: Option<Bytes>,
    ) -> Result<H256> {
        let wallet_guard = self.wallet.read().await;
        let wallet = wallet_guard
            .as_ref()
            .ok_or_else(|| anyhow!("No wallet configured"))?;

        let mut tx = TransactionRequest::new().to(to).value(value);

        if let Some(data) = data {
            tx = tx.data(data);
        }

        // CRITICAL: Use send_transaction which signs locally with SignerMiddleware
        // This should use eth_sendRawTransaction, not eth_sendTransaction
        let pending_tx = wallet.send_transaction(tx, None).await
            .map_err(|e| {
                // Check if it's the eth_sendTransaction error
                if e.to_string().contains("eth_sendTransaction") ||
                   e.to_string().contains("Unsupported method") {
                    anyhow!("RPC doesn't support eth_sendTransaction. Ensure HOST_PRIVATE_KEY is set and wallet is properly configured. Error: {}", e)
                } else {
                    anyhow!("Transaction failed: {}", e)
                }
            })?;

        info!(
            "Transaction sent via eth_sendRawTransaction: {:?}",
            pending_tx.tx_hash()
        );
        Ok(pending_tx.tx_hash())
    }

    pub async fn wait_for_confirmation(&self, tx_hash: H256) -> Result<TransactionReceipt> {
        // Poll for the transaction receipt with retries
        // Base Sepolia can take 15-30 seconds to mine a transaction
        let max_attempts = 60; // 60 attempts with 1 second delay = 60 seconds max
        let mut attempts = 0;

        loop {
            attempts += 1;

            // Try to get the transaction receipt
            match self.provider.get_transaction_receipt(tx_hash).await {
                Ok(Some(receipt)) => {
                    // Transaction mined! Now wait for confirmations if needed
                    if self.config.confirmations > 1 {
                        let tx_block = receipt
                            .block_number
                            .ok_or_else(|| anyhow!("Receipt missing block number"))?;

                        // Wait for required confirmations
                        loop {
                            let current_block = self.provider.get_block_number().await?;
                            let confirmations = current_block.saturating_sub(tx_block);

                            if confirmations >= U64::from(self.config.confirmations) {
                                break;
                            }

                            // Wait a bit before checking again
                            tokio::time::sleep(Duration::from_secs(2)).await;
                        }
                    }

                    return Ok(receipt);
                }
                Ok(None) => {
                    // Transaction not mined yet
                    if attempts >= max_attempts {
                        return Err(anyhow!(
                            "Transaction not mined after {} seconds. Tx hash: {:?}",
                            max_attempts,
                            tx_hash
                        ));
                    }

                    // Wait before retrying
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(e) => {
                    // RPC error - retry a few times
                    if attempts >= 3 {
                        return Err(anyhow!("Failed to get transaction receipt: {}", e));
                    }
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }

    pub async fn create_multicall(&self) -> Result<Multicall3<Provider<Http>>> {
        // Try to load from environment variable, fall back to default universal address
        let multicall_address = std::env::var("MULTICALL3_ADDRESS")
            .unwrap_or_else(|_| {
                eprintln!("⚠️  WARNING: MULTICALL3_ADDRESS not set, using default universal address: 0xcA11bde05977b3631167028862bE2a173976CA11");
                "0xcA11bde05977b3631167028862bE2a173976CA11".to_string()
            })
            .parse::<Address>()?;

        let multicall = Multicall3::new(multicall_address, self.provider.clone());

        // Store for future use
        *self.multicall.write().await = Some(multicall.clone());

        Ok(multicall)
    }

    pub async fn switch_network(&mut self, chain_config: ChainConfig) -> Result<()> {
        self.config.rpc_url = chain_config.rpc_url;
        self.config.chain_id = chain_config.chain_id;

        // Recreate provider
        let provider = Provider::<Http>::try_from(&self.config.rpc_url)?
            .interval(self.config.polling_interval);

        self.provider = Arc::new(provider);

        // Clear wallet to avoid issues
        *self.wallet.write().await = None;

        Ok(())
    }

    pub async fn get_nonce(&self) -> Result<U256> {
        let address = self.address();
        if address.is_zero() {
            return Err(anyhow!("No wallet configured"));
        }

        let nonce = self.provider.get_transaction_count(address, None).await?;
        Ok(nonce)
    }

    pub fn create_event_filter(
        &self,
        addresses: Vec<Address>,
        topics: Vec<H256>,
        from_block: u64,
        to_block: Option<u64>,
    ) -> Filter {
        let mut filter = Filter::new().from_block(from_block);

        if let Some(to) = to_block {
            filter = filter.to_block(to);
        }

        if !addresses.is_empty() {
            filter = filter.address(addresses);
        }

        if !topics.is_empty() {
            filter = filter.topic0(topics);
        }

        filter
    }

    pub fn into_client(self) -> Self {
        self
    }

    pub async fn update_rpc_url(&mut self, new_url: &str) -> Result<()> {
        self.config.rpc_url = new_url.to_string();

        let provider = Provider::<Http>::try_from(new_url)?.interval(self.config.polling_interval);

        self.provider = Arc::new(provider);
        Ok(())
    }

    pub async fn get_gas_price(&self) -> Result<U256> {
        let gas_price = self.provider.get_gas_price().await?;
        Ok(gas_price)
    }

    pub async fn get_eip1559_gas_price(&self) -> Result<(U256, U256)> {
        let (max_fee, priority_fee) = self.provider.estimate_eip1559_fees(None).await?;
        Ok((max_fee, priority_fee))
    }

    pub async fn subscribe_blocks(&self) -> Result<mpsc::Receiver<Block<H256>>> {
        let (tx, rx) = mpsc::channel(100);

        let provider = self.provider.clone();
        let interval = self.config.polling_interval;

        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let mut last_block = 0u64;

            loop {
                if let Ok(block_number) = provider.get_block_number().await {
                    let current = block_number.as_u64();

                    if current > last_block {
                        for block_num in (last_block + 1)..=current {
                            if let Ok(Some(block)) = provider.get_block(block_num).await {
                                let _ = tx_clone.send(block).await;
                            }
                        }
                        last_block = current;
                    }
                }

                tokio::time::sleep(interval).await;
            }
        });

        *self.block_stream_sender.write().await = Some(tx);
        Ok(rx)
    }
}
