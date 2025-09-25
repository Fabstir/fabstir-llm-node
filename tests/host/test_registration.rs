use ethers::prelude::*;
use fabstir_llm_node::host::registration::{NodeRegistration, NodeMetadata, RegistrationConfig};
use std::sync::Arc;
use tokio::sync::RwLock;

// Mock provider and wallet for testing
fn create_mock_provider() -> Provider<Http> {
    Provider::<Http>::try_from("http://localhost:8545").unwrap()
}

fn create_mock_wallet() -> LocalWallet {
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
        .parse::<LocalWallet>()
        .unwrap()
}

#[tokio::test]
async fn test_node_registration_with_valid_stake() {
    // Test that node can register with valid metadata and stake
    let provider = create_mock_provider();
    let wallet = create_mock_wallet();
    let stake_amount = U256::from(1000000u64); // 1M wei
    
    let metadata = NodeMetadata {
        models: vec!["llama-3.2".to_string(), "tiny-vicuna".to_string()],
        model_ids: vec![],  // Will be filled during registration
        gpu: "RTX 4090".to_string(),
        ram_gb: 64,
        cost_per_token: 0.0001,
        max_concurrent_jobs: 5,
        api_url: "http://localhost:8080".to_string(),
    };

    let config = RegistrationConfig {
        contract_address: "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
            .parse::<Address>()
            .unwrap(),
        model_registry_address: "0x92b2De840bB2171203011A6dBA928d855cA8183E"
            .parse::<Address>()
            .unwrap(),
        stake_amount,
        auto_register: false,
        heartbeat_interval: 60,
        use_new_registry: false,
    };
    
    let mut registration = NodeRegistration::new(
        Arc::new(provider),
        wallet,
        metadata.clone(),
        config
    ).await.unwrap();
    
    // Mock registration - should succeed
    let result = registration.register_node().await;
    assert!(result.is_ok());
    
    // Check that metadata is properly set
    let json = registration.build_metadata_json();
    assert!(json.contains("llama-3.2"));
    assert!(json.contains("RTX 4090"));
    assert!(json.contains("64"));
}

#[tokio::test]
async fn test_registration_fails_with_insufficient_stake() {
    // Test that registration fails with insufficient stake
    let provider = create_mock_provider();
    let wallet = create_mock_wallet();
    let stake_amount = U256::from(100u64); // Too small
    
    let metadata = NodeMetadata {
        models: vec!["llama-3.2".to_string()],
        model_ids: vec![],
        gpu: "RTX 3090".to_string(),
        ram_gb: 32,
        cost_per_token: 0.0002,
        max_concurrent_jobs: 3,
        api_url: "http://localhost:8080".to_string(),
    };
    
    let config = RegistrationConfig {
        contract_address: "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
            .parse::<Address>()
            .unwrap(),
        model_registry_address: "0x92b2De840bB2171203011A6dBA928d855cA8183E"
            .parse::<Address>()
            .unwrap(),
        stake_amount,
        auto_register: false,
        heartbeat_interval: 60,
        use_new_registry: false,
    };
    
    let registration = NodeRegistration::new(
        Arc::new(provider),
        wallet,
        metadata,
        config
    ).await.unwrap();
    
    // Should fail due to insufficient stake (mocked)
    let result = registration.check_stake_requirement().await;
    assert!(!result);
}

#[tokio::test]
async fn test_update_capabilities() {
    // Test that capabilities can be updated after registration
    let provider = create_mock_provider();
    let wallet = create_mock_wallet();
    let stake_amount = U256::from(1000000u64);
    
    let initial_metadata = NodeMetadata {
        models: vec!["llama-3.2".to_string()],
        model_ids: vec![],
        gpu: "RTX 3090".to_string(),
        ram_gb: 32,
        cost_per_token: 0.0002,
        max_concurrent_jobs: 3,
        api_url: "http://localhost:8080".to_string(),
    };
    
    let config = RegistrationConfig {
        contract_address: "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
            .parse::<Address>()
            .unwrap(),
        model_registry_address: "0x92b2De840bB2171203011A6dBA928d855cA8183E"
            .parse::<Address>()
            .unwrap(),
        stake_amount,
        auto_register: false,
        heartbeat_interval: 60,
        use_new_registry: false,
    };
    
    let mut registration = NodeRegistration::new(
        Arc::new(provider),
        wallet,
        initial_metadata,
        config
    ).await.unwrap();
    
    // Register first
    let _ = registration.register_node().await;
    
    // Update capabilities
    let new_metadata = NodeMetadata {
        models: vec!["llama-3.2".to_string(), "mistral-7b".to_string()],
        model_ids: vec![],
        gpu: "RTX 4090".to_string(), // Upgraded GPU
        ram_gb: 64, // Upgraded RAM
        cost_per_token: 0.0001,
        max_concurrent_jobs: 5,
        api_url: "http://localhost:8080".to_string(),
    };
    
    let result = registration.update_capabilities(new_metadata).await;
    assert!(result.is_ok());
    
    // Check updated metadata
    let json = registration.build_metadata_json();
    assert!(json.contains("mistral-7b"));
    assert!(json.contains("RTX 4090"));
    assert!(json.contains("64"));
}

#[tokio::test]
async fn test_unregister_node() {
    // Test that node can unregister successfully
    let provider = create_mock_provider();
    let wallet = create_mock_wallet();
    let stake_amount = U256::from(1000000u64);
    
    let metadata = NodeMetadata {
        models: vec!["llama-3.2".to_string()],
        model_ids: vec![],
        gpu: "RTX 3090".to_string(),
        ram_gb: 32,
        cost_per_token: 0.0002,
        max_concurrent_jobs: 3,
        api_url: "http://localhost:8080".to_string(),
    };
    
    let config = RegistrationConfig {
        contract_address: "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
            .parse::<Address>()
            .unwrap(),
        model_registry_address: "0x92b2De840bB2171203011A6dBA928d855cA8183E"
            .parse::<Address>()
            .unwrap(),
        stake_amount,
        auto_register: false,
        heartbeat_interval: 60,
        use_new_registry: false,
    };
    
    let mut registration = NodeRegistration::new(
        Arc::new(provider),
        wallet,
        metadata,
        config
    ).await.unwrap();
    
    // Register first
    let _ = registration.register_node().await;
    
    // Then unregister
    let result = registration.unregister_node().await;
    assert!(result.is_ok());
    
    // Heartbeat should be stopped
    assert!(!registration.is_heartbeat_running());
}

#[tokio::test]
async fn test_heartbeat_mechanism() {
    // Test that heartbeat updates last_seen (mocked)
    let provider = create_mock_provider();
    let wallet = create_mock_wallet();
    let stake_amount = U256::from(1000000u64);
    
    let metadata = NodeMetadata {
        models: vec!["llama-3.2".to_string()],
        model_ids: vec![],
        gpu: "RTX 3090".to_string(),
        ram_gb: 32,
        cost_per_token: 0.0002,
        max_concurrent_jobs: 3,
        api_url: "http://localhost:8080".to_string(),
    };
    
    let config = RegistrationConfig {
        contract_address: "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
            .parse::<Address>()
            .unwrap(),
        model_registry_address: "0x92b2De840bB2171203011A6dBA928d855cA8183E"
            .parse::<Address>()
            .unwrap(),
        stake_amount,
        auto_register: false,
        heartbeat_interval: 1, // 1 second for testing
        use_new_registry: false,
    };
    
    let mut registration = NodeRegistration::new(
        Arc::new(provider),
        wallet,
        metadata,
        config
    ).await.unwrap();
    
    // Start heartbeat
    registration.start_heartbeat();
    assert!(registration.is_heartbeat_running());
    
    // Wait for a heartbeat
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    
    // Check last seen was updated
    let last_seen = registration.get_last_heartbeat();
    assert!(last_seen > 0);
    
    // Stop heartbeat
    registration.stop_heartbeat().await;
    assert!(!registration.is_heartbeat_running());
}

#[tokio::test]
async fn test_auto_registration_on_startup() {
    // Test that auto-registration works on startup
    let provider = create_mock_provider();
    let wallet = create_mock_wallet();
    let stake_amount = U256::from(1000000u64);
    
    let metadata = NodeMetadata {
        models: vec!["llama-3.2".to_string()],
        model_ids: vec![],
        gpu: "RTX 3090".to_string(),
        ram_gb: 32,
        cost_per_token: 0.0002,
        max_concurrent_jobs: 3,
        api_url: "http://localhost:8080".to_string(),
    };
    
    let config = RegistrationConfig {
        contract_address: "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
            .parse::<Address>()
            .unwrap(),
        model_registry_address: "0x92b2De840bB2171203011A6dBA928d855cA8183E"
            .parse::<Address>()
            .unwrap(),
        stake_amount,
        auto_register: true, // Enable auto-registration
        heartbeat_interval: 60,
        use_new_registry: false,
    };
    
    let mut registration = NodeRegistration::new(
        Arc::new(provider),
        wallet,
        metadata,
        config
    ).await.unwrap();
    
    // Should be registered automatically
    assert!(registration.is_registered());
    assert!(registration.is_heartbeat_running());
}

#[tokio::test]
async fn test_metadata_json_formatting() {
    // Test that metadata JSON is properly formatted
    let provider = create_mock_provider();
    let wallet = create_mock_wallet();
    let stake_amount = U256::from(1000000u64);
    
    let metadata = NodeMetadata {
        models: vec!["llama-3.2".to_string(), "mistral-7b".to_string()],
        model_ids: vec![],
        gpu: "A100 80GB".to_string(),
        ram_gb: 128,
        cost_per_token: 0.00005,
        max_concurrent_jobs: 10,
        api_url: "http://localhost:8080".to_string(),
    };
    
    let config = RegistrationConfig {
        contract_address: "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
            .parse::<Address>()
            .unwrap(),
        model_registry_address: "0x92b2De840bB2171203011A6dBA928d855cA8183E"
            .parse::<Address>()
            .unwrap(),
        stake_amount,
        auto_register: false,
        heartbeat_interval: 60,
        use_new_registry: false,
    };
    
    let registration = NodeRegistration::new(
        Arc::new(provider),
        wallet,
        metadata,
        config
    ).await.unwrap();
    
    let json = registration.build_metadata_json();
    
    // Parse JSON to verify format
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    
    assert_eq!(parsed["gpu"], "A100 80GB");
    assert_eq!(parsed["ram"], 128);
    assert_eq!(parsed["cost_per_token"], 0.00005);
    assert_eq!(parsed["max_concurrent_jobs"], 10);
    
    let models = parsed["models"].as_array().unwrap();
    assert_eq!(models.len(), 2);
    assert!(models.contains(&serde_json::json!("llama-3.2")));
    assert!(models.contains(&serde_json::json!("mistral-7b")));
}

#[tokio::test]
async fn test_concurrent_registration_operations() {
    // Test thread-safe concurrent operations
    let provider = Arc::new(create_mock_provider());
    let wallet = create_mock_wallet();
    let stake_amount = U256::from(1000000u64);
    
    let metadata = NodeMetadata {
        models: vec!["llama-3.2".to_string()],
        model_ids: vec![],
        gpu: "RTX 3090".to_string(),
        ram_gb: 32,
        cost_per_token: 0.0002,
        max_concurrent_jobs: 3,
        api_url: "http://localhost:8080".to_string(),
    };
    
    let config = RegistrationConfig {
        contract_address: "0x87516C13Ea2f99de598665e14cab64E191A0f8c4"
            .parse::<Address>()
            .unwrap(),
        model_registry_address: "0x92b2De840bB2171203011A6dBA928d855cA8183E"
            .parse::<Address>()
            .unwrap(),
        stake_amount,
        auto_register: false,
        heartbeat_interval: 60,
        use_new_registry: false,
    };
    
    let registration = Arc::new(RwLock::new(
        NodeRegistration::new(provider, wallet, metadata, config).await.unwrap()
    ));
    
    let mut handles = vec![];
    
    // Spawn multiple tasks
    for i in 0..5 {
        let reg_clone = registration.clone();
        let handle = tokio::spawn(async move {
            match i % 3 {
                0 => {
                    let reg = reg_clone.read().await;
                    let _ = reg.build_metadata_json();
                }
                1 => {
                    let reg = reg_clone.read().await;
                    let _ = reg.check_stake_requirement().await;
                }
                _ => {
                    let reg = reg_clone.read().await;
                    let _ = reg.is_registered();
                }
            }
        });
        handles.push(handle);
    }
    
    // Wait for all tasks
    for handle in handles {
        handle.await.unwrap();
    }
}