use fabstir_llm_node::api::websocket::session::{SessionConfig, WebSocketSession};
use fabstir_llm_node::api::websocket::session_store::{SessionStore, SessionStoreConfig};
use fabstir_llm_node::job_processor::Message;
use std::time::Duration;
use uuid::Uuid;

#[tokio::test]
async fn test_store_creation() {
    let config = SessionStoreConfig::default();
    let store = SessionStore::new(config);

    assert_eq!(store.async_session_count().await, 0);
    assert_eq!(store.async_active_sessions().await, 0);
}

#[tokio::test]
async fn test_create_session() {
    let config = SessionStoreConfig::default();
    let mut store = SessionStore::new(config);

    let session_id = store.create_session(SessionConfig::default()).await;

    assert!(!session_id.is_empty());
    assert_eq!(store.async_session_count().await, 1);
    assert_eq!(store.async_active_sessions().await, 1);
}

#[tokio::test]
async fn test_get_session() {
    let config = SessionStoreConfig::default();
    let mut store = SessionStore::new(config);

    let session_id = store.create_session(SessionConfig::default()).await;

    let session = store.get_session(&session_id).await;
    assert!(session.is_some());
    assert_eq!(session.unwrap().id(), &session_id);
}

#[tokio::test]
async fn test_get_nonexistent_session() {
    let config = SessionStoreConfig::default();
    let store = SessionStore::new(config);

    let fake_id = Uuid::new_v4().to_string();
    let session = store.get_session(&fake_id).await;

    assert!(session.is_none());
}

#[tokio::test]
async fn test_update_session() {
    let config = SessionStoreConfig::default();
    let mut store = SessionStore::new(config);

    let session_id = store.create_session(SessionConfig::default()).await;

    let message = Message {
        role: "user".to_string(),
        content: "Test message".to_string(),
        timestamp: None,
    };

    let result = store.update_session(&session_id, message).await;
    assert!(result.is_ok());

    let session = store.get_session(&session_id).await.unwrap();
    assert_eq!(session.message_count(), 1);
}

#[tokio::test]
async fn test_update_nonexistent_session() {
    let config = SessionStoreConfig::default();
    let mut store = SessionStore::new(config);

    let fake_id = Uuid::new_v4().to_string();
    let message = Message {
        role: "user".to_string(),
        content: "Test message".to_string(),
        timestamp: None,
    };

    let result = store.update_session(&fake_id, message).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_destroy_session() {
    let config = SessionStoreConfig::default();
    let mut store = SessionStore::new(config);

    let session_id = store.create_session(SessionConfig::default()).await;
    assert_eq!(store.async_session_count().await, 1);

    let destroyed = store.destroy_session(&session_id).await;
    assert!(destroyed);
    assert_eq!(store.async_session_count().await, 0);

    // Should not be able to get destroyed session
    let session = store.get_session(&session_id).await;
    assert!(session.is_none());
}

#[tokio::test]
async fn test_destroy_nonexistent_session() {
    let config = SessionStoreConfig::default();
    let mut store = SessionStore::new(config);

    let fake_id = Uuid::new_v4().to_string();
    let destroyed = store.destroy_session(&fake_id).await;

    assert!(!destroyed);
}

#[tokio::test]
async fn test_max_sessions_limit() {
    let config = SessionStoreConfig {
        max_sessions: 3,
        ..Default::default()
    };
    let mut store = SessionStore::new(config);

    // Create 3 sessions (at limit)
    let _id1 = store.create_session(SessionConfig::default()).await;
    let _id2 = store.create_session(SessionConfig::default()).await;
    let _id3 = store.create_session(SessionConfig::default()).await;

    assert_eq!(store.async_session_count().await, 3);

    // Try to create 4th session - should fail
    let result = store.try_create_session(SessionConfig::default()).await;
    assert!(result.is_err());
    assert_eq!(store.async_session_count().await, 3);
}

#[tokio::test]
async fn test_cleanup_expired_sessions() {
    let config = SessionStoreConfig::default();
    let mut store = SessionStore::new(config);

    // Create sessions with very short timeout
    let short_config = SessionConfig {
        timeout_seconds: 1,
        ..Default::default()
    };

    let id1 = store.create_session(short_config.clone()).await;
    let id2 = store.create_session(short_config).await;
    let id3 = store.create_session(SessionConfig::default()).await; // Normal timeout

    assert_eq!(store.async_session_count().await, 3);

    // Wait for short timeout sessions to expire
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Clean up expired sessions
    let cleaned = store.cleanup_expired().await;
    assert_eq!(cleaned, 2);
    assert_eq!(store.async_session_count().await, 1);

    // Only the session with normal timeout should remain
    assert!(store.get_session(&id1).await.is_none());
    assert!(store.get_session(&id2).await.is_none());
    assert!(store.get_session(&id3).await.is_some());
}

#[tokio::test]
async fn test_get_all_sessions() {
    let config = SessionStoreConfig::default();
    let mut store = SessionStore::new(config);

    let id1 = store.create_session(SessionConfig::default()).await;
    let id2 = store.create_session(SessionConfig::default()).await;
    let id3 = store.create_session(SessionConfig::default()).await;

    let sessions = store.get_all_sessions().await;
    assert_eq!(sessions.len(), 3);

    let ids: Vec<String> = sessions.iter().map(|s| s.id().to_string()).collect();
    assert!(ids.contains(&id1));
    assert!(ids.contains(&id2));
    assert!(ids.contains(&id3));
}

#[tokio::test]
async fn test_clear_all_sessions() {
    let config = SessionStoreConfig::default();
    let mut store = SessionStore::new(config);

    // Create multiple sessions
    for _ in 0..5 {
        store.create_session(SessionConfig::default()).await;
    }

    assert_eq!(store.async_session_count().await, 5);

    // Clear all sessions
    store.clear_all().await;

    assert_eq!(store.async_session_count().await, 0);
    assert_eq!(store.async_active_sessions().await, 0);
}

#[tokio::test]
async fn test_session_metrics_tracking() {
    let config = SessionStoreConfig::default();
    let mut store = SessionStore::new(config);

    let id1 = store.create_session(SessionConfig::default()).await;
    let id2 = store.create_session(SessionConfig::default()).await;

    // Add messages to sessions
    let message = Message {
        role: "user".to_string(),
        content: "Test".to_string(),
        timestamp: None,
    };

    store.update_session(&id1, message.clone()).await.unwrap();
    store.update_session(&id2, message).await.unwrap();

    let metrics = store.get_store_metrics().await;

    assert_eq!(metrics.total_sessions, 2);
    assert_eq!(metrics.active_sessions, 2);
    assert!(metrics.total_messages >= 2);
    assert!(metrics.total_memory_bytes > 0);
}

#[tokio::test]
async fn test_concurrent_session_access() {
    let config = SessionStoreConfig::default();
    let store = SessionStore::new(config);
    let store_arc = std::sync::Arc::new(tokio::sync::RwLock::new(store));

    let session_id = store_arc
        .write()
        .await
        .create_session(SessionConfig::default())
        .await;

    // Spawn multiple tasks accessing the same session
    let mut handles = vec![];

    for i in 0..10 {
        let store_clone = store_arc.clone();
        let session_id_clone = session_id.clone();

        let handle = tokio::spawn(async move {
            let message = Message {
                role: "user".to_string(),
                content: format!("Message {}", i),
                timestamp: None,
            };

            store_clone
                .write()
                .await
                .update_session(&session_id_clone, message)
                .await
        });

        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    // Check that all messages were added
    let session = store_arc
        .read()
        .await
        .get_session(&session_id)
        .await
        .unwrap();
    assert_eq!(session.message_count(), 10);
}

#[tokio::test]
async fn test_session_store_config_defaults() {
    let config = SessionStoreConfig::default();

    assert_eq!(config.max_sessions, 1000);
    assert_eq!(config.cleanup_interval_seconds, 300); // 5 minutes
    assert!(config.enable_metrics);
    assert!(!config.enable_persistence);
}

#[tokio::test]
async fn test_session_exists() {
    let config = SessionStoreConfig::default();
    let mut store = SessionStore::new(config);

    let session_id = store.create_session(SessionConfig::default()).await;

    assert!(store.session_exists(&session_id).await);

    let fake_id = Uuid::new_v4().to_string();
    assert!(!store.session_exists(&fake_id).await);
}
