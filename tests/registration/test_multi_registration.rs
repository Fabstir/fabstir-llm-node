// Tests for multi-chain node registration system
// Following requirements from HOST_REGISTRATION_GUIDE.md

use anyhow::Result;
use ethers::prelude::*;
use std::sync::Arc;
use std::str::FromStr;

use fabstir_llm_node::blockchain::multi_chain_registrar::{MultiChainRegistrar, RegistrationStatus, NodeMetadata};
use fabstir_llm_node::config::chains::ChainRegistry;

// Helper function to setup test environment
async fn setup_test_registrar(use_host_2: bool) -> Result<MultiChainRegistrar> {
    // Use test accounts from .env.local.test
    let host_private_key = if use_host_2 {
        // Host 2 account
        std::env::var("TEST_HOST_2_PRIVATE_KEY")
            .unwrap_or_else(|_| "0x9ac736a402fa7163b3a30c31b379aa2e3979eb9a3a2b01890485c334a6da575b".to_string())
    } else {
        // Host 1 account
        std::env::var("TEST_HOST_1_PRIVATE_KEY")
            .unwrap_or_else(|_| "0xe7855c0ea54ccca55126d40f97d90868b2a73bad0363e92ccdec0c4fbd6c0ce2".to_string())
    };

    let chain_registry = Arc::new(ChainRegistry::new());

    let metadata = NodeMetadata {
        name: format!("Test Host {}", if use_host_2 { 2 } else { 1 }),
        version: "1.0.0".to_string(),
        api_url: format!("http://test-host-{}.example.com:8080", if use_host_2 { 2 } else { 1 }),
        capabilities: vec!["inference".to_string(), "streaming".to_string()],
        performance_tier: "standard".to_string(),
    };

    MultiChainRegistrar::new(
        chain_registry,
        &host_private_key,
        metadata,
    ).await
}

// Test checking if node is already registered
#[tokio::test]
#[ignore] // Remove #[ignore] to run against real Base Sepolia
async fn test_check_registration_status() -> Result<()> {
    println!("🔍 Checking registration status on Base Sepolia...");

    let registrar = setup_test_registrar(false).await?;

    // Check if already registered
    let chain_id = 84532; // Base Sepolia
    let is_registered = registrar.verify_registration_on_chain(chain_id).await?;

    println!("Registration status: {}", if is_registered { "✅ Registered" } else { "❌ Not registered" });

    // Get detailed status
    let status = registrar.get_registration_status(chain_id).await?;
    match status {
        RegistrationStatus::NotRegistered => println!("Status: Not registered"),
        RegistrationStatus::Pending { tx_hash } => println!("Status: Pending (tx: {:?})", tx_hash),
        RegistrationStatus::Confirmed { block_number } => println!("Status: Confirmed at block {}", block_number),
        RegistrationStatus::Failed { error } => println!("Status: Failed - {}", error),
    }

    Ok(())
}

// Test registration on Base Sepolia with real FAB tokens
#[tokio::test]
#[ignore] // Remove #[ignore] to run against real Base Sepolia
async fn test_register_on_base_sepolia() -> Result<()> {
    println!("🚀 Starting registration test on Base Sepolia...");
    println!("📋 Requirements:");
    println!("  - 1000 FAB tokens for staking");
    println!("  - ETH for gas fees");
    println!("  - Connection to Base Sepolia RPC");

    // Setup
    let registrar = setup_test_registrar(false).await?;

    let chain_id = 84532; // Base Sepolia

    // First check if already registered
    let is_already_registered = registrar.verify_registration_on_chain(chain_id).await?;

    if is_already_registered {
        println!("⚠️  Node is already registered on Base Sepolia");
        println!("    To test registration, you would need to:");
        println!("    1. Unregister the node first");
        println!("    2. Wait for transaction confirmation");
        println!("    3. Run this test again");
        return Ok(());
    }

    println!("📝 Attempting to register node on Base Sepolia...");

    // Attempt registration
    match registrar.register_on_chain(chain_id).await {
        Ok(tx_hash) => {
            println!("✅ Registration transaction submitted!");
            println!("   Transaction hash: {:?}", tx_hash);
            println!("   View on BaseScan: https://sepolia.basescan.org/tx/{:?}", tx_hash);

            // Wait a bit for confirmation
            println!("⏳ Waiting 30 seconds for confirmation...");
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

            // Check status
            let status = registrar.get_registration_status(chain_id).await?;
            match status {
                RegistrationStatus::Confirmed { block_number } => {
                    println!("✅ Registration confirmed at block {}", block_number);
                },
                RegistrationStatus::Pending { .. } => {
                    println!("⏳ Registration still pending, check back later");
                },
                RegistrationStatus::Failed { ref error } => {
                    println!("❌ Registration failed: {}", error);
                },
                _ => {}
            }

            // Verify on-chain
            let is_registered = registrar.verify_registration_on_chain(chain_id).await?;
            assert!(is_registered || matches!(status, RegistrationStatus::Pending { .. }),
                    "Node should be registered or pending after successful transaction");
        },
        Err(e) => {
            println!("❌ Registration failed: {}", e);

            // Common error causes:
            if e.to_string().contains("Insufficient FAB") {
                println!("   💡 Need 1000 FAB tokens. Check balance at:");
                println!("      https://sepolia.basescan.org/token/0xC78949004B4EB6dEf2D66e49Cd81231472612D62");
            } else if e.to_string().contains("already registered") {
                println!("   💡 Node is already registered. Unregister first to test again.");
            } else if e.to_string().contains("gas") {
                println!("   💡 Insufficient ETH for gas. Add ETH to the test account.");
            }

            return Err(e);
        }
    }

    Ok(())
}

// Test verification of registration on-chain
#[tokio::test]
#[ignore] // Remove #[ignore] to run against real Base Sepolia
async fn test_registration_verification() -> Result<()> {
    println!("🔍 Testing registration verification on Base Sepolia...");

    let registrar = setup_test_registrar(false).await?;
    let chain_id = 84532;

    // Check registration
    let is_registered = registrar.verify_registration_on_chain(chain_id).await?;

    if is_registered {
        println!("✅ Node is registered and active on Base Sepolia");

        // Could also check contract directly for more details
        // like staked amount, metadata, API URL, etc.
    } else {
        println!("❌ Node is not registered on Base Sepolia");
    }

    Ok(())
}

// Test registration status tracking
#[tokio::test]
async fn test_registration_status() -> Result<()> {
    println!("📊 Testing registration status tracking...");

    let registrar = setup_test_registrar(false).await?;

    // Check initial status
    let status = registrar.get_registration_status(84532).await?;
    match status {
        RegistrationStatus::NotRegistered => println!("Initial status: NotRegistered ✅"),
        _ => println!("Initial status: {:?}", status),
    }

    // Get all chain statuses
    let all_status = registrar.get_all_registration_status().await?;
    println!("Status across all chains:");
    for (chain_id, status) in all_status {
        let chain_name = match chain_id {
            84532 => "Base Sepolia",
            5611 => "opBNB Testnet",
            _ => "Unknown",
        };
        println!("  {}: {:?}", chain_name, status);
    }

    Ok(())
}

// Test with Host 2 account
#[tokio::test]
#[ignore] // Remove #[ignore] to run against real Base Sepolia
async fn test_register_host_2() -> Result<()> {
    println!("🚀 Testing registration with Host 2 account...");

    let registrar = setup_test_registrar(true).await?;
    let chain_id = 84532;

    // Check if Host 2 is registered
    let is_registered = registrar.verify_registration_on_chain(chain_id).await?;

    println!("Host 2 registration status: {}",
             if is_registered { "✅ Registered" } else { "❌ Not registered" });

    if !is_registered {
        println!("📝 Host 2 could be registered by running:");
        println!("   cargo test test_register_host_2 -- --ignored --nocapture");
    }

    Ok(())
}

// Test concurrent registration (when opBNB is available)
#[tokio::test]
async fn test_concurrent_registration() -> Result<()> {
    // Skip since opBNB contracts not deployed yet
    if std::env::var("OPBNB_NODE_REGISTRY").is_err() {
        println!("⏭️  Skipping concurrent test - opBNB contracts not deployed");
        return Ok(());
    }

    // Will be implemented when opBNB contracts are available
    println!("Test skipped - requires both chains to be available");
    Ok(())
}

// Test registration on opBNB testnet (when contracts are deployed)
#[tokio::test]
async fn test_register_on_opbnb() -> Result<()> {
    // Skip if opBNB contracts not deployed yet
    if std::env::var("OPBNB_NODE_REGISTRY").is_err() {
        println!("⏭️  Skipping opBNB test - contracts not deployed");
        return Ok(());
    }

    // Will be implemented when opBNB contracts are deployed
    println!("Test skipped - opBNB contracts not yet deployed");
    Ok(())
}