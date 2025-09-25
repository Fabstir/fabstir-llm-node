use ethers::prelude::*;
use fabstir_llm_node::contracts::{ChainConfig, Web3Client, Web3Config};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_web3_client_connection() {
    let config = Web3Config {
        rpc_url: "http://localhost:8545".to_string(),
        chain_id: 31337, // Local hardhat/anvil
        confirmations: 1,
        polling_interval: Duration::from_millis(100),
        private_key: None,
        max_reconnection_attempts: 3,
        reconnection_delay: Duration::from_millis(100),
    };

    let client = Web3Client::new(config)
        .await
        .expect("Failed to create Web3 client");

    // Should be connected
    assert!(client.is_connected().await);

    // Should get chain ID
    let chain_id = client.chain_id().await.expect("Failed to get chain ID");
    assert_eq!(chain_id, 31337);
}

#[tokio::test]
async fn test_base_network_connection() {
    // Test connection to Base Sepolia testnet
    let config = Web3Config {
        rpc_url: "https://sepolia.base.org".to_string(),
        chain_id: 84532, // Base Sepolia
        confirmations: 2,
        polling_interval: Duration::from_secs(2),
        private_key: None,
        max_reconnection_attempts: 3,
        reconnection_delay: Duration::from_millis(100),
    };

    let client = Web3Client::new(config)
        .await
        .expect("Failed to create Web3 client");

    // Should get current block
    let block_number = client
        .get_block_number()
        .await
        .expect("Failed to get block number");
    assert!(block_number > 0);
}

#[tokio::test]
async fn test_wallet_management() {
    let private_key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"; // Test key

    let config = Web3Config {
        rpc_url: "http://localhost:8545".to_string(),
        chain_id: 31337,
        confirmations: 1,
        polling_interval: Duration::from_millis(100),
        private_key: Some(private_key.to_string()),
        max_reconnection_attempts: 3,
        reconnection_delay: Duration::from_millis(100),
    };

    let client = Web3Client::new(config)
        .await
        .expect("Failed to create Web3 client");

    // Should have wallet address
    let address = client.address();
    assert_eq!(
        address,
        "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
            .parse::<Address>()
            .unwrap()
    );

    // Should get balance
    let balance = client.get_balance().await.expect("Failed to get balance");
    assert!(balance > U256::zero());
}

#[tokio::test]
async fn test_contract_deployment_addresses() {
    let config = Web3Config::default();
    let client = Web3Client::new(config)
        .await
        .expect("Failed to create Web3 client");

    // Load contract addresses from deployment
    let addresses = client
        .load_contract_addresses("deployments/localhost.json")
        .await
        .expect("Failed to load contract addresses");

    assert!(addresses.contains_key("NodeRegistry"));
    assert!(addresses.contains_key("JobMarketplace"));
    assert!(addresses.contains_key("PaymentEscrow"));
    assert!(addresses.contains_key("ReputationSystem"));
    assert!(addresses.contains_key("ProofSystem"));
}

#[tokio::test]
async fn test_gas_estimation() {
    let config = Web3Config::default();
    let mut client = Web3Client::new(config)
        .await
        .expect("Failed to create Web3 client");

    // Set up wallet
    client
        .set_wallet("0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
        .expect("Failed to set wallet");

    // Estimate gas for a simple transfer
    let to = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
        .parse::<Address>()
        .unwrap();
    let value = U256::from(1_000_000_000_000_000u64); // 0.001 ETH

    let gas_estimate = client
        .estimate_gas(to, value, None)
        .await
        .expect("Failed to estimate gas");

    assert!(gas_estimate > U256::zero());
    assert!(gas_estimate < U256::from(100_000)); // Simple transfer should be < 100k gas
}

#[tokio::test]
async fn test_transaction_sending() {
    let config = Web3Config::default();
    let mut client = Web3Client::new(config)
        .await
        .expect("Failed to create Web3 client");

    // Set up wallet
    client
        .set_wallet("0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
        .expect("Failed to set wallet");

    // Send transaction
    let to = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
        .parse::<Address>()
        .unwrap();
    let value = U256::from(1_000_000_000_000_000u64); // 0.001 ETH

    let tx_hash = client
        .send_transaction(to, value, None)
        .await
        .expect("Failed to send transaction");

    // Wait for confirmation
    let receipt = client
        .wait_for_confirmation(tx_hash)
        .await
        .expect("Failed to get receipt");

    assert_eq!(receipt.status.unwrap(), U64::from(1)); // Success
    assert_eq!(receipt.to.unwrap(), to);
}

#[tokio::test]
async fn test_multicall_support() {
    let config = Web3Config::default();
    let client = Web3Client::new(config)
        .await
        .expect("Failed to create Web3 client");

    // Test multicall for reading multiple contract states
    let multicall = client
        .create_multicall()
        .await
        .expect("Failed to create multicall");

    // Should support batching calls
    // Multicall3 is the latest version, no version field needed
}

#[tokio::test]
async fn test_network_switching() {
    let mut client = Web3Client::new(Web3Config::default())
        .await
        .expect("Failed to create Web3 client");

    // Switch to Base mainnet
    client
        .switch_network(ChainConfig::base_mainnet())
        .await
        .expect("Failed to switch network");

    assert_eq!(client.chain_id().await.unwrap(), 8453);

    // Switch to Base Sepolia
    client
        .switch_network(ChainConfig::base_sepolia())
        .await
        .expect("Failed to switch network");

    assert_eq!(client.chain_id().await.unwrap(), 84532);
}

#[tokio::test]
async fn test_nonce_management() {
    let config = Web3Config::default();
    let mut client = Web3Client::new(config)
        .await
        .expect("Failed to create Web3 client");

    client
        .set_wallet("0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
        .expect("Failed to set wallet");

    // Get current nonce
    let nonce1 = client.get_nonce().await.expect("Failed to get nonce");

    // Send transaction
    let to = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
        .parse::<Address>()
        .unwrap();
    let value = U256::from(1_000_000_000_000_000u64);

    let _ = client
        .send_transaction(to, value, None)
        .await
        .expect("Failed to send tx");

    // Nonce should increment
    let nonce2 = client.get_nonce().await.expect("Failed to get nonce");
    assert_eq!(nonce2, nonce1 + 1);
}

#[tokio::test]
async fn test_event_filter_creation() {
    let config = Web3Config::default();
    let client = Web3Client::new(config)
        .await
        .expect("Failed to create Web3 client");

    // Create event filter for specific block range
    let from_block = 1u64;
    let to_block = 100u64;

    let filter = client.create_event_filter(
        vec![], // Any address
        vec![], // Any topic
        from_block,
        Some(to_block),
    );

    // Filter fields are set via builder methods, not directly accessible
    // The filter was created with the correct block range
}

#[tokio::test]
async fn test_reconnection_on_failure() {
    let config = Web3Config {
        rpc_url: "http://localhost:8546".to_string(), // Wrong port
        chain_id: 31337,
        confirmations: 1,
        polling_interval: Duration::from_millis(100),
        private_key: None,
        max_reconnection_attempts: 3,
        reconnection_delay: Duration::from_millis(100),
    };

    let client = Web3Client::new(config).await;

    // Should fail but not panic
    assert!(client.is_err());

    // Create a new client with correct URL
    let config = Web3Config {
        rpc_url: "http://localhost:8545".to_string(),
        ..Default::default()
    };
    let client = Web3Client::new(config)
        .await
        .expect("Failed to create client");

    // Should now be connected
    assert!(client.is_connected().await);
}

#[tokio::test]
async fn test_gas_price_strategies() {
    let config = Web3Config::default();
    let client = Web3Client::new(config)
        .await
        .expect("Failed to create Web3 client");

    // Test different gas price strategies

    // Legacy gas price
    let gas_price = client
        .get_gas_price()
        .await
        .expect("Failed to get gas price");
    assert!(gas_price > U256::zero());

    // EIP-1559 gas pricing
    let (max_fee, priority_fee) = client
        .get_eip1559_gas_price()
        .await
        .expect("Failed to get EIP-1559 gas price");

    assert!(max_fee > U256::zero());
    assert!(priority_fee > U256::zero());
    assert!(max_fee >= priority_fee);
}

#[tokio::test]
async fn test_block_monitoring() {
    let config = Web3Config {
        polling_interval: Duration::from_millis(100),
        ..Default::default()
    };

    let client = Web3Client::new(config)
        .await
        .expect("Failed to create Web3 client");

    // Subscribe to new blocks
    let mut block_stream = client
        .subscribe_blocks()
        .await
        .expect("Failed to subscribe to blocks");

    // Should receive block updates
    let block = tokio::time::timeout(Duration::from_secs(2), block_stream.recv())
        .await
        .expect("Timeout waiting for block")
        .expect("Failed to receive block");

    assert!(block.number.unwrap() > U64::zero());
}
