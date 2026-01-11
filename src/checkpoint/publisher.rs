// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Checkpoint Publisher for S5 storage
//!
//! Handles publishing checkpoint deltas and indices to S5 storage.
//! CRITICAL: Publishing MUST complete BEFORE proof submission to chain.
//!
//! ## Usage
//! ```ignore
//! let publisher = CheckpointPublisher::new(s5_client, host_address);
//! publisher.buffer_message(session_id, message);
//! // Before proof submission:
//! let delta_cid = publisher.publish_checkpoint(session_id, proof_hash, ...).await?;
//! // Now safe to submit proof to chain
//! ```

use crate::checkpoint::{CheckpointDelta, CheckpointEntry, CheckpointIndex, CheckpointMessage};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// State for tracking checkpoints within a session
#[derive(Debug, Clone)]
pub struct SessionCheckpointState {
    /// Current checkpoint index (0-based)
    pub checkpoint_index: u32,

    /// Messages buffered since last checkpoint
    pub message_buffer: Vec<CheckpointMessage>,

    /// Token count at last checkpoint
    pub last_checkpoint_tokens: u64,

    /// Cached checkpoint index (for session resumption)
    pub index: Option<CheckpointIndex>,
}

impl SessionCheckpointState {
    /// Create new state for a fresh session
    pub fn new() -> Self {
        Self {
            checkpoint_index: 0,
            message_buffer: Vec::new(),
            last_checkpoint_tokens: 0,
            index: None,
        }
    }

    /// Create state from existing index (session resumption)
    pub fn from_index(index: CheckpointIndex) -> Self {
        let checkpoint_index = index.next_checkpoint_index();
        let last_checkpoint_tokens = index
            .last_checkpoint()
            .map(|c| c.token_range[1])
            .unwrap_or(0);

        Self {
            checkpoint_index,
            message_buffer: Vec::new(),
            last_checkpoint_tokens,
            index: Some(index),
        }
    }
}

impl Default for SessionCheckpointState {
    fn default() -> Self {
        Self::new()
    }
}

/// Publisher for checkpoint data to S5 storage
pub struct CheckpointPublisher {
    /// Host's Ethereum address (lowercase)
    host_address: String,

    /// Per-session checkpoint state
    sessions: Arc<RwLock<HashMap<String, SessionCheckpointState>>>,
}

impl CheckpointPublisher {
    /// Create a new checkpoint publisher
    pub fn new(host_address: String) -> Self {
        Self {
            host_address: host_address.to_lowercase(),
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the host address
    pub fn host_address(&self) -> &str {
        &self.host_address
    }

    /// Buffer a message for the given session
    pub async fn buffer_message(&self, session_id: &str, message: CheckpointMessage) {
        let mut sessions = self.sessions.write().await;
        let state = sessions
            .entry(session_id.to_string())
            .or_insert_with(SessionCheckpointState::new);
        state.message_buffer.push(message);
    }

    /// Get current state for a session (for testing)
    pub async fn get_session_state(&self, session_id: &str) -> Option<SessionCheckpointState> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// Clear session state (for cleanup)
    pub async fn remove_session(&self, session_id: &str) {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
    }

    /// Get number of active sessions
    pub async fn session_count(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_checkpoint_publisher_new() {
        let publisher = CheckpointPublisher::new("0xABC123".to_string());
        assert_eq!(publisher.host_address(), "0xabc123"); // lowercase
    }

    #[tokio::test]
    async fn test_buffer_message_accumulates() {
        let publisher = CheckpointPublisher::new("0xhost".to_string());

        let msg1 = CheckpointMessage::new_user("Hello".to_string(), 100);
        let msg2 = CheckpointMessage::new_assistant("Hi".to_string(), 200, false);

        publisher.buffer_message("session-1", msg1).await;
        publisher.buffer_message("session-1", msg2).await;

        let state = publisher.get_session_state("session-1").await.unwrap();
        assert_eq!(state.message_buffer.len(), 2);
        assert_eq!(state.message_buffer[0].role, "user");
        assert_eq!(state.message_buffer[1].role, "assistant");
    }

    #[tokio::test]
    async fn test_buffer_message_separate_sessions() {
        let publisher = CheckpointPublisher::new("0xhost".to_string());

        publisher
            .buffer_message("session-1", CheckpointMessage::new_user("A".to_string(), 100))
            .await;
        publisher
            .buffer_message("session-2", CheckpointMessage::new_user("B".to_string(), 200))
            .await;

        let state1 = publisher.get_session_state("session-1").await.unwrap();
        let state2 = publisher.get_session_state("session-2").await.unwrap();

        assert_eq!(state1.message_buffer.len(), 1);
        assert_eq!(state2.message_buffer.len(), 1);
        assert_eq!(state1.message_buffer[0].content, "A");
        assert_eq!(state2.message_buffer[0].content, "B");
    }

    #[tokio::test]
    async fn test_session_checkpoint_state_new() {
        let state = SessionCheckpointState::new();
        assert_eq!(state.checkpoint_index, 0);
        assert!(state.message_buffer.is_empty());
        assert_eq!(state.last_checkpoint_tokens, 0);
        assert!(state.index.is_none());
    }

    #[tokio::test]
    async fn test_session_checkpoint_state_from_index() {
        use crate::checkpoint::CheckpointEntry;

        let mut index = CheckpointIndex::new("session".to_string(), "0xhost".to_string());
        index.add_checkpoint(CheckpointEntry::with_timestamp(
            0,
            "0x1234".to_string(),
            "bafybeig123".to_string(),
            0,
            1000,
            1704844800000,
        ));
        index.add_checkpoint(CheckpointEntry::with_timestamp(
            1,
            "0x5678".to_string(),
            "bafybeig456".to_string(),
            1000,
            2500,
            1704844900000,
        ));

        let state = SessionCheckpointState::from_index(index);
        assert_eq!(state.checkpoint_index, 2); // Next index after 0, 1
        assert_eq!(state.last_checkpoint_tokens, 2500); // End of last checkpoint
        assert!(state.message_buffer.is_empty());
        assert!(state.index.is_some());
    }

    #[tokio::test]
    async fn test_remove_session() {
        let publisher = CheckpointPublisher::new("0xhost".to_string());

        publisher
            .buffer_message("session-1", CheckpointMessage::new_user("A".to_string(), 100))
            .await;
        assert_eq!(publisher.session_count().await, 1);

        publisher.remove_session("session-1").await;
        assert_eq!(publisher.session_count().await, 0);
        assert!(publisher.get_session_state("session-1").await.is_none());
    }

    #[tokio::test]
    async fn test_session_count() {
        let publisher = CheckpointPublisher::new("0xhost".to_string());
        assert_eq!(publisher.session_count().await, 0);

        publisher
            .buffer_message("s1", CheckpointMessage::new_user("A".to_string(), 100))
            .await;
        publisher
            .buffer_message("s2", CheckpointMessage::new_user("B".to_string(), 200))
            .await;

        assert_eq!(publisher.session_count().await, 2);
    }
}
