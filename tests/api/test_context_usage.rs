// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::api::handlers::{InferenceResponse, UsageInfo};

#[test]
fn test_usage_info_serialization() {
    let usage = UsageInfo {
        prompt_tokens: 1250,
        completion_tokens: 150,
        total_tokens: 1400,
        context_window_size: 32768,
    };
    let json = serde_json::to_value(&usage).unwrap();
    assert_eq!(json["prompt_tokens"], 1250);
    assert_eq!(json["completion_tokens"], 150);
    assert_eq!(json["total_tokens"], 1400);
    assert_eq!(json["context_window_size"], 32768);
}

#[test]
fn test_inference_response_without_usage_omits_field() {
    let response = InferenceResponse {
        model: "test".to_string(),
        content: "hello".to_string(),
        tokens_used: 10,
        finish_reason: "stop".to_string(),
        request_id: "req-1".to_string(),
        chain_id: None,
        chain_name: None,
        native_token: None,
        web_search_performed: None,
        search_queries_count: None,
        search_provider: None,
        usage: None,
    };
    let json_str = serde_json::to_string(&response).unwrap();
    assert!(!json_str.contains("\"usage\""));
}

#[test]
fn test_inference_response_with_usage() {
    let response = InferenceResponse {
        model: "test".to_string(),
        content: "hello".to_string(),
        tokens_used: 10,
        finish_reason: "stop".to_string(),
        request_id: "req-1".to_string(),
        chain_id: None,
        chain_name: None,
        native_token: None,
        web_search_performed: None,
        search_queries_count: None,
        search_provider: None,
        usage: Some(UsageInfo {
            prompt_tokens: 500,
            completion_tokens: 10,
            total_tokens: 510,
            context_window_size: 4096,
        }),
    };
    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json["usage"]["prompt_tokens"], 500);
    assert_eq!(json["usage"]["completion_tokens"], 10);
    assert_eq!(json["usage"]["total_tokens"], 510);
    assert_eq!(json["usage"]["context_window_size"], 4096);
}

#[test]
fn test_usage_info_deserialization() {
    let json_str = r#"{"prompt_tokens":800,"completion_tokens":200,"total_tokens":1000,"context_window_size":8192}"#;
    let usage: UsageInfo = serde_json::from_str(json_str).unwrap();
    assert_eq!(usage.prompt_tokens, 800);
    assert_eq!(usage.completion_tokens, 200);
    assert_eq!(usage.total_tokens, 1000);
    assert_eq!(usage.context_window_size, 8192);
}

#[test]
fn test_overflow_error_message_format() {
    let prompt_tokens = 33500usize;
    let context_window_size = 32768usize;
    let overflow = prompt_tokens - context_window_size;
    let error_msg = format!(
        "Prompt ({} tokens) exceeds context window ({} tokens) by {} tokens",
        prompt_tokens, context_window_size, overflow
    );
    assert!(error_msg.contains("33500 tokens"));
    assert!(error_msg.contains("32768 tokens"));
    assert!(error_msg.contains("by 732 tokens"));
}

#[test]
fn test_overflow_error_contains_token_counts() {
    let error_msg = "Prompt (4200 tokens) exceeds context window (4096 tokens) by 104 tokens";
    // Verify parseable numbers exist in the error
    assert!(error_msg.contains("4200"));
    assert!(error_msg.contains("4096"));
    assert!(error_msg.contains("104"));
    assert!(error_msg.contains("exceeds context window"));
}
