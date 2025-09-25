use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Configuration for chain-specific connection pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConnectionConfig {
    pub chain_id: u64,
    pub max_connections: usize,
    pub rate_limit_per_minute: usize,
    pub burst_size: usize,
    pub health_check_interval: Duration,
    pub connection_timeout: Duration,
}

impl ChainConnectionConfig {
    /// Create config for Base Sepolia
    pub fn base_sepolia() -> Self {
        Self {
            chain_id: 84532,
            max_connections: 100,
            rate_limit_per_minute: 600,
            burst_size: 100,
            health_check_interval: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(5),
        }
    }

    /// Create config for opBNB Testnet
    pub fn opbnb_testnet() -> Self {
        Self {
            chain_id: 5611,
            max_connections: 50,
            rate_limit_per_minute: 300,
            burst_size: 50,
            health_check_interval: Duration::from_secs(60),
            connection_timeout: Duration::from_secs(10),
        }
    }
}

/// Individual connection in a pool
#[derive(Debug, Clone)]
pub struct PoolConnection {
    id: String,
    chain_id: u64,
    created_at: Instant,
    last_used: Instant,
    is_active: bool,
}

impl PoolConnection {
    pub fn new(id: String, chain_id: u64) -> Self {
        Self {
            id,
            chain_id,
            created_at: Instant::now(),
            last_used: Instant::now(),
            is_active: false,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }

    pub fn mark_active(&mut self) {
        self.is_active = true;
        self.last_used = Instant::now();
    }

    pub fn mark_idle(&mut self) {
        self.is_active = false;
    }

    pub fn is_expired(&self, max_lifetime: Duration) -> bool {
        self.created_at.elapsed() > max_lifetime
    }
}

/// Connection pool for a specific chain
pub struct ChainPool {
    chain_id: u64,
    config: ChainConnectionConfig,
    connections: Arc<RwLock<HashMap<String, PoolConnection>>>,
    active_connections: Arc<RwLock<HashMap<String, PoolConnection>>>,
    idle_connections: Arc<RwLock<Vec<PoolConnection>>>,
}

impl ChainPool {
    pub fn new(config: ChainConnectionConfig) -> Self {
        Self {
            chain_id: config.chain_id,
            config,
            connections: Arc::new(RwLock::new(HashMap::new())),
            active_connections: Arc::new(RwLock::new(HashMap::new())),
            idle_connections: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn max_connections(&self) -> usize {
        self.config.max_connections
    }

    pub async fn acquire_connection(&self, conn_id: &str) -> Result<PoolConnection> {
        // Check if we have an idle connection
        let mut idle = self.idle_connections.write().await;
        if let Some(mut conn) = idle.pop() {
            conn.mark_active();
            self.active_connections
                .write()
                .await
                .insert(conn.id.clone(), conn.clone());
            debug!(
                "Reusing idle connection {} for chain {}",
                conn.id, self.chain_id
            );
            return Ok(conn);
        }
        drop(idle);

        // Check if we can create a new connection
        let connections = self.connections.read().await;
        if connections.len() >= self.config.max_connections {
            return Err(anyhow!(
                "Connection limit reached for chain {}: {}/{}",
                self.chain_id,
                connections.len(),
                self.config.max_connections
            ));
        }
        drop(connections);

        // Create new connection
        let mut conn = PoolConnection::new(conn_id.to_string(), self.chain_id);
        conn.mark_active();

        let mut connections = self.connections.write().await;
        connections.insert(conn.id.clone(), conn.clone());

        let mut active = self.active_connections.write().await;
        active.insert(conn.id.clone(), conn.clone());

        info!(
            "Created new connection {} for chain {}",
            conn.id, self.chain_id
        );

        Ok(conn)
    }

    pub async fn release_connection(&self, conn_id: &str) -> Result<()> {
        let mut active = self.active_connections.write().await;
        if let Some(mut conn) = active.remove(conn_id) {
            conn.mark_idle();

            // Check if connection is expired
            if conn.is_expired(Duration::from_secs(300)) {
                self.connections.write().await.remove(conn_id);
                info!(
                    "Expired connection {} removed from chain {}",
                    conn_id, self.chain_id
                );
            } else {
                self.idle_connections.write().await.push(conn);
                debug!(
                    "Connection {} returned to idle pool for chain {}",
                    conn_id, self.chain_id
                );
            }
            Ok(())
        } else {
            Err(anyhow!("Connection {} not found in active pool", conn_id))
        }
    }

    pub async fn get_stats(&self) -> ConnectionPoolStats {
        ConnectionPoolStats {
            chain_id: self.chain_id,
            total_connections: self.connections.read().await.len(),
            active_connections: self.active_connections.read().await.len(),
            idle_connections: self.idle_connections.read().await.len(),
            max_connections: self.config.max_connections,
        }
    }

    pub async fn cleanup_expired(&self) {
        let max_lifetime = Duration::from_secs(300);

        // Clean idle connections
        let mut idle = self.idle_connections.write().await;
        idle.retain(|conn| !conn.is_expired(max_lifetime));

        // Clean from main connection pool
        let mut connections = self.connections.write().await;
        connections.retain(|_, conn| !conn.is_expired(max_lifetime));
    }
}

/// Statistics for a connection pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionPoolStats {
    pub chain_id: u64,
    pub total_connections: usize,
    pub active_connections: usize,
    pub idle_connections: usize,
    pub max_connections: usize,
}

/// Manager for multiple chain connection pools
pub struct ChainConnectionPool {
    pools: Arc<RwLock<HashMap<u64, Arc<ChainPool>>>>,
    configs: Arc<RwLock<HashMap<u64, ChainConnectionConfig>>>,
}

impl ChainConnectionPool {
    pub fn new() -> Self {
        Self {
            pools: Arc::new(RwLock::new(HashMap::new())),
            configs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_chain_config(&self, config: ChainConnectionConfig) {
        let chain_id = config.chain_id;
        self.configs.write().await.insert(chain_id, config.clone());

        // Create pool for this chain
        let pool = Arc::new(ChainPool::new(config));
        self.pools.write().await.insert(chain_id, pool);

        info!("Added connection pool for chain {}", chain_id);
    }

    pub async fn get_or_create_pool(&self, chain_id: u64) -> Result<Arc<ChainPool>> {
        // Check if pool exists
        if let Some(pool) = self.pools.read().await.get(&chain_id) {
            return Ok(pool.clone());
        }

        // Check if we have config for this chain
        let configs = self.configs.read().await;
        if let Some(config) = configs.get(&chain_id) {
            let pool = Arc::new(ChainPool::new(config.clone()));
            drop(configs);

            self.pools.write().await.insert(chain_id, pool.clone());
            info!("Created new pool for chain {}", chain_id);
            Ok(pool)
        } else {
            drop(configs);

            // Use default config based on chain
            let config = match chain_id {
                84532 => ChainConnectionConfig::base_sepolia(),
                5611 => ChainConnectionConfig::opbnb_testnet(),
                _ => return Err(anyhow!("No configuration found for chain {}", chain_id)),
            };

            let pool = Arc::new(ChainPool::new(config.clone()));
            self.pools.write().await.insert(chain_id, pool.clone());
            self.configs.write().await.insert(chain_id, config);
            info!("Created default pool for chain {}", chain_id);
            Ok(pool)
        }
    }

    pub async fn get_connection_stats(&self, chain_id: u64) -> Result<ConnectionPoolStats> {
        if let Some(pool) = self.pools.read().await.get(&chain_id) {
            Ok(pool.get_stats().await)
        } else {
            Err(anyhow!("No pool found for chain {}", chain_id))
        }
    }

    pub async fn get_all_stats(&self) -> HashMap<u64, ConnectionPoolStats> {
        let mut all_stats = HashMap::new();
        let pools = self.pools.read().await;

        for (chain_id, pool) in pools.iter() {
            all_stats.insert(*chain_id, pool.get_stats().await);
        }

        all_stats
    }

    pub async fn cleanup_all_expired(&self) {
        let pools = self.pools.read().await;
        for pool in pools.values() {
            pool.cleanup_expired().await;
        }
        debug!("Cleaned up expired connections across all chains");
    }

    pub async fn shutdown_chain(&self, chain_id: u64) -> Result<()> {
        if let Some(_) = self.pools.write().await.remove(&chain_id) {
            self.configs.write().await.remove(&chain_id);
            info!("Shut down connection pool for chain {}", chain_id);
            Ok(())
        } else {
            Err(anyhow!("No pool found for chain {}", chain_id))
        }
    }
}

impl Default for ChainConnectionPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_chain_pool_creation() {
        let config = ChainConnectionConfig::base_sepolia();
        let pool = ChainPool::new(config);
        assert_eq!(pool.max_connections(), 100);
        assert_eq!(pool.chain_id, 84532);
    }

    #[tokio::test]
    async fn test_connection_lifecycle() {
        let config = ChainConnectionConfig::opbnb_testnet();
        let pool = ChainPool::new(config);

        // Acquire connection
        let conn = pool.acquire_connection("test-conn").await.unwrap();
        assert_eq!(conn.chain_id(), 5611);
        assert_eq!(conn.id(), "test-conn");

        let stats = pool.get_stats().await;
        assert_eq!(stats.active_connections, 1);
        assert_eq!(stats.idle_connections, 0);

        // Release connection
        pool.release_connection("test-conn").await.unwrap();

        let stats = pool.get_stats().await;
        assert_eq!(stats.active_connections, 0);
        assert_eq!(stats.idle_connections, 1);
    }

    #[tokio::test]
    async fn test_multi_chain_pool_manager() {
        let manager = ChainConnectionPool::new();

        // Add configs
        manager
            .add_chain_config(ChainConnectionConfig::base_sepolia())
            .await;
        manager
            .add_chain_config(ChainConnectionConfig::opbnb_testnet())
            .await;

        // Get pools
        let base_pool = manager.get_or_create_pool(84532).await.unwrap();
        assert_eq!(base_pool.max_connections(), 100);

        let opbnb_pool = manager.get_or_create_pool(5611).await.unwrap();
        assert_eq!(opbnb_pool.max_connections(), 50);

        // Get stats
        let all_stats = manager.get_all_stats().await;
        assert_eq!(all_stats.len(), 2);
        assert!(all_stats.contains_key(&84532));
        assert!(all_stats.contains_key(&5611));
    }
}
