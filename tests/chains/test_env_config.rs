use fabstir_llm_node::config::chains::{ChainConfig, ChainConfigLoader, ChainRegistry};
use std::env;
use ethers::types::Address;
use std::str::FromStr;

#[tokio::test]
async fn test_load_base_sepolia_from_env() {
    // Set environment variables
    env::set_var("BASE_SEPOLIA_RPC_URL", "https://custom.base.sepolia.rpc");
    env::set_var("BASE_SEPOLIA_CHAIN_ID", "84532");
    env::set_var("BASE_SEPOLIA_CONFIRMATIONS", "5");

    let loader = ChainConfigLoader::new();
    let config = loader.load_base_sepolia().await.unwrap();

    assert_eq!(config.chain_id, 84532);
    assert_eq!(config.rpc_url, "https://custom.base.sepolia.rpc");
    assert_eq!(config.confirmation_blocks, 5);
    assert_eq!(config.native_token.symbol, "ETH");

    // Cleanup
    env::remove_var("BASE_SEPOLIA_RPC_URL");
    env::remove_var("BASE_SEPOLIA_CHAIN_ID");
    env::remove_var("BASE_SEPOLIA_CONFIRMATIONS");
}

#[tokio::test]
async fn test_load_opbnb_from_env() {
    // Set environment variables
    env::set_var("OPBNB_TESTNET_RPC_URL", "https://custom.opbnb.rpc");
    env::set_var("OPBNB_TESTNET_CHAIN_ID", "5611");
    env::set_var("OPBNB_TESTNET_CONFIRMATIONS", "20");
    env::set_var("OPBNB_JOB_MARKETPLACE", "0x1234567890123456789012345678901234567890");
    env::set_var("OPBNB_NODE_REGISTRY", "0x2345678901234567890123456789012345678901");

    let loader = ChainConfigLoader::new();
    let config = loader.load_opbnb_testnet().await.unwrap();

    assert_eq!(config.chain_id, 5611);
    assert_eq!(config.rpc_url, "https://custom.opbnb.rpc");
    assert_eq!(config.confirmation_blocks, 20);
    assert_eq!(config.native_token.symbol, "BNB");
    assert_eq!(
        config.contracts.job_marketplace,
        Address::from_str("0x1234567890123456789012345678901234567890").unwrap()
    );
    assert_eq!(
        config.contracts.node_registry,
        Address::from_str("0x2345678901234567890123456789012345678901").unwrap()
    );

    // Cleanup
    env::remove_var("OPBNB_TESTNET_RPC_URL");
    env::remove_var("OPBNB_TESTNET_CHAIN_ID");
    env::remove_var("OPBNB_TESTNET_CONFIRMATIONS");
    env::remove_var("OPBNB_JOB_MARKETPLACE");
    env::remove_var("OPBNB_NODE_REGISTRY");
}

#[tokio::test]
async fn test_rpc_url_validation() {
    let loader = ChainConfigLoader::new();

    // Valid URLs
    assert!(loader.validate_rpc_url("https://sepolia.base.org").is_ok());
    assert!(loader.validate_rpc_url("http://localhost:8545").is_ok());
    assert!(loader.validate_rpc_url("wss://sepolia.base.org").is_ok());

    // Invalid URLs
    assert!(loader.validate_rpc_url("not-a-url").is_err());
    assert!(loader.validate_rpc_url("").is_err());
    assert!(loader.validate_rpc_url("ftp://invalid.protocol").is_err());
}

#[tokio::test]
async fn test_contract_override() {
    // Set override for specific contract
    env::set_var("BASE_SEPOLIA_JOB_MARKETPLACE", "0xABCDEF0123456789012345678901234567890123");

    let loader = ChainConfigLoader::new();
    let config = loader.load_base_sepolia().await.unwrap();

    assert_eq!(
        config.contracts.job_marketplace,
        Address::from_str("0xABCDEF0123456789012345678901234567890123").unwrap()
    );

    // Other contracts should use defaults
    assert_ne!(config.contracts.node_registry, Address::zero());
    assert_ne!(config.contracts.payment_escrow, Address::zero());

    // Cleanup
    env::remove_var("BASE_SEPOLIA_JOB_MARKETPLACE");
}

#[tokio::test]
async fn test_missing_env_fallback() {
    // Clear any existing env vars
    env::remove_var("BASE_SEPOLIA_RPC_URL");
    env::remove_var("OPBNB_TESTNET_RPC_URL");

    let loader = ChainConfigLoader::new();

    // Should fall back to defaults
    let base_config = loader.load_base_sepolia().await.unwrap();
    assert_eq!(base_config.rpc_url, "https://sepolia.base.org");
    assert_eq!(base_config.chain_id, 84532);

    let opbnb_config = loader.load_opbnb_testnet().await.unwrap();
    assert_eq!(opbnb_config.rpc_url, "https://opbnb-testnet-rpc.bnbchain.org");
    assert_eq!(opbnb_config.chain_id, 5611);
}

#[tokio::test]
async fn test_load_from_file() {
    // Create a temporary config file
    let config_content = r#"
[base_sepolia]
chain_id = 84532
rpc_url = "https://file.base.sepolia.rpc"
confirmations = 3

[opbnb_testnet]
chain_id = 5611
rpc_url = "https://file.opbnb.rpc"
confirmations = 15
"#;

    let temp_file = "/tmp/test_chains_config.toml";
    std::fs::write(temp_file, config_content).unwrap();

    let loader = ChainConfigLoader::from_file(temp_file).unwrap();
    let registry = loader.build_registry().await.unwrap();

    let base_config = registry.get_chain(84532).unwrap();
    assert_eq!(base_config.rpc_url, "https://file.base.sepolia.rpc");

    let opbnb_config = registry.get_chain(5611).unwrap();
    assert_eq!(opbnb_config.rpc_url, "https://file.opbnb.rpc");

    // Cleanup
    std::fs::remove_file(temp_file).ok();
}

#[tokio::test]
async fn test_invalid_address_handling() {
    env::set_var("OPBNB_JOB_MARKETPLACE", "invalid-address");

    let loader = ChainConfigLoader::new();
    let config = loader.load_opbnb_testnet().await.unwrap();

    // Should fall back to zero address on invalid input
    assert_eq!(config.contracts.job_marketplace, Address::zero());

    // Cleanup
    env::remove_var("OPBNB_JOB_MARKETPLACE");
}

#[tokio::test]
async fn test_chain_specific_gas_multiplier() {
    env::set_var("BASE_GAS_MULTIPLIER", "1.2");
    env::set_var("OPBNB_GAS_MULTIPLIER", "1.5");

    let loader = ChainConfigLoader::new();

    let base_config = loader.load_base_sepolia().await.unwrap();
    assert_eq!(base_config.gas_multiplier, Some(1.2));

    let opbnb_config = loader.load_opbnb_testnet().await.unwrap();
    assert_eq!(opbnb_config.gas_multiplier, Some(1.5));

    // Cleanup
    env::remove_var("BASE_GAS_MULTIPLIER");
    env::remove_var("OPBNB_GAS_MULTIPLIER");
}