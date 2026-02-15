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

use crate::checkpoint::{
    encrypt_checkpoint_delta, sign_checkpoint_data, CheckpointDelta, CheckpointEntry,
    CheckpointIndex, CheckpointMessage,
};
use crate::storage::S5Storage;
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

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

    /// In-progress streaming response (for partial checkpoints)
    /// This is accumulated during streaming and included as partial if checkpoint triggers mid-stream
    pub streaming_response: Option<String>,

    /// User's recovery public key for encrypted checkpoints (SDK v1.8.7+)
    /// Compressed secp256k1 public key (0x-prefixed hex, 68 chars)
    /// When present, checkpoint deltas are encrypted before S5 upload
    pub recovery_public_key: Option<String>,
}

impl SessionCheckpointState {
    /// Create new state for a fresh session
    pub fn new() -> Self {
        Self {
            checkpoint_index: 0,
            message_buffer: Vec::new(),
            last_checkpoint_tokens: 0,
            index: None,
            streaming_response: None,
            recovery_public_key: None,
        }
    }

    /// Create state from existing index (session resumption)
    /// Note: recovery_public_key is not persisted in index, must be set separately
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
            streaming_response: None,
            recovery_public_key: None, // Set separately after session init
        }
    }

    /// Set the recovery public key for encrypted checkpoints
    pub fn set_recovery_public_key(&mut self, key: Option<String>) {
        self.recovery_public_key = key;
    }

    /// Get the recovery public key (if set)
    pub fn get_recovery_public_key(&self) -> Option<&str> {
        self.recovery_public_key.as_deref()
    }

    /// Check if this session has a recovery key (encrypted checkpoints enabled)
    pub fn has_recovery_key(&self) -> bool {
        self.recovery_public_key.is_some()
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

    /// Update the streaming response buffer with a new chunk
    /// Used during streaming inference to accumulate partial response
    pub fn update_streaming_response(&mut self, chunk: &str) {
        match &mut self.streaming_response {
            Some(buffer) => buffer.push_str(chunk),
            None => self.streaming_response = Some(chunk.to_string()),
        }
    }

    /// Clear the streaming response buffer
    /// Called when response completes (full message is tracked separately)
    pub fn clear_streaming_response(&mut self) {
        self.streaming_response = None;
    }

    /// Get the current streaming response (if any)
    pub fn get_streaming_response(&self) -> Option<&str> {
        self.streaming_response.as_deref()
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

    /// Update streaming response buffer with a new chunk
    /// Call this during streaming inference to accumulate partial response
    pub async fn update_streaming_response(&self, session_id: &str, chunk: &str) {
        let mut sessions = self.sessions.write().await;
        let state = sessions
            .entry(session_id.to_string())
            .or_insert_with(SessionCheckpointState::new);
        state.update_streaming_response(chunk);
    }

    /// Clear streaming response buffer
    /// Call this when response completes (after tracking full message)
    pub async fn clear_streaming_response(&self, session_id: &str) {
        let mut sessions = self.sessions.write().await;
        if let Some(state) = sessions.get_mut(session_id) {
            state.clear_streaming_response();
        }
    }

    /// Set the recovery public key for a session (enables encrypted checkpoints)
    /// Call this during session init when SDK provides recoveryPublicKey
    pub async fn set_recovery_public_key(&self, session_id: &str, key: String) {
        let mut sessions = self.sessions.write().await;
        let state = sessions
            .entry(session_id.to_string())
            .or_insert_with(SessionCheckpointState::new);
        state.set_recovery_public_key(Some(key));
    }

    /// Get the recovery public key for a session (if set)
    pub async fn get_recovery_public_key(&self, session_id: &str) -> Option<String> {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .and_then(|s| s.recovery_public_key.clone())
    }

    /// Check if a session has encrypted checkpoints enabled
    pub async fn has_recovery_key(&self, session_id: &str) -> bool {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .map(|s| s.has_recovery_key())
            .unwrap_or(false)
    }

    /// CRITICAL: Publish checkpoint to S5 BEFORE proof submission
    ///
    /// This method MUST be called before submitting proof on-chain.
    /// If this method returns Err, the caller MUST NOT submit the proof.
    ///
    /// # Arguments
    /// * `session_id` - Session identifier
    /// * `proof_hash` - 32-byte keccak256 hash of proof data
    /// * `start_token` - Token count at start of this checkpoint
    /// * `end_token` - Token count at end of this checkpoint
    /// * `private_key` - 32-byte host private key for signing
    /// * `s5_storage` - S5 storage backend
    ///
    /// # Returns
    /// * `Ok(delta_cid)` - CID of uploaded delta (raw format without s5:// prefix)
    /// * `Err` - S5 upload failed, caller must NOT submit proof
    pub async fn publish_checkpoint(
        &self,
        session_id: &str,
        proof_hash: [u8; 32],
        start_token: u64,
        end_token: u64,
        private_key: &[u8; 32],
        s5_storage: &dyn S5Storage,
    ) -> Result<String> {
        let proof_hash_hex = format!("0x{}", hex::encode(proof_hash));

        // 1. Get session state and messages
        let mut sessions = self.sessions.write().await;
        let state = sessions
            .entry(session_id.to_string())
            .or_insert_with(SessionCheckpointState::new);

        let mut messages = state.get_buffered_messages();
        let checkpoint_index = state.checkpoint_index;

        // Phase 4.3: Include streaming response as partial message if checkpoint triggers mid-stream
        if let Some(partial_response) = state.get_streaming_response() {
            if !partial_response.is_empty() {
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64;
                messages.push(CheckpointMessage::new_assistant(
                    partial_response.to_string(),
                    timestamp,
                    true, // partial - response continues in next delta
                ));
                info!(
                    "Including partial streaming response ({} chars) in checkpoint",
                    partial_response.len()
                );
            }
        }

        info!(
            "Publishing checkpoint {} for session {} ({} messages, tokens {}-{})",
            checkpoint_index,
            session_id,
            messages.len(),
            start_token,
            end_token
        );

        // 2. Create delta and sign messages
        let mut delta = CheckpointDelta {
            session_id: session_id.to_string(),
            checkpoint_index,
            proof_hash: proof_hash_hex.clone(),
            start_token,
            end_token,
            messages,
            host_signature: String::new(), // Will be filled after signing
        };

        // Sign the messages JSON (sorted keys for SDK compatibility)
        let messages_json = delta.compute_messages_json();
        let delta_signature = sign_checkpoint_data(private_key, &messages_json)?;
        delta.host_signature = delta_signature;

        // 3. Conditionally encrypt delta when recovery_public_key is present
        let is_encrypted = state.recovery_public_key.is_some();
        let delta_bytes = if let Some(recovery_pubkey) = &state.recovery_public_key {
            // Encrypt the delta for privacy-preserving recovery
            let encrypted_delta = encrypt_checkpoint_delta(&delta, recovery_pubkey, private_key)
                .map_err(|e| {
                    error!(
                        "📤 [CHECKPOINT] ❌ Encryption FAILED: session='{}', checkpoint={}, error={}",
                        session_id, checkpoint_index, e
                    );
                    anyhow!("Checkpoint encryption failed - NOT uploading: {}", e)
                })?;

            info!(
                "🔐 [CHECKPOINT] Encrypting checkpoint {} for session {} (recovery key present)",
                checkpoint_index, session_id
            );

            serde_json::to_vec_pretty(&encrypted_delta)
                .map_err(|e| anyhow!("Failed to serialize encrypted delta: {}", e))?
        } else {
            // Legacy plaintext mode (no recovery key)
            delta.to_json_bytes()
        };

        // 4. Upload delta to S5 (with retry)
        let delta_path = format!(
            "home/checkpoints/{}/{}/delta_{}.json",
            self.host_address, session_id, checkpoint_index
        );

        info!(
            "📤 [CHECKPOINT] Uploading delta: session='{}', checkpoint={}, path='{}', size={} bytes, encrypted={}",
            session_id, checkpoint_index, delta_path, delta_bytes.len(), is_encrypted
        );

        let delta_cid = upload_with_retry(s5_storage, &delta_path, delta_bytes)
            .await
            .map_err(|e| {
                error!(
                    "📤 [CHECKPOINT] ❌ Delta upload FAILED: session='{}', checkpoint={}, error={}",
                    session_id, checkpoint_index, e
                );
                anyhow!("S5 delta upload failed - NOT submitting proof: {}", e)
            })?;

        // Strip s5:// prefix if present (SDK expects raw CID)
        let delta_cid_raw = delta_cid
            .strip_prefix("s5://")
            .unwrap_or(&delta_cid)
            .to_string();

        info!(
            "📤 [CHECKPOINT] ✅ Delta uploaded: session='{}', checkpoint={}, cid='{}', cid_len={}",
            session_id,
            checkpoint_index,
            delta_cid_raw,
            delta_cid_raw.len()
        );

        // 5. Update checkpoint index
        let index = state.index.get_or_insert_with(|| {
            CheckpointIndex::new(session_id.to_string(), self.host_address.clone())
        });

        // Use encrypted constructor when encryption is enabled
        let entry = if is_encrypted {
            CheckpointEntry::new_encrypted(
                checkpoint_index,
                proof_hash_hex,
                delta_cid_raw.clone(),
                start_token,
                end_token,
            )
        } else {
            CheckpointEntry::new(
                checkpoint_index,
                proof_hash_hex,
                delta_cid_raw.clone(),
                start_token,
                end_token,
            )
        };
        index.add_checkpoint(entry);

        // 5. Sign and upload index
        let checkpoints_json = index.compute_checkpoints_json();
        let index_signature = sign_checkpoint_data(private_key, &checkpoints_json)?;
        index.host_signature = index_signature;

        let index_path = CheckpointIndex::s5_path(&self.host_address, session_id);
        let index_bytes = index.to_json_bytes();

        upload_with_retry(s5_storage, &index_path, index_bytes)
            .await
            .map_err(|e| {
                error!(
                    "Index upload failed for session {} checkpoint {}: {}",
                    session_id, checkpoint_index, e
                );
                anyhow!("S5 index upload failed - NOT submitting proof: {}", e)
            })?;

        info!("Index uploaded to {}", index_path);

        // 6. Update state for next checkpoint
        state.clear_buffer();
        state.increment_checkpoint_index();
        state.last_checkpoint_tokens = end_token;

        Ok(delta_cid_raw)
    }

    /// Initialize or resume a session from S5
    ///
    /// Call this when a session starts to check for existing checkpoint index.
    /// If found, resumes numbering from last checkpoint.
    pub async fn init_session(&self, session_id: &str, s5_storage: &dyn S5Storage) -> Result<()> {
        let index_path = CheckpointIndex::s5_path(&self.host_address, session_id);

        // Try to fetch existing index from S5
        match s5_storage.get(&index_path).await {
            Ok(bytes) => {
                let existing_index: CheckpointIndex = serde_json::from_slice(&bytes)
                    .map_err(|e| anyhow!("Failed to parse existing index: {}", e))?;

                info!(
                    "Resuming session {} from checkpoint {} (last token: {})",
                    session_id,
                    existing_index.next_checkpoint_index(),
                    existing_index
                        .last_checkpoint()
                        .map(|c| c.token_range[1])
                        .unwrap_or(0)
                );

                let mut sessions = self.sessions.write().await;
                let state = SessionCheckpointState::from_index(existing_index);
                sessions.insert(session_id.to_string(), state);
            }
            Err(_) => {
                // No existing index - fresh session
                info!("Starting fresh session {}", session_id);
            }
        }

        Ok(())
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
    let data_size = data.len();

    info!(
        "📤 [S5-RUST] Starting upload: path='{}', size={} bytes",
        path, data_size
    );

    for attempt in 0..MAX_S5_RETRIES {
        info!(
            "📤 [S5-RUST] Upload attempt {}/{} for path '{}'",
            attempt + 1,
            MAX_S5_RETRIES,
            path
        );

        let start_time = std::time::Instant::now();

        match s5_storage.put(path, data.clone()).await {
            Ok(cid) => {
                let duration_ms = start_time.elapsed().as_millis();
                info!(
                    "📤 [S5-RUST] ✅ Upload SUCCESS: path='{}', cid='{}', cid_len={}, size={} bytes, duration={}ms",
                    path, cid, cid.len(), data_size, duration_ms
                );
                return Ok(cid);
            }
            Err(e) => {
                let duration_ms = start_time.elapsed().as_millis();
                warn!(
                    "📤 [S5-RUST] ❌ Upload attempt {}/{} FAILED for path '{}': {:?} (took {}ms)",
                    attempt + 1,
                    MAX_S5_RETRIES,
                    path,
                    e,
                    duration_ms
                );
                last_error = Some(e);
                if attempt < MAX_S5_RETRIES - 1 {
                    let delay_ms = S5_RETRY_BASE_DELAY_MS * 2u64.pow(attempt);
                    info!("📤 [S5-RUST] Waiting {}ms before retry...", delay_ms);
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }
    }

    error!(
        "📤 [S5-RUST] 🚨 UPLOAD FAILED after {} retries: path='{}', last_error={:?}",
        MAX_S5_RETRIES, path, last_error
    );

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
            .buffer_message(
                "session-1",
                CheckpointMessage::new_user("A".to_string(), 100),
            )
            .await;
        publisher
            .buffer_message(
                "session-2",
                CheckpointMessage::new_user("B".to_string(), 200),
            )
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
            "babcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrst".to_string(),
            0,
            1000,
            1704844800000,
        ));
        index.add_checkpoint(CheckpointEntry::with_timestamp(
            1,
            "0x5678".to_string(),
            "b234567abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrst".to_string(),
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
            .buffer_message(
                "session-1",
                CheckpointMessage::new_user("A".to_string(), 100),
            )
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
        state.buffer_message(CheckpointMessage::new_assistant(
            "Hi".to_string(),
            200,
            false,
        ));

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
        state.buffer_message(CheckpointMessage::new_assistant(
            "Answer!".to_string(),
            200,
            false,
        ));

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
        state.buffer_message(CheckpointMessage::new_user(
            "Next question".to_string(),
            600,
        ));
        assert_eq!(state.buffer_size(), 1);
    }

    // S5 Upload with Retry Tests
    use crate::storage::s5_client::MockS5Backend;
    use crate::storage::StorageError;

    #[tokio::test]
    async fn test_upload_succeeds_first_try() {
        let mock = MockS5Backend::new();
        // MockS5Backend requires paths starting with "home/" or "archive/"
        let result = upload_with_retry(&mock, "home/checkpoints/test", b"test data".to_vec()).await;

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
        let result = upload_with_retry(&mock, "home/checkpoints/test", b"test data".to_vec()).await;

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

    // ==================== Publish Checkpoint Tests ====================

    fn generate_test_private_key() -> [u8; 32] {
        use k256::ecdsa::SigningKey;
        use rand::rngs::OsRng;
        let signing_key = SigningKey::random(&mut OsRng);
        signing_key.to_bytes().into()
    }

    #[tokio::test]
    async fn test_publish_checkpoint_creates_delta() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhost123".to_string());
        let private_key = generate_test_private_key();

        // Buffer some messages
        publisher
            .buffer_message(
                "session-1",
                CheckpointMessage::new_user("Hello".to_string(), 100),
            )
            .await;
        publisher
            .buffer_message(
                "session-1",
                CheckpointMessage::new_assistant("Hi there!".to_string(), 200, false),
            )
            .await;

        // Publish checkpoint
        let proof_hash = [0x12u8; 32];
        let result = publisher
            .publish_checkpoint("session-1", proof_hash, 0, 1000, &private_key, &mock)
            .await;

        assert!(result.is_ok(), "publish_checkpoint should succeed");
        let cid = result.unwrap();
        assert!(!cid.is_empty(), "CID should not be empty");
    }

    #[tokio::test]
    async fn test_publish_checkpoint_signs_messages() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhost456".to_string());
        let private_key = generate_test_private_key();

        publisher
            .buffer_message(
                "session-2",
                CheckpointMessage::new_user("Test message".to_string(), 100),
            )
            .await;

        let proof_hash = [0xABu8; 32];
        let result = publisher
            .publish_checkpoint("session-2", proof_hash, 0, 500, &private_key, &mock)
            .await;

        assert!(result.is_ok());

        // Verify index was updated with a signature
        let state = publisher.get_session_state("session-2").await.unwrap();
        assert!(state.index.is_some());
        let index = state.index.unwrap();
        assert!(
            index.host_signature.starts_with("0x"),
            "Index should have signature"
        );
        assert_eq!(
            index.host_signature.len(),
            132,
            "Signature should be 65 bytes (132 hex chars with 0x)"
        );
    }

    #[tokio::test]
    async fn test_publish_checkpoint_uploads_delta() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostdelta".to_string());
        let private_key = generate_test_private_key();

        publisher
            .buffer_message(
                "session-delta",
                CheckpointMessage::new_user("Delta test".to_string(), 100),
            )
            .await;

        let proof_hash = [0xDEu8; 32];
        let result = publisher
            .publish_checkpoint("session-delta", proof_hash, 0, 500, &private_key, &mock)
            .await;

        assert!(result.is_ok());

        // Verify delta was uploaded by checking mock's stored data
        let delta_path = "home/checkpoints/0xhostdelta/session-delta/delta_0.json";
        let stored = mock.get(delta_path).await;
        assert!(stored.is_ok(), "Delta should be stored at expected path");

        let stored_bytes = stored.unwrap();
        let delta: CheckpointDelta = serde_json::from_slice(&stored_bytes).unwrap();
        assert_eq!(delta.session_id, "session-delta");
        assert_eq!(delta.checkpoint_index, 0);
        assert_eq!(delta.messages.len(), 1);
        assert_eq!(delta.messages[0].content, "Delta test");
    }

    #[tokio::test]
    async fn test_publish_checkpoint_updates_index() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostindex".to_string());
        let private_key = generate_test_private_key();

        publisher
            .buffer_message(
                "session-index",
                CheckpointMessage::new_user("Index test".to_string(), 100),
            )
            .await;

        let proof_hash = [0x11u8; 32];
        let result = publisher
            .publish_checkpoint("session-index", proof_hash, 0, 500, &private_key, &mock)
            .await;

        assert!(result.is_ok(), "publish_checkpoint failed: {:?}", result);

        // Verify index was uploaded
        let index_path = "home/checkpoints/0xhostindex/session-index/index.json";
        let stored = mock.get(index_path).await;
        assert!(
            stored.is_ok(),
            "Index should be stored at expected path '{}', got error: {:?}",
            index_path,
            stored
        );

        let stored_bytes = stored.unwrap();
        let index: CheckpointIndex = serde_json::from_slice(&stored_bytes).unwrap();
        assert_eq!(index.session_id, "session-index");
        assert_eq!(index.host_address, "0xhostindex");
        assert_eq!(index.checkpoints.len(), 1);
        assert_eq!(index.checkpoints[0].index, 0);
        assert_eq!(index.checkpoints[0].token_range, [0, 500]);
    }

    #[tokio::test]
    async fn test_publish_checkpoint_returns_delta_cid() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostcid".to_string());
        let private_key = generate_test_private_key();

        publisher
            .buffer_message(
                "session-cid",
                CheckpointMessage::new_user("CID test".to_string(), 100),
            )
            .await;

        let proof_hash = [0x22u8; 32];
        let result = publisher
            .publish_checkpoint("session-cid", proof_hash, 0, 500, &private_key, &mock)
            .await;

        assert!(result.is_ok());
        let cid = result.unwrap();

        // CID should not have s5:// prefix (SDK requirement)
        assert!(
            !cid.starts_with("s5://"),
            "CID should not have s5:// prefix"
        );
        assert!(!cid.is_empty(), "CID should not be empty");

        // CID MUST be in BlobIdentifier format: 58-70 chars (varies by file size)
        // Old 53-char raw hash format is DEPRECATED - portals reject it
        // NOTE: S5 does NOT use IPFS format (bafkrei/bafybei)
        assert!(
            cid.starts_with('b') && cid.len() >= 58 && cid.len() <= 70,
            "deltaCid MUST be BlobIdentifier format (58-70 chars), got {} chars: {}",
            cid.len(),
            cid
        );
        // Must NOT be IPFS format
        assert!(
            !cid.starts_with("bafkrei") && !cid.starts_with("bafybei"),
            "deltaCid MUST NOT be IPFS format (bafkrei/bafybei), got: {}",
            cid
        );

        // CID should be recorded in the index
        let state = publisher.get_session_state("session-cid").await.unwrap();
        let index = state.index.unwrap();
        assert_eq!(index.checkpoints[0].delta_cid, cid);

        // Verify the checkpoint entry in index also has proper CID format
        assert!(
            index.checkpoints[0].delta_cid.starts_with('b')
                && index.checkpoints[0].delta_cid.len() >= 58
                && index.checkpoints[0].delta_cid.len() <= 70,
            "CheckpointEntry.delta_cid MUST be BlobIdentifier format (58-70 chars), got {}: {}",
            index.checkpoints[0].delta_cid.len(),
            index.checkpoints[0].delta_cid
        );
    }

    #[tokio::test]
    async fn test_publish_checkpoint_blocks_on_s5_failure() {
        let bad_mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostfail".to_string());
        let private_key = generate_test_private_key();

        publisher
            .buffer_message(
                "session-fail",
                CheckpointMessage::new_user("Fail test".to_string(), 100),
            )
            .await;

        // Set quota to 0 to make all uploads fail persistently
        bad_mock.set_quota_limit(0).await;

        let proof_hash = [0x33u8; 32];
        let result = publisher
            .publish_checkpoint("session-fail", proof_hash, 0, 500, &private_key, &bad_mock)
            .await;

        // Should fail and return error
        assert!(result.is_err(), "Should fail when S5 upload fails");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("NOT submitting proof"),
            "Error should indicate proof blocked: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_publish_checkpoint_clears_buffer() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostbuffer".to_string());
        let private_key = generate_test_private_key();

        publisher
            .buffer_message(
                "session-buffer",
                CheckpointMessage::new_user("Buffer test".to_string(), 100),
            )
            .await;

        // Verify buffer has message before publish
        let state_before = publisher.get_session_state("session-buffer").await.unwrap();
        assert_eq!(state_before.buffer_size(), 1);

        let proof_hash = [0x44u8; 32];
        let result = publisher
            .publish_checkpoint("session-buffer", proof_hash, 0, 500, &private_key, &mock)
            .await;
        assert!(result.is_ok());

        // Buffer should be cleared after successful publish
        let state_after = publisher.get_session_state("session-buffer").await.unwrap();
        assert_eq!(state_after.buffer_size(), 0, "Buffer should be cleared");
    }

    #[tokio::test]
    async fn test_publish_checkpoint_increments_index() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostincr".to_string());
        let private_key = generate_test_private_key();

        // First checkpoint
        publisher
            .buffer_message(
                "session-incr",
                CheckpointMessage::new_user("First".to_string(), 100),
            )
            .await;

        let proof_hash1 = [0x55u8; 32];
        let result1 = publisher
            .publish_checkpoint("session-incr", proof_hash1, 0, 500, &private_key, &mock)
            .await;
        assert!(result1.is_ok());

        // Second checkpoint
        publisher
            .buffer_message(
                "session-incr",
                CheckpointMessage::new_user("Second".to_string(), 600),
            )
            .await;

        let proof_hash2 = [0x66u8; 32];
        let result2 = publisher
            .publish_checkpoint("session-incr", proof_hash2, 500, 1000, &private_key, &mock)
            .await;
        assert!(result2.is_ok());

        // Verify checkpoint index was incremented
        let state = publisher.get_session_state("session-incr").await.unwrap();
        assert_eq!(
            state.checkpoint_index, 2,
            "Checkpoint index should be 2 after two checkpoints"
        );

        // Verify index has both checkpoints
        let index = state.index.unwrap();
        assert_eq!(index.checkpoints.len(), 2);
        assert_eq!(index.checkpoints[0].index, 0);
        assert_eq!(index.checkpoints[1].index, 1);
    }

    #[tokio::test]
    async fn test_publish_checkpoint_proof_hash_in_delta() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostproof".to_string());
        let private_key = generate_test_private_key();

        publisher
            .buffer_message(
                "session-proof",
                CheckpointMessage::new_user("Proof test".to_string(), 100),
            )
            .await;

        let proof_hash = [
            0xAB, 0xCD, 0xEF, 0x12, 0x34, 0x56, 0x78, 0x90, 0xAB, 0xCD, 0xEF, 0x12, 0x34, 0x56,
            0x78, 0x90, 0xAB, 0xCD, 0xEF, 0x12, 0x34, 0x56, 0x78, 0x90, 0xAB, 0xCD, 0xEF, 0x12,
            0x34, 0x56, 0x78, 0x90,
        ];

        let result = publisher
            .publish_checkpoint("session-proof", proof_hash, 0, 500, &private_key, &mock)
            .await;
        assert!(result.is_ok());

        // Verify proof hash is in delta
        let delta_path = "home/checkpoints/0xhostproof/session-proof/delta_0.json";
        let stored = mock.get(delta_path).await.unwrap();
        let delta: CheckpointDelta = serde_json::from_slice(&stored).unwrap();

        let expected_hash = "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        assert_eq!(delta.proof_hash, expected_hash);
    }

    // ==================== Session Resumption Tests ====================

    #[tokio::test]
    async fn test_init_session_fetches_index_from_s5() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostresume".to_string());

        // Pre-populate S5 with an existing index
        let mut existing_index =
            CheckpointIndex::new("session-resume".to_string(), "0xhostresume".to_string());
        existing_index.add_checkpoint(CheckpointEntry::with_timestamp(
            0,
            "0xproof1".to_string(),
            "bafycid1".to_string(),
            0,
            1000,
            1704844800000,
        ));
        existing_index.host_signature = "0xsig".to_string();

        let index_path = "home/checkpoints/0xhostresume/session-resume/index.json";
        let index_bytes = serde_json::to_vec(&existing_index).unwrap();
        mock.put(index_path, index_bytes).await.unwrap();

        // Initialize session - should fetch existing index
        let result = publisher.init_session("session-resume", &mock).await;
        assert!(result.is_ok(), "init_session should succeed");

        // Verify state was loaded from index
        let state = publisher
            .get_session_state("session-resume")
            .await
            .expect("Session state should exist");
        assert!(state.index.is_some(), "Index should be loaded");
        let loaded_index = state.index.unwrap();
        assert_eq!(loaded_index.checkpoints.len(), 1);
        assert_eq!(loaded_index.checkpoints[0].proof_hash, "0xproof1");
    }

    #[tokio::test]
    async fn test_init_session_continues_checkpoint_numbering() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostcont".to_string());

        // Pre-populate S5 with an index that has 2 checkpoints
        let mut existing_index =
            CheckpointIndex::new("session-cont".to_string(), "0xhostcont".to_string());
        existing_index.add_checkpoint(CheckpointEntry::with_timestamp(
            0,
            "0xproof0".to_string(),
            "bafycid0".to_string(),
            0,
            1000,
            1704844800000,
        ));
        existing_index.add_checkpoint(CheckpointEntry::with_timestamp(
            1,
            "0xproof1".to_string(),
            "bafycid1".to_string(),
            1000,
            2000,
            1704844900000,
        ));
        existing_index.host_signature = "0xsig".to_string();

        let index_path = "home/checkpoints/0xhostcont/session-cont/index.json";
        let index_bytes = serde_json::to_vec(&existing_index).unwrap();
        mock.put(index_path, index_bytes).await.unwrap();

        // Initialize session
        publisher.init_session("session-cont", &mock).await.unwrap();

        // Verify checkpoint index continues from last
        let state = publisher.get_session_state("session-cont").await.unwrap();
        assert_eq!(
            state.checkpoint_index, 2,
            "Checkpoint index should continue from 2 (after 0, 1)"
        );
        assert_eq!(
            state.last_checkpoint_tokens, 2000,
            "Last checkpoint tokens should be 2000"
        );
    }

    #[tokio::test]
    async fn test_new_session_starts_at_zero() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostnew".to_string());

        // Don't pre-populate S5 - this is a new session
        // Initialize session - should start fresh
        let result = publisher.init_session("session-new", &mock).await;
        assert!(
            result.is_ok(),
            "init_session should succeed for new session"
        );

        // For a new session, no state is created until buffer_message is called
        // Verify no error was returned (success means it handled missing index gracefully)

        // Now buffer a message to create the session
        publisher
            .buffer_message(
                "session-new",
                CheckpointMessage::new_user("First message".to_string(), 100),
            )
            .await;

        let state = publisher.get_session_state("session-new").await.unwrap();
        assert_eq!(
            state.checkpoint_index, 0,
            "New session should start at index 0"
        );
        assert_eq!(
            state.last_checkpoint_tokens, 0,
            "New session should have 0 tokens"
        );
        assert!(
            state.index.is_none(),
            "New session should have no preloaded index"
        );
    }

    #[tokio::test]
    async fn test_init_session_handles_missing_index() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostmissing".to_string());

        // Don't pre-populate S5 - simulate missing index
        let result = publisher.init_session("session-missing", &mock).await;

        // Should succeed (not error) when index is missing
        assert!(
            result.is_ok(),
            "init_session should succeed even with missing index"
        );

        // Session state should not exist yet (no pre-loaded index)
        let state = publisher.get_session_state("session-missing").await;
        assert!(
            state.is_none(),
            "No session state should be created for missing index"
        );
    }

    #[tokio::test]
    async fn test_init_session_then_publish_continues_correctly() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostfull".to_string());
        let private_key = generate_test_private_key();

        // Pre-populate S5 with an index that has 1 checkpoint
        let mut existing_index =
            CheckpointIndex::new("session-full".to_string(), "0xhostfull".to_string());
        existing_index.add_checkpoint(CheckpointEntry::with_timestamp(
            0,
            "0xoldproof".to_string(),
            "bafyoldcid".to_string(),
            0,
            1000,
            1704844800000,
        ));
        existing_index.host_signature = "0xoldsig".to_string();

        let index_path = "home/checkpoints/0xhostfull/session-full/index.json";
        let index_bytes = serde_json::to_vec(&existing_index).unwrap();
        mock.put(index_path, index_bytes).await.unwrap();

        // Initialize session (resume)
        publisher.init_session("session-full", &mock).await.unwrap();

        // Buffer a new message
        publisher
            .buffer_message(
                "session-full",
                CheckpointMessage::new_user("New message after resume".to_string(), 2000),
            )
            .await;

        // Publish new checkpoint
        let proof_hash = [0x77u8; 32];
        let result = publisher
            .publish_checkpoint("session-full", proof_hash, 1000, 2000, &private_key, &mock)
            .await;

        assert!(
            result.is_ok(),
            "publish_checkpoint should succeed after resume"
        );

        // Verify the new checkpoint was added at index 1 (not 0)
        let state = publisher.get_session_state("session-full").await.unwrap();
        assert_eq!(
            state.checkpoint_index, 2,
            "After publishing, index should be 2"
        );

        let index = state.index.unwrap();
        assert_eq!(
            index.checkpoints.len(),
            2,
            "Index should have 2 checkpoints"
        );
        assert_eq!(
            index.checkpoints[0].index, 0,
            "First checkpoint should be index 0"
        );
        assert_eq!(
            index.checkpoints[1].index, 1,
            "Second checkpoint should be index 1"
        );
    }

    // ==================== Streaming Response Tests (Phase 4.3) ====================

    #[test]
    fn test_update_streaming_response_accumulates() {
        let mut state = SessionCheckpointState::new();
        assert!(state.streaming_response.is_none());

        state.update_streaming_response("Hello");
        assert_eq!(state.get_streaming_response(), Some("Hello"));

        state.update_streaming_response(" World");
        assert_eq!(state.get_streaming_response(), Some("Hello World"));

        state.update_streaming_response("!");
        assert_eq!(state.get_streaming_response(), Some("Hello World!"));
    }

    #[test]
    fn test_clear_streaming_response() {
        let mut state = SessionCheckpointState::new();
        state.update_streaming_response("Some content");
        assert!(state.get_streaming_response().is_some());

        state.clear_streaming_response();
        assert!(state.streaming_response.is_none());
    }

    #[tokio::test]
    async fn test_publisher_update_streaming_response() {
        let publisher = CheckpointPublisher::new("0xhost".to_string());

        publisher
            .update_streaming_response("session-1", "Hello")
            .await;
        publisher
            .update_streaming_response("session-1", " World")
            .await;

        let state = publisher.get_session_state("session-1").await.unwrap();
        assert_eq!(state.get_streaming_response(), Some("Hello World"));
    }

    #[tokio::test]
    async fn test_publisher_clear_streaming_response() {
        let publisher = CheckpointPublisher::new("0xhost".to_string());

        publisher
            .update_streaming_response("session-1", "Hello")
            .await;
        publisher.clear_streaming_response("session-1").await;

        let state = publisher.get_session_state("session-1").await.unwrap();
        assert!(state.get_streaming_response().is_none());
    }

    #[tokio::test]
    async fn test_streaming_response_marked_partial() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostpartial".to_string());
        let private_key = generate_test_private_key();

        // Buffer a user message
        publisher
            .buffer_message(
                "session-partial",
                CheckpointMessage::new_user("What is 2+2?".to_string(), 100),
            )
            .await;

        // Simulate streaming response in progress (not complete)
        publisher
            .update_streaming_response("session-partial", "The answer is")
            .await;

        // Publish checkpoint - should include partial response
        let proof_hash = [0xAAu8; 32];
        let result = publisher
            .publish_checkpoint("session-partial", proof_hash, 0, 500, &private_key, &mock)
            .await;

        assert!(result.is_ok());

        // Verify delta includes the partial response
        let delta_path = "home/checkpoints/0xhostpartial/session-partial/delta_0.json";
        let stored = mock.get(delta_path).await.unwrap();
        let delta: CheckpointDelta = serde_json::from_slice(&stored).unwrap();

        // Should have 2 messages: user question + partial assistant response
        assert_eq!(
            delta.messages.len(),
            2,
            "Should have user + partial assistant"
        );
        assert_eq!(delta.messages[0].role, "user");
        assert_eq!(delta.messages[1].role, "assistant");
        assert_eq!(delta.messages[1].content, "The answer is");

        // Verify partial flag is set
        assert!(
            delta.messages[1].metadata.is_some(),
            "Partial response should have metadata"
        );
        let metadata = delta.messages[1].metadata.as_ref().unwrap();
        assert_eq!(metadata.partial, Some(true), "Partial flag should be true");
    }

    #[tokio::test]
    async fn test_partial_replaced_on_completion() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostreplace".to_string());
        let private_key = generate_test_private_key();

        // Buffer a user message
        publisher
            .buffer_message(
                "session-replace",
                CheckpointMessage::new_user("What is 2+2?".to_string(), 100),
            )
            .await;

        // Simulate streaming response in progress
        publisher
            .update_streaming_response("session-replace", "The answer")
            .await;

        // First checkpoint with partial response
        let proof_hash1 = [0xBBu8; 32];
        let result1 = publisher
            .publish_checkpoint("session-replace", proof_hash1, 0, 500, &private_key, &mock)
            .await;
        assert!(result1.is_ok());

        // Now response completes - clear streaming buffer and add full message
        publisher.clear_streaming_response("session-replace").await;
        publisher
            .buffer_message(
                "session-replace",
                CheckpointMessage::new_assistant("The answer is 4.".to_string(), 200, false),
            )
            .await;

        // Second checkpoint with complete response
        let proof_hash2 = [0xCCu8; 32];
        let result2 = publisher
            .publish_checkpoint(
                "session-replace",
                proof_hash2,
                500,
                1000,
                &private_key,
                &mock,
            )
            .await;
        assert!(result2.is_ok());

        // Verify second delta has complete (non-partial) response
        let delta_path2 = "home/checkpoints/0xhostreplace/session-replace/delta_1.json";
        let stored2 = mock.get(delta_path2).await.unwrap();
        let delta2: CheckpointDelta = serde_json::from_slice(&stored2).unwrap();

        assert_eq!(
            delta2.messages.len(),
            1,
            "Should have 1 complete assistant message"
        );
        assert_eq!(delta2.messages[0].role, "assistant");
        assert_eq!(delta2.messages[0].content, "The answer is 4.");

        // Should NOT have partial flag
        assert!(
            delta2.messages[0].metadata.is_none(),
            "Complete response should not have metadata with partial flag"
        );
    }

    // ==================== Sub-phase 9.8: Recovery Public Key Tests ====================

    const TEST_RECOVERY_PUBKEY: &str =
        "0x02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";

    #[test]
    fn test_session_state_recovery_key_default_none() {
        let state = SessionCheckpointState::new();
        assert!(state.recovery_public_key.is_none());
    }

    #[test]
    fn test_session_state_set_recovery_key() {
        let mut state = SessionCheckpointState::new();
        state.set_recovery_public_key(Some(TEST_RECOVERY_PUBKEY.to_string()));
        assert_eq!(
            state.recovery_public_key,
            Some(TEST_RECOVERY_PUBKEY.to_string())
        );
    }

    #[test]
    fn test_session_state_get_recovery_key() {
        let mut state = SessionCheckpointState::new();
        assert!(state.get_recovery_public_key().is_none());

        state.set_recovery_public_key(Some(TEST_RECOVERY_PUBKEY.to_string()));
        assert_eq!(state.get_recovery_public_key(), Some(TEST_RECOVERY_PUBKEY));
    }

    #[test]
    fn test_session_state_has_recovery_key() {
        let mut state = SessionCheckpointState::new();
        assert!(!state.has_recovery_key());

        state.set_recovery_public_key(Some(TEST_RECOVERY_PUBKEY.to_string()));
        assert!(state.has_recovery_key());
    }

    #[test]
    fn test_session_state_from_index_preserves_recovery_key() {
        // When resuming from index, recovery key should be None (set separately)
        let index = CheckpointIndex::new("session".to_string(), "0xhost".to_string());
        let state = SessionCheckpointState::from_index(index);
        assert!(
            state.recovery_public_key.is_none(),
            "From index should not have recovery key"
        );
    }

    #[tokio::test]
    async fn test_publisher_set_recovery_key() {
        let publisher = CheckpointPublisher::new("0xhost".to_string());

        publisher
            .set_recovery_public_key("session-1", TEST_RECOVERY_PUBKEY.to_string())
            .await;

        let state = publisher.get_session_state("session-1").await.unwrap();
        assert_eq!(
            state.recovery_public_key,
            Some(TEST_RECOVERY_PUBKEY.to_string())
        );
    }

    #[tokio::test]
    async fn test_publisher_get_recovery_key() {
        let publisher = CheckpointPublisher::new("0xhost".to_string());

        // Before setting
        let key_before = publisher.get_recovery_public_key("session-1").await;
        assert!(key_before.is_none());

        // After setting
        publisher
            .set_recovery_public_key("session-1", TEST_RECOVERY_PUBKEY.to_string())
            .await;
        let key_after = publisher.get_recovery_public_key("session-1").await;
        assert_eq!(key_after, Some(TEST_RECOVERY_PUBKEY.to_string()));
    }

    #[tokio::test]
    async fn test_publisher_has_recovery_key() {
        let publisher = CheckpointPublisher::new("0xhost".to_string());

        // Before setting - session doesn't exist
        assert!(!publisher.has_recovery_key("session-1").await);

        // Create session without recovery key
        publisher
            .buffer_message(
                "session-1",
                CheckpointMessage::new_user("Hi".to_string(), 100),
            )
            .await;
        assert!(!publisher.has_recovery_key("session-1").await);

        // After setting recovery key
        publisher
            .set_recovery_public_key("session-1", TEST_RECOVERY_PUBKEY.to_string())
            .await;
        assert!(publisher.has_recovery_key("session-1").await);
    }

    // ==================== Sub-phase 9.9: Encrypted Checkpoint Publishing Tests ====================

    #[tokio::test]
    async fn test_publish_checkpoint_encrypts_when_recovery_key_present() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostencrypt".to_string());
        let private_key = generate_test_private_key();

        // Set recovery public key BEFORE buffering messages
        publisher
            .set_recovery_public_key("session-encrypt", TEST_RECOVERY_PUBKEY.to_string())
            .await;

        // Buffer a message
        publisher
            .buffer_message(
                "session-encrypt",
                CheckpointMessage::new_user("Encrypt me!".to_string(), 100),
            )
            .await;

        // Publish checkpoint - should encrypt
        let proof_hash = [0xEEu8; 32];
        let result = publisher
            .publish_checkpoint("session-encrypt", proof_hash, 0, 500, &private_key, &mock)
            .await;

        assert!(
            result.is_ok(),
            "publish_checkpoint should succeed: {:?}",
            result
        );

        // Verify uploaded delta is encrypted (has "encrypted":true field)
        let delta_path = "home/checkpoints/0xhostencrypt/session-encrypt/delta_0.json";
        let stored = mock.get(delta_path).await.unwrap();
        let stored_str = String::from_utf8(stored).unwrap();

        // Check for encrypted delta format (pretty-printed JSON has spaces)
        assert!(
            stored_str.contains("\"encrypted\": true") || stored_str.contains("\"encrypted\":true"),
            "Delta should be encrypted, got: {}",
            &stored_str[..stored_str.len().min(200)]
        );
        assert!(
            stored_str.contains("\"ciphertext\""),
            "Delta should have ciphertext field"
        );
        assert!(
            stored_str.contains("\"ephemeralPublicKey\""),
            "Delta should have ephemeral key"
        );
        assert!(stored_str.contains("\"nonce\""), "Delta should have nonce");
    }

    #[tokio::test]
    async fn test_publish_checkpoint_plaintext_when_no_recovery_key() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostplain".to_string());
        let private_key = generate_test_private_key();

        // Do NOT set recovery key - should publish plaintext
        publisher
            .buffer_message(
                "session-plain",
                CheckpointMessage::new_user("Plain text!".to_string(), 100),
            )
            .await;

        let proof_hash = [0xFFu8; 32];
        let result = publisher
            .publish_checkpoint("session-plain", proof_hash, 0, 500, &private_key, &mock)
            .await;

        assert!(result.is_ok());

        // Verify uploaded delta is plaintext (has sessionId, messages, etc.)
        let delta_path = "home/checkpoints/0xhostplain/session-plain/delta_0.json";
        let stored = mock.get(delta_path).await.unwrap();
        let stored_str = String::from_utf8(stored).unwrap();

        assert!(
            stored_str.contains("\"sessionId\""),
            "Delta should have sessionId (plaintext format)"
        );
        assert!(
            stored_str.contains("\"messages\""),
            "Delta should have messages array (plaintext format)"
        );
        assert!(
            stored_str.contains("Plain text!"),
            "Delta should contain actual message content"
        );
        assert!(
            !stored_str.contains("\"ciphertext\""),
            "Delta should NOT have ciphertext field"
        );
    }

    #[tokio::test]
    async fn test_publish_checkpoint_sets_encrypted_marker_in_index() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostmarker".to_string());
        let private_key = generate_test_private_key();

        // Set recovery key - should mark index entry as encrypted
        publisher
            .set_recovery_public_key("session-marker", TEST_RECOVERY_PUBKEY.to_string())
            .await;

        publisher
            .buffer_message(
                "session-marker",
                CheckpointMessage::new_user("Test marker".to_string(), 100),
            )
            .await;

        let proof_hash = [0xAAu8; 32];
        let result = publisher
            .publish_checkpoint("session-marker", proof_hash, 0, 500, &private_key, &mock)
            .await;

        assert!(result.is_ok());

        // Verify index has encrypted marker
        let index_path = "home/checkpoints/0xhostmarker/session-marker/index.json";
        let stored = mock.get(index_path).await.unwrap();
        let stored_str = String::from_utf8(stored).unwrap();

        // Check for encrypted marker (handles both compact and pretty JSON)
        assert!(
            stored_str.contains("\"encrypted\": true") || stored_str.contains("\"encrypted\":true"),
            "Index entry should have encrypted:true marker, got: {}",
            &stored_str[..stored_str.len().min(300)]
        );
    }

    #[tokio::test]
    async fn test_publish_checkpoint_no_encrypted_marker_for_plaintext() {
        let mock = MockS5Backend::new();
        let publisher = CheckpointPublisher::new("0xhostnomark".to_string());
        let private_key = generate_test_private_key();

        // No recovery key - plaintext
        publisher
            .buffer_message(
                "session-nomark",
                CheckpointMessage::new_user("No marker".to_string(), 100),
            )
            .await;

        let proof_hash = [0xBBu8; 32];
        let result = publisher
            .publish_checkpoint("session-nomark", proof_hash, 0, 500, &private_key, &mock)
            .await;

        assert!(result.is_ok());

        // Verify index does NOT have encrypted marker
        let index_path = "home/checkpoints/0xhostnomark/session-nomark/index.json";
        let stored = mock.get(index_path).await.unwrap();
        let stored_str = String::from_utf8(stored).unwrap();

        assert!(
            !stored_str.contains("\"encrypted\""),
            "Index entry should NOT have encrypted field for plaintext"
        );
    }
}
