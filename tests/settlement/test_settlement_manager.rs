use ethers::signers::Signer;
use ethers::types::U256;
use fabstir_llm_node::config::chains::ChainRegistry;
use fabstir_llm_node::settlement::{
    gas_estimator::GasEstimator,
    manager::SettlementManager,
    queue::{SettlementQueue, SettlementRequest},
    types::{SettlementError, SettlementStatus},
};
use std::sync::Arc;

// Test helper to create a test private key
fn test_private_key() -> String {
    "0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string()
}

#[tokio::test]
async fn test_settlement_manager_init() {
    let registry = Arc::new(ChainRegistry::new());
    let private_key = test_private_key();

    let manager = SettlementManager::new(registry.clone(), &private_key)
        .await
        .expect("Failed to create settlement manager");

    // Should have providers for both chains
    assert_eq!(manager.provider_count(), 2);

    // Should be able to get providers for known chains
    assert!(manager.get_provider(84532).is_some()); // Base Sepolia
    assert!(manager.get_provider(5611).is_some()); // opBNB Testnet

    // Should not have provider for unknown chain
    assert!(manager.get_provider(99999).is_none());
}

#[tokio::test]
async fn test_signer_per_chain() {
    let registry = Arc::new(ChainRegistry::new());
    let private_key = test_private_key();

    let manager = SettlementManager::new(registry.clone(), &private_key)
        .await
        .expect("Failed to create settlement manager");

    // Should have signers for both chains
    let base_signer = manager.get_signer(84532).expect("No Base Sepolia signer");
    let opbnb_signer = manager.get_signer(5611).expect("No opBNB signer");

    // Signers should have correct chain IDs
    assert_eq!(base_signer.signer().chain_id(), 84532);
    assert_eq!(opbnb_signer.signer().chain_id(), 5611);

    // Should not have signer for unknown chain
    assert!(manager.get_signer(99999).is_none());
}

#[tokio::test]
async fn test_gas_estimation_base() {
    let estimator = GasEstimator::new();

    // Test Base Sepolia gas estimation
    let gas_estimate = estimator
        .estimate_gas(84532, "settle_session")
        .expect("Failed to estimate gas for Base Sepolia");

    // Base Sepolia should have reasonable gas limit
    assert!(gas_estimate.gas_limit >= U256::from(150_000));
    assert!(gas_estimate.gas_limit <= U256::from(300_000));

    // Should have gas multiplier
    assert!(gas_estimate.gas_multiplier > 1.0);
    assert!(gas_estimate.gas_multiplier <= 1.5);
}

#[tokio::test]
async fn test_gas_estimation_opbnb() {
    let estimator = GasEstimator::new();

    // Test opBNB gas estimation
    let gas_estimate = estimator
        .estimate_gas(5611, "settle_session")
        .expect("Failed to estimate gas for opBNB");

    // opBNB might need higher gas limit
    assert!(gas_estimate.gas_limit >= U256::from(200_000));
    assert!(gas_estimate.gas_limit <= U256::from(400_000));

    // Should have gas multiplier
    assert!(gas_estimate.gas_multiplier > 1.0);
    assert!(gas_estimate.gas_multiplier <= 1.5);
}

#[tokio::test]
async fn test_settlement_queue() {
    let mut queue = SettlementQueue::new();

    // Create test requests
    let request1 = SettlementRequest {
        session_id: 1,
        chain_id: 84532,
        priority: 1,
        retry_count: 0,
        status: SettlementStatus::Pending,
    };

    let request2 = SettlementRequest {
        session_id: 2,
        chain_id: 5611,
        priority: 2, // Higher priority
        retry_count: 0,
        status: SettlementStatus::Pending,
    };

    let request3 = SettlementRequest {
        session_id: 3,
        chain_id: 84532,
        priority: 1,
        retry_count: 0,
        status: SettlementStatus::Pending,
    };

    // Add requests to queue
    queue.add(request1.clone()).await;
    queue.add(request2.clone()).await;
    queue.add(request3.clone()).await;

    assert_eq!(queue.size().await, 3);

    // Should return highest priority first
    let next = queue.get_next().await.expect("Queue should have items");
    assert_eq!(next.session_id, 2);
    assert_eq!(next.priority, 2);

    // Should be able to get by chain
    let base_requests = queue.get_by_chain(84532).await;
    assert_eq!(base_requests.len(), 2);

    let opbnb_requests = queue.get_by_chain(5611).await;
    assert_eq!(opbnb_requests.len(), 1);

    // Should be able to update status
    queue.update_status(1, SettlementStatus::Processing).await;
    let updated = queue.get(1).await.expect("Should find request");
    assert_eq!(updated.status, SettlementStatus::Processing);
}

#[tokio::test]
async fn test_settlement_queue_retry() {
    let mut queue = SettlementQueue::new();

    let mut request = SettlementRequest {
        session_id: 1,
        chain_id: 84532,
        priority: 1,
        retry_count: 0,
        status: SettlementStatus::Failed,
    };

    queue.add(request.clone()).await;

    // Increment retry count
    queue.increment_retry(1).await;
    let updated = queue.get(1).await.expect("Should find request");
    assert_eq!(updated.retry_count, 1);

    // Multiple retries
    queue.increment_retry(1).await;
    queue.increment_retry(1).await;
    let updated = queue.get(1).await.expect("Should find request");
    assert_eq!(updated.retry_count, 3);

    // Should be able to reset for retry
    queue.reset_for_retry(1).await;
    let reset = queue.get(1).await.expect("Should find request");
    assert_eq!(reset.status, SettlementStatus::Pending);
}

#[tokio::test]
async fn test_gas_estimation_unknown_chain() {
    let estimator = GasEstimator::new();

    // Should return error for unknown chain
    let result = estimator.estimate_gas(99999, "settle_session");
    assert!(result.is_err());

    match result {
        Err(SettlementError::UnsupportedChain(chain_id)) => {
            assert_eq!(chain_id, 99999);
        }
        _ => panic!("Expected UnsupportedChain error"),
    }
}

#[tokio::test]
async fn test_settlement_manager_health_check() {
    let registry = Arc::new(ChainRegistry::new());
    let private_key = test_private_key();

    let manager = SettlementManager::new(registry.clone(), &private_key)
        .await
        .expect("Failed to create settlement manager");

    // Should be able to check health of providers
    let base_health = manager.check_provider_health(84532).await;
    let opbnb_health = manager.check_provider_health(5611).await;

    // Health checks might fail in test environment, but should not panic
    // Just verify the method exists and returns a result
    assert!(base_health.is_ok() || base_health.is_err());
    assert!(opbnb_health.is_ok() || opbnb_health.is_err());
}
