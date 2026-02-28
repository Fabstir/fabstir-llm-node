// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::api::{
    ApiError, ErrorResponse, InferenceResponse, ModelInfo, ModelsResponse, StreamingResponse,
};
use fabstir_llm_node::blockchain::{ChainConfig, ChainRegistry};
use serde_json::json;
use std::collections::HashMap;

fn setup_test_env() {
    std::env::set_var("RUST_LOG", "debug");
}

#[test]
fn test_inference_response_chain() {
    setup_test_env();

    let response = InferenceResponse {
        model: "tinyllama".to_string(),
        content: "Hello, world!".to_string(),
        tokens_used: 10,
        finish_reason: "stop".to_string(),
        request_id: "test-123".to_string(),
        chain_id: Some(84532),
        chain_name: Some("Base Sepolia".to_string()),
        native_token: Some("ETH".to_string()),
        web_search_performed: None,
        search_queries_count: None,
        search_provider: None,
        usage: None,
    };

    // Serialize and check
    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json["chain_id"], 84532);
    assert_eq!(json["chain_name"], "Base Sepolia");
    assert_eq!(json["native_token"], "ETH");
    assert_eq!(json["content"], "Hello, world!");
}

#[test]
fn test_native_token_in_response() {
    setup_test_env();

    // Test Base Sepolia response
    let base_response = InferenceResponse {
        model: "model1".to_string(),
        content: "Response".to_string(),
        tokens_used: 5,
        finish_reason: "stop".to_string(),
        request_id: "req-1".to_string(),
        chain_id: Some(84532),
        chain_name: Some("Base Sepolia".to_string()),
        native_token: Some("ETH".to_string()),
        web_search_performed: None,
        search_queries_count: None,
        search_provider: None,
        usage: None,
    };

    assert_eq!(base_response.native_token, Some("ETH".to_string()));

    // Test opBNB response
    let opbnb_response = InferenceResponse {
        model: "model1".to_string(),
        content: "Response".to_string(),
        tokens_used: 5,
        finish_reason: "stop".to_string(),
        request_id: "req-2".to_string(),
        chain_id: Some(5611),
        chain_name: Some("opBNB Testnet".to_string()),
        native_token: Some("BNB".to_string()),
        web_search_performed: None,
        search_queries_count: None,
        search_provider: None,
        usage: None,
    };

    assert_eq!(opbnb_response.native_token, Some("BNB".to_string()));
}

#[test]
fn test_chain_name_included() {
    setup_test_env();

    let registry = ChainRegistry::new();

    // Test that we can get chain names
    let base_chain = registry.get_chain(84532).unwrap();
    assert_eq!(base_chain.name, "Base Sepolia");

    // Create response with chain name
    let response = InferenceResponse {
        model: "test".to_string(),
        content: "test".to_string(),
        tokens_used: 1,
        finish_reason: "stop".to_string(),
        request_id: "test".to_string(),
        chain_id: Some(base_chain.chain_id),
        chain_name: Some(base_chain.name.clone()),
        native_token: Some(base_chain.native_token.symbol.clone()),
        web_search_performed: None,
        search_queries_count: None,
        search_provider: None,
        usage: None,
    };

    assert_eq!(response.chain_name, Some("Base Sepolia".to_string()));
}

#[test]
fn test_error_with_chain_context() {
    setup_test_env();

    // Create an error response with chain context
    let mut details = HashMap::new();
    details.insert("chain_id".to_string(), json!(84532));
    details.insert("chain_name".to_string(), json!("Base Sepolia"));
    details.insert("native_token".to_string(), json!("ETH"));

    let error_response = ErrorResponse {
        error_type: "model_not_found".to_string(),
        message: "Model not available on Base Sepolia".to_string(),
        request_id: Some("req-123".to_string()),
        details: Some(details),
        chain_id: Some(84532),
    };

    assert_eq!(error_response.chain_id, Some(84532));

    // Check details contain chain info
    let details = error_response.details.unwrap();
    assert_eq!(details["chain_id"], json!(84532));
    assert_eq!(details["chain_name"], json!("Base Sepolia"));
    assert_eq!(details["native_token"], json!("ETH"));
}

#[test]
fn test_streaming_response_with_chain() {
    setup_test_env();

    let streaming_response = StreamingResponse {
        content: "Streaming content".to_string(),
        tokens: 5,
        finish_reason: None,
        chain_id: Some(84532),
        chain_name: Some("Base Sepolia".to_string()),
        native_token: Some("ETH".to_string()),
    };

    // Serialize and verify
    let json = serde_json::to_value(&streaming_response).unwrap();
    assert_eq!(json["chain_id"], 84532);
    assert_eq!(json["chain_name"], "Base Sepolia");
    assert_eq!(json["native_token"], "ETH");
    assert_eq!(json["content"], "Streaming content");
    assert_eq!(json["tokens"], 5);
}

#[test]
fn test_response_formatting() {
    setup_test_env();

    use fabstir_llm_node::api::response_formatter::ResponseFormatter;

    // Test formatting with chain context
    let formatter = ResponseFormatter::new(84532);

    // Format a simple response
    let response = InferenceResponse {
        model: "test".to_string(),
        content: "formatted".to_string(),
        tokens_used: 1,
        finish_reason: "stop".to_string(),
        request_id: "test".to_string(),
        chain_id: None,
        chain_name: None,
        native_token: None,
        web_search_performed: None,
        search_queries_count: None,
        search_provider: None,
        usage: None,
    };

    let formatted = formatter.format_inference_response(response);

    // Should have chain info added
    assert_eq!(formatted.chain_id, Some(84532));
    assert_eq!(formatted.chain_name, Some("Base Sepolia".to_string()));
    assert_eq!(formatted.native_token, Some("ETH".to_string()));
}

#[test]
fn test_models_response_with_chain_context() {
    setup_test_env();

    let response = ModelsResponse {
        models: vec![ModelInfo {
            id: "model1".to_string(),
            name: "Model 1".to_string(),
            description: Some("Test model".to_string()),
        }],
        chain_id: Some(5611),
        chain_name: Some("opBNB Testnet".to_string()),
    };

    // Verify chain info is included
    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json["chain_id"], 5611);
    assert_eq!(json["chain_name"], "opBNB Testnet");
}

#[test]
fn test_websocket_response_with_chain() {
    setup_test_env();

    // Simulate WebSocket response with chain data
    let ws_response = json!({
        "type": "stream_chunk",
        "content": "WebSocket streaming",
        "tokens": 3,
        "chain_id": 84532,
        "chain_name": "Base Sepolia",
        "native_token": "ETH"
    });

    assert_eq!(ws_response["chain_id"], 84532);
    assert_eq!(ws_response["chain_name"], "Base Sepolia");
    assert_eq!(ws_response["native_token"], "ETH");
}

#[test]
fn test_error_chain_context_formatting() {
    setup_test_env();

    // Test that ApiError can include chain context
    let error = ApiError::ModelNotFound {
        model: "llama3".to_string(),
        available_models: vec!["tinyllama".to_string()],
    };

    let error_response =
        error.to_response_with_chain(Some("req-123".to_string()), 84532, "Base Sepolia", "ETH");

    assert_eq!(error_response.chain_id, Some(84532));
    assert_eq!(error_response.message.contains("Base Sepolia"), true);

    let details = error_response.details.unwrap();
    assert_eq!(details["chain_name"], json!("Base Sepolia"));
    assert_eq!(details["native_token"], json!("ETH"));
}
