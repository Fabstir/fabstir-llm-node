use fabstir_llm_node::api::{ApiServer, ApiConfig, InferenceRequest, InferenceResponse};
use reqwest::Client;
use serde_json::json;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_api_server_start() {
    let config = ApiConfig {
        listen_addr: "127.0.0.1:0".to_string(),
        max_connections: 100,
        request_timeout: Duration::from_secs(30),
        cors_allowed_origins: vec!["*".to_string()],
        ..Default::default()
    };
    
    let server = ApiServer::new(config).await.expect("Failed to create server");
    let addr = server.local_addr();
    
    // Server should be listening
    assert!(addr.port() > 0);
    
    // Should respond to health check
    let client = Client::new();
    let url = format!("http://{}/health", addr);
    let resp = client.get(&url).send().await.expect("Failed to send request");
    
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.expect("Failed to parse JSON");
    assert_eq!(body["status"], "healthy");
}

#[tokio::test]
async fn test_inference_request_endpoint() {
    let config = ApiConfig::default();
    let mut server = ApiServer::new(config).await.expect("Failed to create server");
    
    // Start server with P2P node
    let p2p_node = create_test_node().await;
    server.set_node(p2p_node);
    let addr = server.local_addr();
    
    // Send inference request
    let client = Client::new();
    let url = format!("http://{}/v1/inference", addr);
    
    let request = json!({
        "model": "llama-7b",
        "prompt": "Once upon a time",
        "max_tokens": 50,
        "temperature": 0.7,
        "stream": false
    });
    
    let resp = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");
    
    assert_eq!(resp.status(), 200);
    
    let body: InferenceResponse = resp.json().await.expect("Failed to parse response");
    assert!(!body.content.is_empty());
    assert!(body.tokens_used > 0);
    assert_eq!(body.model, "llama-7b");
}

#[tokio::test]
async fn test_request_validation() {
    let config = ApiConfig::default();
    let server = ApiServer::new(config).await.expect("Failed to create server");
    let addr = server.local_addr();
    
    let client = Client::new();
    let url = format!("http://{}/v1/inference", addr);
    
    // Missing required fields
    let invalid_request = json!({
        "prompt": "test"
        // missing model
    });
    
    let resp = client
        .post(&url)
        .json(&invalid_request)
        .send()
        .await
        .expect("Failed to send request");
    
    assert_eq!(resp.status(), 400);
    let error: serde_json::Value = resp.json().await.expect("Failed to parse error");
    assert!(error["error"].as_str().unwrap().contains("model"));
}

#[tokio::test]
async fn test_model_availability_check() {
    let config = ApiConfig::default();
    let mut server = ApiServer::new(config).await.expect("Failed to create server");
    
    let p2p_node = create_test_node_with_models(vec!["llama-7b", "mistral-7b"]).await;
    server.set_node(p2p_node);
    let addr = server.local_addr();
    
    let client = Client::new();
    
    // Check available models
    let url = format!("http://{}/v1/models", addr);
    let resp = client.get(&url).send().await.expect("Failed to send request");
    
    assert_eq!(resp.status(), 200);
    let models: serde_json::Value = resp.json().await.expect("Failed to parse models");
    let model_list = models["models"].as_array().unwrap();
    
    assert_eq!(model_list.len(), 2);
    assert!(model_list.iter().any(|m| m["id"] == "llama-7b"));
    assert!(model_list.iter().any(|m| m["id"] == "mistral-7b"));
}

#[tokio::test]
async fn test_request_with_api_key() {
    let mut config = ApiConfig::default();
    config.require_api_key = true;
    config.api_keys = vec!["test-key-123".to_string()];
    
    let server = ApiServer::new(config).await.expect("Failed to create server");
    let addr = server.local_addr();
    
    let client = Client::new();
    let url = format!("http://{}/v1/inference", addr);
    
    let request = json!({
        "model": "llama-7b",
        "prompt": "test",
        "max_tokens": 10
    });
    
    // Without API key
    let resp = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");
    
    assert_eq!(resp.status(), 401);
    
    // With valid API key
    let resp = client
        .post(&url)
        .header("Authorization", "Bearer test-key-123")
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");
    
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_rate_limiting() {
    let mut config = ApiConfig::default();
    config.rate_limit_per_minute = 5;
    
    let server = ApiServer::new(config).await.expect("Failed to create server");
    let addr = server.local_addr();
    
    let client = Client::new();
    let url = format!("http://{}/v1/inference", addr);
    
    let request = json!({
        "model": "llama-7b",
        "prompt": "test",
        "max_tokens": 10
    });
    
    // Send 6 requests quickly
    for i in 0..6 {
        let resp = client
            .post(&url)
            .json(&request)
            .send()
            .await
            .expect("Failed to send request");
        
        if i < 5 {
            assert_eq!(resp.status(), 200, "Request {} should succeed", i);
        } else {
            assert_eq!(resp.status(), 429, "Request {} should be rate limited", i);
            let error: serde_json::Value = resp.json().await.expect("Failed to parse error");
            assert!(error["error"].as_str().unwrap().contains("rate limit"));
        }
    }
}

#[tokio::test]
async fn test_cors_headers() {
    let mut config = ApiConfig::default();
    config.cors_allowed_origins = vec!["https://example.com".to_string()];
    
    let server = ApiServer::new(config).await.expect("Failed to create server");
    let addr = server.local_addr();
    
    let client = Client::new();
    let url = format!("http://{}/v1/models", addr);
    
    // Preflight request
    let resp = client
        .request(reqwest::Method::OPTIONS, &url)
        .header("Origin", "https://example.com")
        .header("Access-Control-Request-Method", "POST")
        .send()
        .await
        .expect("Failed to send request");
    
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("Access-Control-Allow-Origin").unwrap(),
        "https://example.com"
    );
}

#[tokio::test]
async fn test_request_timeout() {
    let mut config = ApiConfig::default();
    config.request_timeout = Duration::from_millis(100);
    
    let mut server = ApiServer::new(config).await.expect("Failed to create server");
    
    // Set up a slow node
    let p2p_node = create_slow_test_node(Duration::from_secs(1)).await;
    server.set_node(p2p_node);
    let addr = server.local_addr();
    
    let client = Client::new();
    let url = format!("http://{}/v1/inference", addr);
    
    let request = json!({
        "model": "llama-7b",
        "prompt": "test",
        "max_tokens": 10
    });
    
    let start = std::time::Instant::now();
    let resp = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");
    
    let duration = start.elapsed();
    
    assert_eq!(resp.status(), 504); // Gateway Timeout
    assert!(duration < Duration::from_millis(200));
}

#[tokio::test]
async fn test_metrics_endpoint() {
    let config = ApiConfig::default();
    let server = ApiServer::new(config).await.expect("Failed to create server");
    let addr = server.local_addr();
    
    let client = Client::new();
    
    // Make some requests
    for _ in 0..3 {
        let _ = client
            .get(&format!("http://{}/health", addr))
            .send()
            .await;
    }
    
    // Check metrics
    let url = format!("http://{}/metrics", addr);
    let resp = client.get(&url).send().await.expect("Failed to send request");
    
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.expect("Failed to get text");
    
    // Should contain Prometheus metrics
    assert!(body.contains("http_requests_total"));
    assert!(body.contains("http_request_duration_seconds"));
}

#[tokio::test]
async fn test_graceful_shutdown() {
    let config = ApiConfig::default();
    let server = ApiServer::new(config).await.expect("Failed to create server");
    let addr = server.local_addr();
    
    let client = Client::new();
    let url = format!("http://{}/health", addr);
    
    // Server should be running
    let resp = client.get(&url).send().await.expect("Failed to send request");
    assert_eq!(resp.status(), 200);
    
    // Shutdown server
    server.shutdown().await;
    
    // Server should not respond
    let result = timeout(Duration::from_millis(100), client.get(&url).send()).await;
    assert!(result.is_err() || result.unwrap().is_err());
}

// Helper functions
async fn create_test_node() -> fabstir_llm_node::p2p::Node {
    let config = fabstir_llm_node::config::NodeConfig::default();
    fabstir_llm_node::p2p::Node::new(config).await.expect("Failed to create node")
}

async fn create_test_node_with_models(models: Vec<&str>) -> fabstir_llm_node::p2p::Node {
    let config = fabstir_llm_node::config::NodeConfig {
        capabilities: models.iter().map(|m| m.to_string()).collect(),
        ..Default::default()
    };
    fabstir_llm_node::p2p::Node::new(config).await.expect("Failed to create node")
}

async fn create_slow_test_node(_delay: Duration) -> fabstir_llm_node::p2p::Node {
    // This would be a mock that delays responses
    create_test_node().await
}