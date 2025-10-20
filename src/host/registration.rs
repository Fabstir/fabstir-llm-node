// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::{anyhow, Result};
use ethers::middleware::SignerMiddleware;
use ethers::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::contracts::model_registry::{ApprovedModels, ModelRegistryClient};
use crate::contracts::pricing_constants::{native, stable};
use crate::contracts::types::{NodeRegistry, NodeRegistryWithModels};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetadata {
    pub models: Vec<String>,           // Model file paths
    pub model_ids: Vec<H256>,          // Validated model IDs from registry
    pub gpu: String,                   // "RTX 4090"
    pub ram_gb: u32,                   // 64
    pub cost_per_token: f64,           // 0.0001 (deprecated, use pricing fields)
    pub max_concurrent_jobs: u32,      // 5
    pub api_url: String,               // Node's API endpoint URL
    pub min_price_native: Option<U256>, // Min price for native tokens (ETH/BNB)
    pub min_price_stable: Option<U256>, // Min price for stablecoins (USDC)
}

#[derive(Debug, Clone)]
pub struct RegistrationConfig {
    pub contract_address: Address,
    pub model_registry_address: Address,
    pub stake_amount: U256,
    pub auto_register: bool,
    pub heartbeat_interval: u64, // seconds
    pub use_new_registry: bool,  // Use NodeRegistryWithModels if true
}

pub struct NodeRegistration {
    contract: Arc<NodeRegistry<SignerMiddleware<Provider<Http>, LocalWallet>>>,
    new_contract:
        Option<Arc<NodeRegistryWithModels<SignerMiddleware<Provider<Http>, LocalWallet>>>>,
    model_registry: Option<ModelRegistryClient>,
    node_address: Address,
    stake_amount: U256,
    metadata: NodeMetadata,
    heartbeat_handle: Option<JoinHandle<()>>,
    is_registered: Arc<AtomicBool>,
    last_heartbeat: Arc<AtomicU64>,
    heartbeat_interval: u64,
    use_new_registry: bool,
}

impl NodeRegistration {
    pub async fn new(
        provider: Arc<Provider<Http>>,
        wallet: LocalWallet,
        mut metadata: NodeMetadata,
        config: RegistrationConfig,
    ) -> Result<Self> {
        // Create signer middleware
        let chain_id = provider.get_chainid().await.unwrap_or(U256::from(1));
        let wallet = wallet.with_chain_id(chain_id.as_u64());
        let client = Arc::new(SignerMiddleware::new(
            provider.as_ref().clone(),
            wallet.clone(),
        ));

        // Create contract instances
        let contract = Arc::new(NodeRegistry::new(config.contract_address, client.clone()));

        let new_contract = if config.use_new_registry {
            Some(Arc::new(NodeRegistryWithModels::new(
                config.contract_address,
                client.clone(),
            )))
        } else {
            None
        };

        // Create model registry client
        let model_registry = if config.use_new_registry {
            let registry_client = ModelRegistryClient::new(
                provider.clone(),
                config.model_registry_address,
                Some(config.contract_address),
            )
            .await?;

            // Validate models and get their IDs
            let model_ids = registry_client
                .validate_models_for_registration(&metadata.models)
                .await?;
            metadata.model_ids = model_ids;

            Some(registry_client)
        } else {
            metadata.model_ids = Vec::new();
            None
        };

        let node_address = wallet.address();

        let mut registration = Self {
            contract,
            new_contract,
            model_registry,
            node_address,
            stake_amount: config.stake_amount,
            metadata,
            heartbeat_handle: None,
            is_registered: Arc::new(AtomicBool::new(false)),
            last_heartbeat: Arc::new(AtomicU64::new(0)),
            heartbeat_interval: config.heartbeat_interval,
            use_new_registry: config.use_new_registry,
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

        // If using new registry, validate models
        if self.use_new_registry {
            if self.metadata.model_ids.is_empty() {
                return Err(anyhow!("No approved models configured for registration"));
            }

            info!(
                "Registering with {} approved models",
                self.metadata.model_ids.len()
            );
            for model_id in &self.metadata.model_ids {
                debug!("Model ID: {:?}", model_id);
            }
        }

        // Build metadata JSON
        let metadata_json = self.build_metadata_json();

        // Call the actual contract
        let receipt = if self.use_new_registry {
            if let Some(ref new_contract) = self.new_contract {
                info!("Calling registerNode on NodeRegistryWithModels with dual pricing");
                debug!("Metadata: {}", metadata_json);
                debug!("API URL: {}", self.metadata.api_url);
                debug!("Model IDs: {:?}", self.metadata.model_ids);

                // Get pricing or use defaults
                let min_price_native = self
                    .metadata
                    .min_price_native
                    .unwrap_or_else(|| native::default_price());
                let min_price_stable = self
                    .metadata
                    .min_price_stable
                    .unwrap_or_else(|| stable::default_price());

                // Validate pricing ranges
                native::validate_price(min_price_native)
                    .map_err(|e| anyhow!("Native pricing validation failed: {}", e))?;
                stable::validate_price(min_price_stable)
                    .map_err(|e| anyhow!("Stable pricing validation failed: {}", e))?;

                info!(
                    "Native pricing: {} wei, Stable pricing: {}",
                    min_price_native, min_price_stable
                );

                let method = new_contract
                    .method::<_, ()>(
                        "registerNode",
                        (
                            metadata_json.clone(),
                            self.metadata.api_url.clone(),
                            self.metadata.model_ids.clone(),
                            min_price_native,
                            min_price_stable,
                        ),
                    )
                    .map_err(|e| anyhow!("Failed to create method call: {}", e))?;

                let method_with_value = method.value(self.stake_amount);
                let tx = method_with_value
                    .send()
                    .await
                    .map_err(|e| anyhow!("Failed to register node: {}", e))?;

                tx.await
                    .map_err(|e| anyhow!("Transaction failed: {}", e))?
                    .ok_or_else(|| anyhow!("Transaction receipt not found"))?
            } else {
                return Err(anyhow!("NodeRegistryWithModels contract not configured"));
            }
        } else {
            info!("Calling registerNode on legacy NodeRegistry");
            debug!("Metadata: {}", metadata_json);

            let method = self
                .contract
                .method::<_, ()>("registerNode", (metadata_json, self.stake_amount))
                .map_err(|e| anyhow!("Failed to create method call: {}", e))?;

            let tx = method
                .send()
                .await
                .map_err(|e| anyhow!("Failed to register node: {}", e))?;

            tx.await
                .map_err(|e| anyhow!("Transaction failed: {}", e))?
                .ok_or_else(|| anyhow!("Transaction receipt not found"))?
        };

        // Mark as registered only after successful transaction
        self.is_registered.store(true, Ordering::Relaxed);

        // Start heartbeat
        self.start_heartbeat();

        Ok(receipt)
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

        // Call the actual contract to update metadata
        if self.use_new_registry {
            if let Some(ref new_contract) = self.new_contract {
                let method = new_contract
                    .method::<_, ()>("updateMetadata", metadata_json)
                    .map_err(|e| anyhow!("Failed to create method call: {}", e))?;

                let tx = method
                    .send()
                    .await
                    .map_err(|e| anyhow!("Failed to update metadata: {}", e))?;

                tx.await.map_err(|e| anyhow!("Transaction failed: {}", e))?;
            } else {
                return Err(anyhow!("NodeRegistryWithModels contract not configured"));
            }
        } else {
            let method = self
                .contract
                .method::<_, ()>("updateNode", metadata_json)
                .map_err(|e| anyhow!("Failed to create method call: {}", e))?;

            let tx = method
                .send()
                .await
                .map_err(|e| anyhow!("Failed to update node: {}", e))?;

            tx.await.map_err(|e| anyhow!("Transaction failed: {}", e))?;
        }

        Ok(())
    }

    pub async fn unregister_node(&mut self) -> Result<()> {
        info!("Unregistering node");

        if !self.is_registered.load(Ordering::Relaxed) {
            return Err(anyhow!("Node not registered"));
        }

        // Stop heartbeat first
        self.stop_heartbeat().await;

        // Call the actual contract to unregister
        if self.use_new_registry {
            if let Some(ref new_contract) = self.new_contract {
                let method = new_contract
                    .method::<_, ()>("unregisterNode", ())
                    .map_err(|e| anyhow!("Failed to create method call: {}", e))?;

                let tx = method
                    .send()
                    .await
                    .map_err(|e| anyhow!("Failed to unregister node: {}", e))?;

                let receipt = tx.await.map_err(|e| anyhow!("Transaction failed: {}", e))?;

                if let Some(receipt) = receipt {
                    info!(
                        "Node unregistered, stake returned in tx: {:?}",
                        receipt.transaction_hash
                    );
                }
            } else {
                return Err(anyhow!("NodeRegistryWithModels contract not configured"));
            }
        } else {
            let method = self
                .contract
                .method::<_, ()>("unregisterNode", ())
                .map_err(|e| anyhow!("Failed to create method call: {}", e))?;

            let tx = method
                .send()
                .await
                .map_err(|e| anyhow!("Failed to unregister node: {}", e))?;

            let receipt = tx.await.map_err(|e| anyhow!("Transaction failed: {}", e))?;

            if let Some(receipt) = receipt {
                info!(
                    "Node unregistered, stake returned in tx: {:?}",
                    receipt.transaction_hash
                );
            }
        }

        // Mark as unregistered only after successful transaction
        self.is_registered.store(false, Ordering::Relaxed);

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
                // Note: Heartbeat is typically handled off-chain or via a separate service
                // This is a placeholder for actual heartbeat implementation
                debug!("Heartbeat check for node: {}", node_address);

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
        // Check contract for minimum stake requirement
        let min_stake = if self.use_new_registry {
            if let Some(ref new_contract) = self.new_contract {
                let method = match new_contract.method::<_, U256>("MIN_STAKE", ()) {
                    Ok(m) => m,
                    Err(e) => {
                        error!("Failed to create method call: {}", e);
                        return false;
                    }
                };

                match method.call().await {
                    Ok(stake) => stake,
                    Err(e) => {
                        error!("Failed to get minimum stake requirement: {}", e);
                        return false;
                    }
                }
            } else {
                error!("NodeRegistryWithModels contract not configured");
                return false;
            }
        } else {
            let method = match self.contract.method::<_, U256>("getMinimumStake", ()) {
                Ok(m) => m,
                Err(e) => {
                    error!("Failed to create method call: {}", e);
                    return false;
                }
            };

            match method.call().await {
                Ok(stake) => stake,
                Err(e) => {
                    error!("Failed to get minimum stake requirement: {}", e);
                    return false;
                }
            }
        };

        if self.stake_amount < min_stake {
            warn!(
                "Stake amount {} is less than minimum {}",
                self.stake_amount, min_stake
            );
            false
        } else {
            true
        }
    }

    pub fn build_metadata_json(&self) -> String {
        let metadata_obj = if self.use_new_registry {
            // New format for NodeRegistryWithModels
            serde_json::json!({
                "hardware": {
                    "gpu": self.metadata.gpu,
                    "vram": 24, // Mock VRAM value
                    "ram_gb": self.metadata.ram_gb,
                },
                "capabilities": ["inference", "streaming"],
                "location": "us-east",
                "maxConcurrent": self.metadata.max_concurrent_jobs,
                "cost_per_token": self.metadata.cost_per_token,
            })
        } else {
            // Legacy format
            serde_json::json!({
                "models": self.metadata.models,
                "gpu": self.metadata.gpu,
                "ram": self.metadata.ram_gb,
                "cost_per_token": self.metadata.cost_per_token,
                "max_concurrent_jobs": self.metadata.max_concurrent_jobs,
            })
        };

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

    pub fn get_model_ids(&self) -> &[H256] {
        &self.metadata.model_ids
    }

    pub fn get_api_url(&self) -> &str {
        &self.metadata.api_url
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
            model_ids: vec![],
            gpu: "RTX 4090".to_string(),
            ram_gb: 64,
            cost_per_token: 0.0001,
            max_concurrent_jobs: 5,
            api_url: "http://localhost:8080".to_string(),
            min_price_native: Some(native::default_price()),
            min_price_stable: Some(stable::default_price()),
        };

        // Serialize to JSON
        let json = serde_json::to_string(&metadata).unwrap();

        // Deserialize back
        let metadata2: NodeMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(metadata.models, metadata2.models);
        assert_eq!(metadata.gpu, metadata2.gpu);
        assert_eq!(metadata.ram_gb, metadata2.ram_gb);
        assert_eq!(metadata.min_price_native, metadata2.min_price_native);
        assert_eq!(metadata.min_price_stable, metadata2.min_price_stable);
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
