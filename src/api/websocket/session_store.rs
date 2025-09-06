use super::session::{WebSocketSession, SessionConfig, SessionMetrics};
use crate::job_processor::Message;
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStoreConfig {
    pub max_sessions: usize,
    pub cleanup_interval_seconds: u64,
    pub enable_metrics: bool,
    pub enable_persistence: bool,
}

impl Default for SessionStoreConfig {
    fn default() -> Self {
        Self {
            max_sessions: 1000,
            cleanup_interval_seconds: 300, // 5 minutes
            enable_metrics: true,
            enable_persistence: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreMetrics {
    pub total_sessions: usize,
    pub active_sessions: usize,
    pub total_messages: usize,
    pub total_memory_bytes: usize,
}

pub struct SessionStore {
    config: SessionStoreConfig,
    sessions: Arc<RwLock<HashMap<String, WebSocketSession>>>,
}

impl SessionStore {
    pub fn new(config: SessionStoreConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn session_count(&self) -> usize {
        // This is a synchronous placeholder
        // Use async_session_count() in async contexts
        0
    }

    pub fn active_sessions(&self) -> usize {
        // This is a synchronous placeholder
        // Use async_active_sessions() in async contexts
        0
    }

    pub async fn create_session(&mut self, config: SessionConfig) -> String {
        let session_id = WebSocketSession::generate_id();
        let session = WebSocketSession::new(session_id.clone(), config);
        
        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id.clone(), session);
        
        session_id
    }

    pub async fn try_create_session(&mut self, config: SessionConfig) -> Result<String> {
        let sessions = self.sessions.read().await;
        
        if sessions.len() >= self.config.max_sessions {
            return Err(anyhow!("Maximum number of sessions reached"));
        }
        
        drop(sessions); // Release read lock before getting write lock
        
        let session_id = WebSocketSession::generate_id();
        let session = WebSocketSession::new(session_id.clone(), config);
        
        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id.clone(), session);
        
        Ok(session_id)
    }

    pub async fn get_session(&self, session_id: &str) -> Option<WebSocketSession> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    pub async fn update_session(&mut self, session_id: &str, message: Message) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        
        match sessions.get_mut(session_id) {
            Some(session) => {
                session.add_message(message)?;
                Ok(())
            }
            None => Err(anyhow!("Session not found")),
        }
    }

    pub async fn destroy_session(&mut self, session_id: &str) -> bool {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id).is_some()
    }

    pub async fn session_exists(&self, session_id: &str) -> bool {
        let sessions = self.sessions.read().await;
        sessions.contains_key(session_id)
    }

    pub async fn cleanup_expired(&mut self) -> usize {
        let mut sessions = self.sessions.write().await;
        let initial_count = sessions.len();
        
        sessions.retain(|_, session| !session.is_expired());
        
        initial_count - sessions.len()
    }

    pub async fn get_all_sessions(&self) -> Vec<WebSocketSession> {
        let sessions = self.sessions.read().await;
        sessions.values().cloned().collect()
    }

    pub async fn clear_all(&mut self) {
        let mut sessions = self.sessions.write().await;
        sessions.clear();
    }

    pub async fn clear_session(&mut self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.clear();
            Ok(())
        } else {
            Err(anyhow!("Session not found"))
        }
    }

    pub async fn get_store_metrics(&self) -> StoreMetrics {
        let sessions = self.sessions.read().await;
        
        let mut total_messages = 0;
        let mut total_memory_bytes = 0;
        
        for session in sessions.values() {
            let metrics = session.metrics();
            total_messages += metrics.total_messages;
            total_memory_bytes += metrics.memory_bytes;
        }
        
        StoreMetrics {
            total_sessions: sessions.len(),
            active_sessions: sessions.len(), // All non-expired sessions are considered active
            total_messages,
            total_memory_bytes,
        }
    }

    // Helper methods for synchronous access in tests
    pub async fn async_session_count(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions.len()
    }

    pub async fn async_active_sessions(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions.values().filter(|s| !s.is_expired()).count()
    }
}

// Implement methods that need to work synchronously for compatibility
impl SessionStore {
    pub fn session_count_sync(&self) -> usize {
        // This would need a different approach in production
        // For now, return a placeholder
        0
    }

    pub fn active_sessions_sync(&self) -> usize {
        // This would need a different approach in production
        // For now, return a placeholder
        0
    }
}