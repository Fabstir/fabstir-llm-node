// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::p2p::{Node, NodeConfig, NodeEvent, DiscoveryEvent};
use libp2p::PeerId;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
#[ignore = "mDNS tests require proper network configuration"]
async fn test_mdns_discovery() {
    // Create two nodes on the same local network
    let config1 = NodeConfig {
        enable_mdns: true,
        ..Default::default()
    };
    let config2 = NodeConfig {
        enable_mdns: true,
        ..Default::default()
    };
    
    let mut node1 = Node::new(config1).await.expect("Failed to create node1");
    let mut node2 = Node::new(config2).await.expect("Failed to create node2");
    
    let peer_id1 = node1.peer_id();
    let peer_id2 = node2.peer_id();
    
    let mut events1 = node1.start().await;
    let mut events2 = node2.start().await;
    
    // Wait for nodes to start up completely
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Nodes should discover each other via mDNS
    let discovered_peer = timeout(Duration::from_secs(10), async {
        loop {
            match events1.recv().await {
                Some(NodeEvent::DiscoveryEvent(DiscoveryEvent::PeerDiscovered { peer_id, .. })) => {
                    if peer_id == peer_id2 {
                        return Ok(peer_id);
                    }
                }
                Some(_) => continue,
                None => return Err("Channel closed"),
            }
        }
    })
    .await
    .expect("Timeout waiting for mDNS discovery")
    .expect("mDNS discovery failed");
    
    assert_eq!(discovered_peer, peer_id2);
    
    // Node2 should also discover node1
    let discovered = node2.discovered_peers().await;
    assert!(discovered.contains(&peer_id1));
}

#[tokio::test]
#[ignore = "mDNS tests require proper network configuration"]
async fn test_mdns_discovery_with_filtering() {
    // Create nodes with different capabilities for filtering
    let config1 = NodeConfig {
        enable_mdns: true,
        capabilities: vec!["fabstir-llm".to_string()],
        ..Default::default()
    };
    
    let config2 = NodeConfig {
        enable_mdns: true,
        capabilities: vec!["fabstir-llm".to_string()],
        ..Default::default()
    };
    
    let config3 = NodeConfig {
        enable_mdns: true,
        capabilities: vec!["other-service".to_string()],
        ..Default::default()
    };
    
    let mut node1 = Node::new(config1).await.expect("Failed to create node1");
    let mut node2 = Node::new(config2).await.expect("Failed to create node2");
    let mut node3 = Node::new(config3).await.expect("Failed to create node3");
    
    let peer_id2 = node2.peer_id();
    let peer_id3 = node3.peer_id();
    
    let mut events1 = node1.start().await;
    let _events2 = node2.start().await;
    let _events3 = node3.start().await;
    
    // Announce capabilities
    node1.announce_capabilities().await.unwrap();
    node2.announce_capabilities().await.unwrap();
    node3.announce_capabilities().await.unwrap();
    
    // Wait for announcements to propagate
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Node1 should discover both nodes via mDNS
    let mut discovered_peers = Vec::new();
    
    timeout(Duration::from_secs(3), async {
        loop {
            match events1.recv().await {
                Some(NodeEvent::DiscoveryEvent(DiscoveryEvent::PeerDiscovered { peer_id, .. })) => {
                    discovered_peers.push(peer_id);
                    if discovered_peers.len() >= 2 {
                        return Ok(());
                    }
                }
                Some(_) => continue,
                None => return Err("Channel closed"),
            }
        }
    })
    .await
    .expect("Timeout waiting for discovery")
    .expect("Discovery failed");
    
    // Both nodes should be discovered via mDNS
    assert!(discovered_peers.contains(&peer_id2));
    assert!(discovered_peers.contains(&peer_id3));
    
    // But capability filtering would happen at a higher level
    let fabstir_peers = node1.discover_peers_with_capability("fabstir-llm").await.unwrap();
    assert!(fabstir_peers.contains(&peer_id2));
    assert!(!fabstir_peers.contains(&peer_id3));
}

#[tokio::test]
async fn test_rendezvous_discovery() {
    // Create rendezvous node
    let rendezvous_config = NodeConfig {
        enable_rendezvous_server: true,
        ..Default::default()
    };
    let mut rendezvous = Node::new(rendezvous_config).await.expect("Failed to create rendezvous");
    let rendezvous_peer_id = rendezvous.peer_id();
    let rendezvous_addr = rendezvous.listeners()[0].clone();
    let _rendezvous_events = rendezvous.start().await;
    
    // Create client nodes
    let config1 = NodeConfig {
        enable_rendezvous_client: true,
        rendezvous_servers: vec![(rendezvous_peer_id, rendezvous_addr.clone())],
        ..Default::default()
    };
    
    let config2 = NodeConfig {
        enable_rendezvous_client: true,
        rendezvous_servers: vec![(rendezvous_peer_id, rendezvous_addr)],
        ..Default::default()
    };
    
    let mut node1 = Node::new(config1).await.expect("Failed to create node1");
    let mut node2 = Node::new(config2).await.expect("Failed to create node2");
    
    let peer_id1 = node1.peer_id();
    let peer_id2 = node2.peer_id();
    
    let mut events1 = node1.start().await;
    let _events2 = node2.start().await;
    
    // Register with rendezvous
    let namespace = "fabstir-llm-nodes";
    node1.register_rendezvous(namespace).await.expect("Failed to register");
    node2.register_rendezvous(namespace).await.expect("Failed to register");
    
    // Discover peers via rendezvous
    node1.discover_rendezvous(namespace).await.expect("Failed to discover");
    
    // Should discover node2
    let discovered = timeout(Duration::from_secs(5), async {
        loop {
            match events1.recv().await {
                Some(NodeEvent::DiscoveryEvent(DiscoveryEvent::PeerDiscovered { peer_id, .. })) => {
                    if peer_id == peer_id2 {
                        return Ok(peer_id);
                    }
                }
                Some(_) => continue,
                None => return Err("Channel closed"),
            }
        }
    })
    .await
    .expect("Timeout waiting for rendezvous discovery")
    .expect("Rendezvous discovery failed");
    
    assert_eq!(discovered, peer_id2);
}

#[tokio::test]
async fn test_capability_based_discovery() {
    // Setup bootstrap node
    let mut bootstrap = create_bootstrap_node().await;
    
    // Create nodes with different capabilities
    let mut llama_node = create_node_with_capability(&bootstrap, "llama-7b").await;
    let mut mistral_node = create_node_with_capability(&bootstrap, "mistral-7b").await;
    let mut multi_node = create_node_with_capabilities(&bootstrap, vec!["llama-7b", "mistral-7b"]).await;
    
    // Announce capabilities
    llama_node.announce_capabilities().await.unwrap();
    mistral_node.announce_capabilities().await.unwrap();
    multi_node.announce_capabilities().await.unwrap();
    
    tokio::time::sleep(Duration::from_secs(1)).await;
    
    // Discover nodes with specific capability
    let llama_peers = bootstrap
        .discover_peers_with_capability("llama-7b")
        .await
        .expect("Failed to discover llama peers");
    
    assert_eq!(llama_peers.len(), 2);
    assert!(llama_peers.contains(&llama_node.peer_id()));
    assert!(llama_peers.contains(&multi_node.peer_id()));
    
    let mistral_peers = bootstrap
        .discover_peers_with_capability("mistral-7b")
        .await
        .expect("Failed to discover mistral peers");
    
    assert_eq!(mistral_peers.len(), 2);
    assert!(mistral_peers.contains(&mistral_node.peer_id()));
    assert!(mistral_peers.contains(&multi_node.peer_id()));
}

#[tokio::test]
async fn test_discovery_with_metadata() {
    let mut bootstrap = create_bootstrap_node().await;
    
    // Create node with metadata
    let metadata = serde_json::json!({
        "gpu": "NVIDIA RTX 4090",
        "vram": "24GB",
        "price_per_token": 0.0001,
        "max_batch_size": 32
    });
    
    let config = NodeConfig {
        bootstrap_peers: vec![(bootstrap.peer_id(), bootstrap.listeners()[0].clone())],
        node_metadata: Some(metadata.clone()),
        ..Default::default()
    };
    
    let mut node = Node::new(config).await.expect("Failed to create node");
    let node_peer_id = node.peer_id();
    let _events = node.start().await;
    
    // Wait for connection to bootstrap
    tokio::time::sleep(Duration::from_secs(1)).await;
    
    // Announce with metadata
    node.announce_with_metadata().await.expect("Failed to announce");
    
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Discover and retrieve metadata
    let peer_info = bootstrap
        .get_peer_metadata(node_peer_id)
        .await
        .expect("Failed to get peer metadata");
    
    assert_eq!(peer_info.peer_id, node_peer_id);
    assert_eq!(peer_info.metadata, metadata);
}

#[tokio::test]
async fn test_discovery_rate_limiting() {
    let config = NodeConfig {
        discovery_rate_limit: Some(Duration::from_secs(1)),
        ..Default::default()
    };
    
    let mut node = Node::new(config).await.expect("Failed to create node");
    let _events = node.start().await;
    
    // First discovery should succeed
    let result1 = node.discover_rendezvous("test").await;
    assert!(result1.is_ok());
    
    // Immediate second discovery should be rate limited
    let result2 = node.discover_rendezvous("test").await;
    assert!(result2.is_err());
    
    // Wait for rate limit
    tokio::time::sleep(Duration::from_secs(1)).await;
    
    // Should succeed again
    let result3 = node.discover_rendezvous("test").await;
    assert!(result3.is_ok());
}

#[tokio::test]
async fn test_peer_expiration() {
    let config = NodeConfig {
        peer_expiration_time: Duration::from_secs(2),
        ..Default::default()
    };
    
    let mut node1 = Node::new(config).await.expect("Failed to create node");
    let mut node2 = Node::new(NodeConfig::default()).await.expect("Failed to create node2");
    
    let peer_id2 = node2.peer_id();
    
    let mut events1 = node1.start().await;
    let _events2 = node2.start().await;
    
    // Wait for nodes to start up
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    let addr2 = node2.listeners()[0].clone();
    
    // Connect nodes
    node1.connect(peer_id2, addr2).await.expect("Failed to connect");
    
    // Wait for connection to be established
    let mut connected = false;
    for _ in 0..10 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        if node1.is_connected(peer_id2) {
            connected = true;
            break;
        }
    }
    assert!(connected, "Node1 should be connected to node2");
    
    // Shutdown node2
    node2.shutdown().await;
    
    // Wait for expiration
    let expired = timeout(Duration::from_secs(5), async {
        loop {
            match events1.recv().await {
                Some(NodeEvent::DiscoveryEvent(DiscoveryEvent::PeerExpired { peer_id })) => {
                    if peer_id == peer_id2 {
                        return Ok(peer_id);
                    }
                }
                Some(_) => continue,
                None => return Err("Channel closed"),
            }
        }
    })
    .await
    .expect("Timeout waiting for peer expiration")
    .expect("Peer expiration failed");
    
    assert_eq!(expired, peer_id2);
    assert!(!node1.is_connected(peer_id2));
}

#[tokio::test]
#[ignore = "This test has timeout issues when run concurrently with other tests due to DHT query contention"]
async fn test_discovery_priority() {
    let mut bootstrap = create_bootstrap_node().await;
    
    // Create nodes with different priorities (but don't announce in helper)
    let mut high_priority_node = create_node_with_priority_no_announce(&bootstrap, 10).await;
    let mut medium_priority_node = create_node_with_priority_no_announce(&bootstrap, 5).await;
    let mut low_priority_node = create_node_with_priority_no_announce(&bootstrap, 1).await;
    
    // Wait for all nodes to be connected
    tokio::time::sleep(Duration::from_secs(1)).await;
    
    // Announce metadata for all nodes
    high_priority_node.announce_with_metadata().await.unwrap();
    medium_priority_node.announce_with_metadata().await.unwrap();
    low_priority_node.announce_with_metadata().await.unwrap();
    
    // Wait for metadata to propagate
    tokio::time::sleep(Duration::from_secs(3)).await;
    
    // Bootstrap should have the connected peers
    // The discover_peers_sorted_by_priority method now looks at both discovered and connected peers
    
    // Discover peers sorted by priority
    let peers = timeout(Duration::from_secs(10), bootstrap
        .discover_peers_sorted_by_priority())
        .await
        .expect("Timeout discovering peers")
        .expect("Failed to discover peers");
    
    // Should be sorted by priority
    assert_eq!(peers.len(), 3);
    assert_eq!(peers[0].0, high_priority_node.peer_id());
    assert_eq!(peers[0].1, 10);
    assert_eq!(peers[1].0, medium_priority_node.peer_id());
    assert_eq!(peers[1].1, 5);
    assert_eq!(peers[2].0, low_priority_node.peer_id());
    assert_eq!(peers[2].1, 1);
}

// Helper functions

async fn create_bootstrap_node() -> Node {
    let config = NodeConfig::default();
    let mut node = Node::new(config).await.expect("Failed to create bootstrap");
    let _events = node.start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    node
}

async fn create_node_with_capability(bootstrap: &Node, capability: &str) -> Node {
    let config = NodeConfig {
        bootstrap_peers: vec![(bootstrap.peer_id(), bootstrap.listeners()[0].clone())],
        capabilities: vec![capability.to_string()],
        ..Default::default()
    };
    let mut node = Node::new(config).await.expect("Failed to create node");
    let _events = node.start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    node
}

async fn create_node_with_capabilities(bootstrap: &Node, capabilities: Vec<&str>) -> Node {
    let config = NodeConfig {
        bootstrap_peers: vec![(bootstrap.peer_id(), bootstrap.listeners()[0].clone())],
        capabilities: capabilities.iter().map(|c| c.to_string()).collect(),
        ..Default::default()
    };
    let mut node = Node::new(config).await.expect("Failed to create node");
    let _events = node.start().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    node
}

async fn create_node_with_priority(bootstrap: &Node, priority: u32) -> Node {
    let metadata = serde_json::json!({
        "priority": priority
    });
    
    let config = NodeConfig {
        bootstrap_peers: vec![(bootstrap.peer_id(), bootstrap.listeners()[0].clone())],
        node_metadata: Some(metadata),
        ..Default::default()
    };
    
    let mut node = Node::new(config).await.expect("Failed to create node");
    let _events = node.start().await;
    
    // Wait for connection to bootstrap
    tokio::time::sleep(Duration::from_secs(1)).await;
    
    node.announce_with_metadata().await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    node
}

async fn create_node_with_priority_no_announce(bootstrap: &Node, priority: u32) -> Node {
    let metadata = serde_json::json!({
        "priority": priority
    });
    
    let config = NodeConfig {
        bootstrap_peers: vec![(bootstrap.peer_id(), bootstrap.listeners()[0].clone())],
        node_metadata: Some(metadata),
        ..Default::default()
    };
    
    let mut node = Node::new(config).await.expect("Failed to create node");
    let _events = node.start().await;
    
    // Wait for connection to bootstrap
    tokio::time::sleep(Duration::from_secs(1)).await;
    
    node
}