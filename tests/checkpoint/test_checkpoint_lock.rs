// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Lock contention regression tests for CheckpointPublisher

use async_trait::async_trait;
use fabstir_llm_node::checkpoint::{CheckpointMessage, CheckpointPublisher};
use fabstir_llm_node::storage::s5_client::{MockS5Backend, S5Entry, S5ListResult, StorageError};
use fabstir_llm_node::storage::S5Storage;
use k256::ecdsa::SigningKey;
use rand::rngs::OsRng;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

/// Mock S5 backend that adds a 2-second delay to every put() call.
struct SlowMockS5(MockS5Backend);

impl SlowMockS5 {
    fn new() -> Self {
        Self(MockS5Backend::new())
    }
}

#[async_trait]
impl S5Storage for SlowMockS5 {
    async fn put(&self, path: &str, data: Vec<u8>) -> Result<String, StorageError> {
        tokio::time::sleep(Duration::from_secs(2)).await;
        self.0.put(path, data).await
    }
    async fn put_with_metadata(
        &self,
        path: &str,
        data: Vec<u8>,
        _meta: HashMap<String, String>,
    ) -> Result<String, StorageError> {
        self.put(path, data).await
    }
    async fn get(&self, path: &str) -> Result<Vec<u8>, StorageError> {
        self.0.get(path).await
    }
    async fn get_metadata(&self, path: &str) -> Result<HashMap<String, String>, StorageError> {
        self.0.get_metadata(path).await
    }
    async fn get_by_cid(&self, cid: &str) -> Result<Vec<u8>, StorageError> {
        self.0.get_by_cid(cid).await
    }
    async fn list(&self, path: &str) -> Result<Vec<S5Entry>, StorageError> {
        self.0.list(path).await
    }
    async fn list_with_options(
        &self,
        path: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<S5ListResult, StorageError> {
        self.0.list_with_options(path, limit, cursor).await
    }
    async fn delete(&self, path: &str) -> Result<(), StorageError> {
        self.0.delete(path).await
    }
    async fn exists(&self, path: &str) -> Result<bool, StorageError> {
        self.0.exists(path).await
    }
    fn clone(&self) -> Box<dyn S5Storage> {
        Box::new(SlowMockS5(MockS5Backend::new()))
    }
}

fn generate_test_private_key() -> [u8; 32] {
    SigningKey::random(&mut OsRng).to_bytes().into()
}

/// Failed S5 uploads must not corrupt the checkpoint index.
#[tokio::test]
async fn test_failed_upload_does_not_write_index_entry() {
    let publisher = CheckpointPublisher::new("0xfail_test".to_string());
    let key = generate_test_private_key();

    // Quota of 0 causes every put() to fail permanently (survives retries)
    let mock = MockS5Backend::new();
    mock.set_quota_limit(0).await;

    publisher
        .buffer_message(
            "sess-fail",
            CheckpointMessage::new_user("test".to_string(), 100),
        )
        .await;
    publisher
        .buffer_message(
            "sess-fail",
            CheckpointMessage::new_assistant("reply".to_string(), 200, false),
        )
        .await;

    let result = publisher
        .publish_checkpoint("sess-fail", [1u8; 32], 0, 200, &key, &mock)
        .await;
    assert!(result.is_err(), "publish should fail with S5 error");

    let state = publisher.get_session_state("sess-fail").await.unwrap();
    // Buffer cleared and index incremented (lock phase 1 ran), but no index entry
    assert!(state.message_buffer.is_empty(), "buffer should be cleared");
    assert_eq!(
        state.checkpoint_index, 1,
        "checkpoint_index should be incremented"
    );
    assert!(
        state
            .index
            .as_ref()
            .map_or(true, |idx| idx.checkpoints.is_empty()),
        "index must have no entries after failed upload"
    );
}

/// Regression test: set_recovery_public_key must NOT be blocked by a concurrent
/// publish_checkpoint waiting on slow S5 uploads.
///
/// Buggy code (lock held during upload): FAILS with timeout.
/// Fixed code (lock released before upload): PASSES.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_set_recovery_key_not_blocked_by_publish() {
    let publisher = Arc::new(CheckpointPublisher::new("0xlock_test_host".to_string()));

    // Buffer messages so publish_checkpoint has work to do
    publisher
        .buffer_message(
            "sess-lock",
            CheckpointMessage::new_user("hello".to_string(), 100),
        )
        .await;
    publisher
        .buffer_message(
            "sess-lock",
            CheckpointMessage::new_assistant("world".to_string(), 200, false),
        )
        .await;

    // Spawn publish_checkpoint — acquires write lock, then sleeps 2s per upload
    let pub_clone = Arc::clone(&publisher);
    let _handle = tokio::spawn(async move {
        let slow_s5 = SlowMockS5::new();
        let key = generate_test_private_key();
        let _ = pub_clone
            .publish_checkpoint("sess-lock", [0u8; 32], 0, 200, &key, &slow_s5)
            .await;
    });

    // Give publish_checkpoint time to acquire the lock
    tokio::time::sleep(Duration::from_millis(100)).await;

    // If lock is NOT held during uploads, this completes in <1ms.
    // If lock IS held, this blocks ~4s and times out (test FAILS).
    let result = timeout(
        Duration::from_secs(1),
        publisher.set_recovery_public_key("sess-lock", "04abcdef1234".to_string()),
    )
    .await;

    assert!(
        result.is_ok(),
        "set_recovery_public_key blocked >1s — lock contention bug still present"
    );
}

/// Messages buffered AFTER lock phase 1 (during the unlocked upload window)
/// must not be lost — they belong to the NEXT checkpoint, not the current one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_buffer_message_during_upload_not_lost() {
    let publisher = Arc::new(CheckpointPublisher::new("0xbuf_test".to_string()));

    publisher
        .buffer_message(
            "sess-buf",
            CheckpointMessage::new_user("first".to_string(), 100),
        )
        .await;
    publisher
        .buffer_message(
            "sess-buf",
            CheckpointMessage::new_assistant("reply".to_string(), 200, false),
        )
        .await;

    // Spawn slow publish — lock phase 1 clears buffer, then uploads take ~4s
    let pub_clone = Arc::clone(&publisher);
    let handle = tokio::spawn(async move {
        let slow_s5 = SlowMockS5::new();
        let key = generate_test_private_key();
        pub_clone
            .publish_checkpoint("sess-buf", [0u8; 32], 0, 200, &key, &slow_s5)
            .await
    });

    // Wait for lock phase 1 to complete (buffer cleared, lock released)
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Buffer a new message while S5 upload is in progress (lock is free)
    publisher
        .buffer_message(
            "sess-buf",
            CheckpointMessage::new_user("second message".to_string(), 300),
        )
        .await;

    // Wait for publish to finish
    let _ = handle.await;

    // The new message must survive — it was added after clear_buffer ran
    let state = publisher.get_session_state("sess-buf").await.unwrap();
    assert_eq!(
        state.message_buffer.len(),
        1,
        "new message should be in buffer"
    );
    assert_eq!(state.message_buffer[0].content, "second message");
}
