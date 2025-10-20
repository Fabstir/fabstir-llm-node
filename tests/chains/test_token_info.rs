// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::config::chains::{ChainConfig, TokenInfo};

#[tokio::test]
async fn test_eth_token_info() {
    let token = TokenInfo {
        symbol: "ETH".to_string(),
        decimals: 18,
    };

    assert_eq!(token.symbol, "ETH");
    assert_eq!(token.decimals, 18);
}

#[tokio::test]
async fn test_bnb_token_info() {
    let token = TokenInfo {
        symbol: "BNB".to_string(),
        decimals: 18,
    };

    assert_eq!(token.symbol, "BNB");
    assert_eq!(token.decimals, 18);
}

#[tokio::test]
async fn test_token_decimals() {
    let eth_token = TokenInfo {
        symbol: "ETH".to_string(),
        decimals: 18,
    };

    let bnb_token = TokenInfo {
        symbol: "BNB".to_string(),
        decimals: 18,
    };

    // Both ETH and BNB use 18 decimals
    assert_eq!(eth_token.decimals, 18);
    assert_eq!(bnb_token.decimals, 18);
}

#[tokio::test]
async fn test_token_info_from_chain_config() {
    let base_config = ChainConfig::base_sepolia();
    assert_eq!(base_config.native_token.symbol, "ETH");
    assert_eq!(base_config.native_token.decimals, 18);

    let opbnb_config = ChainConfig::opbnb_testnet();
    assert_eq!(opbnb_config.native_token.symbol, "BNB");
    assert_eq!(opbnb_config.native_token.decimals, 18);
}

#[tokio::test]
async fn test_token_info_clone() {
    let token = TokenInfo {
        symbol: "ETH".to_string(),
        decimals: 18,
    };

    let cloned = token.clone();
    assert_eq!(token.symbol, cloned.symbol);
    assert_eq!(token.decimals, cloned.decimals);
}

#[tokio::test]
async fn test_token_info_debug() {
    let token = TokenInfo {
        symbol: "ETH".to_string(),
        decimals: 18,
    };

    let debug_str = format!("{:?}", token);
    assert!(debug_str.contains("ETH"));
    assert!(debug_str.contains("18"));
}
