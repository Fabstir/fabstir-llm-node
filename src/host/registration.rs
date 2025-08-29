use ethers::prelude::*;
use ethers::middleware::SignerMiddleware;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::task::JoinHandle;
use anyhow::{Result, anyhow};
use tracing::{info, warn, error, debug};
use serde::{Serialize, Deserialize};
use serde_json;

use crate::contracts::types::NodeRegistry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetadata {
    pub models: Vec<String>,        // ["llama-3.2", "tiny-vicuna"]
    pub gpu: String,                // "RTX 4090"
    pub ram_gb: u32,                // 64
    pub cost_per_token: f64,        // 0.0001
    pub max_concurrent_jobs: u32,   // 5
}

#[derive(Debug, Clone)]
pub struct RegistrationConfig {
    pub contract_address: Address,
    pub stake_amount: U256,
    pub auto_register: bool,
    pub heartbeat_interval: u64, // seconds
}

pub struct NodeRegistration {
    contract: Arc<NodeRegistry<SignerMiddleware<Provider<Http>, LocalWallet>>>,
    node_address: Address,
    stake_amount: U256,
    metadata: NodeMetadata,
    heartbeat_handle: Option<JoinHandle<()>>,
    is_registered: Arc<AtomicBool>,
    last_heartbeat: Arc<AtomicU64>,
    heartbeat_interval: u64,
}

impl NodeRegistration {
    pub async fn new(
        provider: Arc<Provider<Http>>,
        wallet: LocalWallet,
        metadata: NodeMetadata,
        config: RegistrationConfig,
    ) -> Result<Self> {
        // Create signer middleware
        let chain_id = provider.get_chainid().await.unwrap_or(U256::from(1));
        let wallet = wallet.with_chain_id(chain_id.as_u64());
        let client = Arc::new(SignerMiddleware::new(provider.as_ref().clone(), wallet.clone()));
        
        // Create contract instance
        let contract = Arc::new(NodeRegistry::new(config.contract_address, client));
        
        let node_address = wallet.address();
        
        let mut registration = Self {
            contract,
            node_address,
            stake_amount: config.stake_amount,
            metadata,
            heartbeat_handle: None,
            is_registered: Arc::new(AtomicBool::new(false)),
            last_heartbeat: Arc::new(AtomicU64::new(0)),
            heartbeat_interval: config.heartbeat_interval,
        };
        
        // Auto-register if configured
        if config.auto_register {
            info!("Auto-registering node on startup");
            registration.register_node().await?;
        }
        
        Ok(registration)
    }
    
    pub async fn register_node(&mut self) -> Result<TransactionReceipt> {
        info!("Registering node with stake: {}", self.stake_amount);
        
        // Check stake requirement first
        if !self.check_stake_requirement().await {
            return Err(anyhow!("Insufficient stake amount"));
        }
        
        // Mock: In real implementation, would approve stake token transfer
        debug!("Approving stake transfer (mocked)");
        
        // Build metadata JSON
        let metadata_json = self.build_metadata_json();
        
        // Mock: In real implementation, would call contract
        info!("Calling registerNode on contract (mocked)");
        debug!("Metadata: {}", metadata_json);
        
        // Mark as registered
        self.is_registered.store(true, Ordering::Relaxed);
        
        // Start heartbeat
        self.start_heartbeat();
        
        // Return mock receipt
        Ok(TransactionReceipt {
            transaction_hash: H256::random(),
            block_hash: Some(H256::random()),
            block_number: Some(U64::from(12345)),
            gas_used: Some(U256::from(100000)),
            status: Some(U64::from(1)),
            ..Default::default()
        })
    }
    
    pub async fn update_capabilities(&mut self, metadata: NodeMetadata) -> Result<()> {
        info!("Updating node capabilities");
        
        if !self.is_registered.load(Ordering::Relaxed) {
            return Err(anyhow!("Node not registered"));
        }
        
        // Update metadata
        self.metadata = metadata;
        
        // Build new metadata JSON
        let metadata_json = self.build_metadata_json();
        
        // Mock: In real implementation, would call contract update
        debug!("Calling updateNode on contract (mocked)");
        debug!("New metadata: {}", metadata_json);
        
        Ok(())
    }
    
    pub async fn unregister_node(&mut self) -> Result<()> {
        info!("Unregistering node");
        
        if !self.is_registered.load(Ordering::Relaxed) {
            return Err(anyhow!("Node not registered"));
        }
        
        // Stop heartbeat first
        self.stop_heartbeat().await;
        
        // Mock: In real implementation, would call contract
        debug!("Calling unregisterNode on contract (mocked)");
        
        // Mark as unregistered
        self.is_registered.store(false, Ordering::Relaxed);
        
        // Mock: Return stake (in real implementation)
        info!("Stake returned (mocked): {}", self.stake_amount);
        
        Ok(())
    }
    
    pub fn start_heartbeat(&mut self) {
        if self.heartbeat_handle.is_some() {
            warn!("Heartbeat already running");
            return;
        }
        
        // For testing, allow heartbeat even if not registered
        // In production, would require registration
        
        let interval = self.heartbeat_interval;
        let last_heartbeat = self.last_heartbeat.clone();
        let is_registered = self.is_registered.clone();
        let node_address = self.node_address;
        
        let handle = tokio::spawn(async move {
            info!("Starting heartbeat with interval: {}s", interval);
            
            loop {
                // Mock: In real implementation, would send transaction
                debug!("Sending heartbeat for node: {} (mocked)", node_address);
                
                // Update last heartbeat timestamp
                let now = chrono::Utc::now().timestamp() as u64;
                last_heartbeat.store(now, Ordering::Relaxed);
                
                // Wait for next interval
                tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;
                
                // Check if should stop (for unregistration)
                if !is_registered.load(Ordering::Relaxed) {
                    debug!("Node unregistered, stopping heartbeat");
                    break;
                }
            }
        });
        
        self.heartbeat_handle = Some(handle);
    }
    
    pub async fn stop_heartbeat(&mut self) {
        if let Some(handle) = self.heartbeat_handle.take() {
            handle.abort();
            info!("Heartbeat stopped");
        }
    }
    
    pub async fn check_stake_requirement(&self) -> bool {
        // Mock: Check if stake amount meets minimum requirement
        // In real implementation, would check contract for minimum stake
        let min_stake = U256::from(500000u64); // 500K wei minimum
        
        if self.stake_amount < min_stake {
            warn!("Stake amount {} is less than minimum {}", self.stake_amount, min_stake);
            false
        } else {
            true
        }
    }
    
    pub fn build_metadata_json(&self) -> String {
        let metadata_obj = serde_json::json!({
            "models": self.metadata.models,
            "gpu": self.metadata.gpu,
            "ram": self.metadata.ram_gb,
            "cost_per_token": self.metadata.cost_per_token,
            "max_concurrent_jobs": self.metadata.max_concurrent_jobs,
        });
        
        metadata_obj.to_string()
    }
    
    pub fn is_registered(&self) -> bool {
        self.is_registered.load(Ordering::Relaxed)
    }
    
    pub fn is_heartbeat_running(&self) -> bool {
        self.heartbeat_handle.is_some()
    }
    
    pub fn get_last_heartbeat(&self) -> u64 {
        self.last_heartbeat.load(Ordering::Relaxed)
    }
    
    pub fn get_node_address(&self) -> Address {
        self.node_address
    }
    
    pub fn get_stake_amount(&self) -> U256 {
        self.stake_amount
    }
    
    pub fn get_metadata(&self) -> &NodeMetadata {
        &self.metadata
    }
    
    /// Check if heartbeat is healthy (not stale)
    pub fn is_heartbeat_healthy(&self) -> bool {
        if !self.is_registered() || !self.is_heartbeat_running() {
            return false;
        }
        
        let last = self.get_last_heartbeat();
        let now = chrono::Utc::now().timestamp() as u64;
        
        // Consider healthy if heartbeat within 2x interval
        (now - last) < (self.heartbeat_interval * 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_metadata_serialization() {
        let metadata = NodeMetadata {
            models: vec!["llama-3.2".to_string(), "mistral-7b".to_string()],
            gpu: "RTX 4090".to_string(),
            ram_gb: 64,
            cost_per_token: 0.0001,
            max_concurrent_jobs: 5,
        };
        
        // Serialize to JSON
        let json = serde_json::to_string(&metadata).unwrap();
        
        // Deserialize back
        let metadata2: NodeMetadata = serde_json::from_str(&json).unwrap();
        
        assert_eq!(metadata.models, metadata2.models);
        assert_eq!(metadata.gpu, metadata2.gpu);
        assert_eq!(metadata.ram_gb, metadata2.ram_gb);
    }
    
    #[test]
    fn test_stake_validation() {
        let min_stake = U256::from(500000u64);
        
        // Test various amounts
        assert!(U256::from(1000000u64) >= min_stake);
        assert!(U256::from(500000u64) >= min_stake);
        assert!(!(U256::from(100u64) >= min_stake));
    }
}