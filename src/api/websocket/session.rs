// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use crate::config::chains::ChainRegistry;
use crate::job_processor::Message;
use anyhow::{anyhow, Result};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionChainInfo {
    pub chain_id: u64,
    pub chain_name: String,
    pub native_token: String,
    pub native_token_decimals: u8,
}

impl SessionChainInfo {
    pub fn from_chain_id(chain_id: u64) -> Self {
        match chain_id {
            84532 => Self {
                chain_id,
                chain_name: "Base Sepolia".to_string(),
                native_token: "ETH".to_string(),
                native_token_decimals: 18,
            },
            5611 => Self {
                chain_id,
                chain_name: "opBNB Testnet".to_string(),
                native_token: "BNB".to_string(),
                native_token_decimals: 18,
            },
            _ => Self {
                chain_id,
                chain_name: format!("Unknown Chain {}", chain_id),
                native_token: "UNKNOWN".to_string(),
                native_token_decimals: 18,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct WebSocketSession {
    pub id: String,
    pub chain_id: u64,
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
        Self::with_chain(id, config, 84532) // Default to Base Sepolia
    }

    pub fn with_chain(id: impl Into<String>, config: SessionConfig, chain_id: u64) -> Self {
        Self {
            id: id.into(),
            chain_id,
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

    pub fn with_validated_chain(
        id: impl Into<String>,
        config: SessionConfig,
        chain_id: u64,
        registry: &ChainRegistry,
    ) -> Result<Self> {
        if !registry.is_chain_supported(chain_id) {
            return Err(anyhow!("Unsupported chain ID: {}", chain_id));
        }

        Ok(Self::with_chain(id, config, chain_id))
    }

    pub fn with_default_chain(id: impl Into<String>, config: SessionConfig) -> Self {
        let registry = ChainRegistry::new();
        Self::with_chain(id, config, registry.default_chain())
    }

    pub fn migrate_to_chain_aware(mut legacy_session: Self) -> Self {
        // If session doesn't have a valid chain_id (0 or uninitialized), set to default
        if legacy_session.chain_id == 0 {
            legacy_session.chain_id = 84532; // Default to Base Sepolia
        }
        legacy_session
    }

    pub fn get_chain_id(&self) -> u64 {
        self.chain_id
    }

    pub fn get_chain_info(&self) -> SessionChainInfo {
        SessionChainInfo::from_chain_id(self.chain_id)
    }

    pub fn switch_chain(&mut self, new_chain_id: u64, registry: &ChainRegistry) -> Result<()> {
        if !registry.is_chain_supported(new_chain_id) {
            return Err(anyhow!(
                "Cannot switch to unsupported chain: {}",
                new_chain_id
            ));
        }

        self.chain_id = new_chain_id;
        Ok(())
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

    pub async fn to_json(&self) -> Result<String> {
        let metadata = self.metadata.read().await;
        let messages = self.messages.read().await;

        let session_data = serde_json::json!({
            "id": self.id,
            "chain_id": self.chain_id,
            "config": self.config,
            "conversation_history": self.conversation_history,
            "state": self.state,
            "metadata": metadata.clone(),
            "messages": messages.clone(),
            "total_memory_used": self.total_memory_used,
        });

        Ok(serde_json::to_string(&session_data)?)
    }

    pub async fn from_json(json_str: &str) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_str(json_str)?;

        let id = value["id"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing id field"))?
            .to_string();

        let chain_id = value["chain_id"]
            .as_u64()
            .ok_or_else(|| anyhow!("Missing or invalid chain_id field"))?;

        let config: SessionConfig =
            serde_json::from_value(value["config"].clone()).unwrap_or_default();

        let conversation_history: Vec<Message> =
            serde_json::from_value(value["conversation_history"].clone()).unwrap_or_default();

        let state: SessionState =
            serde_json::from_value(value["state"].clone()).unwrap_or(SessionState::Active);

        let metadata: HashMap<String, String> =
            serde_json::from_value(value["metadata"].clone()).unwrap_or_default();

        let messages: Vec<Message> =
            serde_json::from_value(value["messages"].clone()).unwrap_or_default();

        let total_memory_used = value["total_memory_used"].as_u64().unwrap_or(0) as usize;

        let mut session = Self::with_chain(id, config, chain_id);
        session.conversation_history = conversation_history;
        session.state = state;
        session.total_memory_used = total_memory_used;

        // Set metadata
        {
            let mut session_metadata = session.metadata.write().await;
            *session_metadata = metadata;
        }

        // Set messages
        {
            let mut session_messages = session.messages.write().await;
            *session_messages = messages;
        }

        Ok(session)
    }

    pub async fn add_message_async(&mut self, role: &str, content: &str) -> Result<()> {
        let message = Message {
            role: role.to_string(),
            content: content.to_string(),
            timestamp: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs() as i64,
            ),
        };
        self.conversation_history.push(message.clone());
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
