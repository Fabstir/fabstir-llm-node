use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

use super::session::WebSocketSession;

/// Manages WebSocket sessions across the application
#[derive(Clone)]
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, WebSocketSession>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new session
    pub async fn register_session(&self, session: WebSocketSession) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        
        if sessions.contains_key(&session.id) {
            return Err(anyhow!("Session {} already exists", session.id));
        }
        
        info!("Registering session: {}", session.id);
        sessions.insert(session.id.clone(), session);
        Ok(())
    }

    /// Get a session by ID
    pub async fn get_session(&self, session_id: &str) -> Option<WebSocketSession> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// Check if a session exists
    pub async fn has_session(&self, session_id: &str) -> bool {
        self.sessions.read().await.contains_key(session_id)
    }

    /// Remove a session
    pub async fn remove_session(&self, session_id: &str) {
        let mut sessions = self.sessions.write().await;
        if sessions.remove(session_id).is_some() {
            info!("Removed session: {}", session_id);
        }
    }

    /// Get all active sessions
    pub async fn get_active_sessions(&self) -> Vec<WebSocketSession> {
        self.sessions.read().await.values().cloned().collect()
    }

    /// Get the number of active sessions
    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    /// Clear all sessions
    pub async fn clear_all(&self) {
        let mut sessions = self.sessions.write().await;
        let count = sessions.len();
        sessions.clear();
        info!("Cleared {} sessions", count);
    }

    /// Update a session
    pub async fn update_session(&self, session: WebSocketSession) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        
        if !sessions.contains_key(&session.id) {
            return Err(anyhow!("Session {} not found", session.id));
        }
        
        let session_id = session.id.clone();
        sessions.insert(session_id.clone(), session);
        debug!("Updated session: {}", session_id);
        Ok(())
    }

    /// Get sessions by filter
    pub async fn get_sessions_by<F>(&self, filter: F) -> Vec<WebSocketSession>
    where
        F: Fn(&WebSocketSession) -> bool,
    {
        self.sessions
            .read()
            .await
            .values()
            .filter(|s| filter(s))
            .cloned()
            .collect()
    }
}