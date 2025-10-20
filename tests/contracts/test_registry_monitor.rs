// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use ethers::prelude::*;
use fabstir_llm_node::contracts::registry_monitor::{NodeMetadata, RegistryMonitor};
use fabstir_llm_node::contracts::types::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_monitor_initialization() {
    // Test that monitor initializes with empty cache
    let provider = Provider::<Http>::try_from("http://localhost:8545").unwrap();
    let contract_address = "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
        .parse::<Address>()
        .unwrap();

    let monitor = RegistryMonitor::new(contract_address, Arc::new(provider));

    let hosts = monitor.get_registered_hosts().await;
    assert_eq!(hosts.len(), 0);
}

#[tokio::test]
async fn test_node_registered_event_updates_cache() {
    // Test that NodeRegistered events update the cache
    let provider = Provider::<Http>::try_from("http://localhost:8545").unwrap();
    let contract_address = "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
        .parse::<Address>()
        .unwrap();

    let monitor = RegistryMonitor::new(contract_address, Arc::new(provider));

    // Simulate a NodeRegistered event
    let node_addr = "0x1234567890123456789012345678901234567890"
        .parse::<Address>()
        .unwrap();
    let metadata = "gpu:rtx4090,ram:32gb".to_string();
    let stake = U256::from(1000000u64);

    monitor
        .handle_node_registered(node_addr, metadata.clone(), stake)
        .await;

    let hosts = monitor.get_registered_hosts().await;
    assert_eq!(hosts.len(), 1);
    assert!(hosts.contains(&node_addr));

    let node_metadata = monitor.get_host_metadata(node_addr).await;
    assert!(node_metadata.is_some());
    let meta = node_metadata.unwrap();
    assert_eq!(meta.metadata, metadata);
    assert_eq!(meta.stake, stake);
}

#[tokio::test]
async fn test_node_updated_event_modifies_cache() {
    // Test that NodeUpdated events modify existing entries
    let provider = Provider::<Http>::try_from("http://localhost:8545").unwrap();
    let contract_address = "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
        .parse::<Address>()
        .unwrap();

    let monitor = RegistryMonitor::new(contract_address, Arc::new(provider));

    let node_addr = "0x1234567890123456789012345678901234567890"
        .parse::<Address>()
        .unwrap();

    // First register the node
    monitor
        .handle_node_registered(
            node_addr,
            "gpu:rtx3090,ram:16gb".to_string(),
            U256::from(500000u64),
        )
        .await;

    // Then update it
    let new_metadata = "gpu:rtx4090,ram:32gb".to_string();
    monitor
        .handle_node_updated(node_addr, new_metadata.clone())
        .await;

    let node_metadata = monitor.get_host_metadata(node_addr).await;
    assert!(node_metadata.is_some());
    let meta = node_metadata.unwrap();
    assert_eq!(meta.metadata, new_metadata);
    assert_eq!(meta.stake, U256::from(500000u64)); // Stake should remain unchanged
}

#[tokio::test]
async fn test_node_unregistered_removes_from_cache() {
    // Test that NodeUnregistered events remove from cache
    let provider = Provider::<Http>::try_from("http://localhost:8545").unwrap();
    let contract_address = "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
        .parse::<Address>()
        .unwrap();

    let monitor = RegistryMonitor::new(contract_address, Arc::new(provider));

    let node_addr = "0x1234567890123456789012345678901234567890"
        .parse::<Address>()
        .unwrap();

    // Register the node
    monitor
        .handle_node_registered(node_addr, "gpu:rtx4090".to_string(), U256::from(1000000u64))
        .await;

    // Verify it's in cache
    assert_eq!(monitor.get_registered_hosts().await.len(), 1);

    // Unregister the node
    monitor.handle_node_unregistered(node_addr).await;

    // Verify it's removed
    assert_eq!(monitor.get_registered_hosts().await.len(), 0);
    assert!(monitor.get_host_metadata(node_addr).await.is_none());
}

#[tokio::test]
async fn test_concurrent_cache_access() {
    // Test thread-safe concurrent access to cache
    let provider = Provider::<Http>::try_from("http://localhost:8545").unwrap();
    let contract_address = "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
        .parse::<Address>()
        .unwrap();

    let monitor = Arc::new(RegistryMonitor::new(contract_address, Arc::new(provider)));

    let mut handles = vec![];

    // Spawn multiple tasks that read and write concurrently
    for i in 0..10 {
        let monitor_clone = monitor.clone();
        let handle = tokio::spawn(async move {
            let addr = format!("0x{:040x}", i + 1).parse::<Address>().unwrap();

            // Register node
            monitor_clone
                .handle_node_registered(addr, format!("gpu:test{}", i), U256::from(i * 1000))
                .await;

            // Read it back
            let meta = monitor_clone.get_host_metadata(addr).await;
            assert!(meta.is_some());
        });
        handles.push(handle);
    }

    // Wait for all tasks
    for handle in handles {
        handle.await.unwrap();
    }

    // Verify all nodes are in cache
    let hosts = monitor.get_registered_hosts().await;
    assert_eq!(hosts.len(), 10);
}

#[tokio::test]
async fn test_get_host_by_capabilities() {
    // Test filtering hosts by capabilities
    let provider = Provider::<Http>::try_from("http://localhost:8545").unwrap();
    let contract_address = "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
        .parse::<Address>()
        .unwrap();

    let monitor = RegistryMonitor::new(contract_address, Arc::new(provider));

    // Register nodes with different capabilities
    let node1 = "0x1111111111111111111111111111111111111111"
        .parse::<Address>()
        .unwrap();
    let node2 = "0x2222222222222222222222222222222222222222"
        .parse::<Address>()
        .unwrap();

    monitor
        .handle_node_registered(
            node1,
            "gpu:rtx4090,ram:32gb".to_string(),
            U256::from(1000000u64),
        )
        .await;

    monitor
        .handle_node_registered(
            node2,
            "gpu:rtx3090,ram:16gb".to_string(),
            U256::from(500000u64),
        )
        .await;

    // Get all hosts
    let all_hosts = monitor.get_registered_hosts().await;
    assert_eq!(all_hosts.len(), 2);

    // Get hosts with specific capability
    let hosts_with_4090 = monitor.get_hosts_by_capability("gpu:rtx4090").await;
    assert_eq!(hosts_with_4090.len(), 1);
    assert_eq!(hosts_with_4090[0], node1);
}

#[tokio::test]
async fn test_monitor_stop() {
    // Test graceful shutdown
    let provider = Provider::<Http>::try_from("http://localhost:8545").unwrap();
    let contract_address = "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
        .parse::<Address>()
        .unwrap();

    let mut monitor = RegistryMonitor::new(contract_address, Arc::new(provider));

    // Start monitoring (this would normally connect to blockchain)
    // For test, we'll just verify stop works without panic
    monitor.stop_monitoring().await;

    // Should be able to stop multiple times without panic
    monitor.stop_monitoring().await;
}
