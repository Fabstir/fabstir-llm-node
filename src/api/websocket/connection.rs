use anyhow::Result;
use std::sync::Arc;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock, Mutex};
use super::session::WebSocketSession;
use tracing::{info, warn, debug};

#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Connected,
    Authenticated,
    Disconnected,
    Failed,
}

pub struct ConnectionHandler {
    connections: Arc<RwLock<HashMap<String, ConnectionInfo>>>,
    message_tx: Option<mpsc::Sender<ConnectionMessage>>,
    timeout: Duration,
}

struct ConnectionInfo {
    session: WebSocketSession,
    state: ConnectionState,
    last_activity: Instant,
    metrics: ConnectionMetrics,
    user_id: Option<String>,
    cleanup_callback: Option<Box<dyn Fn(&str) + Send + Sync>>,
}

#[derive(Debug, Default)]
pub struct ConnectionMetrics {
    pub messages_received: u64,
    pub messages_sent: u64,
    pub bytes_received: u64,
    pub bytes_sent: u64,
    pub errors: u64,
}

pub struct ConnectionMessage {
    pub connection_id: String,
    pub content: String,
}

pub struct BroadcastResult {
    pub connection_id: String,
    pub success: bool,
    pub error: Option<String>,
}

impl ConnectionHandler {
    pub fn new(tx: mpsc::Sender<ConnectionMessage>) -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            message_tx: Some(tx),
            timeout: Duration::from_secs(300),
        }
    }

    pub fn new_standalone() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            message_tx: None,
            timeout: Duration::from_secs(300),
        }
    }

    pub async fn active_connections(&self) -> usize {
        self.connections.read().await.len()
    }

    pub async fn register_connection(&self, conn_id: &str, session: WebSocketSession) -> Result<()> {
        let info = ConnectionInfo {
            session,
            state: ConnectionState::Connected,
            last_activity: Instant::now(),
            metrics: ConnectionMetrics::default(),
            user_id: None,
            cleanup_callback: None,
        };

        self.connections.write().await.insert(conn_id.to_string(), info);
        info!("Registered connection: {}", conn_id);
        Ok(())
    }

    pub async fn register_connection_with_callback(
        &self,
        conn_id: &str,
        session: WebSocketSession,
        callback: Box<dyn Fn(&str) + Send + Sync>,
    ) -> Result<()> {
        let info = ConnectionInfo {
            session,
            state: ConnectionState::Connected,
            last_activity: Instant::now(),
            metrics: ConnectionMetrics::default(),
            user_id: None,
            cleanup_callback: Some(callback),
        };

        self.connections.write().await.insert(conn_id.to_string(), info);
        info!("Registered connection with callback: {}", conn_id);
        Ok(())
    }

    pub async fn remove_connection(&self, conn_id: &str) -> Result<()> {
        let mut connections = self.connections.write().await;
        if let Some(info) = connections.remove(conn_id) {
            if let Some(callback) = info.cleanup_callback {
                callback(conn_id);
            }
            info!("Removed connection: {}", conn_id);
        }
        Ok(())
    }

    pub async fn has_connection(&self, conn_id: &str) -> bool {
        self.connections.read().await.contains_key(conn_id)
    }

    pub async fn handle_message(&self, conn_id: &str, message: &str) -> Result<()> {
        let mut connections = self.connections.write().await;
        if let Some(info) = connections.get_mut(conn_id) {
            info.last_activity = Instant::now();
            info.metrics.messages_received += 1;
            info.metrics.bytes_received += message.len() as u64;
            
            if let Some(tx) = &self.message_tx {
                tx.send(ConnectionMessage {
                    connection_id: conn_id.to_string(),
                    content: message.to_string(),
                }).await?;
            }
        }
        Ok(())
    }

    pub async fn route_message(&self, conn_id: &str, message: &str) -> Result<String> {
        self.handle_message(conn_id, message).await?;
        Ok(format!("Message processed for {}", conn_id))
    }

    pub async fn broadcast(&self, message: &str) -> Result<Vec<BroadcastResult>> {
        let connections = self.connections.read().await;
        let mut results = Vec::new();

        for (id, _info) in connections.iter() {
            results.push(BroadcastResult {
                connection_id: id.clone(),
                success: true,
                error: None,
            });
        }

        Ok(results)
    }

    pub async fn check_connection_health(&self, conn_id: &str) -> Result<bool> {
        let connections = self.connections.read().await;
        if let Some(info) = connections.get(conn_id) {
            Ok(info.state != ConnectionState::Failed)
        } else {
            Ok(false)
        }
    }

    pub async fn mark_unhealthy(&self, conn_id: &str) -> Result<()> {
        let mut connections = self.connections.write().await;
        if let Some(info) = connections.get_mut(conn_id) {
            info.state = ConnectionState::Failed;
        }
        Ok(())
    }

    pub async fn get_connection_metrics(&self, conn_id: &str) -> Result<ConnectionMetrics> {
        let connections = self.connections.read().await;
        if let Some(info) = connections.get(conn_id) {
            Ok(ConnectionMetrics {
                messages_received: info.metrics.messages_received,
                messages_sent: info.metrics.messages_sent,
                bytes_received: info.metrics.bytes_received,
                bytes_sent: info.metrics.bytes_sent,
                errors: info.metrics.errors,
            })
        } else {
            Err(anyhow::anyhow!("Connection not found"))
        }
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }

    pub async fn cleanup_stale_connections(&self) -> Result<()> {
        let now = Instant::now();
        let mut to_remove = Vec::new();

        {
            let connections = self.connections.read().await;
            for (id, info) in connections.iter() {
                if now.duration_since(info.last_activity) > self.timeout {
                    to_remove.push(id.clone());
                }
            }
        }

        for id in to_remove {
            self.remove_connection(&id).await?;
            warn!("Removed stale connection: {}", id);
        }

        Ok(())
    }

    pub async fn get_session(&self, conn_id: &str) -> Result<WebSocketSession> {
        let connections = self.connections.read().await;
        if let Some(info) = connections.get(conn_id) {
            Ok(info.session.clone())
        } else {
            Err(anyhow::anyhow!("Connection not found"))
        }
    }

    pub async fn get_connection_state(&self, conn_id: &str) -> Result<ConnectionState> {
        let connections = self.connections.read().await;
        if let Some(info) = connections.get(conn_id) {
            Ok(info.state.clone())
        } else {
            Err(anyhow::anyhow!("Connection not found"))
        }
    }

    pub async fn authenticate_connection(&self, conn_id: &str, user_id: &str) -> Result<()> {
        let mut connections = self.connections.write().await;
        if let Some(info) = connections.get_mut(conn_id) {
            info.state = ConnectionState::Authenticated;
            info.user_id = Some(user_id.to_string());
            debug!("Authenticated connection {} for user {}", conn_id, user_id);
        }
        Ok(())
    }

    pub async fn disconnect(&self, conn_id: &str) -> Result<()> {
        let mut connections = self.connections.write().await;
        if let Some(info) = connections.get_mut(conn_id) {
            info.state = ConnectionState::Disconnected;
        }
        Ok(())
    }
}

impl Clone for ConnectionHandler {
    fn clone(&self) -> Self {
        Self {
            connections: self.connections.clone(),
            message_tx: self.message_tx.clone(),
            timeout: self.timeout,
        }
    }
}