//! TDD Tests for Session Key Store
//!
//! These tests define the expected behavior of the SessionKeyStore
//! BEFORE implementation. Following strict TDD methodology.

use fabstir_llm_node::crypto::SessionKeyStore;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_store_and_retrieve_key() {
    // Basic store and retrieve
    let store = SessionKeyStore::new();
    let session_id = "test-session-123".to_string();
    let key = [42u8; 32];

    // Store key
    store.store_key(session_id.clone(), key).await;

    // Retrieve key
    let retrieved = store.get_key(&session_id).await;
    assert_eq!(retrieved, Some(key), "Should retrieve stored key");
}

#[tokio::test]
async fn test_get_nonexistent_key() {
    // Getting a key that doesn't exist should return None
    let store = SessionKeyStore::new();
    let result = store.get_key("nonexistent-session").await;
    assert_eq!(result, None, "Nonexistent key should return None");
}

#[tokio::test]
async fn test_clear_key() {
    // Clearing a key should remove it from storage
    let store = SessionKeyStore::new();
    let session_id = "test-session-456".to_string();
    let key = [99u8; 32];

    // Store key
    store.store_key(session_id.clone(), key).await;

    // Verify it exists
    assert!(store.get_key(&session_id).await.is_some());

    // Clear the key
    store.clear_key(&session_id).await;

    // Verify it's gone
    let retrieved = store.get_key(&session_id).await;
    assert_eq!(retrieved, None, "Cleared key should not be retrievable");
}

#[tokio::test]
async fn test_concurrent_access() {
    // Multiple tasks should be able to access the store concurrently
    let store = SessionKeyStore::new();
    let store_clone1 = store.clone();
    let store_clone2 = store.clone();

    // Spawn two concurrent tasks
    let handle1 = tokio::spawn(async move {
        for i in 0..50 {
            let session_id = format!("session-a-{}", i);
            store_clone1.store_key(session_id, [i as u8; 32]).await;
        }
    });

    let handle2 = tokio::spawn(async move {
        for i in 0..50 {
            let session_id = format!("session-b-{}", i);
            store_clone2.store_key(session_id, [i as u8; 32]).await;
        }
    });

    // Wait for both tasks to complete
    handle1.await.unwrap();
    handle2.await.unwrap();

    // Should have 100 keys stored
    assert_eq!(store.count().await, 100, "Should have 100 keys from concurrent access");
}

#[tokio::test]
async fn test_key_expiration() {
    // Keys should expire after TTL
    let ttl = Duration::from_millis(100);
    let store = SessionKeyStore::with_ttl(ttl);

    let session_id = "expiring-session".to_string();
    let key = [77u8; 32];

    // Store key
    store.store_key(session_id.clone(), key).await;

    // Should be retrievable immediately
    assert!(store.get_key(&session_id).await.is_some(), "Key should exist immediately");

    // Wait for expiration
    sleep(Duration::from_millis(150)).await;

    // Manually trigger cleanup (in real usage, this would be automatic)
    store.clear_expired_keys().await;

    // Key should be gone
    let retrieved = store.get_key(&session_id).await;
    assert_eq!(retrieved, None, "Key should be expired after TTL");
}

#[tokio::test]
async fn test_multiple_sessions() {
    // Store should handle multiple independent sessions
    let store = SessionKeyStore::new();

    let sessions = vec![
        ("session-1", [1u8; 32]),
        ("session-2", [2u8; 32]),
        ("session-3", [3u8; 32]),
        ("session-4", [4u8; 32]),
        ("session-5", [5u8; 32]),
    ];

    // Store all keys
    for (id, key) in &sessions {
        store.store_key(id.to_string(), *key).await;
    }

    // Verify all keys are retrievable
    for (id, expected_key) in &sessions {
        let retrieved = store.get_key(id).await;
        assert_eq!(retrieved, Some(*expected_key), "Should retrieve correct key for {}", id);
    }

    // Count should match
    assert_eq!(store.count().await, sessions.len());
}

#[tokio::test]
async fn test_overwrite_existing_key() {
    // Storing a new key for an existing session should overwrite
    let store = SessionKeyStore::new();
    let session_id = "session-overwrite".to_string();

    let key1 = [11u8; 32];
    let key2 = [22u8; 32];

    // Store first key
    store.store_key(session_id.clone(), key1).await;
    assert_eq!(store.get_key(&session_id).await, Some(key1));

    // Overwrite with second key
    store.store_key(session_id.clone(), key2).await;
    assert_eq!(store.get_key(&session_id).await, Some(key2));

    // Count should still be 1
    assert_eq!(store.count().await, 1);
}

#[tokio::test]
async fn test_clear_all_keys() {
    // clear_all() should remove all keys
    let store = SessionKeyStore::new();

    // Store multiple keys
    for i in 0..10 {
        store.store_key(format!("session-{}", i), [i as u8; 32]).await;
    }

    assert_eq!(store.count().await, 10);

    // Clear all
    store.clear_all().await;

    // Count should be zero
    assert_eq!(store.count().await, 0);

    // None of the keys should be retrievable
    for i in 0..10 {
        assert_eq!(store.get_key(&format!("session-{}", i)).await, None);
    }
}

#[tokio::test]
async fn test_partial_expiration() {
    // Some keys expire while others remain valid
    let ttl = Duration::from_millis(100);
    let store = SessionKeyStore::with_ttl(ttl);

    // Store first batch of keys
    for i in 0..5 {
        store.store_key(format!("session-old-{}", i), [i as u8; 32]).await;
    }

    // Wait for first batch to approach expiration
    sleep(Duration::from_millis(60)).await;

    // Store second batch (these should not expire yet)
    for i in 0..5 {
        store.store_key(format!("session-new-{}", i), [i as u8; 32]).await;
    }

    // Wait for first batch to fully expire
    sleep(Duration::from_millis(60)).await;

    // Clear expired keys
    store.clear_expired_keys().await;

    // Old keys should be gone
    for i in 0..5 {
        assert_eq!(
            store.get_key(&format!("session-old-{}", i)).await,
            None,
            "Old session {} should be expired",
            i
        );
    }

    // New keys should still exist
    for i in 0..5 {
        assert!(
            store.get_key(&format!("session-new-{}", i)).await.is_some(),
            "New session {} should still exist",
            i
        );
    }
}

#[tokio::test]
async fn test_ttl_default_behavior() {
    // Store created without TTL should not expire keys
    let store = SessionKeyStore::new();
    let session_id = "persistent-session".to_string();
    let key = [88u8; 32];

    store.store_key(session_id.clone(), key).await;

    // Wait a while
    sleep(Duration::from_millis(100)).await;

    // Clear expired keys (should do nothing if no TTL)
    store.clear_expired_keys().await;

    // Key should still exist
    assert_eq!(
        store.get_key(&session_id).await,
        Some(key),
        "Key without TTL should not expire"
    );
}

#[tokio::test]
async fn test_clear_nonexistent_key() {
    // Clearing a key that doesn't exist should not panic
    let store = SessionKeyStore::new();
    store.clear_key("nonexistent-session").await;

    // Should complete without error
    assert_eq!(store.count().await, 0);
}

#[tokio::test]
async fn test_concurrent_reads() {
    // Multiple concurrent reads should work correctly
    let store = SessionKeyStore::new();
    let session_id = "shared-session".to_string();
    let key = [55u8; 32];

    store.store_key(session_id.clone(), key).await;

    // Spawn multiple readers
    let mut handles = vec![];
    for _ in 0..10 {
        let store_clone = store.clone();
        let session_id_clone = session_id.clone();
        let handle = tokio::spawn(async move {
            for _ in 0..10 {
                let retrieved = store_clone.get_key(&session_id_clone).await;
                assert_eq!(retrieved, Some(key));
            }
        });
        handles.push(handle);
    }

    // Wait for all readers
    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_store_updates_expiration() {
    // Storing a key again should reset its expiration time
    let ttl = Duration::from_millis(100);
    let store = SessionKeyStore::with_ttl(ttl);
    let session_id = "refresh-session".to_string();
    let key = [66u8; 32];

    // Store key
    store.store_key(session_id.clone(), key).await;

    // Wait almost until expiration
    sleep(Duration::from_millis(80)).await;

    // Store again (should refresh TTL)
    store.store_key(session_id.clone(), key).await;

    // Wait another 80ms (total 160ms from first store, but only 80ms from second)
    sleep(Duration::from_millis(80)).await;

    // Clear expired
    store.clear_expired_keys().await;

    // Key should still exist because we refreshed it
    assert!(
        store.get_key(&session_id).await.is_some(),
        "Refreshed key should not expire"
    );
}

#[tokio::test]
async fn test_empty_session_id() {
    // Should handle empty session IDs (though not recommended in practice)
    let store = SessionKeyStore::new();
    let key = [33u8; 32];

    store.store_key("".to_string(), key).await;
    assert_eq!(store.get_key("").await, Some(key));

    store.clear_key("").await;
    assert_eq!(store.get_key("").await, None);
}
