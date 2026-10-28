// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! SCI-C D18: session keys keyed by connection and session, so a second
//! connection's init on the same session never replaces the first one's key.

use fabstir_llm_node::crypto::session_keys::ConnectionKeys;
use fabstir_llm_node::crypto::SessionKeyStore;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn two_connections_on_one_session_keep_their_own_keys() {
    let store = SessionKeyStore::new();
    store
        .store_key_for("conn-a", "session-1".into(), [1u8; 32])
        .await;
    store
        .store_key_for("conn-b", "session-1".into(), [2u8; 32])
        .await;
    assert_eq!(
        store.get_key_for("conn-a", "session-1").await,
        Some([1u8; 32])
    );
    assert_eq!(
        store.get_key_for("conn-b", "session-1").await,
        Some([2u8; 32])
    );
}

#[tokio::test]
async fn a_connection_cannot_read_another_connections_key() {
    let store = SessionKeyStore::new();
    store
        .store_key_for("conn-a", "session-1".into(), [1u8; 32])
        .await;
    assert_eq!(store.get_key_for("conn-b", "session-1").await, None);
}

#[tokio::test]
async fn removing_a_connection_leaves_the_others() {
    let store = SessionKeyStore::new();
    store
        .store_key_for("conn-a", "session-1".into(), [1u8; 32])
        .await;
    store
        .store_key_for("conn-a", "session-2".into(), [3u8; 32])
        .await;
    store
        .store_key_for("conn-b", "session-1".into(), [2u8; 32])
        .await;
    assert_eq!(store.remove_connection("conn-a").await, 2);
    assert_eq!(store.get_key_for("conn-a", "session-1").await, None);
    assert_eq!(store.get_key_for("conn-a", "session-2").await, None);
    assert_eq!(
        store.get_key_for("conn-b", "session-1").await,
        Some([2u8; 32])
    );
}

#[tokio::test]
async fn the_guard_clears_its_connections_keys_when_dropped() {
    let store = Arc::new(SessionKeyStore::new());
    let guard = ConnectionKeys::new(store.clone(), "conn-a".into());
    store
        .store_key_for(guard.conn_id(), "session-1".into(), [1u8; 32])
        .await;
    store
        .store_key_for("conn-b", "session-1".into(), [2u8; 32])
        .await;
    drop(guard);
    // The removal runs on a spawned task; give it a moment.
    for _ in 0..50 {
        if store.get_key_for("conn-a", "session-1").await.is_none() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(store.get_key_for("conn-a", "session-1").await, None);
    assert_eq!(
        store.get_key_for("conn-b", "session-1").await,
        Some([2u8; 32])
    );
}

#[tokio::test]
async fn connection_keys_expire_with_the_store_ttl() {
    let store = SessionKeyStore::with_ttl(Duration::from_millis(50));
    store
        .store_key_for("conn-a", "session-1".into(), [1u8; 32])
        .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(store.get_key_for("conn-a", "session-1").await, None);
    assert_eq!(store.clear_expired_keys().await, 1);
}

#[tokio::test]
async fn the_session_only_methods_are_unchanged_and_separate() {
    let store = SessionKeyStore::new();
    store.store_key("session-1".into(), [9u8; 32]).await;
    store
        .store_key_for("conn-a", "session-1".into(), [1u8; 32])
        .await;
    assert_eq!(store.get_key("session-1").await, Some([9u8; 32]));
    assert_eq!(
        store.get_key_for("conn-a", "session-1").await,
        Some([1u8; 32])
    );
    assert_eq!(store.count().await, 2);
    store.clear_all().await;
    assert_eq!(store.count().await, 0);
}

#[tokio::test]
async fn a_re_init_on_the_same_connection_replaces_only_its_own_key() {
    let store = SessionKeyStore::new();
    store
        .store_key_for("conn-a", "session-1".into(), [1u8; 32])
        .await;
    store
        .store_key_for("conn-b", "session-1".into(), [2u8; 32])
        .await;
    store
        .store_key_for("conn-a", "session-1".into(), [3u8; 32])
        .await;
    assert_eq!(
        store.get_key_for("conn-a", "session-1").await,
        Some([3u8; 32])
    );
    assert_eq!(
        store.get_key_for("conn-b", "session-1").await,
        Some([2u8; 32])
    );
    assert_eq!(store.count().await, 2);
}
