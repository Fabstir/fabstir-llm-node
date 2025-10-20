// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use fabstir_llm_node::performance::{
    LoadBalancer, LoadBalancerConfig, WorkerNode, LoadStrategy,
    NodeStatus, WorkerMetrics, LoadDistribution, HealthCheck,
    RequestRouter, NodeCapabilities, LoadBalancerError, SessionAffinity
};
use std::sync::Arc;
use std::time::Duration;
use tokio;

async fn create_test_load_balancer() -> Result<LoadBalancer> {
    let config = LoadBalancerConfig {
        strategy: LoadStrategy::LeastConnections,
        health_check_interval_secs: 5,
        node_timeout_secs: 30,
        max_retries: 3,
        enable_session_affinity: false,
        load_threshold: 0.8,
        rebalance_interval_secs: 60,
    };
    
    let nodes = vec![
        WorkerNode {
            id: "node1".to_string(),
            address: "127.0.0.1:8001".to_string(),
            capabilities: NodeCapabilities {
                models: vec!["llama-7b".to_string()],
                max_batch_size: 32,
                gpu_memory_gb: 24,
                supports_streaming: true,
            },
            status: NodeStatus::Healthy,
        },
        WorkerNode {
            id: "node2".to_string(),
            address: "127.0.0.1:8002".to_string(),
            capabilities: NodeCapabilities {
                models: vec!["llama-7b".to_string(), "mistral-7b".to_string()],
                max_batch_size: 64,
                gpu_memory_gb: 48,
                supports_streaming: true,
            },
            status: NodeStatus::Healthy,
        },
    ];
    
    LoadBalancer::new(config, nodes).await
}

#[tokio::test]
async fn test_basic_load_distribution() {
    let balancer = create_test_load_balancer().await.unwrap();
    
    // Make multiple requests
    let mut node_counts = std::collections::HashMap::new();
    
    for _ in 0..100 {
        let node = balancer
            .select_node("llama-7b", None)
            .await
            .unwrap();
        
        *node_counts.entry(node.id.clone()).or_insert(0) += 1;
    }
    
    // Both nodes should get requests
    assert_eq!(node_counts.len(), 2);
    for (_, count) in node_counts {
        assert!(count > 0);
    }
}

#[tokio::test]
async fn test_least_connections_strategy() {
    let balancer = create_test_load_balancer().await.unwrap();
    
    // Simulate connections on node1
    for _ in 0..5 {
        balancer.acquire_connection("node1").await.unwrap();
    }
    
    // Next requests should go to node2
    for _ in 0..3 {
        let node = balancer
            .select_node("llama-7b", None)
            .await
            .unwrap();
        assert_eq!(node.id, "node2");
    }
    
    // Check connection counts
    let metrics = balancer.get_metrics().await;
    assert!(metrics.nodes["node1"].active_connections > metrics.nodes["node2"].active_connections);
}

#[tokio::test]
async fn test_round_robin_strategy() {
    let mut config = LoadBalancerConfig::default();
    config.strategy = LoadStrategy::RoundRobin;
    
    let nodes = vec![
        WorkerNode {
            id: "node1".to_string(),
            address: "127.0.0.1:8001".to_string(),
            capabilities: NodeCapabilities {
                models: vec!["llama-7b".to_string()],
                max_batch_size: 32,
                gpu_memory_gb: 24,
                supports_streaming: true,
            },
            status: NodeStatus::Healthy,
        },
        WorkerNode {
            id: "node2".to_string(),
            address: "127.0.0.1:8002".to_string(),
            capabilities: NodeCapabilities {
                models: vec!["llama-7b".to_string()],
                max_batch_size: 32,
                gpu_memory_gb: 24,
                supports_streaming: true,
            },
            status: NodeStatus::Healthy,
        },
    ];
    
    let balancer = LoadBalancer::new(config, nodes).await.unwrap();
    
    // Should alternate between nodes
    let node1 = balancer.select_node("llama-7b", None).await.unwrap();
    let node2 = balancer.select_node("llama-7b", None).await.unwrap();
    let node3 = balancer.select_node("llama-7b", None).await.unwrap();
    
    assert_eq!(node1.id, "node1");
    assert_eq!(node2.id, "node2");
    assert_eq!(node3.id, "node1");
}

#[tokio::test]
async fn test_weighted_distribution() {
    let mut config = LoadBalancerConfig::default();
    config.strategy = LoadStrategy::WeightedRoundRobin;
    
    let nodes = vec![
        WorkerNode {
            id: "small".to_string(),
            address: "127.0.0.1:8001".to_string(),
            capabilities: NodeCapabilities {
                models: vec!["llama-7b".to_string()],
                max_batch_size: 16,
                gpu_memory_gb: 16, // Smaller capacity
                supports_streaming: true,
            },
            status: NodeStatus::Healthy,
        },
        WorkerNode {
            id: "large".to_string(),
            address: "127.0.0.1:8002".to_string(),
            capabilities: NodeCapabilities {
                models: vec!["llama-7b".to_string()],
                max_batch_size: 64,
                gpu_memory_gb: 48, // 3x capacity
                supports_streaming: true,
            },
            status: NodeStatus::Healthy,
        },
    ];
    
    let balancer = LoadBalancer::new(config, nodes).await.unwrap();
    
    // Count distribution over many requests
    let mut counts = std::collections::HashMap::new();
    for _ in 0..100 {
        let node = balancer.select_node("llama-7b", None).await.unwrap();
        *counts.entry(node.id.clone()).or_insert(0) += 1;
    }
    
    // Large node should get approximately 3x more requests
    let ratio = counts["large"] as f64 / counts["small"] as f64;
    assert!(ratio > 2.0 && ratio < 4.0);
}

#[tokio::test]
async fn test_node_health_checking() {
    let balancer = create_test_load_balancer().await.unwrap();
    
    // Mark node1 as unhealthy
    balancer.mark_node_unhealthy("node1", "Connection timeout").await;
    
    // All requests should go to node2
    for _ in 0..5 {
        let node = balancer.select_node("llama-7b", None).await.unwrap();
        assert_eq!(node.id, "node2");
    }
    
    // Check node status
    let status = balancer.get_node_status("node1").await.unwrap();
    assert_eq!(status, NodeStatus::Unhealthy);
}

#[tokio::test]
async fn test_automatic_health_recovery() {
    let balancer = create_test_load_balancer().await.unwrap();
    
    // Start health monitoring
    balancer.start_health_monitoring().await;
    
    // Mark node unhealthy
    balancer.mark_node_unhealthy("node1", "Temporary failure").await;
    
    // Simulate node recovery
    balancer.mock_health_check_result("node1", true).await;
    
    // Wait for health check cycle
    tokio::time::sleep(Duration::from_secs(6)).await;
    
    // Node should be healthy again
    let status = balancer.get_node_status("node1").await.unwrap();
    assert_eq!(status, NodeStatus::Healthy);
}

#[tokio::test]
async fn test_model_specific_routing() {
    let balancer = create_test_load_balancer().await.unwrap();
    
    // Request mistral model (only on node2)
    let node = balancer
        .select_node("mistral-7b", None)
        .await
        .unwrap();
    
    assert_eq!(node.id, "node2");
    
    // Request llama (on both nodes)
    let mut llama_nodes = std::collections::HashSet::new();
    for _ in 0..10 {
        let node = balancer.select_node("llama-7b", None).await.unwrap();
        llama_nodes.insert(node.id.clone());
    }
    
    assert_eq!(llama_nodes.len(), 2); // Should use both nodes
}

#[tokio::test]
async fn test_session_affinity() {
    let mut config = LoadBalancerConfig::default();
    config.enable_session_affinity = true;
    
    let nodes = vec![
        WorkerNode {
            id: "node1".to_string(),
            address: "127.0.0.1:8001".to_string(),
            capabilities: Default::default(),
            status: NodeStatus::Healthy,
        },
        WorkerNode {
            id: "node2".to_string(),
            address: "127.0.0.1:8002".to_string(),
            capabilities: Default::default(),
            status: NodeStatus::Healthy,
        },
    ];
    
    let balancer = LoadBalancer::new(config, nodes).await.unwrap();
    
    let session_id = "user123";
    
    // First request establishes affinity
    let node1 = balancer
        .select_node("llama-7b", Some(session_id))
        .await
        .unwrap();
    
    // Subsequent requests should go to same node
    for _ in 0..5 {
        let node = balancer
            .select_node("llama-7b", Some(session_id))
            .await
            .unwrap();
        assert_eq!(node.id, node1.id);
    }
}

#[tokio::test]
async fn test_load_based_routing() {
    let balancer = create_test_load_balancer().await.unwrap();
    
    // Simulate high load on node1
    balancer.update_node_metrics("node1", WorkerMetrics {
        cpu_usage: 0.9,
        memory_usage: 0.85,
        gpu_usage: 0.95,
        active_connections: 50,
        queue_depth: 100,
        average_latency_ms: 2000.0,
        error_rate: 0.01,
        cpu_usage_percent: 90.0,
        memory_usage_percent: 85.0,
        gpu_usage_percent: 95.0,
        requests_per_second: 50.0,
        last_health_check: std::time::Instant::now(),
        request_success_rate: 0.99,
    }).await.unwrap();
    
    // Simulate low load on node2
    balancer.update_node_metrics("node2", WorkerMetrics {
        cpu_usage: 0.3,
        memory_usage: 0.4,
        gpu_usage: 0.2,
        active_connections: 5,
        queue_depth: 0,
        average_latency_ms: 100.0,
        error_rate: 0.0,
        cpu_usage_percent: 30.0,
        memory_usage_percent: 40.0,
        gpu_usage_percent: 20.0,
        requests_per_second: 10.0,
        last_health_check: std::time::Instant::now(),
        request_success_rate: 1.0,
    }).await.unwrap();
    
    // Requests should prefer node2
    let mut node2_count = 0;
    for _ in 0..10 {
        let node = balancer.select_node("llama-7b", None).await.unwrap();
        if node.id == "node2" {
            node2_count += 1;
        }
    }
    
    assert!(node2_count >= 8); // Most requests to less loaded node
}

#[tokio::test]
async fn test_circuit_breaker() {
    let balancer = create_test_load_balancer().await.unwrap();
    
    // Simulate multiple failures on node1
    for _ in 0..5 {
        balancer.record_request_failure("node1", "Timeout").await;
    }
    
    // Circuit breaker should open
    let status = balancer.get_node_status("node1").await.unwrap();
    assert_eq!(status, NodeStatus::CircuitOpen);
    
    // Requests should not go to node1
    for _ in 0..5 {
        let node = balancer.select_node("llama-7b", None).await.unwrap();
        assert_eq!(node.id, "node2");
    }
    
    // Wait for circuit breaker cooldown
    tokio::time::sleep(Duration::from_secs(30)).await;
    
    // Circuit should be half-open (ready to test)
    let status = balancer.get_node_status("node1").await.unwrap();
    assert_eq!(status, NodeStatus::CircuitHalfOpen);
}

#[tokio::test]
async fn test_load_shedding() {
    let balancer = create_test_load_balancer().await.unwrap();
    
    // Simulate all nodes at high load
    for node_id in vec!["node1", "node2"] {
        balancer.update_node_metrics(node_id, WorkerMetrics {
            cpu_usage: 0.95,
            memory_usage: 0.95,
            gpu_usage: 0.95,
            active_connections: 100,
            queue_depth: 500,
            average_latency_ms: 5000.0,
            error_rate: 0.1,
            cpu_usage_percent: 95.0,
            memory_usage_percent: 95.0,
            gpu_usage_percent: 95.0,
            requests_per_second: 20.0,
            last_health_check: std::time::Instant::now(),
            request_success_rate: 0.9,
        }).await.unwrap();
    }
    
    // Should reject some requests (load shedding)
    let mut rejected = 0;
    for _ in 0..10 {
        match balancer.select_node("llama-7b", None).await {
            Err(e) => {
                if let Ok(LoadBalancerError::AllNodesOverloaded) = e.downcast::<LoadBalancerError>() {
                    rejected += 1;
                }
            }
            Ok(_) => {}
        }
    }
    
    assert!(rejected > 0); // Some requests should be shed
}

#[tokio::test]
async fn test_graceful_node_drain() {
    let balancer = create_test_load_balancer().await.unwrap();
    
    // Start draining node1
    balancer.start_node_drain("node1").await.unwrap();
    
    // New requests should not go to draining node
    for _ in 0..5 {
        let node = balancer.select_node("llama-7b", None).await.unwrap();
        assert_eq!(node.id, "node2");
    }
    
    // Existing connections should be allowed to complete
    let status = balancer.get_node_status("node1").await.unwrap();
    assert_eq!(status, NodeStatus::Draining);
    
    // Simulate all connections closing
    balancer.release_all_connections("node1").await;
    
    // Node should now be drained
    let status = balancer.get_node_status("node1").await.unwrap();
    assert_eq!(status, NodeStatus::Drained);
}

#[tokio::test]
async fn test_dynamic_rebalancing() {
    let balancer = create_test_load_balancer().await.unwrap();
    
    // Enable auto-rebalancing
    balancer.enable_auto_rebalancing(Duration::from_secs(1)).await;
    
    // Create imbalanced load
    for _ in 0..20 {
        balancer.acquire_connection("node1").await.unwrap();
    }
    
    // Wait for rebalancing
    tokio::time::sleep(Duration::from_secs(2)).await;
    
    // Check if connections were rebalanced
    let metrics = balancer.get_metrics().await;
    let node1_conns = metrics.nodes["node1"].active_connections;
    let node2_conns = metrics.nodes["node2"].active_connections;
    
    // Should be more balanced now
    let diff = (node1_conns as i32 - node2_conns as i32).abs();
    assert!(diff < 10); // Reasonable balance
}