// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use ethers::types::Address;
use fabstir_llm_node::config::chains::{ChainConfig, ChainRegistry, ContractAddresses, TokenInfo};

#[tokio::test]
async fn test_base_sepolia_config() {
    let config = ChainConfig::base_sepolia();

    assert_eq!(config.chain_id, 84532);
    assert_eq!(config.name, "Base Sepolia");
    assert!(config.rpc_url.contains("sepolia") || config.rpc_url.contains("base"));
    assert_eq!(config.native_token.symbol, "ETH");
    assert_eq!(config.native_token.decimals, 18);
    assert_eq!(config.confirmation_blocks, 3);

    // Verify contract addresses are valid
    assert_ne!(config.contracts.job_marketplace, Address::zero());
    assert_ne!(config.contracts.node_registry, Address::zero());
    assert_ne!(config.contracts.payment_escrow, Address::zero());
    assert_ne!(config.contracts.host_earnings, Address::zero());
}

#[tokio::test]
async fn test_opbnb_config() {
    let config = ChainConfig::opbnb_testnet();

    assert_eq!(config.chain_id, 5611);
    assert_eq!(config.name, "opBNB Testnet");
    assert!(config.rpc_url.contains("opbnb") || config.rpc_url.contains("bnbchain"));
    assert_eq!(config.native_token.symbol, "BNB");
    assert_eq!(config.native_token.decimals, 18);
    assert_eq!(config.confirmation_blocks, 15); // BNB chains need more confirmations

    // Contract addresses may be zero initially (to be deployed)
    // But the config should still be valid
}

#[tokio::test]
async fn test_chain_registry_initialization() {
    let registry = ChainRegistry::new();

    // Should have both chains registered
    assert!(registry.get_chain(84532).is_some());
    assert!(registry.get_chain(5611).is_some());

    // Default chain should be Base Sepolia
    assert_eq!(registry.default_chain(), 84532);
}

#[tokio::test]
async fn test_get_chain_by_id() {
    let registry = ChainRegistry::new();

    // Test Base Sepolia
    let base_config = registry.get_chain(84532);
    assert!(base_config.is_some());
    assert_eq!(base_config.unwrap().name, "Base Sepolia");

    // Test opBNB
    let opbnb_config = registry.get_chain(5611);
    assert!(opbnb_config.is_some());
    assert_eq!(opbnb_config.unwrap().name, "opBNB Testnet");
}

#[tokio::test]
async fn test_invalid_chain_id() {
    let registry = ChainRegistry::new();

    // Test with invalid chain ID
    let invalid_config = registry.get_chain(99999);
    assert!(invalid_config.is_none());

    // Test with mainnet chain IDs (not supported yet)
    let mainnet_config = registry.get_chain(1);
    assert!(mainnet_config.is_none());
}

#[tokio::test]
async fn test_chain_config_clone() {
    let config = ChainConfig::base_sepolia();
    let cloned = config.clone();

    assert_eq!(config.chain_id, cloned.chain_id);
    assert_eq!(config.name, cloned.name);
    assert_eq!(config.rpc_url, cloned.rpc_url);
}

#[tokio::test]
async fn test_supported_chains_list() {
    let registry = ChainRegistry::new();
    let supported_chains = registry.list_supported_chains();

    assert_eq!(supported_chains.len(), 2);
    assert!(supported_chains.contains(&84532));
    assert!(supported_chains.contains(&5611));
}

#[tokio::test]
async fn test_is_chain_supported() {
    let registry = ChainRegistry::new();

    assert!(registry.is_chain_supported(84532));
    assert!(registry.is_chain_supported(5611));
    assert!(!registry.is_chain_supported(99999));
}
