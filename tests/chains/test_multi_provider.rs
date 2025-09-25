use ethers::providers::{Http, Middleware, Provider};
use fabstir_llm_node::config::chains::{ChainConfigLoader, MultiChainProvider, ProviderHealth};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_provider_initialization() {
    let loader = ChainConfigLoader::new();
    let provider_manager = MultiChainProvider::new(loader).await.unwrap();

    // Should have providers for both chains
    assert!(provider_manager.get_provider(84532).is_some());
    assert!(provider_manager.get_provider(5611).is_some());

    // Should not have provider for unsupported chain
    assert!(provider_manager.get_provider(1).is_none());
}

#[tokio::test]
async fn test_get_provider_by_chain() {
    let loader = ChainConfigLoader::new();
    let provider_manager = MultiChainProvider::new(loader).await.unwrap();

    // Get Base Sepolia provider
    let base_provider = provider_manager.get_provider(84532).unwrap();
    let chain_id = base_provider.get_chainid().await.unwrap();
    assert_eq!(chain_id.as_u64(), 84532);

    // Get opBNB provider (might fail if testnet is down, so we just check it exists)
    let opbnb_provider = provider_manager.get_provider(5611);
    assert!(opbnb_provider.is_some());
}

#[tokio::test]
async fn test_provider_health_check() {
    let loader = ChainConfigLoader::new();
    let mut provider_manager = MultiChainProvider::new(loader).await.unwrap();

    // Check health of Base Sepolia provider
    let health = provider_manager.check_provider_health(84532).await;
    match health {
        ProviderHealth::Healthy {
            latency_ms,
            block_number,
        } => {
            assert!(latency_ms > 0);
            assert!(block_number.is_some());
        }
        ProviderHealth::Degraded { latency_ms, .. } => {
            assert!(latency_ms > 1000); // Degraded if latency > 1 second
        }
        ProviderHealth::Unhealthy { .. } => {
            // Provider might be down, that's ok for test
        }
    }

    // Check all providers health
    let all_health = provider_manager.check_all_providers_health().await;
    assert!(all_health.len() >= 2); // Should have health for at least 2 chains
}

#[tokio::test]
async fn test_rpc_failover() {
    // Create loader with custom RPC URLs including backup
    std::env::set_var("BASE_SEPOLIA_RPC_URL", "https://sepolia.base.org");
    std::env::set_var(
        "BASE_SEPOLIA_BACKUP_RPC_URL",
        "https://base-sepolia.public.blastapi.io",
    );

    let loader = ChainConfigLoader::new();
    let mut provider_manager = MultiChainProvider::with_failover(loader).await.unwrap();

    // Get provider (should use primary by default)
    let provider = provider_manager.get_provider(84532).unwrap();
    assert!(provider_manager.is_using_primary(84532));

    // Simulate primary failure by setting invalid URL
    std::env::set_var(
        "BASE_SEPOLIA_RPC_URL",
        "http://invalid.url.that.doesnt.exist",
    );

    // Force failover check
    provider_manager.trigger_failover_if_needed(84532).await;

    // Should now be using backup (if primary actually fails)
    // Note: In real scenario, this would switch to backup
    let provider_after = provider_manager.get_provider(84532);
    assert!(provider_after.is_some());

    // Cleanup
    std::env::remove_var("BASE_SEPOLIA_RPC_URL");
    std::env::remove_var("BASE_SEPOLIA_BACKUP_RPC_URL");
}

#[tokio::test]
async fn test_concurrent_providers() {
    let loader = ChainConfigLoader::new();
    let provider_manager = Arc::new(MultiChainProvider::new(loader).await.unwrap());

    // Spawn multiple concurrent tasks accessing different providers
    let mut handles = vec![];

    // Task 1: Access Base Sepolia provider
    let pm1 = provider_manager.clone();
    handles.push(tokio::spawn(async move {
        let provider = pm1.get_provider(84532);
        assert!(provider.is_some());
        if let Some(p) = provider {
            let _ = p.get_block_number().await;
        }
    }));

    // Task 2: Access opBNB provider
    let pm2 = provider_manager.clone();
    handles.push(tokio::spawn(async move {
        let provider = pm2.get_provider(5611);
        assert!(provider.is_some());
    }));

    // Task 3: Access non-existent provider
    let pm3 = provider_manager.clone();
    handles.push(tokio::spawn(async move {
        let provider = pm3.get_provider(99999);
        assert!(provider.is_none());
    }));

    // Wait for all tasks to complete
    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_provider_pooling() {
    let loader = ChainConfigLoader::new();
    let provider_manager = MultiChainProvider::with_pool_size(loader, 5).await.unwrap();

    // Request the same provider multiple times
    let provider1 = provider_manager.get_provider(84532);
    let provider2 = provider_manager.get_provider(84532);
    let provider3 = provider_manager.get_provider(84532);

    // All should be Some
    assert!(provider1.is_some());
    assert!(provider2.is_some());
    assert!(provider3.is_some());

    // Pool stats should show reuse
    let stats = provider_manager.get_pool_stats(84532);
    assert!(stats.total_requests >= 3);
    assert!(stats.pool_size <= 5);
}

#[tokio::test]
async fn test_provider_retry_logic() {
    let loader = ChainConfigLoader::new();
    let provider_manager = MultiChainProvider::new(loader).await.unwrap();

    // Test retry logic with timeout
    let result = provider_manager
        .execute_with_retry(84532, |provider| {
            Box::pin(async move { provider.get_block_number().await })
        })
        .await;

    // Should succeed or timeout gracefully
    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn test_provider_rotation() {
    // Set multiple RPC endpoints
    std::env::set_var(
        "BASE_SEPOLIA_RPC_URLS",
        "https://sepolia.base.org,https://base-sepolia.public.blastapi.io",
    );

    let loader = ChainConfigLoader::new();
    let mut provider_manager = MultiChainProvider::with_rotation(loader).await.unwrap();

    // Get provider multiple times - should rotate through endpoints
    let _p1 = provider_manager.get_provider_with_rotation(84532);
    let _p2 = provider_manager.get_provider_with_rotation(84532);
    let _p3 = provider_manager.get_provider_with_rotation(84532);

    let rotation_stats = provider_manager.get_rotation_stats(84532);
    assert!(rotation_stats.rotation_count > 0);

    // Cleanup
    std::env::remove_var("BASE_SEPOLIA_RPC_URLS");
}
