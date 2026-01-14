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

    /// Handle session initialization with chain support (backward compatible)
    pub async fn handle_session_init_with_chain(
        &self,
        session_id: &str,
        job_id: u64,
        conversation_context: Vec<ConversationMessage>,
        chain_id: Option<u64>,
    ) -> Result<SessionInitResponse> {
        self.handle_session_init_with_recovery_key(
            session_id,
            job_id,
            conversation_context,
            chain_id,
            None,
        )
        .await
    }

    /// Handle session initialization with chain and recovery public key support
    ///
    /// # Arguments
    /// * `session_id` - Unique session identifier
    /// * `job_id` - Job ID from blockchain
    /// * `conversation_context` - Optional conversation context to restore
    /// * `chain_id` - Optional chain ID for multi-chain support
    /// * `recovery_public_key` - Optional recovery public key for encrypted checkpoints (SDK v1.8.7+)
    pub async fn handle_session_init_with_recovery_key(
        &self,
        session_id: &str,
        job_id: u64,
        conversation_context: Vec<ConversationMessage>,
        chain_id: Option<u64>,
        recovery_public_key: Option<String>,
    ) -> Result<SessionInitResponse> {
        info!(
            "Initializing session {} with job_id {} on chain {:?}{}",
            session_id,
            job_id,
            chain_id,
            if recovery_public_key.is_some() {
                " (encrypted checkpoints enabled)"
            } else {
                ""
            }
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
            recovery_public_key,
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

    // Phase 9 Tests: Recovery Public Key Support

    #[tokio::test]
    async fn test_session_init_with_recovery_public_key() {
        let handler = SessionInitHandler::new();
        let recovery_key = "0x02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";

        let result = handler
            .handle_session_init_with_recovery_key(
                "recovery-session",
                789,
                vec![],
                Some(84532),
                Some(recovery_key.to_string()),
            )
            .await
            .unwrap();

        assert_eq!(result.session_id, "recovery-session");
        assert_eq!(result.job_id, 789);
        assert_eq!(result.recovery_public_key, Some(recovery_key.to_string()));
    }

    #[tokio::test]
    async fn test_session_init_without_recovery_key_is_backwards_compatible() {
        let handler = SessionInitHandler::new();

        let result = handler
            .handle_session_init_with_recovery_key(
                "no-recovery-session",
                101,
                vec![],
                Some(84532),
                None,
            )
            .await
            .unwrap();

        assert_eq!(result.session_id, "no-recovery-session");
        assert!(result.recovery_public_key.is_none());
    }
}
