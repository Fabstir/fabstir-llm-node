// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use crate::api::websocket::{
    memory_cache::{CacheManager, ConversationCache},
    messages::{ChainInfo, ConversationMessage, MessageValidator, SessionInitResponse},
};
use anyhow::{anyhow, Result};
use std::sync::Arc;
use tracing::{debug, info};

/// Handler for session initialization
pub struct SessionInitHandler {
    cache_manager: Arc<CacheManager>,
    message_validator: MessageValidator,
}

impl SessionInitHandler {
    /// Create a new session init handler
    pub fn new() -> Self {
        Self {
            cache_manager: Arc::new(CacheManager::new()),
            message_validator: MessageValidator::new(),
        }
    }

    /// Handle session initialization with optional context and chain
    pub async fn handle_session_init(
        &self,
        session_id: &str,
        job_id: u64,
        conversation_context: Vec<ConversationMessage>,
    ) -> Result<SessionInitResponse> {
        info!("Initializing session {} with job_id {}", session_id, job_id);

        self.handle_session_init_with_chain(session_id, job_id, conversation_context, None)
            .await
    }

    /// Handle session initialization with chain support
    pub async fn handle_session_init_with_chain(
        &self,
        session_id: &str,
        job_id: u64,
        conversation_context: Vec<ConversationMessage>,
        chain_id: Option<u64>,
    ) -> Result<SessionInitResponse> {
        info!(
            "Initializing session {} with job_id {} on chain {:?}",
            session_id, job_id, chain_id
        );

        // Validate job_id
        if job_id == 0 {
            return Err(anyhow!("Invalid job_id: cannot be 0"));
        }

        // Validate chain if specified
        let chain_info = if let Some(chain) = chain_id {
            if !self.message_validator.is_chain_supported(chain) {
                return Err(anyhow!("Unsupported chain ID: {}", chain));
            }
            // Create chain info based on chain ID
            Some(self.get_chain_info(chain))
        } else {
            None
        };

        // Create or replace cache for this session
        let cache = self
            .cache_manager
            .create_cache(session_id.to_string(), job_id)
            .await;

        // Initialize with provided context
        if !conversation_context.is_empty() {
            cache
                .initialize_with_context(conversation_context.clone())
                .await?;
            debug!(
                "Initialized cache with {} messages",
                conversation_context.len()
            );
        }

        // Calculate total tokens
        let total_tokens = conversation_context
            .iter()
            .map(|m| m.tokens.unwrap_or(0))
            .sum();

        Ok(SessionInitResponse {
            session_id: session_id.to_string(),
            job_id,
            message_count: cache.message_count().await,
            total_tokens,
            chain_info,
        })
    }

    /// Get cache for a session
    pub async fn get_cache(&self, session_id: &str) -> Result<ConversationCache> {
        self.cache_manager
            .get_cache(session_id)
            .await
            .ok_or_else(|| anyhow!("Session not found: {}", session_id))
    }

    /// Check if session exists
    pub async fn session_exists(&self, session_id: &str) -> bool {
        self.cache_manager.get_cache(session_id).await.is_some()
    }

    /// End session (alias for cleanup_session)
    pub async fn end_session(&self, session_id: &str) -> Result<()> {
        self.cleanup_session(session_id).await;
        Ok(())
    }

    /// Clean up session (called on session end)
    pub async fn cleanup_session(&self, session_id: &str) {
        if let Some(cache) = self.cache_manager.remove_cache(session_id).await {
            cache.clear().await;
            info!("Cleaned up session {}", session_id);
        }
    }

    /// Get number of active sessions
    pub async fn active_session_count(&self) -> usize {
        self.cache_manager.cache_count().await
    }

    /// Clean up old sessions
    pub async fn cleanup_old_sessions(&self) {
        self.cache_manager.cleanup_old_caches().await;
    }

    /// Get chain information for a chain ID
    fn get_chain_info(&self, chain_id: u64) -> ChainInfo {
        match chain_id {
            84532 => ChainInfo {
                chain_id,
                chain_name: "Base Sepolia".to_string(),
                native_token: "ETH".to_string(),
                rpc_url: "https://sepolia.base.org".to_string(),
            },
            5611 => ChainInfo {
                chain_id,
                chain_name: "opBNB Testnet".to_string(),
                native_token: "BNB".to_string(),
                rpc_url: "https://opbnb-testnet-rpc.bnbchain.org".to_string(),
            },
            _ => ChainInfo {
                chain_id,
                chain_name: "Unknown".to_string(),
                native_token: "UNKNOWN".to_string(),
                rpc_url: String::new(),
            },
        }
    }
}

impl Default for SessionInitHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_init() {
        let handler = SessionInitHandler::new();

        let context = vec![ConversationMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
            timestamp: None,
            tokens: Some(2),
            proof: None,
        }];

        let result = handler
            .handle_session_init("test-session", 123, context)
            .await
            .unwrap();

        assert_eq!(result.session_id, "test-session");
        assert_eq!(result.job_id, 123);
        assert_eq!(result.message_count, 1);
        assert_eq!(result.total_tokens, 2);
    }

    #[tokio::test]
    async fn test_session_cleanup() {
        let handler = SessionInitHandler::new();

        handler
            .handle_session_init("temp-session", 456, vec![])
            .await
            .unwrap();

        assert!(handler.session_exists("temp-session").await);

        handler.cleanup_session("temp-session").await;

        assert!(!handler.session_exists("temp-session").await);
    }
}
