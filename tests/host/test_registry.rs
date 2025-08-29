use ethers::prelude::*;
use fabstir_llm_node::host::registry::{HostRegistry, HostInfo};
use fabstir_llm_node::contracts::registry_monitor::{RegistryMonitor, NodeMetadata};
use std::sync::Arc;
use std::collections::HashSet;

// Helper function to create a mock RegistryMonitor
async fn create_mock_monitor() -> Arc<RegistryMonitor> {
    let provider = Provider::<Http>::try_from("http://localhost:8545").unwrap();
    let contract_address = "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
        .parse::<Address>()
        .unwrap();
    
    let monitor = Arc::new(RegistryMonitor::new(contract_address, Arc::new(provider)));
    
    // Add some test hosts
    let host1 = "0x1111111111111111111111111111111111111111"
        .parse::<Address>()
        .unwrap();
    let host2 = "0x2222222222222222222222222222222222222222"
        .parse::<Address>()
        .unwrap();
    let host3 = "0x3333333333333333333333333333333333333333"
        .parse::<Address>()
        .unwrap();
    
    monitor.handle_node_registered(
        host1,
        r#"{"gpu":"rtx4090","ram":"32gb","models":["llama-7b","mistral-7b"]}"#.to_string(),
        U256::from(1000000u64)
    ).await;
    
    monitor.handle_node_registered(
        host2,
        r#"{"gpu":"rtx3090","ram":"16gb","models":["llama-7b"]}"#.to_string(),
        U256::from(500000u64)
    ).await;
    
    monitor.handle_node_registered(
        host3,
        r#"{"gpu":"a100","ram":"80gb","models":["llama-70b","mistral-7b","gpt-j"]}"#.to_string(),
        U256::from(2000000u64)
    ).await;
    
    monitor
}

#[tokio::test]
async fn test_get_registered_hosts() {
    // Test that getRegisteredHosts returns all hosts
    let monitor = create_mock_monitor().await;
    let registry = HostRegistry::new(monitor);
    
    let hosts = registry.get_registered_hosts().await;
    assert_eq!(hosts.len(), 3);
    
    let host_set: HashSet<Address> = hosts.into_iter().collect();
    assert!(host_set.contains(&"0x1111111111111111111111111111111111111111".parse::<Address>().unwrap()));
    assert!(host_set.contains(&"0x2222222222222222222222222222222222222222".parse::<Address>().unwrap()));
    assert!(host_set.contains(&"0x3333333333333333333333333333333333333333".parse::<Address>().unwrap()));
}

#[tokio::test]
async fn test_get_host_metadata() {
    // Test that getHostMetadata retrieves correct metadata
    let monitor = create_mock_monitor().await;
    let registry = HostRegistry::new(monitor);
    
    let host1 = "0x1111111111111111111111111111111111111111"
        .parse::<Address>()
        .unwrap();
    
    let metadata = registry.get_host_metadata(host1).await;
    assert!(metadata.is_some());
    
    let info = metadata.unwrap();
    assert_eq!(info.address, host1);
    assert!(info.metadata.contains("rtx4090"));
    assert_eq!(info.stake, U256::from(1000000u64));
    assert!(info.is_online); // Should be true for mocked implementation
}

#[tokio::test]
async fn test_is_host_online() {
    // Test that isHostOnline returns status (mocked)
    let monitor = create_mock_monitor().await;
    let registry = HostRegistry::new(monitor);
    
    let host1 = "0x1111111111111111111111111111111111111111"
        .parse::<Address>()
        .unwrap();
    let unknown_host = "0x9999999999999999999999999999999999999999"
        .parse::<Address>()
        .unwrap();
    
    // Registered host should be online (mocked)
    assert!(registry.is_host_online(host1).await);
    
    // Unknown host should be offline
    assert!(!registry.is_host_online(unknown_host).await);
}

#[tokio::test]
async fn test_get_available_hosts_by_model() {
    // Test that getAvailableHosts filters by model correctly
    let monitor = create_mock_monitor().await;
    let registry = HostRegistry::new(monitor);
    
    // Get hosts that support llama-7b
    let llama_hosts = registry.get_available_hosts("llama-7b").await;
    assert_eq!(llama_hosts.len(), 2); // host1 and host2
    
    // Get hosts that support llama-70b
    let llama70b_hosts = registry.get_available_hosts("llama-70b").await;
    assert_eq!(llama70b_hosts.len(), 1); // only host3
    
    // Get hosts that support mistral-7b
    let mistral_hosts = registry.get_available_hosts("mistral-7b").await;
    assert_eq!(mistral_hosts.len(), 2); // host1 and host3
    
    // Non-existent model should return empty
    let unknown_hosts = registry.get_available_hosts("unknown-model").await;
    assert_eq!(unknown_hosts.len(), 0);
}

#[tokio::test]
async fn test_get_hosts_by_capability() {
    // Test that getHostsByCapability filters properly
    let monitor = create_mock_monitor().await;
    let registry = HostRegistry::new(monitor);
    
    // Get hosts with rtx4090
    let rtx4090_hosts = registry.get_hosts_by_capability("rtx4090").await;
    assert_eq!(rtx4090_hosts.len(), 1);
    
    // Get hosts with 32gb RAM
    let ram32_hosts = registry.get_hosts_by_capability("32gb").await;
    assert_eq!(ram32_hosts.len(), 1);
    
    // Get hosts with a100
    let a100_hosts = registry.get_hosts_by_capability("a100").await;
    assert_eq!(a100_hosts.len(), 1);
    
    // Non-existent capability should return empty
    let unknown_hosts = registry.get_hosts_by_capability("nonexistent").await;
    assert_eq!(unknown_hosts.len(), 0);
}

#[tokio::test]
async fn test_concurrent_access() {
    // Test thread-safe concurrent access
    let monitor = create_mock_monitor().await;
    let registry = Arc::new(HostRegistry::new(monitor));
    
    let mut handles = vec![];
    
    // Spawn multiple tasks accessing the registry concurrently
    for i in 0..10 {
        let registry_clone = registry.clone();
        let handle = tokio::spawn(async move {
            // Each task does different operations
            match i % 4 {
                0 => {
                    let hosts = registry_clone.get_registered_hosts().await;
                    assert!(!hosts.is_empty());
                }
                1 => {
                    let host = "0x1111111111111111111111111111111111111111"
                        .parse::<Address>()
                        .unwrap();
                    let meta = registry_clone.get_host_metadata(host).await;
                    assert!(meta.is_some());
                }
                2 => {
                    let hosts = registry_clone.get_available_hosts("llama-7b").await;
                    assert!(!hosts.is_empty());
                }
                _ => {
                    let hosts = registry_clone.get_hosts_by_capability("gpu").await;
                    assert!(!hosts.is_empty());
                }
            }
        });
        handles.push(handle);
    }
    
    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_empty_results_handled() {
    // Test empty results handled gracefully
    let provider = Provider::<Http>::try_from("http://localhost:8545").unwrap();
    let contract_address = "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
        .parse::<Address>()
        .unwrap();
    
    let monitor = Arc::new(RegistryMonitor::new(contract_address, Arc::new(provider)));
    let registry = HostRegistry::new(monitor);
    
    // All methods should return empty results gracefully
    let hosts = registry.get_registered_hosts().await;
    assert_eq!(hosts.len(), 0);
    
    let unknown_host = "0x9999999999999999999999999999999999999999"
        .parse::<Address>()
        .unwrap();
    let metadata = registry.get_host_metadata(unknown_host).await;
    assert!(metadata.is_none());
    
    let model_hosts = registry.get_available_hosts("any-model").await;
    assert_eq!(model_hosts.len(), 0);
    
    let capability_hosts = registry.get_hosts_by_capability("any-capability").await;
    assert_eq!(capability_hosts.len(), 0);
}

#[tokio::test]
async fn test_host_info_structure() {
    // Test that HostInfo contains all expected fields
    let monitor = create_mock_monitor().await;
    let registry = HostRegistry::new(monitor);
    
    let host2 = "0x2222222222222222222222222222222222222222"
        .parse::<Address>()
        .unwrap();
    
    let info = registry.get_host_metadata(host2).await.unwrap();
    
    // Verify all fields are present and correct
    assert_eq!(info.address, host2);
    assert!(info.metadata.contains("rtx3090"));
    assert!(info.metadata.contains("16gb"));
    assert!(info.metadata.contains("llama-7b"));
    assert_eq!(info.stake, U256::from(500000u64));
    assert!(info.is_online); // Mocked to true
}

#[tokio::test]
async fn test_model_index_performance() {
    // Test that model lookups are efficient
    let monitor = create_mock_monitor().await;
    let registry = HostRegistry::new(monitor.clone());
    
    // Add many more hosts
    for i in 4..20 {
        let host = format!("0x{:040x}", i)
            .parse::<Address>()
            .unwrap();
        
        let models = if i % 2 == 0 {
            r#"["llama-7b","test-model"]"#
        } else {
            r#"["mistral-7b","test-model"]"#
        };
        
        monitor.handle_node_registered(
            host,
            format!(r#"{{"gpu":"gpu{}","models":{}}}"#, i, models),
            U256::from(100000u64)
        ).await;
    }
    
    // Should still be fast with many hosts
    let start = std::time::Instant::now();
    let test_hosts = registry.get_available_hosts("test-model").await;
    let duration = start.elapsed();
    
    assert_eq!(test_hosts.len(), 16); // All newly added hosts have test-model
    assert!(duration.as_millis() < 100); // Should be very fast
}