use crate::job_processor::Message;
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    Active,
    Idle,
    Failed,
    Closed,
}

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
    pub id: String,
    pub config: SessionConfig,
    pub conversation_history: Vec<Message>,
    pub created_at: Instant,
    pub last_activity: Instant,
    pub total_memory_used: usize,
    pub state: SessionState,
    pub messages: Arc<RwLock<Vec<Message>>>,
    pub metadata: Arc<RwLock<HashMap<String, String>>>,
}

impl WebSocketSession {
    pub fn new(id: impl Into<String>) -> Self {
        Self::with_config(id, SessionConfig::default())
    }
    
    pub fn with_config(id: impl Into<String>, config: SessionConfig) -> Self {
        Self {
            id: id.into(),
            config,
            conversation_history: Vec::new(),
            created_at: Instant::now(),
            last_activity: Instant::now(),
            total_memory_used: 0,
            state: SessionState::Active,
            messages: Arc::new(RwLock::new(Vec::new())),
            metadata: Arc::new(RwLock::new(HashMap::new())),
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
        // Apply session's context window for backward compatibility
        let history_len = self.conversation_history.len();
        if history_len <= self.config.context_window_size {
            self.conversation_history.clone()
        } else {
            let start_idx = history_len - self.config.context_window_size;
            self.conversation_history[start_idx..].to_vec()
        }
    }
    
    pub fn get_all_messages(&self) -> Vec<Message> {
        // Return all messages for ContextManager to process
        self.conversation_history.clone()
    }

    pub fn clear(&mut self) {
        self.conversation_history.clear();
        self.total_memory_used = 0;
        self.last_activity = Instant::now();
    }

    pub fn is_expired(&self) -> bool {
        self.last_activity.elapsed().as_secs() > self.config.timeout_seconds
    }

    pub fn created_at_iso(&self) -> String {
        // Return ISO timestamp of creation time
        let elapsed = self.created_at.elapsed();
        let now = std::time::SystemTime::now();
        let created = now - elapsed;
        let datetime: chrono::DateTime<chrono::Utc> = created.into();
        datetime.to_rfc3339()
    }

    pub fn last_activity_iso(&self) -> String {
        // Return ISO timestamp of last activity
        let elapsed = self.last_activity.elapsed();
        let now = std::time::SystemTime::now();
        let last = now - elapsed;
        let datetime: chrono::DateTime<chrono::Utc> = last.into();
        datetime.to_rfc3339()
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

    pub fn get_metadata(&self) -> HashMap<String, serde_json::Value> {
        // Return empty metadata for now
        // In a real implementation, this would store session metadata
        HashMap::new()
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
    
    pub async fn add_message_async(&self, role: &str, content: &str) -> Result<()> {
        let message = Message {
            role: role.to_string(),
            content: content.to_string(),
            timestamp: Some(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs() as i64),
        };
        self.messages.write().await.push(message);
        Ok(())
    }
    
    pub async fn set_state(&self, state: SessionState) -> Result<()> {
        // Note: state field is not behind RwLock in current structure
        // This would need refactoring to make state mutable
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation() {
        let session = WebSocketSession::new("test-id");
        
        assert_eq!(session.id(), "test-id");
        assert_eq!(session.message_count(), 0);
    }

    #[test]
    fn test_add_message() {
        let mut session = WebSocketSession::new("test-id");
        
        let message = Message {
            role: "user".to_string(),
            content: "Hello".to_string(),
            timestamp: None,
        };
        
        session.add_message(message).unwrap();
        assert_eq!(session.message_count(), 1);
    }
}