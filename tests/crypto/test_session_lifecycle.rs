// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Session Lifecycle Integration Tests (TDD - Phase 6.2.1, Sub-phase 3.2)
//!
//! These tests verify that session keys are properly integrated with the
//! session lifecycle: stored on init, retrieved for decryption, and cleared
//! on disconnect/timeout.
//!
//! **TDD Approach**: Tests written BEFORE implementation.

use fabstir_llm_node::crypto::SessionKeyStore;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;

/// Mock session state to simulate ApiServer session tracking
struct MockSessionState {
    session_key_store: SessionKeyStore,
    active_sessions: Arc<RwLock<Vec<String>>>,
}

impl MockSessionState {
    fn new() -> Self {
        Self {
            session_key_store: SessionKeyStore::new(),
            active_sessions: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn with_ttl(ttl: Duration) -> Self {
        Self {
            session_key_store: SessionKeyStore::with_ttl(ttl),
            active_sessions: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Simulate successful session init - stores session key
    async fn handle_session_init(&self, session_id: String, session_key: [u8; 32]) {
        self.session_key_store
            .store_key(session_id.clone(), session_key)
            .await;
        self.active_sessions.write().await.push(session_id);
    }

    /// Simulate retrieving session key for message decryption
    async fn get_session_key_for_decryption(&self, session_id: &str) -> Option<[u8; 32]> {
        self.session_key_store.get_key(session_id).await
    }

    /// Simulate WebSocket disconnect - clears session key
    async fn handle_disconnect(&self, session_id: &str) {
        self.session_key_store.clear_key(session_id).await;
        let mut sessions = self.active_sessions.write().await;
        sessions.retain(|s| s != session_id);
    }

    /// Simulate timeout cleanup - clears all expired keys
    async fn handle_timeout_cleanup(&self) -> usize {
        self.session_key_store.clear_expired_keys().await
    }

    /// Check if session is active
    async fn is_session_active(&self, session_id: &str) -> bool {
        self.active_sessions
            .read()
            .await
            .contains(&session_id.to_string())
    }

    /// Get session key count
    async fn session_count(&self) -> usize {
        self.session_key_store.count().await
    }
}

#[tokio::test]
async fn test_session_key_stored_on_init() {
    // Test that session keys are stored when session is initialized
    let state = MockSessionState::new();
    let session_id = "test-session-123".to_string();
    let session_key = [42u8; 32];

    // Verify session doesn't exist initially
    assert_eq!(state.session_count().await, 0);
    assert!(state
        .get_session_key_for_decryption(&session_id)
        .await
        .is_none());

    // Handle session init
    state
        .handle_session_init(session_id.clone(), session_key)
        .await;

    // Verify session key is stored
    assert_eq!(state.session_count().await, 1);
    assert!(state.is_session_active(&session_id).await);
    let retrieved_key = state
        .get_session_key_for_decryption(&session_id)
        .await
        .expect("Session key should be stored");
    assert_eq!(retrieved_key, session_key);
}

#[tokio::test]
async fn test_session_key_used_for_decryption() {
    // Test that session keys can be retrieved for message decryption
    let state = MockSessionState::new();
    let session_id = "decrypt-session-456".to_string();
    let session_key = [99u8; 32];

    // Initialize session
    state
        .handle_session_init(session_id.clone(), session_key)
        .await;

    // Simulate multiple message decryptions using the same session key
    for _ in 0..5 {
        let retrieved_key = state
            .get_session_key_for_decryption(&session_id)
            .await
            .expect("Session key should be available for decryption");
        assert_eq!(
            retrieved_key, session_key,
            "Key should be consistent across multiple retrievals"
        );
    }
}

#[tokio::test]
async fn test_session_key_cleared_on_disconnect() {
    // Test that session keys are cleared when WebSocket disconnects
    let state = MockSessionState::new();
    let session_id = "disconnect-session-789".to_string();
    let session_key = [77u8; 32];

    // Initialize session
    state
        .handle_session_init(session_id.clone(), session_key)
        .await;
    assert_eq!(state.session_count().await, 1);
    assert!(state.is_session_active(&session_id).await);

    // Handle disconnect
    state.handle_disconnect(&session_id).await;

    // Verify session key is cleared
    assert_eq!(state.session_count().await, 0);
    assert!(!state.is_session_active(&session_id).await);
    assert!(state
        .get_session_key_for_decryption(&session_id)
        .await
        .is_none());
}

#[tokio::test]
async fn test_session_key_cleared_on_timeout() {
    // Test that session keys are automatically cleared after TTL expires
    let ttl = Duration::from_millis(100);
    let state = MockSessionState::with_ttl(ttl);
    let session_id = "timeout-session-abc".to_string();
    let session_key = [11u8; 32];

    // Initialize session
    state
        .handle_session_init(session_id.clone(), session_key)
        .await;
    assert_eq!(state.session_count().await, 1);

    // Key should be available immediately
    assert!(state
        .get_session_key_for_decryption(&session_id)
        .await
        .is_some());

    // Wait for TTL to expire
    sleep(ttl + Duration::from_millis(50)).await;

    // Run timeout cleanup
    let cleared_count = state.handle_timeout_cleanup().await;
    assert_eq!(cleared_count, 1, "Should clear 1 expired key");

    // Verify key is cleared
    assert_eq!(state.session_count().await, 0);
    assert!(state
        .get_session_key_for_decryption(&session_id)
        .await
        .is_none());
}

#[tokio::test]
async fn test_session_without_encryption() {
    // Test that sessions can work without encryption (backward compatibility)
    let state = MockSessionState::new();
    let session_id = "plaintext-session-xyz".to_string();

    // Session without encryption - no key stored
    state.active_sessions.write().await.push(session_id.clone());

    // Session is active but has no encryption key
    assert!(state.is_session_active(&session_id).await);
    assert!(state
        .get_session_key_for_decryption(&session_id)
        .await
        .is_none());
    assert_eq!(
        state.session_count().await,
        0,
        "No keys stored for plaintext session"
    );
}

#[tokio::test]
async fn test_multiple_concurrent_sessions() {
    // Test managing multiple sessions concurrently
    let state = MockSessionState::new();
    let sessions = vec![
        ("session-1".to_string(), [1u8; 32]),
        ("session-2".to_string(), [2u8; 32]),
        ("session-3".to_string(), [3u8; 32]),
    ];

    // Initialize all sessions
    for (session_id, session_key) in &sessions {
        state
            .handle_session_init(session_id.clone(), *session_key)
            .await;
    }

    // Verify all sessions are active
    assert_eq!(state.session_count().await, 3);
    for (session_id, expected_key) in &sessions {
        assert!(state.is_session_active(session_id).await);
        let retrieved_key = state
            .get_session_key_for_decryption(session_id)
            .await
            .expect("Each session should have its key");
        assert_eq!(retrieved_key, *expected_key);
    }

    // Disconnect one session
    state.handle_disconnect(&sessions[1].0).await;

    // Verify only that session is cleared
    assert_eq!(state.session_count().await, 2);
    assert!(state.is_session_active(&sessions[0].0).await);
    assert!(!state.is_session_active(&sessions[1].0).await);
    assert!(state.is_session_active(&sessions[2].0).await);
}

#[tokio::test]
async fn test_session_key_retrieval_nonexistent() {
    // Test that retrieving key for nonexistent session returns None
    let state = MockSessionState::new();
    let nonexistent_session = "does-not-exist".to_string();

    let result = state
        .get_session_key_for_decryption(&nonexistent_session)
        .await;
    assert!(
        result.is_none(),
        "Should return None for nonexistent session"
    );
}

#[tokio::test]
async fn test_disconnect_nonexistent_session() {
    // Test that disconnecting a nonexistent session doesn't cause errors
    let state = MockSessionState::new();
    let session_id = "valid-session".to_string();
    let session_key = [88u8; 32];

    // Initialize one session
    state
        .handle_session_init(session_id.clone(), session_key)
        .await;
    assert_eq!(state.session_count().await, 1);

    // Disconnect nonexistent session
    state.handle_disconnect("nonexistent").await;

    // Verify the valid session is unaffected
    assert_eq!(state.session_count().await, 1);
    assert!(state.is_session_active(&session_id).await);
}

#[tokio::test]
async fn test_session_key_overwrite() {
    // Test that re-initializing a session with new key overwrites the old one
    let state = MockSessionState::new();
    let session_id = "overwrite-session".to_string();
    let old_key = [10u8; 32];
    let new_key = [20u8; 32];

    // Initialize with old key
    state.handle_session_init(session_id.clone(), old_key).await;
    let retrieved = state.get_session_key_for_decryption(&session_id).await;
    assert_eq!(retrieved, Some(old_key));

    // Re-initialize with new key
    state.handle_session_init(session_id.clone(), new_key).await;
    let retrieved = state.get_session_key_for_decryption(&session_id).await;
    assert_eq!(
        retrieved,
        Some(new_key),
        "Key should be updated to new value"
    );
}

#[tokio::test]
async fn test_partial_timeout_cleanup() {
    // Test that timeout cleanup only clears expired keys, not all keys
    let ttl = Duration::from_millis(150);
    let state = MockSessionState::with_ttl(ttl);

    // Create session 1
    state
        .handle_session_init("session-1".to_string(), [1u8; 32])
        .await;

    // Wait for session 1 to partially age
    sleep(Duration::from_millis(80)).await;

    // Create session 2 (newer)
    state
        .handle_session_init("session-2".to_string(), [2u8; 32])
        .await;

    // Wait for session 1 to expire but not session 2
    sleep(Duration::from_millis(80)).await;

    // Run cleanup
    let cleared_count = state.handle_timeout_cleanup().await;
    assert_eq!(cleared_count, 1, "Should clear only expired session 1");
    assert_eq!(state.session_count().await, 1, "Session 2 should remain");

    // Verify session 1 is gone but session 2 remains
    assert!(state
        .get_session_key_for_decryption("session-1")
        .await
        .is_none());
    assert!(state
        .get_session_key_for_decryption("session-2")
        .await
        .is_some());
}

#[tokio::test]
async fn test_no_timeout_without_ttl() {
    // Test that sessions without TTL never expire
    let state = MockSessionState::new(); // No TTL
    let session_id = "no-timeout-session".to_string();
    let session_key = [33u8; 32];

    // Initialize session
    state
        .handle_session_init(session_id.clone(), session_key)
        .await;

    // Wait longer than typical TTL
    sleep(Duration::from_millis(200)).await;

    // Run cleanup (should clear 0 keys)
    let cleared_count = state.handle_timeout_cleanup().await;
    assert_eq!(cleared_count, 0, "No keys should expire without TTL");

    // Verify key still exists
    assert_eq!(state.session_count().await, 1);
    assert!(state
        .get_session_key_for_decryption(&session_id)
        .await
        .is_some());
}

#[tokio::test]
async fn test_session_lifecycle_complete_flow() {
    // Integration test: complete session lifecycle from init to disconnect
    let state = MockSessionState::new();
    let session_id = "complete-lifecycle".to_string();
    let session_key = [55u8; 32];

    // 1. Session init
    state
        .handle_session_init(session_id.clone(), session_key)
        .await;
    assert!(state.is_session_active(&session_id).await);
    assert_eq!(state.session_count().await, 1);

    // 2. Multiple message decryptions
    for _ in 0..10 {
        let key = state
            .get_session_key_for_decryption(&session_id)
            .await
            .expect("Key should be available during session");
        assert_eq!(key, session_key);
    }

    // 3. Session remains active
    assert!(state.is_session_active(&session_id).await);
    assert_eq!(state.session_count().await, 1);

    // 4. Disconnect
    state.handle_disconnect(&session_id).await;

    // 5. Verify cleanup
    assert!(!state.is_session_active(&session_id).await);
    assert_eq!(state.session_count().await, 0);
    assert!(state
        .get_session_key_for_decryption(&session_id)
        .await
        .is_none());
}

#[tokio::test]
async fn test_session_key_isolation() {
    // Test that session keys are properly isolated between sessions
    let state = MockSessionState::new();
    let session1 = ("session-alpha".to_string(), [100u8; 32]);
    let session2 = ("session-beta".to_string(), [200u8; 32]);

    // Initialize both sessions
    state
        .handle_session_init(session1.0.clone(), session1.1)
        .await;
    state
        .handle_session_init(session2.0.clone(), session2.1)
        .await;

    // Verify each session gets its own key
    let key1 = state
        .get_session_key_for_decryption(&session1.0)
        .await
        .unwrap();
    let key2 = state
        .get_session_key_for_decryption(&session2.0)
        .await
        .unwrap();

    assert_eq!(key1, session1.1);
    assert_eq!(key2, session2.1);
    assert_ne!(key1, key2, "Sessions should have different keys");
}
