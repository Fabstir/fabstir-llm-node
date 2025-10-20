// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::contracts::{Web3Client, Web3Config};

#[tokio::test]
async fn test_web3_client_creation_basic() {
    let config = Web3Config::default();
    
    // Just test that we can create a client
    let result = Web3Client::new(config).await;
    
    // Should fail since we don't have a real RPC endpoint
    assert!(result.is_err());
}