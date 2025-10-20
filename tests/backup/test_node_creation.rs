// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::p2p::{Node, NodeConfig, NodeEvent};
use libp2p::{identity, multiaddr::Protocol, Multiaddr, PeerId};
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_node_creation_with_identity() {
    // Create a new node with generated identity
    let config = NodeConfig::default();
    let mut node = Node::new(config).await.expect("Failed to create node");
    
    // Node should have a peer ID
    assert!(!node.peer_id().to_string().is_empty());
    
    // Start the node to get listeners
    let _event_receiver = node.start().await;
    
    // Wait for node to start listening
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Node should be listening on configured addresses
    let listeners = node.listeners();
    assert!(!listeners.is_empty());
    
    // Should be listening on TCP and QUIC
    let has_tcp = listeners.iter().any(|addr| {
        addr.iter().any(|p| matches!(p, Protocol::Tcp(_)))
    });
    let has_quic = listeners.iter().any(|addr| {
        addr.iter().any(|p| matches!(p, Protocol::QuicV1))
    });
    
    assert!(has_tcp, "Node should listen on TCP");
    assert!(has_quic, "Node should listen on QUIC");
}

#[tokio::test]
async fn test_node_with_persistent_identity() {
    // Create identity
    let keypair = identity::Keypair::generate_ed25519();
    let peer_id = PeerId::from(keypair.public());
    
    // Create node with specific identity
    let config = NodeConfig {
        keypair: Some(keypair.clone()),
        ..Default::default()
    };
    
    let node = Node::new(config).await.expect("Failed to create node");
    
    // Should use provided identity
    assert_eq!(node.peer_id(), peer_id);
}

#[tokio::test]
async fn test_node_listen_addresses() {
    let config = NodeConfig {
        listen_addresses: vec![
            "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
            "/ip4/127.0.0.1/udp/0/quic-v1".parse().unwrap(),
        ],
        ..Default::default()
    };
    
    let node = Node::new(config).await.expect("Failed to create node");
    
    // Wait for node to start listening
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    let listeners = node.listeners();
    assert_eq!(listeners.len(), 2, "Should have 2 listen addresses");
}

#[tokio::test]
async fn test_node_event_stream() {
    let config = NodeConfig::default();
    let mut node = Node::new(config).await.expect("Failed to create node");
    
    // Start the node
    let mut event_receiver = node.start().await;
    
    // Should receive NewListenAddr events
    let event = timeout(Duration::from_secs(1), event_receiver.recv())
        .await
        .expect("Timeout waiting for event")
        .expect("Channel closed");
    
    match event {
        NodeEvent::NewListenAddr { address, .. } => {
            assert!(!address.to_string().is_empty());
        }
        _ => panic!("Expected NewListenAddr event"),
    }
}

#[tokio::test]
async fn test_node_shutdown() {
    let config = NodeConfig::default();
    let mut node = Node::new(config).await.expect("Failed to create node");
    
    let mut event_receiver = node.start().await;
    
    // Node should be running
    assert!(node.is_running());
    
    // Shutdown the node
    node.shutdown().await;
    
    // Node should not be running
    assert!(!node.is_running());
    
    // Event channel should be closed
    assert!(event_receiver.recv().await.is_none());
}

#[tokio::test]
async fn test_node_external_addresses() {
    let config = NodeConfig {
        external_addresses: vec![
            "/ip4/1.2.3.4/tcp/4001".parse().unwrap(),
        ],
        ..Default::default()
    };
    
    let node = Node::new(config).await.expect("Failed to create node");
    
    // Should have external address configured
    let external_addrs = node.external_addresses();
    assert_eq!(external_addrs.len(), 1);
    assert_eq!(external_addrs[0].to_string(), "/ip4/1.2.3.4/tcp/4001");
}

#[tokio::test]
async fn test_node_with_bootstrap_peers() {
    // Create bootstrap node
    let bootstrap_config = NodeConfig::default();
    let mut bootstrap_node = Node::new(bootstrap_config).await.expect("Failed to create bootstrap node");
    let bootstrap_peer_id = bootstrap_node.peer_id();
    
    // Start bootstrap node to get listeners
    let _bootstrap_events = bootstrap_node.start().await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    let bootstrap_addr = bootstrap_node.listeners()[0].clone();
    
    // Create node with bootstrap peer
    let config = NodeConfig {
        bootstrap_peers: vec![(bootstrap_peer_id, bootstrap_addr.clone())],
        ..Default::default()
    };
    
    let node = Node::new(config).await.expect("Failed to create node");
    
    // Should have bootstrap peer configured
    let bootstrap_peers = node.bootstrap_peers();
    assert_eq!(bootstrap_peers.len(), 1);
    assert_eq!(bootstrap_peers[0].0, bootstrap_peer_id);
}

#[tokio::test]
async fn test_node_metrics() {
    let config = NodeConfig::default();
    let mut node = Node::new(config).await.expect("Failed to create node");
    
    let _event_receiver = node.start().await;
    
    // Wait for node to initialize
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Should have metrics available
    let metrics = node.metrics();
    assert_eq!(metrics.connected_peers, 0);
    assert!(metrics.bandwidth_in >= 0);
    assert!(metrics.bandwidth_out >= 0);
    assert!(metrics.uptime.as_secs() < 1);
}

#[tokio::test]
async fn test_node_connection_limits() {
    let config = NodeConfig {
        max_connections: 50,
        max_connections_per_peer: 2,
        connection_idle_timeout: Duration::from_secs(30),
        ..Default::default()
    };
    
    let node = Node::new(config.clone()).await.expect("Failed to create node");
    
    // Should respect connection limits
    let limits = node.connection_limits();
    assert_eq!(limits.max_connections, 50);
    assert_eq!(limits.max_connections_per_peer, 2);
    assert_eq!(limits.idle_timeout, Duration::from_secs(30));
}

#[tokio::test]
async fn test_node_capabilities() {
    let config = NodeConfig {
        capabilities: vec![
            "llama-7b".to_string(),
            "llama-13b".to_string(),
            "mistral-7b".to_string(),
        ],
        ..Default::default()
    };
    
    let node = Node::new(config).await.expect("Failed to create node");
    
    // Should have configured capabilities
    let capabilities = node.capabilities();
    assert_eq!(capabilities.len(), 3);
    assert!(capabilities.contains(&"llama-7b".to_string()));
    assert!(capabilities.contains(&"llama-13b".to_string()));
    assert!(capabilities.contains(&"mistral-7b".to_string()));
}

#[tokio::test]
async fn test_node_reconnect_on_failure() {
    let config = NodeConfig {
        enable_auto_reconnect: true,
        reconnect_interval: Duration::from_secs(5),
        ..Default::default()
    };
    
    let node = Node::new(config).await.expect("Failed to create node");
    
    // Should have auto-reconnect enabled
    assert!(node.is_auto_reconnect_enabled());
    assert_eq!(node.reconnect_interval(), Duration::from_secs(5));
}