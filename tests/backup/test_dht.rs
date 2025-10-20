// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::p2p::{Node, NodeConfig, NodeEvent, DhtEvent};
use libp2p::{kad::RecordKey, PeerId};
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_dht_bootstrap() {
    // Create bootstrap node
    let bootstrap_config = NodeConfig::default();
    let mut bootstrap_node = Node::new(bootstrap_config)
        .await
        .expect("Failed to create bootstrap node");
    let bootstrap_peer_id = bootstrap_node.peer_id();
    let bootstrap_addr = bootstrap_node.listeners()[0].clone();
    let _bootstrap_events = bootstrap_node.start().await;
    
    // Create regular node
    let config = NodeConfig {
        bootstrap_peers: vec![(bootstrap_peer_id, bootstrap_addr)],
        ..Default::default()
    };
    let mut node = Node::new(config).await.expect("Failed to create node");
    let mut event_receiver = node.start().await;
    
    // Should receive bootstrap completed event
    let event = timeout(Duration::from_secs(5), async {
        loop {
            match event_receiver.recv().await {
                Some(NodeEvent::DhtEvent(DhtEvent::BootstrapCompleted { .. })) => return Ok(()),
                Some(_) => continue,
                None => return Err("Channel closed"),
            }
        }
    })
    .await
    .expect("Timeout waiting for bootstrap")
    .expect("Bootstrap failed");
}

#[tokio::test]
async fn test_dht_put_get_record() {
    // Setup two connected nodes
    let mut node1 = create_node().await;
    let mut node2 = create_connected_node(&node1).await;
    
    // Put a record in node1
    let key = RecordKey::new(&b"test_key");
    let value = b"test_value".to_vec();
    
    node1.dht_put(key.clone(), value.clone()).await.expect("Failed to put record");
    
    // Wait for propagation
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Get record from node2
    let result = node2.dht_get(key).await.expect("Failed to get record");
    assert_eq!(result, value);
}

#[tokio::test]
async fn test_dht_provider_records() {
    let mut node1 = create_node().await;
    let mut node2 = create_connected_node(&node1).await;
    let node1_peer_id = node1.peer_id();
    
    // Node1 provides a key
    let key = RecordKey::new(&b"llama-7b-model");
    node1.dht_start_providing(key.clone()).await.expect("Failed to start providing");
    
    // Wait for propagation
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Node2 finds providers
    let providers = node2.dht_get_providers(key).await.expect("Failed to get providers");
    
    assert!(!providers.is_empty());
    assert!(providers.contains(&node1_peer_id));
}

#[tokio::test]
async fn test_dht_capability_announcement() {
    let capabilities = vec!["llama-7b".to_string(), "mistral-7b".to_string()];
    
    let config = NodeConfig {
        capabilities: capabilities.clone(),
        ..Default::default()
    };
    
    let mut node = Node::new(config).await.expect("Failed to create node");
    let mut event_receiver = node.start().await;
    
    // Node should announce capabilities
    node.announce_capabilities().await.expect("Failed to announce capabilities");
    
    // Should receive announcement confirmation
    let event = timeout(Duration::from_secs(2), async {
        loop {
            match event_receiver.recv().await {
                Some(NodeEvent::DhtEvent(DhtEvent::CapabilitiesAnnounced { capabilities: caps })) => {
                    return Ok(caps);
                }
                Some(_) => continue,
                None => return Err("Channel closed"),
            }
        }
    })
    .await
    .expect("Timeout waiting for announcement")
    .expect("Announcement failed");
    
    assert_eq!(event, capabilities);
}

#[tokio::test]
async fn test_dht_find_nodes_with_capability() {
    // Create nodes with different capabilities
    let mut node1 = create_node_with_capability("llama-7b").await;
    let mut node2 = create_node_with_capability("llama-13b").await;
    let mut node3 = create_node_with_capability("llama-7b").await;
    
    // Connect them
    connect_nodes(&mut node1, &mut node2).await;
    connect_nodes(&mut node2, &mut node3).await;
    
    // Announce capabilities
    node1.announce_capabilities().await.unwrap();
    node2.announce_capabilities().await.unwrap();
    node3.announce_capabilities().await.unwrap();
    
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Find nodes with llama-7b capability
    let nodes = node2.find_nodes_with_capability("llama-7b").await.expect("Failed to find nodes");
    
    assert_eq!(nodes.len(), 2);
    assert!(nodes.contains(&node1.peer_id()));
    assert!(nodes.contains(&node3.peer_id()));
}

#[tokio::test]
async fn test_dht_routing_table_health() {
    let mut bootstrap = create_node().await;
    let _bootstrap_events = bootstrap.start().await;
    
    // Create multiple nodes connected to bootstrap
    let mut nodes = Vec::new();
    for _ in 0..5 {
        let node = create_connected_node(&bootstrap).await;
        nodes.push(node);
    }
    
    // Wait for routing tables to populate
    tokio::time::sleep(Duration::from_secs(2)).await;
    
    // Check routing table health
    for node in &nodes {
        let health = node.dht_routing_table_health();
        assert!(health.num_peers >= 1);
        assert!(health.num_buckets > 0);
        assert!(health.pending_queries == 0);
    }
}

#[tokio::test]
async fn test_dht_periodic_bootstrap() {
    let config = NodeConfig {
        dht_bootstrap_interval: Duration::from_secs(1),
        ..Default::default()
    };
    
    let mut node = Node::new(config).await.expect("Failed to create node");
    let mut event_receiver = node.start().await;
    
    // Should receive multiple bootstrap events
    let mut bootstrap_count = 0;
    
    timeout(Duration::from_secs(3), async {
        loop {
            match event_receiver.recv().await {
                Some(NodeEvent::DhtEvent(DhtEvent::BootstrapStarted)) => {
                    bootstrap_count += 1;
                    if bootstrap_count >= 2 {
                        return Ok(());
                    }
                }
                Some(_) => continue,
                None => return Err("Channel closed"),
            }
        }
    })
    .await
    .expect("Timeout waiting for periodic bootstrap")
    .expect("Periodic bootstrap failed");
    
    assert!(bootstrap_count >= 2);
}

#[tokio::test]
async fn test_dht_record_expiration() {
    // Setup two connected nodes
    let mut node1 = create_node().await;
    let mut node2 = create_connected_node(&node1).await;
    
    // Put record with expiration
    let key = RecordKey::new(&b"temp_key");
    let value = b"temp_value".to_vec();
    let expiration = Duration::from_secs(1);
    
    node1.dht_put_with_expiration(key.clone(), value.clone(), expiration)
        .await
        .expect("Failed to put record");
    
    // Wait a bit for DHT propagation
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Record should exist immediately
    let result = node1.dht_get(key.clone()).await;
    assert!(result.is_ok());
    
    // Wait for expiration
    tokio::time::sleep(Duration::from_secs(2)).await;
    
    // Record should be expired
    let result = node1.dht_get(key).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_dht_republish_records() {
    // Create bootstrap node first
    let mut bootstrap = create_node().await;
    let _bootstrap_events = bootstrap.start().await;
    
    let config = NodeConfig {
        dht_republish_interval: Duration::from_secs(1),
        bootstrap_peers: vec![(bootstrap.peer_id(), bootstrap.listeners()[0].clone())],
        ..Default::default()
    };
    
    let mut node = Node::new(config).await.expect("Failed to create node");
    let mut event_receiver = node.start().await;
    
    // Wait for connection
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Put a record
    let key = RecordKey::new(&b"persistent_key");
    let value = b"persistent_value".to_vec();
    node.dht_put(key.clone(), value).await.expect("Failed to put record");
    
    // Should receive republish events
    let _event = timeout(Duration::from_secs(3), async {
        loop {
            match event_receiver.recv().await {
                Some(NodeEvent::DhtEvent(DhtEvent::RecordRepublished { key: k })) => {
                    if k == key {
                        return Ok(());
                    }
                }
                Some(_) => continue,
                None => return Err("Channel closed"),
            }
        }
    })
    .await
    .expect("Timeout waiting for republish")
    .expect("Republish failed");
}

#[tokio::test]
async fn test_dht_closest_peers() {
    // Create a network of nodes
    let mut bootstrap = create_node().await;
    let _bootstrap_events = bootstrap.start().await;
    
    let mut nodes = Vec::new();
    for _ in 0..10 {
        let node = create_connected_node(&bootstrap).await;
        nodes.push(node);
    }
    
    tokio::time::sleep(Duration::from_secs(1)).await;
    
    // Find closest peers to a random key
    let target_key = PeerId::random();
    let closest_peers = nodes[0]
        .dht_get_closest_peers(target_key)
        .await
        .expect("Failed to get closest peers");
    
    // Should return some peers (K-value, typically 20)
    assert!(!closest_peers.is_empty());
    assert!(closest_peers.len() <= 20);
}

// Helper functions

async fn create_node() -> Node {
    let config = NodeConfig::default();
    Node::new(config).await.expect("Failed to create node")
}

async fn create_node_with_capability(capability: &str) -> Node {
    let config = NodeConfig {
        capabilities: vec![capability.to_string()],
        ..Default::default()
    };
    Node::new(config).await.expect("Failed to create node")
}

async fn create_connected_node(bootstrap: &Node) -> Node {
    let config = NodeConfig {
        bootstrap_peers: vec![(bootstrap.peer_id(), bootstrap.listeners()[0].clone())],
        ..Default::default()
    };
    let mut node = Node::new(config).await.expect("Failed to create node");
    let _events = node.start().await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    node
}

async fn connect_nodes(node1: &mut Node, node2: &mut Node) {
    let node2_peer_id = node2.peer_id();
    let node2_addr = node2.listeners()[0].clone();
    node1.connect(node2_peer_id, node2_addr).await.expect("Failed to connect nodes");
    tokio::time::sleep(Duration::from_millis(200)).await;
}