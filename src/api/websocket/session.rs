use crate::job_processor::Message;
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub max_memory_bytes: usize,
    pub context_window_size: usize,
    pub timeout_seconds: u64,
    pub enable_compression: bool,
    pub enable_persistence: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_memory_bytes: 10 * 1024 * 1024, // 10MB
            context_window_size: 20,
            timeout_seconds: 1800, // 30 minutes
            enable_compression: true,
            enable_persistence: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetrics {
    pub total_messages: usize,
    pub user_messages: usize,
    pub assistant_messages: usize,
    pub system_messages: usize,
    pub total_tokens: usize,
    pub memory_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct WebSocketSession {
    id: String,
    config: SessionConfig,
    conversation_history: Vec<Message>,
    created_at: Instant,
    last_activity: Instant,
    total_memory_used: usize,
}

impl WebSocketSession {
    pub fn new(id: String, config: SessionConfig) -> Self {
        Self {
            id,
            config,
            conversation_history: Vec::new(),
            created_at: Instant::now(),
            last_activity: Instant::now(),
            total_memory_used: 0,
        }
    }

    pub fn generate_id() -> String {
        Uuid::new_v4().to_string()
    }

    pub fn id(&self) -> &String {
        &self.id
    }

    pub fn message_count(&self) -> usize {
        self.conversation_history.len()
    }

    pub fn conversation_history(&self) -> &[Message] {
        &self.conversation_history
    }

    pub fn created_at(&self) -> Instant {
        self.created_at
    }

    pub fn last_activity(&self) -> Instant {
        self.last_activity
    }

    pub fn add_message(&mut self, message: Message) -> Result<()> {
        // Calculate memory size of the new message
        let message_size = Self::calculate_message_size(&message);
        
        // Check if adding this message would exceed memory limit
        if self.total_memory_used + message_size > self.config.max_memory_bytes {
            return Err(anyhow!("Session memory limit exceeded"));
        }

        // Add message and update metrics
        self.conversation_history.push(message);
        self.total_memory_used += message_size;
        self.last_activity = Instant::now();
        
        Ok(())
    }

    pub fn get_context_messages(&self) -> Vec<Message> {
        let history_len = self.conversation_history.len();
        if history_len <= self.config.context_window_size {
            self.conversation_history.clone()
        } else {
            let start_idx = history_len - self.config.context_window_size;
            self.conversation_history[start_idx..].to_vec()
        }
    }

    pub fn clear(&mut self) {
        self.conversation_history.clear();
        self.total_memory_used = 0;
        self.last_activity = Instant::now();
    }

    pub fn is_expired(&self) -> bool {
        self.last_activity.elapsed().as_secs() > self.config.timeout_seconds
    }

    pub fn total_tokens(&self) -> usize {
        // Rough estimate: ~1 token per 4 characters
        self.conversation_history
            .iter()
            .map(|msg| (msg.content.len() + msg.role.len()) / 4)
            .sum()
    }

    pub fn memory_used(&self) -> usize {
        self.total_memory_used
    }

    pub fn metrics(&self) -> SessionMetrics {
        let mut user_messages = 0;
        let mut assistant_messages = 0;
        let mut system_messages = 0;

        for msg in &self.conversation_history {
            match msg.role.as_str() {
                "user" => user_messages += 1,
                "assistant" => assistant_messages += 1,
                "system" => system_messages += 1,
                _ => {}
            }
        }

        SessionMetrics {
            total_messages: self.conversation_history.len(),
            user_messages,
            assistant_messages,
            system_messages,
            total_tokens: self.total_tokens(),
            memory_bytes: self.total_memory_used,
        }
    }

    fn calculate_message_size(message: &Message) -> usize {
        // Calculate approximate memory size
        std::mem::size_of::<Message>() 
            + message.role.len() 
            + message.content.len()
            + if message.timestamp.is_some() { 8 } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation() {
        let session = WebSocketSession::new(
            "test-id".to_string(),
            SessionConfig::default()
        );
        
        assert_eq!(session.id(), "test-id");
        assert_eq!(session.message_count(), 0);
    }

    #[test]
    fn test_add_message() {
        let mut session = WebSocketSession::new(
            "test-id".to_string(),
            SessionConfig::default()
        );
        
        let message = Message {
            role: "user".to_string(),
            content: "Hello".to_string(),
            timestamp: None,
        };
        
        session.add_message(message).unwrap();
        assert_eq!(session.message_count(), 1);
    }
}