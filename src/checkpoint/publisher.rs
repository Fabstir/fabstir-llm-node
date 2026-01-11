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
use crate::storage::S5Storage;
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::warn;

/// Maximum number of S5 upload retry attempts
const MAX_S5_RETRIES: u32 = 3;

/// Base delay for exponential backoff (1s, 2s, 4s)
const S5_RETRY_BASE_DELAY_MS: u64 = 1000;

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

    /// Get a copy of buffered messages
    pub fn get_buffered_messages(&self) -> Vec<CheckpointMessage> {
        self.message_buffer.clone()
    }

    /// Clear the message buffer (after checkpoint published)
    pub fn clear_buffer(&mut self) {
        self.message_buffer.clear();
    }

    /// Increment checkpoint index after successful publish
    pub fn increment_checkpoint_index(&mut self) {
        self.checkpoint_index += 1;
    }

    /// Add a message to the buffer
    pub fn buffer_message(&mut self, message: CheckpointMessage) {
        self.message_buffer.push(message);
    }

    /// Get current buffer size
    pub fn buffer_size(&self) -> usize {
        self.message_buffer.len()
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

/// Upload data to S5 with exponential backoff retry
///
/// Attempts up to MAX_S5_RETRIES times with increasing delays.
/// Delays follow exponential backoff: 1s, 2s, 4s.
///
/// # Arguments
/// * `s5_storage` - S5 storage backend
/// * `path` - S5 path to upload to
/// * `data` - Data bytes to upload
///
/// # Returns
/// CID on success, error after all retries exhausted
pub async fn upload_with_retry(
    s5_storage: &dyn S5Storage,
    path: &str,
    data: Vec<u8>,
) -> Result<String> {
    let mut last_error = None;

    for attempt in 0..MAX_S5_RETRIES {
        match s5_storage.put(path, data.clone()).await {
            Ok(cid) => return Ok(cid),
            Err(e) => {
                warn!(
                    "S5 upload attempt {}/{} failed for path '{}': {:?}",
                    attempt + 1,
                    MAX_S5_RETRIES,
                    path,
                    e
                );
                last_error = Some(e);
                if attempt < MAX_S5_RETRIES - 1 {
                    let delay_ms = S5_RETRY_BASE_DELAY_MS * 2u64.pow(attempt);
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }
    }

    Err(anyhow!(
        "S5 upload failed after {} retries: {:?}",
        MAX_S5_RETRIES,
        last_error
    ))
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

    #[test]
    fn test_get_buffered_messages_returns_copy() {
        let mut state = SessionCheckpointState::new();
        state.buffer_message(CheckpointMessage::new_user("Hello".to_string(), 100));
        state.buffer_message(CheckpointMessage::new_assistant("Hi".to_string(), 200, false));

        let messages = state.get_buffered_messages();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "Hello");
        assert_eq!(messages[1].content, "Hi");

        // Original buffer should still have messages
        assert_eq!(state.buffer_size(), 2);
    }

    #[test]
    fn test_clear_buffer_empties_messages() {
        let mut state = SessionCheckpointState::new();
        state.buffer_message(CheckpointMessage::new_user("Hello".to_string(), 100));
        state.buffer_message(CheckpointMessage::new_user("World".to_string(), 200));
        assert_eq!(state.buffer_size(), 2);

        state.clear_buffer();
        assert_eq!(state.buffer_size(), 0);
        assert!(state.get_buffered_messages().is_empty());
    }

    #[test]
    fn test_increment_checkpoint_index() {
        let mut state = SessionCheckpointState::new();
        assert_eq!(state.checkpoint_index, 0);

        state.increment_checkpoint_index();
        assert_eq!(state.checkpoint_index, 1);

        state.increment_checkpoint_index();
        assert_eq!(state.checkpoint_index, 2);
    }

    #[test]
    fn test_buffer_message_increments_count() {
        let mut state = SessionCheckpointState::new();
        assert_eq!(state.buffer_size(), 0);

        state.buffer_message(CheckpointMessage::new_user("A".to_string(), 100));
        assert_eq!(state.buffer_size(), 1);

        state.buffer_message(CheckpointMessage::new_user("B".to_string(), 200));
        assert_eq!(state.buffer_size(), 2);

        state.buffer_message(CheckpointMessage::new_user("C".to_string(), 300));
        assert_eq!(state.buffer_size(), 3);
    }

    #[test]
    fn test_session_state_workflow() {
        // Simulate a full checkpoint workflow
        let mut state = SessionCheckpointState::new();

        // Buffer some messages
        state.buffer_message(CheckpointMessage::new_user("Question?".to_string(), 100));
        state.buffer_message(CheckpointMessage::new_assistant("Answer!".to_string(), 200, false));

        // Get messages for checkpoint
        let messages = state.get_buffered_messages();
        assert_eq!(messages.len(), 2);

        // Simulate checkpoint published
        state.clear_buffer();
        state.increment_checkpoint_index();
        state.last_checkpoint_tokens = 500;

        // Verify state after checkpoint
        assert_eq!(state.checkpoint_index, 1);
        assert_eq!(state.buffer_size(), 0);
        assert_eq!(state.last_checkpoint_tokens, 500);

        // Buffer more messages for next checkpoint
        state.buffer_message(CheckpointMessage::new_user("Next question".to_string(), 600));
        assert_eq!(state.buffer_size(), 1);
    }

    // S5 Upload with Retry Tests
    use crate::storage::s5_client::MockS5Backend;
    use crate::storage::StorageError;

    #[tokio::test]
    async fn test_upload_succeeds_first_try() {
        let mock = MockS5Backend::new();
        // MockS5Backend requires paths starting with "home/" or "archive/"
        let result =
            upload_with_retry(&mock, "home/checkpoints/test", b"test data".to_vec()).await;

        assert!(result.is_ok());
        let cid = result.unwrap();
        assert!(!cid.is_empty());
    }

    #[tokio::test]
    async fn test_upload_retries_on_transient_failure() {
        let mock = MockS5Backend::new();
        // Inject a single transient error - mock uses take() so only first attempt fails
        mock.inject_error(StorageError::NetworkError(
            "simulated network failure".to_string(),
        ))
        .await;

        // First attempt fails, retry should succeed
        let result =
            upload_with_retry(&mock, "home/checkpoints/test", b"test data".to_vec()).await;

        // Should succeed on retry
        assert!(result.is_ok(), "Should succeed after retry");
        let cid = result.unwrap();
        assert!(!cid.is_empty());
    }

    #[tokio::test]
    async fn test_upload_fails_with_invalid_path() {
        let mock = MockS5Backend::new();

        // MockS5Backend validates paths - this should fail with InvalidPath error
        let result = upload_with_retry(&mock, "invalid/path", b"test data".to_vec()).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("failed after 3 retries"));
    }

    #[tokio::test]
    async fn test_upload_returns_cid_from_mock() {
        let mock = MockS5Backend::new();

        // Upload some data (path must start with home/ or archive/)
        let result = upload_with_retry(
            &mock,
            "home/checkpoints/session123/delta.json",
            b"checkpoint data".to_vec(),
        )
        .await;
        assert!(result.is_ok());

        // Verify we got a CID back (MockS5Backend uses s5:// prefix)
        let cid = result.unwrap();
        assert!(cid.starts_with("s5://") || !cid.is_empty());
    }

    #[tokio::test]
    async fn test_upload_different_paths_succeed() {
        let mock = MockS5Backend::new();

        // Upload to multiple paths (must start with home/ or archive/)
        let result1 = upload_with_retry(&mock, "home/path/1", b"data1".to_vec()).await;
        let result2 = upload_with_retry(&mock, "home/path/2", b"data2".to_vec()).await;

        assert!(result1.is_ok());
        assert!(result2.is_ok());
    }
}
