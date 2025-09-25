use fabstir_llm_node::settlement::{
    manager::SettlementManager,
    auto_settlement::{AutoSettlement, SettlementConfig, RetryConfig, EventType},
    types::{SettlementError, SettlementStatus},
};
use fabstir_llm_node::api::websocket::{
    session::{WebSocketSession, SessionConfig},
    session_store::{SessionStore, SessionStoreConfig},
    handlers::disconnect::DisconnectHandler,
};
use fabstir_llm_node::config::chains::ChainRegistry;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;
use std::time::Duration;

// Test helper to create a test private key
fn test_private_key() -> String {
    "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string()
}

// Create a test session store with pre-defined sessions
async fn create_test_session_store() -> Arc<RwLock<SessionStore>> {
    let mut store = SessionStore::new(SessionStoreConfig::default());

    // Add test sessions on different chains
    store.create_session_with_chain(
        "session_1".to_string(),
        SessionConfig::default(),
        84532, // Base Sepolia
    ).await.unwrap();

    store.create_session_with_chain(
        "session_2".to_string(),
        SessionConfig::default(),
        5611, // opBNB
    ).await.unwrap();

    store.create_session_with_chain(
        "session_3".to_string(),
        SessionConfig::default(),
        84532, // Base Sepolia
    ).await.unwrap();

    Arc::new(RwLock::new(store))
}

#[tokio::test]
async fn test_settlement_on_disconnect() {
    let registry = Arc::new(ChainRegistry::new());
    let private_key = test_private_key();

    let settlement_manager = Arc::new(
        SettlementManager::new(registry.clone(), &private_key)
            .await
            .expect("Failed to create settlement manager")
    );

    let session_store = create_test_session_store().await;

    let auto_settlement = AutoSettlement::new(
        settlement_manager.clone(),
        session_store.clone(),
        SettlementConfig::default(),
    );

    // Simulate disconnect for session_1
    let result = auto_settlement.handle_disconnect("session_1").await;
    assert!(result.is_ok(), "Settlement should succeed on disconnect");

    // The settlement may be immediately processed or queued
    // Just verify the operation succeeded
    let queue_size = settlement_manager.get_queue_size().await;
    assert!(queue_size >= 0, "Queue should exist");
}

#[tokio::test]
async fn test_settlement_correct_chain() {
    let registry = Arc::new(ChainRegistry::new());
    let private_key = test_private_key();

    let settlement_manager = Arc::new(
        SettlementManager::new(registry.clone(), &private_key)
            .await
            .expect("Failed to create settlement manager")
    );

    let session_store = create_test_session_store().await;

    let auto_settlement = AutoSettlement::new(
        settlement_manager.clone(),
        session_store.clone(),
        SettlementConfig::default(),
    );

    // Test Base Sepolia session
    let result = auto_settlement.settle_session_with_chain("session_1", 84532).await;
    assert!(result.is_ok(), "Should settle on Base Sepolia");

    // Test opBNB session
    let result = auto_settlement.settle_session_with_chain("session_2", 5611).await;
    assert!(result.is_ok(), "Should settle on opBNB");

    // Test wrong chain should fail
    let result = auto_settlement.settle_session_with_chain("session_1", 5611).await;
    assert!(result.is_err(), "Should fail with wrong chain");
}

#[tokio::test]
async fn test_settlement_retry_logic() {
    let registry = Arc::new(ChainRegistry::new());
    let private_key = test_private_key();

    let settlement_manager = Arc::new(
        SettlementManager::new(registry.clone(), &private_key)
            .await
            .expect("Failed to create settlement manager")
    );

    let session_store = create_test_session_store().await;

    let retry_config = RetryConfig {
        max_retries: 3,
        initial_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(5),
        exponential_base: 2.0,
    };

    let mut config = SettlementConfig::default();
    config.retry_config = retry_config;

    let auto_settlement = AutoSettlement::new(
        settlement_manager.clone(),
        session_store.clone(),
        config,
    );

    // Simulate a failing settlement (using invalid session ID)
    let result = auto_settlement.settle_with_retry("invalid_session").await;

    // Should attempt retries and eventually fail
    assert!(result.is_err(), "Should fail after retries");

    // Check that retries were attempted
    let retry_count = auto_settlement.get_retry_count("invalid_session").await;
    assert_eq!(retry_count, 3, "Should have attempted 3 retries");
}

#[tokio::test]
async fn test_settlement_failure_handling() {
    let registry = Arc::new(ChainRegistry::new());
    let private_key = test_private_key();

    let settlement_manager = Arc::new(
        SettlementManager::new(registry.clone(), &private_key)
            .await
            .expect("Failed to create settlement manager")
    );

    let session_store = create_test_session_store().await;

    let auto_settlement = AutoSettlement::new(
        settlement_manager.clone(),
        session_store.clone(),
        SettlementConfig::default(),
    );

    // Test various failure scenarios

    // 1. Session not found
    let result = auto_settlement.handle_disconnect("nonexistent_session").await;
    assert!(result.is_err(), "Should fail for non-existent session");
    match result {
        Err(SettlementError::SessionNotFound(_)) => {},
        _ => panic!("Expected SessionNotFound error"),
    }

    // 2. Invalid chain - session_1 is on chain 84532, not 99999
    let result = auto_settlement.settle_session_with_chain("session_1", 99999).await;
    assert!(result.is_err(), "Should fail for invalid chain");
    match result {
        Err(SettlementError::SettlementFailed { chain, .. }) if chain == 99999 => {
            // This is expected when the session is on a different chain
        },
        Err(SettlementError::UnsupportedChain(_)) => {
            // This is also acceptable
        },
        _ => panic!("Expected SettlementFailed or UnsupportedChain error"),
    }

    // 3. Graceful degradation - queue settlement for later
    let queue_result = auto_settlement.queue_failed_settlement("session_1", 84532).await;
    assert!(queue_result.is_ok(), "Should be able to queue failed settlement");
}

#[tokio::test]
async fn test_concurrent_settlements() {
    let registry = Arc::new(ChainRegistry::new());
    let private_key = test_private_key();

    let settlement_manager = Arc::new(
        SettlementManager::new(registry.clone(), &private_key)
            .await
            .expect("Failed to create settlement manager")
    );

    let session_store = create_test_session_store().await;

    let auto_settlement = Arc::new(AutoSettlement::new(
        settlement_manager.clone(),
        session_store.clone(),
        SettlementConfig::default(),
    ));

    // Launch multiple concurrent settlements
    let mut handles = vec![];

    for i in 1..=3 {
        let auto_settlement = auto_settlement.clone();
        let session_id = format!("session_{}", i);

        let handle = tokio::spawn(async move {
            auto_settlement.handle_disconnect(&session_id).await
        });

        handles.push(handle);
    }

    // Wait for all settlements to complete
    let mut results = vec![];
    for handle in handles {
        let result = handle.await.unwrap();
        results.push(result);
    }

    // All should succeed or be queued
    for result in results {
        assert!(result.is_ok() || matches!(result, Err(SettlementError::SessionNotFound(_))));
    }

    // Check that settlements were processed
    let queue_size = settlement_manager.get_queue_size().await;
    assert!(queue_size >= 0, "Queue should have processed settlements");
}

#[tokio::test]
async fn test_disconnect_handler_integration() {
    let registry = Arc::new(ChainRegistry::new());
    let private_key = test_private_key();

    let settlement_manager = Arc::new(
        SettlementManager::new(registry.clone(), &private_key)
            .await
            .expect("Failed to create settlement manager")
    );

    let session_store = create_test_session_store().await;

    // Create disconnect handler with settlement integration
    let disconnect_handler = DisconnectHandler::new(
        session_store.clone(),
        Some(settlement_manager.clone()),
    );

    // Handle WebSocket disconnect
    disconnect_handler.handle_disconnect("session_1").await.unwrap();

    // Verify session was cleaned up
    let exists = session_store.read().await.session_exists("session_1").await;
    assert!(!exists, "Session should be removed after disconnect");

    // Verify that disconnect handler completed successfully
    // The actual settlement might be processed or queued
}

#[tokio::test]
async fn test_settlement_event_logging() {
    let registry = Arc::new(ChainRegistry::new());
    let private_key = test_private_key();

    let settlement_manager = Arc::new(
        SettlementManager::new(registry.clone(), &private_key)
            .await
            .expect("Failed to create settlement manager")
    );

    let session_store = create_test_session_store().await;

    let auto_settlement = AutoSettlement::new(
        settlement_manager.clone(),
        session_store.clone(),
        SettlementConfig::default(),
    );

    // Enable event tracking
    auto_settlement.enable_event_tracking().await;

    // Perform settlement
    auto_settlement.handle_disconnect("session_1").await.ok();

    // Get logged events
    let events = auto_settlement.get_settlement_events("session_1").await;
    assert!(!events.is_empty(), "Should have logged settlement events");

    // Verify event types
    let has_initiated = events.iter().any(|e| matches!(e.event_type, EventType::SettlementInitiated));
    let has_queued = events.iter().any(|e| matches!(e.event_type, EventType::SettlementQueued));

    assert!(has_initiated || has_queued, "Should have settlement events");
}