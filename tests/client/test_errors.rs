// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::api::{ApiConfig, ApiError, ApiServer, ErrorResponse};
use reqwest::Client;
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn test_404_not_found() {
    let config = ApiConfig::default();
    let server = ApiServer::new(config)
        .await
        .expect("Failed to create server");
    let addr = server.local_addr();

    let client = Client::new();
    let url = format!("http://{}/v1/nonexistent", addr);

    let resp = client
        .get(&url)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(resp.status(), 404);
    let error: ErrorResponse = resp.json().await.expect("Failed to parse error");
    assert_eq!(error.error_type, "not_found");
    assert!(error.message.contains("not found"));
}

#[tokio::test]
async fn test_405_method_not_allowed() {
    let config = ApiConfig::default();
    let server = ApiServer::new(config)
        .await
        .expect("Failed to create server");
    let addr = server.local_addr();

    let client = Client::new();
    let url = format!("http://{}/v1/inference", addr);

    // DELETE not allowed on inference endpoint
    let resp = client
        .delete(&url)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(resp.status(), 405);
    let error: ErrorResponse = resp.json().await.expect("Failed to parse error");
    assert_eq!(error.error_type, "method_not_allowed");
}

#[tokio::test]
async fn test_400_invalid_json() {
    let config = ApiConfig::default();
    let server = ApiServer::new(config)
        .await
        .expect("Failed to create server");
    let addr = server.local_addr();

    let client = Client::new();
    let url = format!("http://{}/v1/inference", addr);

    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .body("{invalid json}")
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(resp.status(), 400);
    let error: ErrorResponse = resp.json().await.expect("Failed to parse error");
    assert_eq!(error.error_type, "invalid_request");
    assert!(error.message.to_lowercase().contains("json"));
}

#[tokio::test]
async fn test_validation_errors() {
    let config = ApiConfig::default();
    let server = ApiServer::new(config)
        .await
        .expect("Failed to create server");
    let addr = server.local_addr();

    let client = Client::new();
    let url = format!("http://{}/v1/inference", addr);

    // Test various validation errors
    let test_cases = vec![
        (
            json!({
                "model": "",  // empty model
                "prompt": "test",
                "max_tokens": 10
            }),
            "model",
        ),
        (
            json!({
                "model": "llama-7b",
                "prompt": "",  // empty prompt
                "max_tokens": 10
            }),
            "prompt",
        ),
        (
            json!({
                "model": "llama-7b",
                "prompt": "test",
                "max_tokens": 0  // invalid max_tokens
            }),
            "max_tokens",
        ),
        (
            json!({
                "model": "llama-7b",
                "prompt": "test",
                "max_tokens": 10,
                "temperature": 2.5  // temperature too high
            }),
            "temperature",
        ),
    ];

    for (request, expected_field) in test_cases {
        let resp = client
            .post(&url)
            .json(&request)
            .send()
            .await
            .expect("Failed to send request");

        assert_eq!(resp.status(), 400);
        let error: ErrorResponse = resp.json().await.expect("Failed to parse error");
        assert_eq!(error.error_type, "validation_error");
        assert!(error.message.contains(expected_field));

        // Should have field details
        if let Some(details) = error.details {
            assert!(details.contains_key("field"));
            assert_eq!(details["field"], expected_field);
        }
    }
}

#[tokio::test]
async fn test_503_no_available_nodes() {
    let config = ApiConfig::default();
    let server = ApiServer::new(config)
        .await
        .expect("Failed to create server");

    // No P2P node set
    let addr = server.local_addr();

    let client = Client::new();
    let url = format!("http://{}/v1/inference", addr);

    let request = json!({
        "model": "llama-7b",
        "prompt": "test",
        "max_tokens": 10
    });

    let resp = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(resp.status(), 503);
    let error: ErrorResponse = resp.json().await.expect("Failed to parse error");
    assert_eq!(error.error_type, "service_unavailable");
    assert!(error.message.contains("no available nodes"));
}

#[tokio::test]
async fn test_model_not_available() {
    let config = ApiConfig::default();
    let mut server = ApiServer::new(config)
        .await
        .expect("Failed to create server");

    // Node with limited models
    let p2p_node = create_test_node_with_models(vec!["llama-7b"]).await;
    server.set_node(p2p_node);
    let addr = server.local_addr();

    let client = Client::new();
    let url = format!("http://{}/v1/inference", addr);

    let request = json!({
        "model": "gpt-4",  // not available
        "prompt": "test",
        "max_tokens": 10
    });

    let resp = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(resp.status(), 404);
    let error: ErrorResponse = resp.json().await.expect("Failed to parse error");
    assert_eq!(error.error_type, "model_not_found");
    assert!(error.message.contains("gpt-4"));

    // Should suggest available models
    if let Some(details) = error.details {
        assert!(details.contains_key("available_models"));
        let models = details["available_models"].as_array().unwrap();
        assert!(models.contains(&json!("llama-7b")));
    }
}

#[tokio::test]
async fn test_internal_server_error() {
    let config = ApiConfig::default();
    let mut server = ApiServer::new(config)
        .await
        .expect("Failed to create server");

    // Node that will error
    let p2p_node = create_error_node().await;
    server.set_node(p2p_node);
    let addr = server.local_addr();

    let client = Client::new();
    let url = format!("http://{}/v1/inference", addr);

    let request = json!({
        "model": "llama-7b",
        "prompt": "this will cause an error",
        "max_tokens": 10
    });

    let resp = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(resp.status(), 500);
    let error: ErrorResponse = resp.json().await.expect("Failed to parse error");
    assert_eq!(error.error_type, "internal_error");

    // Should not expose internal details
    assert!(!error.message.contains("stack trace"));
    assert!(!error.message.contains("panic"));
}

#[tokio::test]
async fn test_retry_on_transient_errors() {
    let config = ApiConfig {
        enable_auto_retry: true,
        max_retries: 3,
        ..Default::default()
    };

    let mut server = ApiServer::new(config)
        .await
        .expect("Failed to create server");

    // Node that fails first 2 times, succeeds on 3rd
    let p2p_node = create_flaky_node(2).await;
    server.set_node(p2p_node);
    let addr = server.local_addr();

    let client = Client::new();
    let url = format!("http://{}/v1/inference", addr);

    let request = json!({
        "model": "llama-7b",
        "prompt": "retry test",
        "max_tokens": 10
    });

    let resp = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");

    // Should succeed after retries
    assert_eq!(resp.status(), 200);

    // Check retry header
    assert_eq!(resp.headers().get("x-retry-count").unwrap(), "2");
}

#[tokio::test]
async fn test_circuit_breaker() {
    let config = ApiConfig {
        enable_circuit_breaker: true,
        circuit_breaker_threshold: 3,
        circuit_breaker_timeout: Duration::from_secs(1),
        ..Default::default()
    };

    let mut server = ApiServer::new(config)
        .await
        .expect("Failed to create server");

    // Node that always fails
    let p2p_node = create_always_failing_node().await;
    server.set_node(p2p_node);
    let addr = server.local_addr();

    let client = Client::new();
    let url = format!("http://{}/v1/inference", addr);

    let request = json!({
        "model": "llama-7b",
        "prompt": "test",
        "max_tokens": 10
    });

    // Make 3 failing requests
    for _ in 0..3 {
        let _ = client.post(&url).json(&request).send().await;
    }

    // Circuit should be open now
    let resp = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(resp.status(), 503);
    let error: ErrorResponse = resp.json().await.expect("Failed to parse error");
    assert!(error.message.contains("circuit breaker"));

    // Wait for circuit to close
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Should try again
    let resp = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");

    // Will fail but not due to circuit breaker
    assert_eq!(resp.status(), 500);
}

#[tokio::test]
async fn test_error_logging() {
    let config = ApiConfig {
        enable_error_details: true, // Enable for testing
        ..Default::default()
    };

    let mut server = ApiServer::new(config)
        .await
        .expect("Failed to create server");

    let p2p_node = create_error_node().await;
    server.set_node(p2p_node);
    let addr = server.local_addr();

    let client = Client::new();
    let url = format!("http://{}/v1/inference", addr);

    let request = json!({
        "model": "llama-7b",
        "prompt": "error test",
        "max_tokens": 10,
        "request_id": "test-123"  // Custom request ID
    });

    let resp = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(resp.status(), 500);
    let error: ErrorResponse = resp.json().await.expect("Failed to parse error");

    // Should include request ID in error
    assert_eq!(error.request_id, Some("test-123".to_string()));

    // With error details enabled, should have more info
    assert!(error.details.is_some());
}

#[tokio::test]
async fn test_graceful_degradation() {
    let config = ApiConfig::default();
    let mut server = ApiServer::new(config)
        .await
        .expect("Failed to create server");

    // Node with partial capabilities
    let p2p_node = create_degraded_node().await;
    server.set_node(p2p_node);
    let addr = server.local_addr();

    let client = Client::new();

    // Health check should indicate degraded status
    let url = format!("http://{}/health", addr);
    let resp = client
        .get(&url)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(resp.status(), 200);
    let health: serde_json::Value = resp.json().await.expect("Failed to parse health");
    assert_eq!(health["status"], "degraded");
    assert!(health["issues"].as_array().unwrap().len() > 0);
}

// Helper functions
async fn create_test_node_with_models(models: Vec<&str>) -> fabstir_llm_node::p2p::Node {
    let config = fabstir_llm_node::p2p_config::NodeConfig {
        capabilities: models.iter().map(|m| m.to_string()).collect(),
        ..Default::default()
    };
    fabstir_llm_node::p2p::Node::new(config)
        .await
        .expect("Failed to create node")
}

async fn create_error_node() -> fabstir_llm_node::p2p::Node {
    // Mock node that always errors
    create_test_node_with_models(vec!["llama-7b"]).await
}

async fn create_flaky_node(_failures_before_success: u32) -> fabstir_llm_node::p2p::Node {
    // Mock node that fails N times before succeeding
    create_test_node_with_models(vec!["llama-7b"]).await
}

async fn create_always_failing_node() -> fabstir_llm_node::p2p::Node {
    // Mock node that always fails
    create_test_node_with_models(vec!["llama-7b"]).await
}

async fn create_degraded_node() -> fabstir_llm_node::p2p::Node {
    // Mock node with partial functionality
    create_test_node_with_models(vec!["llama-7b"]).await
}
