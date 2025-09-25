use crate::api::websocket::{
    handlers::session_init::SessionInitHandler,
    memory_cache::CacheManager,
    messages::{ConversationMessage, SessionResumeResponse},
};
use anyhow::{anyhow, Result};
use std::sync::Arc;
use tracing::{debug, info};

/// Handler for session resume operations
pub struct SessionResumeHandler {
    cache_manager: Arc<CacheManager>,
}

impl SessionResumeHandler {
    /// Create a new session resume handler
    pub fn new() -> Self {
        Self {
            cache_manager: Arc::new(CacheManager::new()),
        }
    }
    
    /// Handle session resume with full context
    pub async fn handle_session_resume(
        &self,
        session_id: &str,
        job_id: u64,
        conversation_context: Vec<ConversationMessage>,
        last_message_index: u32,
    ) -> Result<SessionResumeResponse> {
        info!(
            "Resuming session {} with job_id {} and {} messages",
            session_id,
            job_id,
            conversation_context.len()
        );
        
        // Validate job_id
        if job_id == 0 {
            return Err(anyhow!("Invalid job_id: cannot be 0"));
        }
        
        // Validate message index matches context length
        if last_message_index as usize != conversation_context.len() && last_message_index != 0 {
            return Err(anyhow!(
                "Message index mismatch: expected {} but got {}",
                conversation_context.len(),
                last_message_index
            ));
        }
        
        // Create or replace cache for this session
        let cache = self.cache_manager.create_cache(session_id.to_string(), job_id).await;
        
        // Initialize with provided context
        cache.initialize_with_context(conversation_context.clone()).await?;
        debug!("Rebuilt cache with {} messages", conversation_context.len());
        
        // Calculate total tokens
        let total_tokens = conversation_context
            .iter()
            .map(|m| m.tokens.unwrap_or(0))
            .sum();
        
        Ok(SessionResumeResponse {
            session_id: session_id.to_string(),
            job_id,
            message_count: cache.message_count().await,
            total_tokens,
            last_message_index: if last_message_index == 0 {
                conversation_context.len() as u32
            } else {
                last_message_index
            },
            resumed_successfully: true,
            chain_info: None, // Add chain info support later if needed
        })
    }
    
    /// Get cache for a session
    pub async fn get_cache(&self, session_id: &str) -> Result<crate::api::websocket::memory_cache::ConversationCache> {
        self.cache_manager
            .get_cache(session_id)
            .await
            .ok_or_else(|| anyhow!("Session not found: {}", session_id))
    }
    
    /// Check if session exists
    pub async fn session_exists(&self, session_id: &str) -> bool {
        self.cache_manager.get_cache(session_id).await.is_some()
    }
}

impl Default for SessionResumeHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_resume() {
        let handler = SessionResumeHandler::new();
        
        let context = vec![
            ConversationMessage {
                role: "user".to_string(),
                content: "Question 1".to_string(),
                timestamp: None,
                tokens: Some(3),
                proof: None,
            },
            ConversationMessage {
                role: "assistant".to_string(),
                content: "Answer 1".to_string(),
                timestamp: None,
                tokens: Some(5),
                proof: None,
            },
        ];
        
        let result = handler
            .handle_session_resume("resumed-session", 789, context.clone(), 2)
            .await
            .unwrap();
        
        assert_eq!(result.session_id, "resumed-session");
        assert_eq!(result.job_id, 789);
        assert_eq!(result.message_count, 2);
        assert_eq!(result.total_tokens, 8);
        assert_eq!(result.last_message_index, 2);
        assert!(result.resumed_successfully);
    }
    
    #[tokio::test]
    async fn test_resume_with_index_mismatch() {
        let handler = SessionResumeHandler::new();
        
        let context = vec![
            ConversationMessage {
                role: "user".to_string(),
                content: "Test".to_string(),
                timestamp: None,
                tokens: None,
                proof: None,
            },
        ];
        
        let result = handler
            .handle_session_resume("bad-session", 100, context, 5)
            .await;
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("index mismatch"));
    }
}