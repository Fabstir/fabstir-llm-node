use super::chains::{ChainConfig, ChainConfigLoader, ChainRegistry};
use ethers::providers::{Provider, Http, Middleware};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use anyhow::{Result, anyhow};
use tokio::sync::Mutex;
use ethers::types::{U64, Block, H256};

#[derive(Debug, Clone)]
pub enum ProviderHealth {
    Healthy {
        latency_ms: u64,
        block_number: Option<U64>,
    },
    Degraded {
        latency_ms: u64,
        error_rate: f32,
    },
    Unhealthy {
        last_error: String,
        consecutive_failures: u32,
    },
}

#[derive(Debug, Clone)]
pub struct PoolStats {
    pub total_requests: u64,
    pub pool_size: usize,
    pub cache_hits: u64,
    pub cache_misses: u64,
}

#[derive(Debug, Clone)]
pub struct RotationStats {
    pub rotation_count: u64,
    pub current_index: usize,
    pub total_endpoints: usize,
}

struct ProviderEntry {
    provider: Arc<Provider<Http>>,
    health: ProviderHealth,
    last_check: Instant,
    is_primary: bool,
}

pub struct MultiChainProvider {
    providers: Arc<RwLock<HashMap<u64, ProviderEntry>>>,
    backup_providers: Arc<RwLock<HashMap<u64, Arc<Provider<Http>>>>>,
    config_loader: ChainConfigLoader,
    pool_size: usize,
    pool_stats: Arc<RwLock<HashMap<u64, PoolStats>>>,
    rotation_stats: Arc<RwLock<HashMap<u64, RotationStats>>>,
    enable_failover: bool,
    enable_rotation: bool,
}

impl MultiChainProvider {
    pub async fn new(config_loader: ChainConfigLoader) -> Result<Self> {
        let mut provider = Self {
            providers: Arc::new(RwLock::new(HashMap::new())),
            backup_providers: Arc::new(RwLock::new(HashMap::new())),
            config_loader,
            pool_size: 1,
            pool_stats: Arc::new(RwLock::new(HashMap::new())),
            rotation_stats: Arc::new(RwLock::new(HashMap::new())),
            enable_failover: false,
            enable_rotation: false,
        };

        provider.initialize_providers().await?;
        Ok(provider)
    }

    pub async fn with_failover(config_loader: ChainConfigLoader) -> Result<Self> {
        let mut provider = Self::new(config_loader).await?;
        provider.enable_failover = true;
        provider.initialize_backup_providers().await?;
        Ok(provider)
    }

    pub async fn with_pool_size(config_loader: ChainConfigLoader, pool_size: usize) -> Result<Self> {
        let mut provider = Self::new(config_loader).await?;
        provider.pool_size = pool_size;
        provider.initialize_pool_stats();
        Ok(provider)
    }

    pub async fn with_rotation(config_loader: ChainConfigLoader) -> Result<Self> {
        let mut provider = Self::new(config_loader).await?;
        provider.enable_rotation = true;
        provider.initialize_rotation_stats();
        Ok(provider)
    }

    async fn initialize_providers(&mut self) -> Result<()> {
        let registry = self.config_loader.build_registry().await
            .map_err(|e| anyhow!("Failed to build registry: {}", e))?;
        let mut providers = self.providers.write().unwrap();

        for chain_id in registry.list_supported_chains() {
            if let Some(config) = registry.get_chain(chain_id) {
                match Provider::<Http>::try_from(&config.rpc_url) {
                    Ok(provider) => {
                        let entry = ProviderEntry {
                            provider: Arc::new(provider),
                            health: ProviderHealth::Healthy {
                                latency_ms: 0,
                                block_number: None,
                            },
                            last_check: Instant::now(),
                            is_primary: true,
                        };
                        providers.insert(chain_id, entry);
                    }
                    Err(e) => {
                        eprintln!("Failed to create provider for chain {}: {}", chain_id, e);
                    }
                }
            }
        }

        Ok(())
    }

    async fn initialize_backup_providers(&mut self) -> Result<()> {
        let mut backup_providers = self.backup_providers.write().unwrap();

        // Base Sepolia backup
        if let Ok(backup_url) = std::env::var("BASE_SEPOLIA_BACKUP_RPC_URL") {
            if let Ok(provider) = Provider::<Http>::try_from(backup_url) {
                backup_providers.insert(84532, Arc::new(provider));
            }
        }

        // opBNB backup
        if let Ok(backup_url) = std::env::var("OPBNB_BACKUP_RPC_URL") {
            if let Ok(provider) = Provider::<Http>::try_from(backup_url) {
                backup_providers.insert(5611, Arc::new(provider));
            }
        }

        Ok(())
    }

    fn initialize_pool_stats(&mut self) {
        let mut stats = self.pool_stats.write().unwrap();
        stats.insert(84532, PoolStats {
            total_requests: 0,
            pool_size: self.pool_size,
            cache_hits: 0,
            cache_misses: 0,
        });
        stats.insert(5611, PoolStats {
            total_requests: 0,
            pool_size: self.pool_size,
            cache_hits: 0,
            cache_misses: 0,
        });
    }

    fn initialize_rotation_stats(&mut self) {
        let mut stats = self.rotation_stats.write().unwrap();

        // Parse multiple RPC URLs if available
        if let Ok(urls) = std::env::var("BASE_SEPOLIA_RPC_URLS") {
            let endpoints: Vec<&str> = urls.split(',').collect();
            stats.insert(84532, RotationStats {
                rotation_count: 0,
                current_index: 0,
                total_endpoints: endpoints.len(),
            });
        }
    }

    pub fn get_provider(&self, chain_id: u64) -> Option<Arc<Provider<Http>>> {
        // Update pool stats if enabled
        if self.pool_size > 1 {
            let mut stats = self.pool_stats.write().unwrap();
            if let Some(pool_stat) = stats.get_mut(&chain_id) {
                pool_stat.total_requests += 1;
                pool_stat.cache_hits += 1; // Simplified: always a hit if provider exists
            }
        }

        let providers = self.providers.read().unwrap();
        providers.get(&chain_id).map(|entry| entry.provider.clone())
    }

    pub fn get_provider_with_rotation(&mut self, chain_id: u64) -> Option<Arc<Provider<Http>>> {
        if self.enable_rotation {
            let mut stats = self.rotation_stats.write().unwrap();
            if let Some(rotation_stat) = stats.get_mut(&chain_id) {
                rotation_stat.rotation_count += 1;
                rotation_stat.current_index = (rotation_stat.current_index + 1) % rotation_stat.total_endpoints;
            }
        }

        self.get_provider(chain_id)
    }

    pub async fn check_provider_health(&mut self, chain_id: u64) -> ProviderHealth {
        let providers = self.providers.read().unwrap();

        if let Some(entry) = providers.get(&chain_id) {
            let provider = entry.provider.clone();
            drop(providers); // Release lock before async operation

            let start = Instant::now();
            match tokio::time::timeout(
                Duration::from_secs(5),
                provider.get_block_number()
            ).await {
                Ok(Ok(block_number)) => {
                    let latency_ms = start.elapsed().as_millis() as u64;
                    let health = if latency_ms < 1000 {
                        ProviderHealth::Healthy {
                            latency_ms,
                            block_number: Some(block_number),
                        }
                    } else {
                        ProviderHealth::Degraded {
                            latency_ms,
                            error_rate: 0.0,
                        }
                    };

                    // Update health status
                    let mut providers = self.providers.write().unwrap();
                    if let Some(entry) = providers.get_mut(&chain_id) {
                        entry.health = health.clone();
                        entry.last_check = Instant::now();
                    }

                    health
                }
                Ok(Err(e)) => {
                    let health = ProviderHealth::Unhealthy {
                        last_error: e.to_string(),
                        consecutive_failures: 1,
                    };

                    // Update health status
                    let mut providers = self.providers.write().unwrap();
                    if let Some(entry) = providers.get_mut(&chain_id) {
                        entry.health = health.clone();
                        entry.last_check = Instant::now();
                    }

                    health
                }
                Err(_) => {
                    let health = ProviderHealth::Unhealthy {
                        last_error: "Timeout".to_string(),
                        consecutive_failures: 1,
                    };

                    // Update health status
                    let mut providers = self.providers.write().unwrap();
                    if let Some(entry) = providers.get_mut(&chain_id) {
                        entry.health = health.clone();
                        entry.last_check = Instant::now();
                    }

                    health
                }
            }
        } else {
            ProviderHealth::Unhealthy {
                last_error: "Provider not found".to_string(),
                consecutive_failures: 0,
            }
        }
    }

    pub async fn check_all_providers_health(&mut self) -> HashMap<u64, ProviderHealth> {
        let mut health_map = HashMap::new();
        let chain_ids: Vec<u64> = {
            let providers = self.providers.read().unwrap();
            providers.keys().cloned().collect()
        };

        for chain_id in chain_ids {
            let health = self.check_provider_health(chain_id).await;
            health_map.insert(chain_id, health);
        }

        health_map
    }

    pub async fn trigger_failover_if_needed(&mut self, chain_id: u64) {
        if !self.enable_failover {
            return;
        }

        let health = self.check_provider_health(chain_id).await;

        if let ProviderHealth::Unhealthy { .. } = health {
            // Try to switch to backup
            let backup_providers = self.backup_providers.read().unwrap();
            if let Some(backup) = backup_providers.get(&chain_id) {
                let mut providers = self.providers.write().unwrap();
                if let Some(entry) = providers.get_mut(&chain_id) {
                    entry.provider = backup.clone();
                    entry.is_primary = false;
                    entry.health = ProviderHealth::Healthy {
                        latency_ms: 0,
                        block_number: None,
                    };
                }
            }
        }
    }

    pub fn is_using_primary(&self, chain_id: u64) -> bool {
        let providers = self.providers.read().unwrap();
        providers.get(&chain_id).map(|entry| entry.is_primary).unwrap_or(true)
    }

    pub fn get_pool_stats(&self, chain_id: u64) -> PoolStats {
        let stats = self.pool_stats.read().unwrap();
        stats.get(&chain_id).cloned().unwrap_or(PoolStats {
            total_requests: 0,
            pool_size: 0,
            cache_hits: 0,
            cache_misses: 0,
        })
    }

    pub fn get_rotation_stats(&self, chain_id: u64) -> RotationStats {
        let stats = self.rotation_stats.read().unwrap();
        stats.get(&chain_id).cloned().unwrap_or(RotationStats {
            rotation_count: 0,
            current_index: 0,
            total_endpoints: 1,
        })
    }

    pub async fn execute_with_retry<F, Fut, T>(
        &self,
        chain_id: u64,
        f: F,
    ) -> Result<T>
    where
        F: Fn(Arc<Provider<Http>>) -> Fut,
        Fut: std::future::Future<Output = Result<T, ethers::providers::ProviderError>>,
    {
        let provider = self.get_provider(chain_id)
            .ok_or_else(|| anyhow!("Provider not found for chain {}", chain_id))?;

        let mut retries = 3;
        let mut last_error = None;

        while retries > 0 {
            match tokio::time::timeout(
                Duration::from_secs(10),
                f(provider.clone())
            ).await {
                Ok(Ok(result)) => return Ok(result),
                Ok(Err(e)) => {
                    last_error = Some(e.to_string());
                    retries -= 1;
                    if retries > 0 {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                }
                Err(_) => {
                    last_error = Some("Request timeout".to_string());
                    retries -= 1;
                    if retries > 0 {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                }
            }
        }

        Err(anyhow!("Failed after 3 retries: {}",
            last_error.unwrap_or_else(|| "Unknown error".to_string())))
    }
}