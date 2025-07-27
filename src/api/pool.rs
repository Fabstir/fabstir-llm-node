use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use std::collections::HashMap;
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub min_connections: usize,
    pub max_connections: usize,
    pub connection_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_lifetime: Duration,
    pub scale_up_threshold: f64,
    pub scale_down_threshold: f64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            min_connections: 2,
            max_connections: 10,
            connection_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(60),
            max_lifetime: Duration::from_secs(300),
            scale_up_threshold: 0.8,
            scale_down_threshold: 0.2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionStats {
    pub total_connections: usize,
    pub idle_connections: usize,
    pub active_connections: usize,
}

pub struct Connection {
    id: String,
    created_at: Instant,
    last_used: Instant,
}

pub struct ConnectionPool {
    config: PoolConfig,
    connections: Arc<RwLock<Vec<Arc<Connection>>>>,
    idle_connections: Arc<RwLock<Vec<Arc<Connection>>>>,
    active_connections: Arc<RwLock<HashMap<String, Arc<Connection>>>>,
}

impl ConnectionPool {
    pub async fn new(config: PoolConfig) -> Result<Self> {
        let pool = Self {
            connections: Arc::new(RwLock::new(Vec::new())),
            idle_connections: Arc::new(RwLock::new(Vec::new())),
            active_connections: Arc::new(RwLock::new(HashMap::new())),
            config,
        };
        
        // Create minimum connections
        for i in 0..pool.config.min_connections {
            let conn = Arc::new(Connection {
                id: format!("conn-{}", i),
                created_at: Instant::now(),
                last_used: Instant::now(),
            });
            pool.connections.write().await.push(conn.clone());
            pool.idle_connections.write().await.push(conn);
        }
        
        // Start background tasks
        let pool_arc = Arc::new(pool.clone());
        tokio::spawn(async move {
            pool_arc.maintenance_loop().await;
        });
        
        Ok(pool)
    }
    
    pub async fn stats(&self) -> ConnectionStats {
        let total = self.connections.read().await.len();
        let idle = self.idle_connections.read().await.len();
        let active = self.active_connections.read().await.len();
        
        ConnectionStats {
            total_connections: total,
            idle_connections: idle,
            active_connections: active,
        }
    }
    
    pub async fn acquire(&self) -> Result<Arc<Connection>> {
        let start = Instant::now();
        
        loop {
            // Try to get an idle connection
            if let Some(conn) = self.idle_connections.write().await.pop() {
                self.active_connections.write().await.insert(conn.id.clone(), conn.clone());
                return Ok(conn);
            }
            
            // Check if we can create a new connection
            let total = self.connections.read().await.len();
            if total < self.config.max_connections {
                let conn = Arc::new(Connection {
                    id: format!("conn-{}", total),
                    created_at: Instant::now(),
                    last_used: Instant::now(),
                });
                self.connections.write().await.push(conn.clone());
                self.active_connections.write().await.insert(conn.id.clone(), conn.clone());
                return Ok(conn);
            }
            
            // Check timeout
            if start.elapsed() > self.config.connection_timeout {
                return Err(anyhow::anyhow!("Connection timeout"));
            }
            
            // Wait a bit before retrying
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    
    pub async fn release(&self, conn: Arc<Connection>) {
        self.active_connections.write().await.remove(&conn.id);
        self.idle_connections.write().await.push(conn);
    }
    
    async fn maintenance_loop(self: Arc<Self>) {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        
        loop {
            interval.tick().await;
            
            // Clean up idle connections
            let now = Instant::now();
            let mut idle = self.idle_connections.write().await;
            idle.retain(|conn| {
                now.duration_since(conn.last_used) < self.config.idle_timeout
                    && now.duration_since(conn.created_at) < self.config.max_lifetime
            });
            
            // Scale based on usage
            let stats = self.stats().await;
            let usage = stats.active_connections as f64 / stats.total_connections.max(1) as f64;
            
            if usage > self.config.scale_up_threshold && stats.total_connections < self.config.max_connections {
                // Scale up
                let new_conn = Arc::new(Connection {
                    id: format!("conn-{}", stats.total_connections),
                    created_at: Instant::now(),
                    last_used: Instant::now(),
                });
                self.connections.write().await.push(new_conn.clone());
                idle.push(new_conn);
            } else if usage < self.config.scale_down_threshold && stats.total_connections > self.config.min_connections {
                // Scale down
                if let Some(_) = idle.pop() {
                    // Connection removed
                }
            }
        }
    }
}

impl Clone for ConnectionPool {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            connections: self.connections.clone(),
            idle_connections: self.idle_connections.clone(),
            active_connections: self.active_connections.clone(),
        }
    }
}