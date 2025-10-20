// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::api::websocket::manager::SessionManager;
use fabstir_llm_node::api::websocket::session::{SessionConfig, WebSocketSession};
use fabstir_llm_node::config::chains::ChainRegistry;

#[tokio::test]
async fn test_create_session_with_chain() {
    let manager = SessionManager::new();
    let config = SessionConfig::default();

    // Create session on Base Sepolia
    let session_id = "test_session_1";
    let result = manager
        .create_session(session_id, config.clone(), 84532)
        .await;
    assert!(result.is_ok());

    // Verify session was created with correct chain
    let session = manager.get_session(session_id).await;
    assert!(session.is_some());
    let session = session.unwrap();
    assert_eq!(session.id, session_id);
    assert_eq!(session.chain_id, 84532);

    // Create session on opBNB
    let session_id2 = "test_session_2";
    let result = manager.create_session(session_id2, config, 5611).await;
    assert!(result.is_ok());

    let session2 = manager.get_session(session_id2).await;
    assert!(session2.is_some());
    assert_eq!(session2.unwrap().chain_id, 5611);
}

#[tokio::test]
async fn test_get_session_chain() {
    let manager = SessionManager::new();
    let config = SessionConfig::default();

    // Create sessions on different chains
    manager
        .create_session("base_session", config.clone(), 84532)
        .await
        .unwrap();
    manager
        .create_session("opbnb_session", config, 5611)
        .await
        .unwrap();

    // Test get_session_chain method
    let chain = manager.get_session_chain("base_session").await;
    assert_eq!(chain, Some(84532));

    let chain = manager.get_session_chain("opbnb_session").await;
    assert_eq!(chain, Some(5611));

    // Non-existent session
    let chain = manager.get_session_chain("non_existent").await;
    assert_eq!(chain, None);
}

#[tokio::test]
async fn test_list_sessions_by_chain() {
    let manager = SessionManager::new();
    let config = SessionConfig::default();

    // Create multiple sessions on different chains
    manager
        .create_session("base_1", config.clone(), 84532)
        .await
        .unwrap();
    manager
        .create_session("base_2", config.clone(), 84532)
        .await
        .unwrap();
    manager
        .create_session("base_3", config.clone(), 84532)
        .await
        .unwrap();
    manager
        .create_session("opbnb_1", config.clone(), 5611)
        .await
        .unwrap();
    manager
        .create_session("opbnb_2", config, 5611)
        .await
        .unwrap();

    // List Base Sepolia sessions
    let base_sessions = manager.list_sessions_by_chain(84532).await;
    assert_eq!(base_sessions.len(), 3);
    let base_ids: Vec<String> = base_sessions.iter().map(|s| s.id.clone()).collect();
    assert!(base_ids.contains(&"base_1".to_string()));
    assert!(base_ids.contains(&"base_2".to_string()));
    assert!(base_ids.contains(&"base_3".to_string()));

    // List opBNB sessions
    let opbnb_sessions = manager.list_sessions_by_chain(5611).await;
    assert_eq!(opbnb_sessions.len(), 2);
    let opbnb_ids: Vec<String> = opbnb_sessions.iter().map(|s| s.id.clone()).collect();
    assert!(opbnb_ids.contains(&"opbnb_1".to_string()));
    assert!(opbnb_ids.contains(&"opbnb_2".to_string()));

    // Non-existent chain
    let empty_sessions = manager.list_sessions_by_chain(99999).await;
    assert_eq!(empty_sessions.len(), 0);
}

#[tokio::test]
async fn test_session_chain_stats() {
    let manager = SessionManager::new();
    let config = SessionConfig::default();

    // Create sessions on different chains
    manager
        .create_session("base_1", config.clone(), 84532)
        .await
        .unwrap();
    manager
        .create_session("base_2", config.clone(), 84532)
        .await
        .unwrap();
    manager
        .create_session("opbnb_1", config.clone(), 5611)
        .await
        .unwrap();
    manager
        .create_session("opbnb_2", config.clone(), 5611)
        .await
        .unwrap();
    manager
        .create_session("opbnb_3", config, 5611)
        .await
        .unwrap();

    // Get chain statistics
    let stats = manager.get_chain_statistics().await;

    assert_eq!(stats.total_sessions, 5);
    assert_eq!(stats.sessions_by_chain.get(&84532), Some(&2));
    assert_eq!(stats.sessions_by_chain.get(&5611), Some(&3));
    assert_eq!(stats.unique_chains, 2);

    // Check chain distribution percentages
    assert_eq!(stats.get_chain_percentage(84532), 40.0); // 2/5 = 40%
    assert_eq!(stats.get_chain_percentage(5611), 60.0); // 3/5 = 60%
}

#[tokio::test]
async fn test_cross_chain_session_query() {
    let manager = SessionManager::new();
    let config = SessionConfig::default();

    // Create sessions on different chains
    manager
        .create_session("session_1", config.clone(), 84532)
        .await
        .unwrap();
    manager
        .create_session("session_2", config.clone(), 5611)
        .await
        .unwrap();
    manager
        .create_session("session_3", config, 84532)
        .await
        .unwrap();

    // Query across all chains
    let all_sessions = manager.get_active_sessions().await;
    assert_eq!(all_sessions.len(), 3);

    // Query with chain filter
    let chain_filter = vec![84532, 5611];
    let filtered = manager.get_sessions_by_chains(&chain_filter).await;
    assert_eq!(filtered.len(), 3);

    // Query with partial chain filter
    let partial_filter = vec![84532];
    let partial = manager.get_sessions_by_chains(&partial_filter).await;
    assert_eq!(partial.len(), 2);
}

#[tokio::test]
async fn test_session_migration_to_chain() {
    let manager = SessionManager::new();
    let registry = ChainRegistry::new();

    // Create a legacy session without specifying chain
    let legacy_session = WebSocketSession::new("legacy_session");
    manager.register_session(legacy_session).await.unwrap();

    // Migrate session to specific chain
    let result = manager
        .migrate_session_to_chain("legacy_session", 5611, &registry)
        .await;
    assert!(result.is_ok());

    // Verify migration
    let migrated = manager.get_session("legacy_session").await.unwrap();
    assert_eq!(migrated.chain_id, 5611);

    // Try to migrate to invalid chain
    let result = manager
        .migrate_session_to_chain("legacy_session", 99999, &registry)
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_batch_migrate_sessions() {
    let manager = SessionManager::new();

    // Create multiple legacy sessions
    for i in 0..5 {
        let session = WebSocketSession::new(format!("legacy_{}", i));
        manager.register_session(session).await.unwrap();
    }

    // Batch migrate all sessions to opBNB
    let result = manager.migrate_all_sessions_to_chain(5611).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 5); // Should migrate 5 sessions

    // Verify all sessions are on opBNB
    let opbnb_sessions = manager.list_sessions_by_chain(5611).await;
    assert_eq!(opbnb_sessions.len(), 5);

    let base_sessions = manager.list_sessions_by_chain(84532).await;
    assert_eq!(base_sessions.len(), 0); // No sessions on Base anymore
}

#[tokio::test]
async fn test_session_chain_validation() {
    let manager = SessionManager::new();
    let config = SessionConfig::default();
    let registry = ChainRegistry::new();

    // Create session with validated chain
    let result = manager
        .create_session_validated("valid_session", config.clone(), 84532, &registry)
        .await;
    assert!(result.is_ok());

    // Try to create session with invalid chain
    let result = manager
        .create_session_validated("invalid_session", config, 99999, &registry)
        .await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Unsupported chain"));

    // Verify invalid session was not created
    let session = manager.get_session("invalid_session").await;
    assert!(session.is_none());
}
