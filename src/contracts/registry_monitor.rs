use ethers::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use anyhow::Result;
use tracing::{info, warn, error};

use super::types::{NodeRegistry, NodeRegisteredEvent, NodeUpdatedEvent, NodeUnregisteredEvent};

#[derive(Debug, Clone)]
pub struct NodeMetadata {
    pub address: Address,
    pub metadata: String,
    pub stake: U256,
    pub registered_at: u64,
    pub last_updated: u64,
}

pub struct RegistryMonitor {
    contract: NodeRegistry<Provider<Http>>,
    cache: Arc<RwLock<HashMap<Address, NodeMetadata>>>,
    monitoring_handle: Option<JoinHandle<()>>,
}

impl RegistryMonitor {
    pub fn new(contract_address: Address, provider: Arc<Provider<Http>>) -> Self {
        let contract = NodeRegistry::new(contract_address, provider);
        Self {
            contract,
            cache: Arc::new(RwLock::new(HashMap::new())),
            monitoring_handle: None,
        }
    }

    pub async fn start_monitoring(&mut self, from_block: Option<u64>) -> Result<()> {
        if self.monitoring_handle.is_some() {
            warn!("Monitoring already started");
            return Ok(());
        }

        let contract = self.contract.clone();
        let cache = self.cache.clone();
        let from = from_block.unwrap_or(0);

        let handle = tokio::spawn(async move {
            info!("Starting registry event monitoring from block {}", from);
            let mut current_block = from;
            
            loop {
                // Poll for new events every 5 seconds
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                
                // Get latest block
                let latest_block = match contract.client().get_block_number().await {
                    Ok(block) => block.as_u64(),
                    Err(e) => {
                        warn!("Failed to get latest block: {}", e);
                        continue;
                    }
                };

                if current_block >= latest_block {
                    continue;
                }

                // Query events from current_block to latest
                let registered_events = match contract
                    .event::<NodeRegisteredEvent>()
                    .from_block(current_block)
                    .to_block(latest_block)
                    .query()
                    .await {
                        Ok(events) => events,
                        Err(e) => {
                            warn!("Failed to query NodeRegistered events: {}", e);
                            vec![]
                        }
                    };
                
                for event in registered_events {
                    Self::handle_registered_event(&cache, event).await;
                }

                let updated_events = match contract
                    .event::<NodeUpdatedEvent>()
                    .from_block(current_block)
                    .to_block(latest_block)
                    .query()
                    .await {
                        Ok(events) => events,
                        Err(e) => {
                            warn!("Failed to query NodeUpdated events: {}", e);
                            vec![]
                        }
                    };
                
                for event in updated_events {
                    Self::handle_updated_event(&cache, event).await;
                }

                let unregistered_events = match contract
                    .event::<NodeUnregisteredEvent>()
                    .from_block(current_block)
                    .to_block(latest_block)
                    .query()
                    .await {
                        Ok(events) => events,
                        Err(e) => {
                            warn!("Failed to query NodeUnregistered events: {}", e);
                            vec![]
                        }
                    };
                
                for event in unregistered_events {
                    Self::handle_unregistered_event(&cache, event).await;
                }

                current_block = latest_block + 1;
            }
        });

        self.monitoring_handle = Some(handle);
        Ok(())
    }

    async fn handle_registered_event(cache: &Arc<RwLock<HashMap<Address, NodeMetadata>>>, event: NodeRegisteredEvent) {
        let metadata = NodeMetadata {
            address: event.node,
            metadata: event.metadata,
            stake: event.stake,
            registered_at: chrono::Utc::now().timestamp() as u64,
            last_updated: chrono::Utc::now().timestamp() as u64,
        };
        
        let mut cache = cache.write().await;
        cache.insert(event.node, metadata);
        info!("Node registered: {}", event.node);
    }

    async fn handle_updated_event(cache: &Arc<RwLock<HashMap<Address, NodeMetadata>>>, event: NodeUpdatedEvent) {
        let mut cache = cache.write().await;
        if let Some(meta) = cache.get_mut(&event.node) {
            meta.metadata = event.metadata;
            meta.last_updated = chrono::Utc::now().timestamp() as u64;
            info!("Node updated: {}", event.node);
        }
    }

    async fn handle_unregistered_event(cache: &Arc<RwLock<HashMap<Address, NodeMetadata>>>, event: NodeUnregisteredEvent) {
        let mut cache = cache.write().await;
        cache.remove(&event.node);
        info!("Node unregistered: {}", event.node);
    }

    pub async fn stop_monitoring(&mut self) {
        if let Some(handle) = self.monitoring_handle.take() {
            handle.abort();
            info!("Registry monitoring stopped");
        }
    }

    pub async fn get_registered_hosts(&self) -> Vec<Address> {
        let cache = self.cache.read().await;
        cache.keys().cloned().collect()
    }

    pub async fn get_host_metadata(&self, address: Address) -> Option<NodeMetadata> {
        let cache = self.cache.read().await;
        cache.get(&address).cloned()
    }

    pub async fn get_hosts_by_capability(&self, capability: &str) -> Vec<Address> {
        let cache = self.cache.read().await;
        cache.iter()
            .filter(|(_, meta)| meta.metadata.contains(capability))
            .map(|(addr, _)| *addr)
            .collect()
    }

    pub async fn replay_events(&self, from_block: u64, to_block: u64) -> Result<()> {
        info!("Replaying events from block {} to {}", from_block, to_block);
        
        // Query historical NodeRegistered events
        let registered_events = self.contract
            .event::<NodeRegisteredEvent>()
            .from_block(from_block)
            .to_block(to_block)
            .query()
            .await?;
        
        for event in registered_events {
            Self::handle_registered_event(&self.cache, event).await;
        }

        // Query historical NodeUpdated events
        let updated_events = self.contract
            .event::<NodeUpdatedEvent>()
            .from_block(from_block)
            .to_block(to_block)
            .query()
            .await?;
        
        for event in updated_events {
            Self::handle_updated_event(&self.cache, event).await;
        }

        // Query historical NodeUnregistered events
        let unregistered_events = self.contract
            .event::<NodeUnregisteredEvent>()
            .from_block(from_block)
            .to_block(to_block)
            .query()
            .await?;
        
        for event in unregistered_events {
            Self::handle_unregistered_event(&self.cache, event).await;
        }

        Ok(())
    }

    // Manual event handlers for testing
    pub async fn handle_node_registered(&self, node: Address, metadata: String, stake: U256) {
        let event = NodeRegisteredEvent { node, metadata, stake };
        Self::handle_registered_event(&self.cache, event).await;
    }

    pub async fn handle_node_updated(&self, node: Address, metadata: String) {
        let event = NodeUpdatedEvent { node, metadata };
        Self::handle_updated_event(&self.cache, event).await;
    }

    pub async fn handle_node_unregistered(&self, node: Address) {
        let event = NodeUnregisteredEvent { node };
        Self::handle_unregistered_event(&self.cache, event).await;
    }
}