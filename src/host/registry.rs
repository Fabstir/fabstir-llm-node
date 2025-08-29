use ethers::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use serde_json::Value;
use tracing::{info, debug, warn};

use crate::contracts::registry_monitor::{RegistryMonitor, NodeMetadata};

#[derive(Debug, Clone)]
pub struct HostInfo {
    pub address: Address,
    pub metadata: String,  // JSON string with capabilities
    pub stake: U256,
    pub is_online: bool,
}

pub struct HostRegistry {
    monitor: Arc<RegistryMonitor>,
    online_hosts: Arc<RwLock<HashSet<Address>>>, // Mock for now
    model_index: Arc<RwLock<HashMap<String, HashSet<Address>>>>, // model_id -> hosts
}

impl HostRegistry {
    pub fn new(monitor: Arc<RegistryMonitor>) -> Self {
        Self {
            monitor,
            online_hosts: Arc::new(RwLock::new(HashSet::new())),
            model_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get all registered host addresses
    pub async fn get_registered_hosts(&self) -> Vec<Address> {
        debug!("Getting all registered hosts");
        self.monitor.get_registered_hosts().await
    }

    /// Get metadata for a specific host
    pub async fn get_host_metadata(&self, address: Address) -> Option<HostInfo> {
        debug!("Getting metadata for host: {}", address);
        
        if let Some(node_meta) = self.monitor.get_host_metadata(address).await {
            // For mock implementation, host is online if it's registered
            let is_online = self.is_host_online_internal(address).await;
            
            Some(HostInfo {
                address: node_meta.address,
                metadata: node_meta.metadata,
                stake: node_meta.stake,
                is_online,
            })
        } else {
            None
        }
    }

    /// Check if a host is online (mocked for now)
    pub async fn is_host_online(&self, address: Address) -> bool {
        self.is_host_online_internal(address).await
    }

    /// Internal method for checking online status
    async fn is_host_online_internal(&self, address: Address) -> bool {
        // Mock implementation: host is online if it's registered
        let registered_hosts = self.monitor.get_registered_hosts().await;
        let is_registered = registered_hosts.contains(&address);
        
        if is_registered {
            // Add to online hosts set (mock behavior)
            let mut online = self.online_hosts.write().await;
            online.insert(address);
            true
        } else {
            false
        }
    }

    /// Get hosts that support a specific model
    pub async fn get_available_hosts(&self, model_id: &str) -> Vec<Address> {
        debug!("Getting available hosts for model: {}", model_id);
        
        // First check the index cache
        {
            let index = self.model_index.read().await;
            if let Some(hosts) = index.get(model_id) {
                return hosts.iter().cloned().collect();
            }
        }
        
        // If not in cache, search through all hosts
        let all_hosts = self.monitor.get_registered_hosts().await;
        let mut available_hosts = Vec::new();
        
        for host_addr in all_hosts {
            if let Some(node_meta) = self.monitor.get_host_metadata(host_addr).await {
                // Parse metadata as JSON to check for models
                if self.host_supports_model(&node_meta.metadata, model_id) {
                    available_hosts.push(host_addr);
                }
            }
        }
        
        // Update the index cache
        {
            let mut index = self.model_index.write().await;
            index.insert(
                model_id.to_string(),
                available_hosts.iter().cloned().collect()
            );
        }
        
        available_hosts
    }

    /// Check if host metadata indicates support for a model
    fn host_supports_model(&self, metadata: &str, model_id: &str) -> bool {
        // Try to parse metadata as JSON
        if let Ok(json) = serde_json::from_str::<Value>(metadata) {
            // Look for models array in the JSON
            if let Some(models) = json.get("models") {
                if let Some(models_array) = models.as_array() {
                    for model in models_array {
                        if let Some(model_str) = model.as_str() {
                            if model_str == model_id {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        
        // Fallback: simple string search (less reliable)
        metadata.contains(model_id)
    }

    /// Get hosts by capability string
    pub async fn get_hosts_by_capability(&self, capability: &str) -> Vec<Address> {
        debug!("Getting hosts with capability: {}", capability);
        
        // Use the monitor's built-in capability search
        self.monitor.get_hosts_by_capability(capability).await
    }

    /// Refresh the model index cache
    pub async fn refresh_model_index(&self) {
        info!("Refreshing model index cache");
        
        let all_hosts = self.monitor.get_registered_hosts().await;
        let mut new_index: HashMap<String, HashSet<Address>> = HashMap::new();
        
        for host_addr in all_hosts {
            if let Some(node_meta) = self.monitor.get_host_metadata(host_addr).await {
                // Parse metadata and extract models
                if let Ok(json) = serde_json::from_str::<Value>(&node_meta.metadata) {
                    if let Some(models) = json.get("models") {
                        if let Some(models_array) = models.as_array() {
                            for model in models_array {
                                if let Some(model_str) = model.as_str() {
                                    new_index
                                        .entry(model_str.to_string())
                                        .or_insert_with(HashSet::new)
                                        .insert(host_addr);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        let mut index = self.model_index.write().await;
        *index = new_index;
        
        info!("Model index refreshed with {} models", index.len());
    }

    /// Get hosts sorted by stake amount (highest first)
    pub async fn get_hosts_by_stake(&self) -> Vec<(Address, U256)> {
        debug!("Getting hosts sorted by stake");
        
        let all_hosts = self.monitor.get_registered_hosts().await;
        let mut hosts_with_stake = Vec::new();
        
        for host_addr in all_hosts {
            if let Some(node_meta) = self.monitor.get_host_metadata(host_addr).await {
                hosts_with_stake.push((host_addr, node_meta.stake));
            }
        }
        
        // Sort by stake descending
        hosts_with_stake.sort_by(|a, b| b.1.cmp(&a.1));
        hosts_with_stake
    }

    /// Get summary statistics about registered hosts
    pub async fn get_registry_stats(&self) -> RegistryStats {
        let all_hosts = self.monitor.get_registered_hosts().await;
        let online_hosts = self.online_hosts.read().await;
        let model_index = self.model_index.read().await;
        
        RegistryStats {
            total_hosts: all_hosts.len(),
            online_hosts: online_hosts.len(),
            unique_models: model_index.len(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RegistryStats {
    pub total_hosts: usize,
    pub online_hosts: usize,
    pub unique_models: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_host_supports_model() {
        let provider = Provider::<Http>::try_from("http://localhost:8545").unwrap();
        let contract_address = "0x0000000000000000000000000000000000000000"
            .parse::<Address>()
            .unwrap();
        
        let monitor = Arc::new(RegistryMonitor::new(contract_address, Arc::new(provider)));
        let registry = HostRegistry::new(monitor);
        
        // Test JSON parsing
        let metadata1 = r#"{"gpu":"rtx4090","models":["llama-7b","mistral-7b"]}"#;
        assert!(registry.host_supports_model(metadata1, "llama-7b"));
        assert!(registry.host_supports_model(metadata1, "mistral-7b"));
        assert!(!registry.host_supports_model(metadata1, "gpt-j"));
        
        // Test fallback string search
        let metadata2 = "gpu:rtx4090,models:llama-7b";
        assert!(registry.host_supports_model(metadata2, "llama-7b"));
    }
}