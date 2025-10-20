// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use crate::api::websocket::{
    handlers::session_init::SessionInitHandler,
    messages::{ConversationMessage, PromptResponse},
};
use anyhow::{anyhow, Result};
use std::sync::Arc;
use tracing::{debug, info};

/// Handler for prompt messages
pub struct PromptHandler {
    session_handler: Arc<SessionInitHandler>,
}

impl PromptHandler {
    /// Create a new prompt handler
    pub fn new(session_handler: Arc<SessionInitHandler>) -> Self {
        Self { session_handler }
    }

    /// Handle a new prompt from the user
    pub async fn handle_prompt(
        &self,
        session_id: &str,
        content: &str,
        message_index: u32,
    ) -> Result<PromptResponse> {
        info!(
            "Handling prompt for session {} at index {}",
            session_id, message_index
        );

        // Validate prompt content
        if content.is_empty() {
            return Err(anyhow!("Empty prompt content"));
        }

        // Get the session cache
        let cache = self.session_handler.get_cache(session_id).await?;

        // Validate message index
        let current_count = cache.message_count().await;
        let expected_index = current_count as u32 + 1;

        if message_index != expected_index && message_index != 0 {
            return Err(anyhow!(
                "Invalid message index: expected {} but got {}",
                expected_index,
                message_index
            ));
        }

        // Add the prompt to the cache
        let message = ConversationMessage {
            role: "user".to_string(),
            content: content.to_string(),
            timestamp: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            ),
            tokens: None, // Will be counted later if needed
            proof: None,
        };

        cache.add_message(message).await;
        debug!("Added prompt to cache for session {}", session_id);

        Ok(PromptResponse {
            session_id: session_id.to_string(),
            message_index: if message_index == 0 {
                expected_index
            } else {
                message_index
            },
            added_to_cache: true,
        })
    }

    /// Check if session exists
    pub async fn session_exists(&self, session_id: &str) -> bool {
        self.session_handler.session_exists(session_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_prompt_handling() {
        let session_handler = Arc::new(SessionInitHandler::new());
        let prompt_handler = PromptHandler::new(session_handler.clone());

        // Initialize session first
        session_handler
            .handle_session_init("test-prompt", 123, vec![])
            .await
            .unwrap();

        // Handle prompt
        let result = prompt_handler
            .handle_prompt("test-prompt", "Hello AI", 1)
            .await
            .unwrap();

        assert_eq!(result.session_id, "test-prompt");
        assert_eq!(result.message_index, 1);
        assert!(result.added_to_cache);
    }

    #[tokio::test]
    async fn test_prompt_without_session() {
        let session_handler = Arc::new(SessionInitHandler::new());
        let prompt_handler = PromptHandler::new(session_handler);

        let result = prompt_handler.handle_prompt("no-session", "Hello", 1).await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Session not found"));
    }

    #[tokio::test]
    async fn test_empty_prompt() {
        let session_handler = Arc::new(SessionInitHandler::new());
        let prompt_handler = PromptHandler::new(session_handler.clone());

        session_handler
            .handle_session_init("empty-prompt-session", 456, vec![])
            .await
            .unwrap();

        let result = prompt_handler
            .handle_prompt("empty-prompt-session", "", 1)
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Empty prompt"));
    }
}
