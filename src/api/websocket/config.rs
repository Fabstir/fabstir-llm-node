use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

/// WebSocket production configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketConfig {
    pub max_connections: usize,
    pub max_connections_per_ip: usize,
    pub rate_limit_per_minute: usize,
    pub compression_enabled: bool,
    pub compression_threshold: usize,
    pub auth_required: bool,
    pub metrics_enabled: bool,
    pub metrics_port: u16,
    pub memory_cache_max_mb: usize,
    pub context_window_max_tokens: usize,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            max_connections: 10000,
            max_connections_per_ip: 100,
            rate_limit_per_minute: 600,
            compression_enabled: true,
            compression_threshold: 1024,
            auth_required: true,
            metrics_enabled: true,
            metrics_port: 9090,
            memory_cache_max_mb: 2048,
            context_window_max_tokens: 4096,
        }
    }
}

/// Production configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductionConfig {
    #[serde(rename = "websocket.production")]
    pub websocket: WebSocketConfig,
}

impl Default for ProductionConfig {
    fn default() -> Self {
        Self {
            websocket: WebSocketConfig::default(),
        }
    }
}

impl ProductionConfig {
    /// Load configuration from file
    pub fn from_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        
        // Parse TOML with nested structure
        let toml_value: toml::Value = toml::from_str(&content)?;
        
        // Extract websocket.production section
        let websocket = if let Some(ws_table) = toml_value.get("websocket") {
            if let Some(prod_table) = ws_table.get("production") {
                let ws_config: WebSocketConfig = prod_table.clone().try_into()?;
                ws_config
            } else {
                WebSocketConfig::default()
            }
        } else {
            WebSocketConfig::default()
        };
        
        Ok(Self { websocket })
    }
    
    /// Load from environment variables
    pub fn from_env() -> Self {
        let mut config = Self::default();
        
        if let Ok(val) = std::env::var("WS_MAX_CONNECTIONS") {
            if let Ok(num) = val.parse() {
                config.websocket.max_connections = num;
            }
        }
        
        if let Ok(val) = std::env::var("WS_RATE_LIMIT") {
            if let Ok(num) = val.parse() {
                config.websocket.rate_limit_per_minute = num;
            }
        }
        
        config
    }
}

/// Configuration manager with hot reload
pub struct ConfigManager {
    path: String,
    config: Arc<RwLock<ProductionConfig>>,
    watcher_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl ConfigManager {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.to_string(),
            config: Arc::new(RwLock::new(ProductionConfig::default())),
            watcher_handle: Arc::new(RwLock::new(None)),
        }
    }
    
    pub async fn load(&self) -> Result<()> {
        let config = ProductionConfig::from_file(&self.path)?;
        *self.config.write().await = config;
        Ok(())
    }
    
    pub async fn get(&self) -> ProductionConfig {
        self.config.read().await.clone()
    }
    
    pub async fn start_watching(&self) {
        let path = self.path.clone();
        let config = self.config.clone();
        
        let handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                
                // Try to reload config
                if let Ok(new_config) = ProductionConfig::from_file(&path) {
                    *config.write().await = new_config;
                }
            }
        });
        
        *self.watcher_handle.write().await = Some(handle);
    }
    
    pub async fn stop_watching(&self) {
        if let Some(handle) = self.watcher_handle.write().await.take() {
            handle.abort();
        }
    }
}