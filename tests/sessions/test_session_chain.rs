use fabstir_llm_node::api::websocket::session::{WebSocketSession, SessionConfig, SessionChainInfo};
use fabstir_llm_node::config::chains::ChainRegistry;
use serde_json;

#[tokio::test]
async fn test_session_with_chain_id() {
    let config = SessionConfig::default();
    let session = WebSocketSession::with_chain("test_session_1", config, 84532);

    assert_eq!(session.id, "test_session_1");
    assert_eq!(session.chain_id, 84532);
    assert_eq!(session.get_chain_id(), 84532);

    // Verify chain info is set correctly
    let chain_info = session.get_chain_info();
    assert_eq!(chain_info.chain_id, 84532);
    assert_eq!(chain_info.chain_name, "Base Sepolia");
    assert_eq!(chain_info.native_token, "ETH");
}

#[tokio::test]
async fn test_session_chain_validation() {
    let registry = ChainRegistry::new();

    // Valid chain ID (Base Sepolia)
    let config = SessionConfig::default();
    let result = WebSocketSession::with_validated_chain("test_session_2", config.clone(), 84532, &registry);
    assert!(result.is_ok());
    let session = result.unwrap();
    assert_eq!(session.chain_id, 84532);

    // Valid chain ID (opBNB)
    let result = WebSocketSession::with_validated_chain("test_session_3", config.clone(), 5611, &registry);
    assert!(result.is_ok());
    let session = result.unwrap();
    assert_eq!(session.chain_id, 5611);

    // Invalid chain ID
    let result = WebSocketSession::with_validated_chain("test_session_4", config, 99999, &registry);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Unsupported chain"));
}

#[tokio::test]
async fn test_legacy_session_migration() {
    // Create a session without chain_id (legacy)
    let legacy_session = WebSocketSession::new("legacy_session");

    // Migrate to chain-aware session (should default to Base Sepolia)
    let migrated_session = WebSocketSession::migrate_to_chain_aware(legacy_session);

    assert_eq!(migrated_session.id, "legacy_session");
    assert_eq!(migrated_session.chain_id, 84532); // Default to Base Sepolia

    // Verify chain info is set correctly
    let chain_info = migrated_session.get_chain_info();
    assert_eq!(chain_info.chain_id, 84532);
    assert_eq!(chain_info.chain_name, "Base Sepolia");
}

#[tokio::test]
async fn test_session_serialization() {
    let config = SessionConfig::default();
    let session = WebSocketSession::with_chain("test_session_5", config, 5611);

    // Add some metadata
    {
        let mut metadata = session.metadata.write().await;
        metadata.insert("user".to_string(), "test_user".to_string());
        metadata.insert("job_id".to_string(), "12345".to_string());
    }

    // Serialize to JSON
    let serialized = session.to_json().await.unwrap();
    let json_value: serde_json::Value = serde_json::from_str(&serialized).unwrap();

    // Check chain_id is serialized
    assert_eq!(json_value["chain_id"], 5611);
    assert_eq!(json_value["id"], "test_session_5");

    // Deserialize back
    let deserialized = WebSocketSession::from_json(&serialized).await.unwrap();

    assert_eq!(deserialized.id, "test_session_5");
    assert_eq!(deserialized.chain_id, 5611);

    // Check metadata preserved
    let metadata = deserialized.metadata.read().await;
    assert_eq!(metadata.get("user"), Some(&"test_user".to_string()));
    assert_eq!(metadata.get("job_id"), Some(&"12345".to_string()));
}

#[tokio::test]
async fn test_invalid_chain_rejection() {
    let registry = ChainRegistry::new();
    let config = SessionConfig::default();

    // Test various invalid chain IDs
    let invalid_chains = vec![0, 1, 999, 12345, u64::MAX];

    for invalid_chain in invalid_chains {
        let result = WebSocketSession::with_validated_chain(
            &format!("invalid_session_{}", invalid_chain),
            config.clone(),
            invalid_chain,
            &registry
        );

        assert!(result.is_err(), "Chain {} should be rejected", invalid_chain);
        let error = result.unwrap_err();
        assert!(error.to_string().contains("Unsupported chain") ||
                error.to_string().contains("Invalid chain"));
    }
}

#[tokio::test]
async fn test_session_chain_info() {
    // Test Base Sepolia
    let session = WebSocketSession::with_chain("base_session", SessionConfig::default(), 84532);
    let chain_info = session.get_chain_info();

    assert_eq!(chain_info.chain_id, 84532);
    assert_eq!(chain_info.chain_name, "Base Sepolia");
    assert_eq!(chain_info.native_token, "ETH");
    assert_eq!(chain_info.native_token_decimals, 18);

    // Test opBNB
    let session = WebSocketSession::with_chain("opbnb_session", SessionConfig::default(), 5611);
    let chain_info = session.get_chain_info();

    assert_eq!(chain_info.chain_id, 5611);
    assert_eq!(chain_info.chain_name, "opBNB Testnet");
    assert_eq!(chain_info.native_token, "BNB");
    assert_eq!(chain_info.native_token_decimals, 18);
}

#[tokio::test]
async fn test_session_chain_switch() {
    let config = SessionConfig::default();
    let mut session = WebSocketSession::with_chain("switch_session", config, 84532);

    assert_eq!(session.chain_id, 84532);

    // Attempt to switch to another valid chain
    let registry = ChainRegistry::new();
    let result = session.switch_chain(5611, &registry);

    assert!(result.is_ok());
    assert_eq!(session.chain_id, 5611);

    // Verify chain info updated
    let chain_info = session.get_chain_info();
    assert_eq!(chain_info.chain_name, "opBNB Testnet");

    // Attempt to switch to invalid chain
    let result = session.switch_chain(99999, &registry);
    assert!(result.is_err());
    assert_eq!(session.chain_id, 5611); // Should remain on previous valid chain
}

#[tokio::test]
async fn test_session_default_chain() {
    // When no chain is specified, should use default from registry
    let session = WebSocketSession::with_default_chain("default_session", SessionConfig::default());

    let registry = ChainRegistry::new();
    let default_chain = registry.default_chain();

    assert_eq!(session.chain_id, default_chain);
    assert_eq!(session.chain_id, 84532); // Base Sepolia is default
}