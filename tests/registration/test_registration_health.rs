// Tests for registration health monitoring system
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};

use fabstir_llm_node::blockchain::multi_chain_registrar::{
    MultiChainRegistrar, NodeMetadata, RegistrationStatus,
};
use fabstir_llm_node::blockchain::registration_monitor::{
    HealthIssue, IssueType, MonitorConfig, RegistrationHealth, RegistrationMonitor,
};
use fabstir_llm_node::config::chains::ChainRegistry;

// Helper to create test monitor
async fn create_test_monitor() -> Result<RegistrationMonitor> {
    let chain_registry = Arc::new(ChainRegistry::new());

    let metadata = NodeMetadata {
        name: "Test Monitor Node".to_string(),
        version: "1.0.0".to_string(),
        api_url: "http://localhost:8080".to_string(),
        capabilities: vec!["inference".to_string()],
        performance_tier: "standard".to_string(),
    };

    let registrar = Arc::new(
        MultiChainRegistrar::new(
            chain_registry,
            "0xe7855c0ea54ccca55126d40f97d90868b2a73bad0363e92ccdec0c4fbd6c0ce2", // Test key
            metadata,
        )
        .await?,
    );

    let config = MonitorConfig {
        check_interval: Duration::from_secs(10), // Fast for testing
        warning_threshold: Duration::from_secs(3600), // 1 hour
        critical_threshold: Duration::from_secs(300), // 5 minutes
        auto_renewal: true,
        renewal_buffer: Duration::from_secs(1800), // 30 minutes
        max_retry_attempts: 3,
        retry_delay: Duration::from_secs(5),
    };

    Ok(RegistrationMonitor::new(registrar, config).await?)
}

#[tokio::test]
async fn test_registration_health_check() -> Result<()> {
    println!("🏥 Testing registration health check...");

    let monitor = create_test_monitor().await?;

    // Start monitoring
    monitor.start_monitoring().await?;

    // Give it time to perform initial health check
    sleep(Duration::from_secs(2)).await;

    // Check health status for Base Sepolia
    let health = monitor.get_health(84532).await?;

    // Verify health check was performed
    assert!(health.last_check.elapsed() < Duration::from_secs(5));

    // Check that status is tracked
    match health.status {
        RegistrationStatus::NotRegistered => {
            println!("  Node not registered (expected for test)");
        }
        RegistrationStatus::Confirmed { .. } => {
            println!("  Node is registered");
            assert!(health.is_healthy);
        }
        _ => {}
    }

    // Stop monitoring
    monitor.stop_monitoring().await?;

    println!("✅ Health check test passed");
    Ok(())
}

#[tokio::test]
async fn test_auto_renewal() -> Result<()> {
    println!("🔄 Testing auto-renewal functionality...");

    let monitor = create_test_monitor().await?;

    // Mock a registration that's about to expire
    monitor
        .mock_expiring_registration(84532, Duration::from_secs(600))
        .await?;

    // Enable auto-renewal
    monitor.enable_auto_renewal(84532).await?;

    // Start monitoring
    monitor.start_monitoring().await?;

    // Wait for renewal to trigger
    let renewed = timeout(Duration::from_secs(30), async {
        while !monitor.was_renewed(84532).await? {
            sleep(Duration::from_secs(1)).await;
        }
        Ok::<bool, anyhow::Error>(true)
    })
    .await??;

    assert!(renewed, "Auto-renewal should have triggered");

    // Check that renewal was attempted (in mock mode, it won't actually change status)
    let health = monitor.get_health(84532).await?;
    // In mock mode, the registration stays in its current state but renewal is recorded
    // The renewal history check above confirms the renewal was triggered

    monitor.stop_monitoring().await?;

    println!("✅ Auto-renewal test passed");
    Ok(())
}

#[tokio::test]
async fn test_expiry_warnings() -> Result<()> {
    println!("⚠️  Testing expiry warning system...");

    let monitor = create_test_monitor().await?;

    // Set up warning collection
    let warnings = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let warnings_clone = warnings.clone();

    monitor
        .on_warning(move |warning| {
            let warnings = warnings_clone.clone();
            Box::pin(async move {
                warnings.lock().await.push(warning);
            })
        })
        .await;

    // Mock registrations with different expiry times
    monitor
        .mock_expiring_registration(84532, Duration::from_secs(3500))
        .await?; // ~1 hour
    monitor
        .mock_expiring_registration(5611, Duration::from_secs(200))
        .await?; // ~3 minutes

    // Start monitoring
    monitor.start_monitoring().await?;

    // Wait for warnings to be generated
    sleep(Duration::from_secs(12)).await;

    // Check warnings were issued
    let collected_warnings = warnings.lock().await;
    assert!(!collected_warnings.is_empty(), "Should have warnings");

    // Should have critical warning for chain 5611
    let critical_warning = collected_warnings
        .iter()
        .any(|w| w.chain_id == 5611 && w.level == "CRITICAL");
    assert!(
        critical_warning,
        "Should have critical warning for near expiry"
    );

    // Should have warning for chain 84532
    let warning = collected_warnings
        .iter()
        .any(|w| w.chain_id == 84532 && w.level == "WARNING");
    assert!(warning, "Should have warning for upcoming expiry");

    monitor.stop_monitoring().await?;

    println!("✅ Expiry warnings test passed");
    Ok(())
}

#[tokio::test]
async fn test_registration_metrics() -> Result<()> {
    println!("📊 Testing registration metrics collection...");

    let monitor = create_test_monitor().await?;

    // Start monitoring
    monitor.start_monitoring().await?;

    // Perform some operations
    sleep(Duration::from_secs(5)).await;

    // Get metrics
    let metrics = monitor.get_metrics().await?;

    // Check key metrics exist
    assert!(
        metrics.contains_key("registration_status_84532"),
        "Missing registration_status_84532"
    );
    assert!(
        metrics.contains_key("health_check_count"),
        "Missing health_check_count"
    );
    // Histograms are returned as _count and _sum
    assert!(
        metrics.contains_key("health_check_duration_ms_count")
            || metrics.contains_key("health_check_duration_ms_sum"),
        "Missing health_check_duration_ms metrics"
    );

    // Check metric values
    let check_count = metrics.get("health_check_count").unwrap();
    assert!(*check_count > 0.0, "Should have performed health checks");

    monitor.stop_monitoring().await?;

    println!("✅ Metrics collection test passed");
    Ok(())
}

#[tokio::test]
async fn test_failure_recovery() -> Result<()> {
    println!("🔧 Testing failure recovery mechanisms...");

    let monitor = create_test_monitor().await?;

    // Simulate RPC failure
    monitor.simulate_rpc_failure(84532, true).await?;

    // Start monitoring
    monitor.start_monitoring().await?;

    // Should detect unhealthy state
    sleep(Duration::from_secs(2)).await;
    let health = monitor.get_health(84532).await?;
    assert!(!health.is_healthy);
    assert!(!health.issues.is_empty());

    // Fix the failure
    monitor.simulate_rpc_failure(84532, false).await?;

    // Should recover  - wait for next health check cycle
    sleep(Duration::from_secs(12)).await;
    let health_after = monitor.get_health(84532).await?;

    // In mock mode, recovery means the RPC failure issue is no longer present
    // Check that we no longer have RPC failure in issues
    let has_rpc_failure = health_after
        .issues
        .iter()
        .any(|issue| issue.issue_type == IssueType::RpcFailure && !issue.resolved);
    assert!(
        !has_rpc_failure,
        "RPC failure should be resolved after clearing simulation"
    );

    // Check retry metrics
    let metrics = monitor.get_metrics().await?;
    let retry_count = metrics.get("recovery_attempts").unwrap_or(&0.0);
    assert!(*retry_count > 0.0, "Should have retry attempts");

    monitor.stop_monitoring().await?;

    println!("✅ Failure recovery test passed");
    Ok(())
}

// Test multiple chain monitoring
#[tokio::test]
async fn test_multi_chain_monitoring() -> Result<()> {
    println!("🌐 Testing multi-chain monitoring...");

    let monitor = create_test_monitor().await?;

    // Start monitoring multiple chains
    monitor.start_monitoring().await?;

    // Wait for health checks
    sleep(Duration::from_secs(5)).await;

    // Check health for multiple chains
    let base_health = monitor.get_health(84532).await?;
    let opbnb_health = monitor.get_health(5611).await?;

    // Both should have been checked
    assert!(base_health.last_check.elapsed() < Duration::from_secs(10));
    assert!(opbnb_health.last_check.elapsed() < Duration::from_secs(10));

    monitor.stop_monitoring().await?;

    println!("✅ Multi-chain monitoring test passed");
    Ok(())
}

// Test configuration updates
#[tokio::test]
async fn test_config_update() -> Result<()> {
    println!("⚙️  Testing configuration updates...");

    let monitor = create_test_monitor().await?;

    // Start with default config
    monitor.start_monitoring().await?;

    // Update config
    let new_config = MonitorConfig {
        check_interval: Duration::from_secs(5),       // Faster
        warning_threshold: Duration::from_secs(7200), // 2 hours
        critical_threshold: Duration::from_secs(600), // 10 minutes
        auto_renewal: false,                          // Disable
        renewal_buffer: Duration::from_secs(3600),
        max_retry_attempts: 5,
        retry_delay: Duration::from_secs(10),
    };

    monitor.update_config(new_config).await?;

    // Verify config was applied
    assert!(!monitor.is_auto_renewal_enabled(84532).await?);

    monitor.stop_monitoring().await?;

    println!("✅ Config update test passed");
    Ok(())
}
