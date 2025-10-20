// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::api::websocket::messages::{
    SessionInitMessage, SessionResponse, ChainInfo, MessageValidator, LegacySessionInitMessage,
};
use serde_json::{json, Value};

#[tokio::test]
async fn test_session_init_with_chain() {
    // Test that SessionInitMessage includes chain_id field
    let msg = SessionInitMessage {
        job_id: 123,
        chain_id: Some(84532), // Base Sepolia
        user_address: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb7".to_string(),
        host_address: "0x5aAeb6053f3E94C9b9A09f33669435E7Ef1BeAed".to_string(),
        model_id: "llama-7b".to_string(),
        timestamp: 1640000000,
    };

    // Verify chain_id is present and correct
    assert_eq!(msg.chain_id, Some(84532));

    // Test serialization includes chain_id
    let json_str = serde_json::to_string(&msg).unwrap();
    let json_val: Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(json_val["chain_id"], 84532);
}

#[tokio::test]
async fn test_response_includes_chain() {
    // Test that response messages include chain information
    let chain_info = ChainInfo {
        chain_id: 5611, // opBNB Testnet
        chain_name: "opBNB Testnet".to_string(),
        native_token: "BNB".to_string(),
        rpc_url: "https://opbnb-testnet-rpc.bnbchain.org".to_string(),
    };

    let response = SessionResponse {
        session_id: 123,
        status: "active".to_string(),
        chain_info: Some(chain_info),
        tokens_used: 450,
        timestamp: 1640000000,
    };

    // Verify chain info is present
    assert!(response.chain_info.is_some());
    let chain = response.chain_info.unwrap();
    assert_eq!(chain.chain_id, 5611);
    assert_eq!(chain.chain_name, "opBNB Testnet");
    assert_eq!(chain.native_token, "BNB");

    // Test JSON serialization
    let json_str = serde_json::to_string(&response).unwrap();
    let json_val: Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(json_val["chain_info"]["chain_id"], 5611);
    assert_eq!(json_val["chain_info"]["native_token"], "BNB");
}

#[tokio::test]
async fn test_invalid_chain_in_message() {
    // Test that invalid chain_id is rejected
    let validator = MessageValidator::new();

    // Valid chains: Base Sepolia (84532) and opBNB Testnet (5611)
    let valid_msg = SessionInitMessage {
        job_id: 123,
        chain_id: Some(84532),
        user_address: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb7".to_string(),
        host_address: "0x5aAeb6053f3E94C9b9A09f33669435E7Ef1BeAed".to_string(),
        model_id: "llama-7b".to_string(),
        timestamp: 1640000000,
    };
    assert!(validator.validate_chain(&valid_msg).is_ok());

    // Invalid chain
    let invalid_msg = SessionInitMessage {
        job_id: 123,
        chain_id: Some(99999), // Invalid chain
        user_address: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb7".to_string(),
        host_address: "0x5aAeb6053f3E94C9b9A09f33669435E7Ef1BeAed".to_string(),
        model_id: "llama-7b".to_string(),
        timestamp: 1640000000,
    };
    assert!(validator.validate_chain(&invalid_msg).is_err());

    // Unsupported chain (Ethereum mainnet)
    let unsupported_msg = SessionInitMessage {
        job_id: 123,
        chain_id: Some(1), // Ethereum mainnet - not supported
        user_address: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb7".to_string(),
        host_address: "0x5aAeb6053f3E94C9b9A09f33669435E7Ef1BeAed".to_string(),
        model_id: "llama-7b".to_string(),
        timestamp: 1640000000,
    };
    assert!(validator.validate_chain(&unsupported_msg).is_err());
}

#[tokio::test]
async fn test_legacy_message_compatibility() {
    // Test that messages without chain_id still work (default to Base Sepolia)
    let legacy_json = json!({
        "job_id": 123,
        "user_address": "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb7",
        "host_address": "0x5aAeb6053f3E94C9b9A09f33669435E7Ef1BeAed",
        "model_id": "llama-7b",
        "timestamp": 1640000000
    });

    // Should deserialize without chain_id
    let msg: SessionInitMessage = serde_json::from_value(legacy_json.clone()).unwrap();
    assert_eq!(msg.chain_id, None); // No chain_id in legacy message

    // Legacy message type for backwards compatibility
    let legacy_msg: LegacySessionInitMessage = serde_json::from_value(legacy_json).unwrap();
    assert_eq!(legacy_msg.job_id, 123);

    // Convert legacy to new format with default chain
    let converted = SessionInitMessage::from_legacy(legacy_msg);
    assert_eq!(converted.chain_id, Some(84532)); // Default to Base Sepolia
}

#[tokio::test]
async fn test_message_serialization() {
    // Test round-trip serialization with chain info
    let original = SessionInitMessage {
        job_id: 456,
        chain_id: Some(5611),
        user_address: "0x123456789abcdef".to_string(),
        host_address: "0xfedcba987654321".to_string(),
        model_id: "gpt-4".to_string(),
        timestamp: 1640000000,
    };

    // Serialize to JSON
    let json_str = serde_json::to_string(&original).unwrap();

    // Deserialize back
    let deserialized: SessionInitMessage = serde_json::from_str(&json_str).unwrap();

    // Verify all fields match
    assert_eq!(original.job_id, deserialized.job_id);
    assert_eq!(original.chain_id, deserialized.chain_id);
    assert_eq!(original.user_address, deserialized.user_address);
    assert_eq!(original.host_address, deserialized.host_address);
    assert_eq!(original.model_id, deserialized.model_id);
    assert_eq!(original.timestamp, deserialized.timestamp);

    // Test with missing optional chain_id
    let json_without_chain = json!({
        "job_id": 789,
        "user_address": "0xabc",
        "host_address": "0xdef",
        "model_id": "llama",
        "timestamp": 1640000000
    });

    let msg: SessionInitMessage = serde_json::from_value(json_without_chain).unwrap();
    assert_eq!(msg.chain_id, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_info_display() {
        let chain_info = ChainInfo {
            chain_id: 84532,
            chain_name: "Base Sepolia".to_string(),
            native_token: "ETH".to_string(),
            rpc_url: "https://sepolia.base.org".to_string(),
        };

        // Test that chain info can be formatted for display
        let display_str = format!("{} ({})", chain_info.chain_name, chain_info.chain_id);
        assert_eq!(display_str, "Base Sepolia (84532)");
    }

    #[test]
    fn test_supported_chains() {
        let validator = MessageValidator::new();

        // Test all supported chains
        assert!(validator.is_chain_supported(84532)); // Base Sepolia
        assert!(validator.is_chain_supported(5611));  // opBNB Testnet

        // Test unsupported chains
        assert!(!validator.is_chain_supported(1));     // Ethereum mainnet
        assert!(!validator.is_chain_supported(137));   // Polygon
        assert!(!validator.is_chain_supported(42161)); // Arbitrum
    }
}