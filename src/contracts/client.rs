use ethers::prelude::*;
use ethers::providers::{Provider, Http};
use std::sync::Arc;
use std::time::Duration;
use std::collections::HashMap;
use anyhow::{Result, anyhow};
use tokio::sync::{RwLock, mpsc};

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
        let chain_id = provider.get_chainid().await
            .map_err(|e| anyhow!("Failed to connect to RPC: {}", e))?;

        if chain_id.as_u64() != config.chain_id {
            return Err(anyhow!("Chain ID mismatch: expected {}, got {}", config.chain_id, chain_id));
        }

        let provider = Arc::new(provider);
        
        let wallet = if let Some(private_key) = &config.private_key {
            let wallet = private_key.parse::<LocalWallet>()
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
        // In a real implementation, this would load from a JSON file
        // For testing, we'll return mock addresses
        let mut addresses = HashMap::new();
        addresses.insert("NodeRegistry".to_string(), "0x5FbDB2315678afecb367f032d93F642f64180aa3".parse()?);
        addresses.insert("JobMarketplace".to_string(), "0xe7f1725E7734CE288F8367e1Bb143E90bb3F0512".parse()?);
        addresses.insert("PaymentEscrow".to_string(), "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0".parse()?);
        addresses.insert("ReputationSystem".to_string(), "0xCf7Ed3AccA5a467e9e704C703E8D87F634fB0Fc9".parse()?);
        addresses.insert("ProofSystem".to_string(), "0xDc64a140Aa3E981100a9becA4E685f962f0cF6C9".parse()?);
        
        *self.contract_addresses.write().await = addresses.clone();
        Ok(addresses)
    }

    pub fn set_wallet(&mut self, private_key: &str) -> Result<()> {
        let wallet = private_key.parse::<LocalWallet>()
            .map_err(|e| anyhow!("Invalid private key: {}", e))?
            .with_chain_id(self.config.chain_id);
        
        let signer = SignerMiddleware::new(self.provider.clone(), wallet);
        
        // This is a blocking operation, should be refactored in production
        futures::executor::block_on(async {
            *self.wallet.write().await = Some(signer);
        });
        
        Ok(())
    }

    pub async fn estimate_gas(&self, to: Address, value: U256, data: Option<Bytes>) -> Result<U256> {
        let from = self.address();
        if from.is_zero() {
            return Err(anyhow!("No wallet configured"));
        }

        let mut tx = TransactionRequest::new()
            .from(from)
            .to(to)
            .value(value);
            
        if let Some(data) = data {
            tx = tx.data(data);
        }

        // ethers 2.0 uses estimate_gas directly on the transaction request
        let gas = self.provider.estimate_gas(&tx.into(), None).await?;
        Ok(gas)
    }

    pub async fn send_transaction(&self, to: Address, value: U256, data: Option<Bytes>) -> Result<H256> {
        let wallet_guard = self.wallet.read().await;
        let wallet = wallet_guard.as_ref()
            .ok_or_else(|| anyhow!("No wallet configured"))?;

        let mut tx = TransactionRequest::new()
            .to(to)
            .value(value);
            
        if let Some(data) = data {
            tx = tx.data(data);
        }

        let pending_tx = wallet.send_transaction(tx, None).await?;
        Ok(pending_tx.tx_hash())
    }

    pub async fn wait_for_confirmation(&self, tx_hash: H256) -> Result<TransactionReceipt> {
        let receipt = self.provider
            .get_transaction_receipt(tx_hash)
            .await?
            .ok_or_else(|| anyhow!("Transaction not found"))?;
        
        // Wait for confirmations
        if self.config.confirmations > 1 {
            let current_block = self.provider.get_block_number().await?;
            let tx_block = receipt.block_number.unwrap();
            let confirmations = current_block.saturating_sub(tx_block);
            
            if confirmations < U64::from(self.config.confirmations) {
                // In production, would implement proper waiting logic
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
        
        Ok(receipt)
    }

    pub async fn create_multicall(&self) -> Result<Multicall3<Provider<Http>>> {
        // Multicall3 address is the same on all chains
        let multicall_address = "0xcA11bde05977b3631167028862bE2a173976CA11".parse::<Address>()?;
        
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
        let mut filter = Filter::new()
            .from_block(from_block);
            
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
        
        let provider = Provider::<Http>::try_from(new_url)?
            .interval(self.config.polling_interval);
        
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