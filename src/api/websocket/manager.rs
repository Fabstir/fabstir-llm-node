use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

use super::session::{SessionConfig, WebSocketSession};
use crate::config::chains::ChainRegistry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainStatistics {
    pub total_sessions: usize,
    pub sessions_by_chain: HashMap<u64, usize>,
    pub unique_chains: usize,
}

impl ChainStatistics {
    pub fn get_chain_percentage(&self, chain_id: u64) -> f64 {
        if self.total_sessions == 0 {
            return 0.0;
        }

        let chain_count = self.sessions_by_chain.get(&chain_id).unwrap_or(&0);
        (*chain_count as f64 / self.total_sessions as f64) * 100.0
    }
}

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

    // Chain-aware methods

    /// Create a new session with specific chain
    pub async fn create_session(
        &self,
        session_id: impl Into<String>,
        config: SessionConfig,
        chain_id: u64,
    ) -> Result<()> {
        let session = WebSocketSession::with_chain(session_id, config, chain_id);
        self.register_session(session).await
    }

    /// Create a session with chain validation
    pub async fn create_session_validated(
        &self,
        session_id: impl Into<String>,
        config: SessionConfig,
        chain_id: u64,
        registry: &ChainRegistry,
    ) -> Result<()> {
        let session =
            WebSocketSession::with_validated_chain(session_id, config, chain_id, registry)?;
        self.register_session(session).await
    }

    /// Get the chain ID for a session
    pub async fn get_session_chain(&self, session_id: &str) -> Option<u64> {
        self.get_session(session_id).await.map(|s| s.chain_id)
    }

    /// List all sessions on a specific chain
    pub async fn list_sessions_by_chain(&self, chain_id: u64) -> Vec<WebSocketSession> {
        self.sessions
            .read()
            .await
            .values()
            .filter(|s| s.chain_id == chain_id)
            .cloned()
            .collect()
    }

    /// Get sessions by multiple chains
    pub async fn get_sessions_by_chains(&self, chain_ids: &[u64]) -> Vec<WebSocketSession> {
        self.sessions
            .read()
            .await
            .values()
            .filter(|s| chain_ids.contains(&s.chain_id))
            .cloned()
            .collect()
    }

    /// Get chain statistics
    pub async fn get_chain_statistics(&self) -> ChainStatistics {
        let sessions = self.sessions.read().await;
        let mut sessions_by_chain: HashMap<u64, usize> = HashMap::new();

        for session in sessions.values() {
            *sessions_by_chain.entry(session.chain_id).or_insert(0) += 1;
        }

        ChainStatistics {
            total_sessions: sessions.len(),
            unique_chains: sessions_by_chain.len(),
            sessions_by_chain,
        }
    }

    /// Migrate a session to a specific chain
    pub async fn migrate_session_to_chain(
        &self,
        session_id: &str,
        new_chain_id: u64,
        registry: &ChainRegistry,
    ) -> Result<()> {
        let mut sessions = self.sessions.write().await;

        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| anyhow!("Session {} not found", session_id))?;

        session.switch_chain(new_chain_id, registry)?;
        info!("Migrated session {} to chain {}", session_id, new_chain_id);
        Ok(())
    }

    /// Migrate all sessions to a specific chain
    pub async fn migrate_all_sessions_to_chain(&self, new_chain_id: u64) -> Result<usize> {
        let mut sessions = self.sessions.write().await;
        let registry = ChainRegistry::new();
        let mut migrated_count = 0;

        for session in sessions.values_mut() {
            if session.chain_id != new_chain_id {
                // Try to migrate, but don't fail the whole operation if one fails
                if session.switch_chain(new_chain_id, &registry).is_ok() {
                    migrated_count += 1;
                }
            }
        }

        info!(
            "Migrated {} sessions to chain {}",
            migrated_count, new_chain_id
        );
        Ok(migrated_count)
    }
}
