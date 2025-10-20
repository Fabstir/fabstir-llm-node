// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use super::messages::ConversationMessage;
use anyhow::{anyhow, Result};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Maximum context window in tokens
pub const MAX_CONTEXT_TOKENS: usize = 4096;
const DEFAULT_MAX_CONTEXT_TOKENS: usize = MAX_CONTEXT_TOKENS;

/// In-memory conversation cache (stateless, cleared on disconnect)
#[derive(Debug, Clone)]
pub struct ConversationCache {
    session_id: String,
    messages: Arc<RwLock<VecDeque<ConversationMessage>>>,
    total_tokens: Arc<RwLock<usize>>,
    max_context_tokens: usize,
    created_at: Instant,
    job_id: Arc<RwLock<u64>>,
}

impl ConversationCache {
    /// Create a new conversation cache
    pub fn new(session_id: String, job_id: u64) -> Self {
        Self {
            session_id,
            messages: Arc::new(RwLock::new(VecDeque::new())),
            total_tokens: Arc::new(RwLock::new(0)),
            max_context_tokens: DEFAULT_MAX_CONTEXT_TOKENS,
            created_at: Instant::now(),
            job_id: Arc::new(RwLock::new(job_id)),
        }
    }

    /// Create cache with custom token limit
    pub fn with_token_limit(session_id: String, job_id: u64, max_tokens: usize) -> Self {
        let mut cache = Self::new(session_id, job_id);
        cache.max_context_tokens = max_tokens;
        cache
    }

    /// Initialize cache with conversation context
    pub async fn initialize_with_context(&self, context: Vec<ConversationMessage>) -> Result<()> {
        let mut messages = self.messages.write().await;
        let mut total_tokens = self.total_tokens.write().await;

        // Clear existing messages
        messages.clear();
        *total_tokens = 0;

        // Add all context messages
        for mut msg in context {
            // Estimate tokens if not provided (roughly 4 chars per token)
            let tokens = msg
                .tokens
                .unwrap_or_else(|| (msg.content.len() / 4).max(1) as u32)
                as usize;
            *total_tokens += tokens;

            // Store with estimated tokens if needed
            if msg.tokens.is_none() {
                msg.tokens = Some(tokens as u32);
            }
            messages.push_back(msg);
        }

        // Trim if exceeds token limit
        self.trim_to_token_limit(&mut messages, &mut total_tokens)
            .await;

        Ok(())
    }

    /// Add a message to the cache
    pub async fn add_message(&self, message: ConversationMessage) {
        let mut messages = self.messages.write().await;
        let mut total_tokens = self.total_tokens.write().await;

        // Estimate tokens if not provided (roughly 4 chars per token)
        let tokens = message
            .tokens
            .unwrap_or_else(|| (message.content.len() / 4).max(1) as u32)
            as usize;
        *total_tokens += tokens;

        // Store message with estimated tokens if needed
        let mut msg = message;
        if msg.tokens.is_none() {
            msg.tokens = Some(tokens as u32);
        }
        messages.push_back(msg);

        // Trim if exceeds token limit
        self.trim_to_token_limit(&mut messages, &mut total_tokens)
            .await;
    }

    /// Get all messages (for building prompt)
    pub async fn get_messages(&self) -> Vec<ConversationMessage> {
        let messages = self.messages.read().await;
        messages.iter().cloned().collect()
    }

    /// Get message count
    pub async fn message_count(&self) -> usize {
        let messages = self.messages.read().await;
        messages.len()
    }

    /// Get total tokens
    pub async fn get_total_tokens(&self) -> usize {
        *self.total_tokens.read().await
    }

    /// Check if within token limit
    pub async fn is_within_token_limit(&self) -> bool {
        *self.total_tokens.read().await <= self.max_context_tokens
    }

    /// Clear the cache (called on session end)
    pub async fn clear(&self) {
        let mut messages = self.messages.write().await;
        let mut total_tokens = self.total_tokens.write().await;

        messages.clear();
        *total_tokens = 0;
    }

    /// Get session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get job ID
    pub async fn job_id(&self) -> u64 {
        *self.job_id.read().await
    }

    /// Update job ID (for session resume with new job)
    pub async fn update_job_id(&self, job_id: u64) {
        *self.job_id.write().await = job_id;
    }

    /// Get age of cache
    pub fn age(&self) -> std::time::Duration {
        self.created_at.elapsed()
    }

    /// Get messages sorted by timestamp
    pub async fn get_messages_sorted(&self) -> Vec<ConversationMessage> {
        let messages = self.messages.read().await;
        let mut sorted: Vec<_> = messages.iter().cloned().collect();
        sorted.sort_by_key(|m| m.timestamp.unwrap_or(0));
        sorted
    }

    /// Trim messages to stay within token limit
    async fn trim_to_token_limit(
        &self,
        messages: &mut VecDeque<ConversationMessage>,
        total_tokens: &mut usize,
    ) {
        // Keep system message if present
        let has_system = messages
            .front()
            .map(|m| m.role == "system")
            .unwrap_or(false);

        while *total_tokens > self.max_context_tokens && messages.len() > 1 {
            // Skip system message if it's first
            if has_system && messages.len() <= 2 {
                break;
            }

            // Remove oldest non-system message
            let start_idx = if has_system { 1 } else { 0 };
            if let Some(removed) = messages.remove(start_idx) {
                *total_tokens -= removed.tokens.unwrap_or(0) as usize;
            }
        }
    }
}

/// Cache manager for all sessions
pub struct CacheManager {
    caches: Arc<RwLock<std::collections::HashMap<String, ConversationCache>>>,
}

impl CacheManager {
    /// Create a new cache manager
    pub fn new() -> Self {
        Self {
            caches: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Create or update cache for a session
    pub async fn create_cache(&self, session_id: String, job_id: u64) -> ConversationCache {
        let cache = ConversationCache::new(session_id.clone(), job_id);
        let mut caches = self.caches.write().await;
        caches.insert(session_id, cache.clone());
        cache
    }

    /// Get cache for a session
    pub async fn get_cache(&self, session_id: &str) -> Option<ConversationCache> {
        let caches = self.caches.read().await;
        caches.get(session_id).cloned()
    }

    /// Remove cache for a session
    pub async fn remove_cache(&self, session_id: &str) -> Option<ConversationCache> {
        let mut caches = self.caches.write().await;
        caches.remove(session_id)
    }

    /// Clear all caches
    pub async fn clear_all(&self) {
        let mut caches = self.caches.write().await;
        caches.clear();
    }

    /// Get number of active caches
    pub async fn cache_count(&self) -> usize {
        let caches = self.caches.read().await;
        caches.len()
    }

    /// Clean up old caches (> 1 hour)
    pub async fn cleanup_old_caches(&self) {
        let mut caches = self.caches.write().await;
        let cutoff = std::time::Duration::from_secs(3600);

        caches.retain(|_, cache| cache.age() < cutoff);
    }
}

impl Default for CacheManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_creation() {
        let cache = ConversationCache::new("test".to_string(), 123);
        assert_eq!(cache.session_id(), "test");
        assert_eq!(cache.job_id().await, 123);
        assert_eq!(cache.message_count().await, 0);
    }

    #[tokio::test]
    async fn test_cache_add_message() {
        let cache = ConversationCache::new("test".to_string(), 123);

        cache
            .add_message(ConversationMessage {
                role: "user".to_string(),
                content: "Hello".to_string(),
                timestamp: None,
                tokens: Some(2),
                proof: None,
            })
            .await;

        assert_eq!(cache.message_count().await, 1);
        assert_eq!(cache.get_total_tokens().await, 2);
    }

    #[tokio::test]
    async fn test_cache_token_limit() {
        let cache = ConversationCache::with_token_limit("test".to_string(), 123, 10);

        // Add messages that exceed limit
        for i in 0..5 {
            cache
                .add_message(ConversationMessage {
                    role: "user".to_string(),
                    content: format!("Message {}", i),
                    timestamp: None,
                    tokens: Some(3),
                    proof: None,
                })
                .await;
        }

        // Should have trimmed old messages
        assert!(cache.get_total_tokens().await <= 10);
    }
}
