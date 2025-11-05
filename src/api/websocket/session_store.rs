// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use super::persistence::{PersistenceConfig, SessionPersistence};
use super::session::{SessionConfig, SessionMetrics, WebSocketSession};
use crate::job_processor::Message;
use anyhow::{anyhow, Result};
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
    persistence: Option<Arc<SessionPersistence>>,
}

impl SessionStore {
    pub fn new(config: SessionStoreConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            persistence: None,
        }
    }

    pub fn with_persistence(
        config: SessionStoreConfig,
        persistence_config: PersistenceConfig,
    ) -> Self {
        let persistence = SessionPersistence::new(persistence_config);
        Self {
            config,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            persistence: Some(Arc::new(persistence)),
        }
    }

    /// Recover sessions from persistence on startup
    pub async fn recover_sessions(&mut self) -> Result<usize> {
        if let Some(persistence) = &self.persistence {
            let recovered = persistence.recover_all_sessions().await?;
            let count = recovered.len();

            let mut sessions = self.sessions.write().await;
            for session in recovered {
                sessions.insert(session.id.clone(), session);
            }

            Ok(count)
        } else {
            Ok(0)
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

    pub async fn create_session_with_id(
        &mut self,
        session_id: String,
        config: SessionConfig,
    ) -> Result<()> {
        let sessions_count = self.sessions.read().await.len();
        if sessions_count >= self.config.max_sessions {
            return Err(anyhow!("Maximum sessions limit reached"));
        }

        let session = WebSocketSession::with_config(session_id.clone(), config);
        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id, session);
        Ok(())
    }

    pub async fn create_session(&mut self, config: SessionConfig) -> String {
        let session_id = WebSocketSession::generate_id();
        let session = WebSocketSession::with_config(session_id.clone(), config);

        // Persist if enabled
        if let Some(persistence) = &self.persistence {
            let _ = persistence.save_session(&session).await;
        }

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
        let session = WebSocketSession::with_config(session_id.clone(), config);

        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id.clone(), session);

        Ok(session_id)
    }

    /// Create a session with a specific chain ID
    pub async fn create_session_with_chain(
        &mut self,
        session_id: String,
        config: SessionConfig,
        chain_id: u64,
    ) -> Result<()> {
        let sessions_count = self.sessions.read().await.len();
        if sessions_count >= self.config.max_sessions {
            return Err(anyhow!("Maximum sessions limit reached"));
        }

        let session = WebSocketSession::with_chain(session_id.clone(), config, chain_id);

        // Persist if enabled
        if let Some(persistence) = &self.persistence {
            let _ = persistence.save_session(&session).await;
        }

        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id, session);
        Ok(())
    }

    pub async fn get_session(&self, session_id: &str) -> Option<WebSocketSession> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    pub async fn get_session_mut(&self, session_id: &str) -> Option<WebSocketSession> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    pub async fn update_session(&mut self, session_id: &str, message: Message) -> Result<()> {
        let mut sessions = self.sessions.write().await;

        match sessions.get_mut(session_id) {
            Some(session) => {
                session.add_message(message)?;

                // Persist if enabled
                if let Some(persistence) = &self.persistence {
                    let _ = persistence.save_session(session).await;
                }

                Ok(())
            }
            None => Err(anyhow!("Session not found")),
        }
    }

    pub async fn destroy_session(&mut self, session_id: &str) -> bool {
        let mut sessions = self.sessions.write().await;

        if let Some(removed_session) = sessions.remove(session_id) {
            // Delete from persistence if enabled
            if let Some(persistence) = &self.persistence {
                let _ = persistence
                    .delete_session(removed_session.chain_id, session_id)
                    .await;
            }
            true
        } else {
            false
        }
    }

    pub async fn session_exists(&self, session_id: &str) -> bool {
        let sessions = self.sessions.read().await;
        sessions.contains_key(session_id)
    }

    pub async fn remove_session(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
        Ok(())
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

    /// Get or create a session and enable RAG with specified max vectors
    ///
    /// This is a convenience method for RAG functionality that:
    /// 1. Creates session if it doesn't exist
    /// 2. Enables RAG on the session if not already enabled
    /// 3. Returns a clone of the session with RAG enabled
    ///
    /// The session remains in the store with RAG enabled, and the returned
    /// clone shares the same vector store Arc.
    pub async fn get_or_create_rag_session(
        &mut self,
        session_id: String,
        max_vectors: usize,
    ) -> Result<crate::api::websocket::session::WebSocketSession> {
        let mut sessions = self.sessions.write().await;

        // Create if doesn't exist
        if !sessions.contains_key(&session_id) {
            let sess = crate::api::websocket::session::WebSocketSession::new(session_id.clone());
            sessions.insert(session_id.clone(), sess);
        }

        // Get mutable reference and enable RAG
        let session = sessions.get_mut(&session_id).ok_or_else(|| anyhow!("Session not found"))?;

        if session.get_vector_store().is_none() {
            session.enable_rag(max_vectors);
        }

        // Return a clone (Arc is shallow-copied, so vector store is shared)
        Ok(session.clone())
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
