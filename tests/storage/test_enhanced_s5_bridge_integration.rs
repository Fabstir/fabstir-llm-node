// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Integration tests for Enhanced S5.js bridge service
//!
//! These tests require the bridge service to be running:
//! ```bash
//! cd services/s5-bridge && npm start
//! ```

use anyhow::Result;
use fabstir_llm_node::storage::enhanced_s5_client::{BridgeHealthResponse, EnhancedS5Client, S5Config};

/// Test 2.1: Enhanced S5 Client Initialization
///
/// Verifies that Rust can successfully connect to the bridge service
#[tokio::test]
async fn test_bridge_connection() -> Result<()> {
    // Connect to bridge service
    let config = S5Config {
        api_url: "http://localhost:5522".to_string(),
        api_key: None,
        timeout_secs: 30,
    };
    let client = EnhancedS5Client::new(config)?;

    // Test bridge health check
    let health = client.bridge_health_check().await?;

    // Verify response
    assert_eq!(health.status, "healthy");
    assert_eq!(health.service, "s5-bridge");
    assert!(health.initialized);
    assert!(health.connected);
    assert_eq!(health.peer_count, 1);

    println!("✅ Bridge connection successful");
    println!("   Status: {}", health.status);
    println!("   Service: {}", health.service);
    println!("   Initialized: {}", health.initialized);
    println!("   Connected: {}", health.connected);
    println!("   Peer Count: {}", health.peer_count);

    Ok(())
}

/// Test 2.1b: Health Check with Invalid URL
///
/// Verifies proper error handling for connection failures
#[tokio::test]
async fn test_bridge_connection_invalid_url() {
    let config = S5Config {
        api_url: "http://localhost:9999".to_string(),
        api_key: None,
        timeout_secs: 30,
    };
    let client = EnhancedS5Client::new(config).expect("Client creation should succeed");

    let result = client.bridge_health_check().await;
    assert!(result.is_err(), "Should fail to connect to non-existent service");

    println!("✅ Error handling works correctly for invalid URLs");
}

/// Test 2.1c: Multiple Sequential Health Checks
///
/// Verifies the client can make multiple requests
#[tokio::test]
async fn test_bridge_multiple_health_checks() -> Result<()> {
    let config = S5Config {
        api_url: "http://localhost:5522".to_string(),
        api_key: None,
        timeout_secs: 30,
    };
    let client = EnhancedS5Client::new(config)?;

    // Make 3 health check requests
    for i in 1..=3 {
        let health = client.bridge_health_check().await?;
        assert_eq!(health.status, "healthy");
        println!("✅ Health check #{} successful", i);
    }

    Ok(())
}
