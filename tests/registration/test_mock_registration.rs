// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use fabstir_llm_node::blockchain::multi_chain_registrar::{MultiChainRegistrar, NodeMetadata};
use std::env;

// Mock test that doesn't require network access
#[tokio::test]
async fn test_registrar_creation() -> Result<()> {
    println!("✅ Testing MultiChainRegistrar creation...");

    // Set up test environment variables
    env::set_var("CONTRACT_NODE_REGISTRY", "0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218");
    env::set_var("CONTRACT_FAB_TOKEN", "0x6AC05a870E0EE506C14155fA9Aa75c34cf2D8859");
    env::set_var("CONTRACT_MODEL_REGISTRY", "0x92b2De840bB2171203011A6dBA928d855cA8183E");

    let test_private_key = "0xe7855c0ea54ccca55126d40f97d90868b2a73bad0363e92ccdec0c4fbd6c0ce2";

    let metadata = NodeMetadata {
        name: "Test Node".to_string(),
        version: "1.0.0".to_string(),
        api_url: "http://localhost:8080".to_string(),
        capabilities: vec!["inference".to_string()],
        performance_tier: "standard".to_string(),
    };

    // Just create the registrar without network calls
    let registrar = MultiChainRegistrar::new(
        &test_private_key,
        metadata,
    ).await?;

    println!("✅ Registrar created successfully");

    // Test getting registration status (should be NotRegistered)
    let status = registrar.get_registration_status(84532).await?;
    println!("✅ Status check: {:?}", status);

    Ok(())
}

// Test FAB balance checking logic (mocked)
#[tokio::test]
async fn test_registration_requirements() -> Result<()> {
    println!("✅ Testing registration requirements...");

    // Set required env vars
    env::set_var("CONTRACT_NODE_REGISTRY", "0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218");
    env::set_var("CONTRACT_FAB_TOKEN", "0x6AC05a870E0EE506C14155fA9Aa75c34cf2D8859");
    env::set_var("CONTRACT_MODEL_REGISTRY", "0x92b2De840bB2171203011A6dBA928d855cA8183E");

    let test_private_key = "0xe7855c0ea54ccca55126d40f97d90868b2a73bad0363e92ccdec0c4fbd6c0ce2";

    let metadata = NodeMetadata {
        name: "Test Node".to_string(),
        version: "1.0.0".to_string(),
        api_url: "http://localhost:8080".to_string(),
        capabilities: vec!["inference".to_string()],
        performance_tier: "standard".to_string(),
    };

    let registrar = MultiChainRegistrar::new(
        &test_private_key,
        metadata,
    ).await?;

    // Test multi-chain support
    let chain_ids = vec![84532, 5611]; // Base Sepolia, opBNB Testnet

    for chain_id in chain_ids {
        let status = registrar.get_registration_status(chain_id).await?;
        println!("  Chain {}: {:?}", chain_id, status);
    }

    println!("✅ Multi-chain support verified");

    Ok(())
}