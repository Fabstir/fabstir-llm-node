use fabstir_llm_node::api::websocket::session::{WebSocketSession, SessionConfig};
use fabstir_llm_node::api::websocket::persistence::{SessionPersistence, PersistenceConfig};
use fabstir_llm_node::api::websocket::storage_trait::{SessionStorage, FileStorage};
use std::path::PathBuf;
use tempfile::TempDir;

async fn create_temp_persistence() -> (SessionPersistence, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let config = PersistenceConfig {
        base_path: temp_dir.path().to_path_buf(),
        enable_backups: true,
        backup_interval_seconds: 60,
    };
    let persistence = SessionPersistence::new(config);
    (persistence, temp_dir)
}

#[tokio::test]
async fn test_save_session_with_chain() {
    let (persistence, _temp_dir) = create_temp_persistence().await;

    // Create a session with chain_id
    let config = SessionConfig::default();
    let mut session = WebSocketSession::with_chain("test_session_1", config, 84532);

    // Add some data to the session
    session.add_message_async("user", "Hello").await.unwrap();
    session.add_message_async("assistant", "Hi there!").await.unwrap();

    // Save the session
    let result = persistence.save_session(&session).await;
    assert!(result.is_ok());

    // Verify file was created in correct chain directory
    let expected_path = persistence.get_session_path(84532, "test_session_1");
    assert!(expected_path.exists());
}

#[tokio::test]
async fn test_load_session_with_chain() {
    let (persistence, _temp_dir) = create_temp_persistence().await;

    // Create and save a session
    let config = SessionConfig::default();
    let mut original_session = WebSocketSession::with_chain("test_session_2", config, 5611);
    original_session.add_message_async("user", "Test message").await.unwrap();

    // Add metadata
    {
        let mut metadata = original_session.metadata.write().await;
        metadata.insert("test_key".to_string(), "test_value".to_string());
    }

    persistence.save_session(&original_session).await.unwrap();

    // Load the session back
    let loaded_session = persistence.load_session(5611, "test_session_2").await.unwrap();

    // Verify chain_id is preserved
    assert_eq!(loaded_session.chain_id, 5611);
    assert_eq!(loaded_session.id, "test_session_2");

    // Verify messages are preserved
    assert_eq!(loaded_session.conversation_history.len(), 1);
    assert_eq!(loaded_session.conversation_history[0].content, "Test message");

    // Verify metadata is preserved
    let metadata = loaded_session.metadata.read().await;
    assert_eq!(metadata.get("test_key"), Some(&"test_value".to_string()));
}

#[tokio::test]
async fn test_session_recovery_after_restart() {
    let temp_dir = TempDir::new().unwrap();
    let config = PersistenceConfig {
        base_path: temp_dir.path().to_path_buf(),
        enable_backups: false,
        backup_interval_seconds: 60,
    };

    // First "run" - save sessions
    {
        let persistence = SessionPersistence::new(config.clone());

        // Create sessions on different chains
        let session1 = WebSocketSession::with_chain("session_1", SessionConfig::default(), 84532);
        let session2 = WebSocketSession::with_chain("session_2", SessionConfig::default(), 84532);
        let session3 = WebSocketSession::with_chain("session_3", SessionConfig::default(), 5611);

        persistence.save_session(&session1).await.unwrap();
        persistence.save_session(&session2).await.unwrap();
        persistence.save_session(&session3).await.unwrap();
    }

    // Second "run" - recover sessions
    {
        let persistence = SessionPersistence::new(config);

        // Recover all sessions
        let recovered_sessions = persistence.recover_all_sessions().await.unwrap();

        assert_eq!(recovered_sessions.len(), 3);

        // Check sessions are on correct chains
        let base_sessions: Vec<_> = recovered_sessions
            .iter()
            .filter(|s| s.chain_id == 84532)
            .collect();
        assert_eq!(base_sessions.len(), 2);

        let opbnb_sessions: Vec<_> = recovered_sessions
            .iter()
            .filter(|s| s.chain_id == 5611)
            .collect();
        assert_eq!(opbnb_sessions.len(), 1);
    }
}

#[tokio::test]
async fn test_chain_specific_backup() {
    let (persistence, _temp_dir) = create_temp_persistence().await;

    // Create sessions on different chains
    let session1 = WebSocketSession::with_chain("base_1", SessionConfig::default(), 84532);
    let session2 = WebSocketSession::with_chain("base_2", SessionConfig::default(), 84532);
    let session3 = WebSocketSession::with_chain("opbnb_1", SessionConfig::default(), 5611);

    persistence.save_session(&session1).await.unwrap();
    persistence.save_session(&session2).await.unwrap();
    persistence.save_session(&session3).await.unwrap();

    // Create backup for Base Sepolia chain
    let backup_id = persistence.create_chain_backup(84532).await.unwrap();

    // Verify backup contains only Base Sepolia sessions
    let backup_path = persistence.get_backup_path(84532, &backup_id);
    assert!(backup_path.exists());

    // Count files in backup
    let backup_files: Vec<_> = std::fs::read_dir(&backup_path)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .collect();
    assert_eq!(backup_files.len(), 2); // Only the 2 Base Sepolia sessions

    // Create backup for opBNB chain
    let opbnb_backup_id = persistence.create_chain_backup(5611).await.unwrap();
    let opbnb_backup_path = persistence.get_backup_path(5611, &opbnb_backup_id);

    let opbnb_files: Vec<_> = std::fs::read_dir(&opbnb_backup_path)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .collect();
    assert_eq!(opbnb_files.len(), 1); // Only the 1 opBNB session
}

#[tokio::test]
async fn test_migrate_session_chain() {
    let (persistence, _temp_dir) = create_temp_persistence().await;

    // Create and save a session on Base Sepolia
    let mut session = WebSocketSession::with_chain("migrate_test", SessionConfig::default(), 84532);
    session.add_message_async("user", "Migration test").await.unwrap();
    persistence.save_session(&session).await.unwrap();

    // Verify it's saved in Base Sepolia directory
    let old_path = persistence.get_session_path(84532, "migrate_test");
    assert!(old_path.exists());

    // Migrate to opBNB
    persistence.migrate_session_chain("migrate_test", 84532, 5611).await.unwrap();

    // Verify old location is cleaned up
    assert!(!old_path.exists());

    // Verify new location exists
    let new_path = persistence.get_session_path(5611, "migrate_test");
    assert!(new_path.exists());

    // Load and verify the migrated session
    let migrated = persistence.load_session(5611, "migrate_test").await.unwrap();
    assert_eq!(migrated.chain_id, 5611);
    assert_eq!(migrated.id, "migrate_test");
    assert_eq!(migrated.conversation_history[0].content, "Migration test");
}

#[tokio::test]
async fn test_list_sessions_by_chain() {
    let (persistence, _temp_dir) = create_temp_persistence().await;

    // Create sessions on different chains
    for i in 0..3 {
        let session = WebSocketSession::with_chain(format!("base_{}", i), SessionConfig::default(), 84532);
        persistence.save_session(&session).await.unwrap();
    }

    for i in 0..2 {
        let session = WebSocketSession::with_chain(format!("opbnb_{}", i), SessionConfig::default(), 5611);
        persistence.save_session(&session).await.unwrap();
    }

    // List Base Sepolia sessions
    let base_sessions = persistence.list_sessions_by_chain(84532).await.unwrap();
    assert_eq!(base_sessions.len(), 3);

    // List opBNB sessions
    let opbnb_sessions = persistence.list_sessions_by_chain(5611).await.unwrap();
    assert_eq!(opbnb_sessions.len(), 2);

    // List non-existent chain
    let empty_sessions = persistence.list_sessions_by_chain(99999).await.unwrap();
    assert_eq!(empty_sessions.len(), 0);
}

#[tokio::test]
async fn test_delete_expired_sessions() {
    let (mut persistence, _temp_dir) = create_temp_persistence().await;

    // Create sessions
    let session1 = WebSocketSession::with_chain("keep_1", SessionConfig::default(), 84532);
    let mut session2 = WebSocketSession::with_chain("expired_1", SessionConfig::default(), 84532);

    // Mark session2 as expired by setting state to Closed
    session2.state = fabstir_llm_node::api::websocket::session::SessionState::Closed;

    persistence.save_session(&session1).await.unwrap();
    persistence.save_session(&session2).await.unwrap();

    // Clean up expired sessions
    let deleted = persistence.delete_expired_sessions().await.unwrap();
    assert_eq!(deleted, 1);

    // Verify only active session remains
    let remaining = persistence.list_sessions_by_chain(84532).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0], "keep_1");
}

#[tokio::test]
async fn test_restore_from_backup() {
    let (persistence, _temp_dir) = create_temp_persistence().await;

    // Create and save sessions
    let session1 = WebSocketSession::with_chain("restore_1", SessionConfig::default(), 84532);
    let session2 = WebSocketSession::with_chain("restore_2", SessionConfig::default(), 84532);

    persistence.save_session(&session1).await.unwrap();
    persistence.save_session(&session2).await.unwrap();

    // Create backup
    let backup_id = persistence.create_chain_backup(84532).await.unwrap();

    // Delete original sessions
    persistence.delete_session(84532, "restore_1").await.unwrap();
    persistence.delete_session(84532, "restore_2").await.unwrap();

    // Verify sessions are gone
    let sessions = persistence.list_sessions_by_chain(84532).await.unwrap();
    assert_eq!(sessions.len(), 0);

    // Restore from backup
    let restored = persistence.restore_from_backup(84532, &backup_id).await.unwrap();
    assert_eq!(restored, 2);

    // Verify sessions are back
    let sessions = persistence.list_sessions_by_chain(84532).await.unwrap();
    assert_eq!(sessions.len(), 2);
    assert!(sessions.contains(&"restore_1".to_string()));
    assert!(sessions.contains(&"restore_2".to_string()));
}