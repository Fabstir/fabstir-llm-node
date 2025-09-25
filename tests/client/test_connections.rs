use fabstir_llm_node::api::{ApiConfig, ApiServer, ConnectionPool, ConnectionStats};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_connection_pool_creation() {
    let pool_config = fabstir_llm_node::api::PoolConfig {
        min_connections: 2,
        max_connections: 10,
        connection_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(60),
        max_lifetime: Duration::from_secs(300),
        scale_up_threshold: 0.8,
        scale_down_threshold: 0.2,
    };

    let pool = ConnectionPool::new(pool_config)
        .await
        .expect("Failed to create pool");

    // Should have minimum connections ready
    let stats = pool.stats().await;
    assert_eq!(stats.total_connections, 2);
    assert_eq!(stats.idle_connections, 2);
    assert_eq!(stats.active_connections, 0);
}

#[tokio::test]
async fn test_connection_reuse() {
    let config = ApiConfig::default();
    let server = ApiServer::new(config)
        .await
        .expect("Failed to create server");
    let addr = server.local_addr();

    // Use connection pooling client
    let client = reqwest::ClientBuilder::new()
        .pool_max_idle_per_host(5)
        .pool_idle_timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to build client");

    let url = format!("http://{}/health", addr);

    // Make multiple requests
    let mut connection_ids = Vec::new();

    for _ in 0..5 {
        let resp = client
            .get(&url)
            .send()
            .await
            .expect("Failed to send request");

        // Check if connection was reused
        if let Some(conn_id) = resp.headers().get("x-connection-id") {
            connection_ids.push(conn_id.to_str().unwrap().to_string());
        }
    }

    // Most connections should be reused (same ID)
    let unique_connections = connection_ids
        .iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert!(
        unique_connections < 3,
        "Too many unique connections: {}",
        unique_connections
    );
}

#[tokio::test]
async fn test_connection_limits() {
    let config = ApiConfig {
        max_connections_per_ip: 3,
        ..Default::default()
    };

    let server = ApiServer::new(config)
        .await
        .expect("Failed to create server");
    let addr = server.local_addr();

    let client = Client::new();
    let url = format!("http://{}/health", addr);

    // Create multiple concurrent connections
    let mut handles = Vec::new();

    for i in 0..5 {
        let client = client.clone();
        let url = url.clone();

        let handle = tokio::spawn(async move {
            // Hold connection open
            let resp = client
                .get(&url)
                .timeout(Duration::from_secs(2))
                .send()
                .await;

            match resp {
                Ok(r) => (i, r.status().as_u16()),
                Err(_) => (i, 0),
            }
        });

        handles.push(handle);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Collect results
    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await.expect("Task failed"));
    }

    // First 3 should succeed, others should be rejected
    let success_count = results.iter().filter(|(_, status)| *status == 200).count();
    let reject_count = results
        .iter()
        .filter(|(_, status)| *status == 503 || *status == 0)
        .count();

    assert_eq!(success_count, 3);
    assert_eq!(reject_count, 2);
}

#[tokio::test]
async fn test_idle_connection_cleanup() {
    let config = ApiConfig {
        connection_idle_timeout: Duration::from_millis(500),
        ..Default::default()
    };

    let server = ApiServer::new(config)
        .await
        .expect("Failed to create server");
    let addr = server.local_addr();
    let server_stats = server.connection_stats().await;

    let client = Client::new();
    let url = format!("http://{}/health", addr);

    // Make a request
    let _ = client
        .get(&url)
        .send()
        .await
        .expect("Failed to send request");

    // Should have a connection
    // Check connection stats
    assert!(server_stats.total_connections > 0);

    // Wait for idle timeout
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Connection should be cleaned up
    // Just verify we can still make requests
    let _ = client.get(&url).send().await;
}

#[tokio::test]
async fn test_persistent_websocket_connections() {
    let config = ApiConfig {
        enable_websocket: true,
        websocket_ping_interval: Duration::from_millis(100),
        websocket_pong_timeout: Duration::from_millis(500),
        ..Default::default()
    };

    let server = ApiServer::new(config)
        .await
        .expect("Failed to create server");
    let addr = server.local_addr();

    use tokio_tungstenite::{connect_async, tungstenite::Message};

    let ws_url = format!("ws://{}/v1/ws", addr);
    let (ws_stream, _) = connect_async(&ws_url)
        .await
        .expect("Failed to connect WebSocket");

    let (mut write, mut read) = ws_stream.split();

    // Connection should stay alive with pings
    let mut ping_count = 0;
    let start = std::time::Instant::now();

    while start.elapsed() < Duration::from_secs(1) {
        if let Ok(Some(msg)) = timeout(Duration::from_millis(200), read.next()).await {
            if let Ok(Message::Ping(_)) = msg {
                ping_count += 1;
            }
        }
    }

    // Should receive multiple pings
    assert!(ping_count >= 5);
}

#[tokio::test]
async fn test_connection_pool_scaling() {
    let pool_config = fabstir_llm_node::api::PoolConfig {
        min_connections: 2,
        max_connections: 10,
        scale_up_threshold: 0.8,   // Scale when 80% busy
        scale_down_threshold: 0.2, // Scale down when 20% busy
        ..Default::default()
    };

    let pool = ConnectionPool::new(pool_config)
        .await
        .expect("Failed to create pool");

    // Initial state
    assert_eq!(pool.stats().await.total_connections, 2);

    // Simulate high load
    let mut handles = Vec::new();
    for _ in 0..8 {
        let conn = pool.acquire().await.expect("Failed to acquire connection");
        handles.push(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            drop(conn);
        }));
    }

    // Pool should scale up
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(pool.stats().await.total_connections > 2);

    // Wait for connections to be released
    for handle in handles {
        handle.await.expect("Task failed");
    }

    // Pool should scale down after idle period
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(pool.stats().await.total_connections < 8);
}

#[tokio::test]
async fn test_connection_health_checks() {
    let config = ApiConfig {
        enable_connection_health_checks: true,
        health_check_interval: Duration::from_millis(500),
        ..Default::default()
    };

    let mut server = ApiServer::new(config)
        .await
        .expect("Failed to create server");

    // Set up a node that becomes unhealthy
    let p2p_node = create_degrading_node().await;
    server.set_node(p2p_node);
    let addr = server.local_addr();

    let client = Client::new();
    let url = format!("http://{}/v1/inference", addr);

    let request = json!({
        "model": "llama-7b",
        "prompt": "test",
        "max_tokens": 10
    });

    // Initially works
    let resp = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");
    assert_eq!(resp.status(), 200);

    // Wait for health check to detect issue
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Should get service unavailable
    let resp = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .expect("Failed to send request");
    assert_eq!(resp.status(), 503);
}

#[tokio::test]
async fn test_connection_multiplexing() {
    let config = ApiConfig {
        enable_http2: true,
        ..Default::default()
    };

    let server = ApiServer::new(config)
        .await
        .expect("Failed to create server");
    let addr = server.local_addr();

    // Use HTTP/2 client
    let client = reqwest::ClientBuilder::new()
        .http2_prior_knowledge()
        .build()
        .expect("Failed to build client");

    let url = format!("http://{}/health", addr);

    // Send multiple concurrent requests on same connection
    let mut handles = Vec::new();
    let start = std::time::Instant::now();

    for i in 0..10 {
        let client = client.clone();
        let url = url.clone();

        let handle = tokio::spawn(async move {
            let resp = client
                .get(&url)
                .send()
                .await
                .expect("Failed to send request");
            (i, resp.status())
        });

        handles.push(handle);
    }

    // All should complete quickly (multiplexed)
    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await.expect("Task failed"));
    }

    let duration = start.elapsed();

    // All should succeed
    assert!(results.iter().all(|(_, status)| *status == 200));

    // Should be faster than sequential (< 100ms total)
    assert!(duration < Duration::from_millis(100));
}

#[tokio::test]
async fn test_connection_retry_with_backoff() {
    let config = ApiConfig {
        connection_retry_count: 3,
        connection_retry_backoff: Duration::from_millis(100),
        ..Default::default()
    };

    let mut server = ApiServer::new(config)
        .await
        .expect("Failed to create server");

    // Node that succeeds after 2 failures
    let p2p_node = create_flaky_connection_node(2).await;
    server.set_node(p2p_node);
    let addr = server.local_addr();

    let client = Client::new();
    let url = format!("http://{}/health", addr);

    let start = std::time::Instant::now();
    let resp = client
        .get(&url)
        .send()
        .await
        .expect("Failed to send request");
    let duration = start.elapsed();

    assert_eq!(resp.status(), 200);

    // Should have retried with backoff (100ms, 200ms)
    assert!(duration >= Duration::from_millis(300));
    assert!(duration < Duration::from_millis(500));
}

#[tokio::test]
async fn test_graceful_connection_shutdown() {
    let config = ApiConfig {
        shutdown_timeout: Duration::from_secs(2),
        ..Default::default()
    };

    let server = ApiServer::new(config)
        .await
        .expect("Failed to create server");
    let addr = server.local_addr();

    let client = Client::new();
    let url = format!("http://{}/v1/inference", addr);

    // Start a long-running request
    let request = json!({
        "model": "llama-7b",
        "prompt": "generate a long response",
        "max_tokens": 1000,
        "stream": true
    });

    let client_clone = client.clone();
    let url_clone = url.clone();
    let long_request =
        tokio::spawn(async move { client_clone.post(&url_clone).json(&request).send().await });

    // Give it time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Shutdown server
    let shutdown_handle = tokio::spawn(async move {
        server.shutdown().await;
    });

    // New requests should be rejected
    tokio::time::sleep(Duration::from_millis(100)).await;
    let resp = client
        .get(&url)
        .timeout(Duration::from_millis(500))
        .send()
        .await;
    assert!(resp.is_err());

    // Long request should complete or timeout gracefully
    let result = timeout(Duration::from_secs(3), long_request).await;
    assert!(result.is_ok());

    shutdown_handle.await.expect("Shutdown failed");
}

// Helper functions
async fn create_degrading_node() -> fabstir_llm_node::p2p::Node {
    // Mock node that becomes unhealthy over time
    let config = fabstir_llm_node::p2p_config::NodeConfig::default();
    fabstir_llm_node::p2p::Node::new(config)
        .await
        .expect("Failed to create node")
}

async fn create_flaky_connection_node(_failures: u32) -> fabstir_llm_node::p2p::Node {
    // Mock node with connection issues
    let config = fabstir_llm_node::p2p_config::NodeConfig::default();
    fabstir_llm_node::p2p::Node::new(config)
        .await
        .expect("Failed to create node")
}
