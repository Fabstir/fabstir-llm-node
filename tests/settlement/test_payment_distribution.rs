// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use ethers::types::{Address, U256};
use fabstir_llm_node::config::chains::ChainRegistry;
use fabstir_llm_node::settlement::{
    payment_distribution::{
        ChainPaymentStats, PaymentConfig, PaymentDistributor, PaymentSplit, PaymentToken,
        RefundCalculation,
    },
    types::{SettlementError, SettlementStatus},
};
use std::str::FromStr;
use std::sync::Arc;

// Test helper to create test addresses
fn test_host_address() -> Address {
    Address::from_str("0x1111111111111111111111111111111111111111").unwrap()
}

fn test_user_address() -> Address {
    Address::from_str("0x2222222222222222222222222222222222222222").unwrap()
}

fn test_treasury_address() -> Address {
    Address::from_str("0x3333333333333333333333333333333333333333").unwrap()
}

fn test_usdc_address() -> Address {
    Address::from_str("0x036CbD53842c5426634e7929541eC2318f3dCF7e").unwrap()
}

#[tokio::test]
async fn test_host_earnings_base_sepolia() {
    let registry = Arc::new(ChainRegistry::new());
    let config = PaymentConfig::default();

    let distributor = PaymentDistributor::new(registry.clone(), config);

    // Test payment on Base Sepolia (ETH)
    // Using PRICE_PRECISION format: price_per_token has 1000x multiplier
    let deposit = U256::from(1_000_000_000_000_000_000u64); // 1 ETH
    let tokens_used = 1000;
    let total_tokens = 2000;
    // 0.001 ETH per token WITH PRICE_PRECISION = 1_000_000_000_000_000 * 1000 = 1e18
    let price_per_token = U256::from(1_000_000_000_000_000_000u64);

    let split = distributor
        .calculate_payment_split(
            84532, // Base Sepolia
            deposit,
            tokens_used,
            total_tokens,
            price_per_token,
        )
        .await
        .unwrap();

    // NEW formula: total_payment = (tokens_used * price_per_token) / PRICE_PRECISION
    // = (1000 * 1e18) / 1000 = 1e18 = 1 ETH
    let expected_payment =
        (U256::from(tokens_used) * price_per_token) / U256::from(PRICE_PRECISION);
    let expected_host_earning = expected_payment * 90 / 100;
    let expected_treasury = expected_payment * 10 / 100;

    assert_eq!(split.host_earnings, expected_host_earning);
    assert_eq!(split.treasury_fee, expected_treasury);
    assert_eq!(split.user_refund, deposit - expected_payment);
}

#[tokio::test]
async fn test_host_earnings_opbnb() {
    let registry = Arc::new(ChainRegistry::new());
    let config = PaymentConfig::default();

    let distributor = PaymentDistributor::new(registry.clone(), config);

    // Test payment on opBNB (BNB)
    // Using PRICE_PRECISION format: price_per_token has 1000x multiplier
    let deposit = U256::from(10_000_000_000_000_000_000u64); // 10 BNB
    let tokens_used = 500;
    let total_tokens = 1000;
    // 0.01 BNB per token WITH PRICE_PRECISION = 10_000_000_000_000_000 * 1000 = 1e19
    let price_per_token = U256::from(10_000_000_000_000_000_000u64);

    let split = distributor
        .calculate_payment_split(
            5611, // opBNB Testnet
            deposit,
            tokens_used,
            total_tokens,
            price_per_token,
        )
        .await
        .unwrap();

    // NEW formula: total_payment = (tokens_used * price_per_token) / PRICE_PRECISION
    // = (500 * 1e19) / 1000 = 5e18 = 5 BNB
    let expected_payment =
        (U256::from(tokens_used) * price_per_token) / U256::from(PRICE_PRECISION);
    let expected_host_earning = expected_payment * 90 / 100;
    let expected_treasury = expected_payment * 10 / 100;

    assert_eq!(split.host_earnings, expected_host_earning);
    assert_eq!(split.treasury_fee, expected_treasury);
    assert_eq!(split.user_refund, deposit - expected_payment);

    // Verify chain-specific configuration is used
    assert_eq!(split.chain_id, 5611);
    assert_eq!(split.native_token_symbol, "BNB");
}

#[tokio::test]
async fn test_treasury_accumulation() {
    let registry = Arc::new(ChainRegistry::new());
    let config = PaymentConfig::default();

    let mut distributor = PaymentDistributor::new(registry.clone(), config);

    // Process multiple payments on different chains
    let payments = vec![
        (84532, U256::from(100_000_000_000_000_000u64)), // 0.1 ETH on Base
        (84532, U256::from(200_000_000_000_000_000u64)), // 0.2 ETH on Base
        (5611, U256::from(1_000_000_000_000_000_000u64)), // 1 BNB on opBNB
    ];

    for (chain_id, amount) in payments {
        distributor.accumulate_treasury_fee(chain_id, amount).await;
    }

    // Check accumulated fees
    let base_fees = distributor.get_treasury_balance(84532).await;
    assert_eq!(base_fees, U256::from(300_000_000_000_000_000u64)); // 0.3 ETH total

    let opbnb_fees = distributor.get_treasury_balance(5611).await;
    assert_eq!(opbnb_fees, U256::from(1_000_000_000_000_000_000u64)); // 1 BNB

    // Test withdrawal
    let withdrawn = distributor.withdraw_treasury_fees(84532).await.unwrap();
    assert_eq!(withdrawn, U256::from(300_000_000_000_000_000u64));

    // Balance should be zero after withdrawal
    let remaining = distributor.get_treasury_balance(84532).await;
    assert_eq!(remaining, U256::zero());
}

#[tokio::test]
async fn test_user_refund_calculation() {
    let registry = Arc::new(ChainRegistry::new());
    let config = PaymentConfig::default();

    let distributor = PaymentDistributor::new(registry.clone(), config);

    // Test various refund scenarios
    // Using PRICE_PRECISION format: price_per_token has 1000x multiplier

    // Scenario 1: Partial usage
    let deposit = U256::from(1_000_000_000_000_000_000u64); // 1 ETH
    let tokens_used = 300u64;
    let max_tokens = 1000u64;
    // 0.001 ETH per token WITH PRICE_PRECISION = 1_000_000_000_000_000 * 1000 = 1e18
    let price_per_token = U256::from(1_000_000_000_000_000_000u64);

    let refund = distributor.calculate_refund(deposit, tokens_used, max_tokens, price_per_token);

    // NEW formula: expected_cost = (tokens_used * price_per_token) / PRICE_PRECISION
    // = (300 * 1e18) / 1000 = 0.3 ETH
    let expected_cost = (U256::from(tokens_used) * price_per_token) / U256::from(PRICE_PRECISION);
    assert_eq!(refund.refund_amount, deposit - expected_cost);
    assert_eq!(refund.tokens_unused, max_tokens - tokens_used);

    // Scenario 2: Full usage
    let refund_full = distributor.calculate_refund(
        deposit,
        max_tokens, // All tokens used
        max_tokens,
        price_per_token,
    );

    // = (1000 * 1e18) / 1000 = 1 ETH = full deposit, so no refund
    assert_eq!(refund_full.refund_amount, U256::zero());
    assert_eq!(refund_full.tokens_unused, 0);

    // Scenario 3: No usage
    let refund_none = distributor.calculate_refund(
        deposit,
        0, // No tokens used
        max_tokens,
        price_per_token,
    );

    assert_eq!(refund_none.refund_amount, deposit);
    assert_eq!(refund_none.tokens_unused, max_tokens);
}

#[tokio::test]
async fn test_payment_verification() {
    let registry = Arc::new(ChainRegistry::new());
    let config = PaymentConfig::default();

    let distributor = PaymentDistributor::new(registry.clone(), config);

    // Create test payment data
    let payment_data = PaymentSplit {
        chain_id: 84532,
        host_earnings: U256::from(900_000_000_000_000_000u64), // 0.9 ETH
        treasury_fee: U256::from(100_000_000_000_000_000u64),  // 0.1 ETH
        user_refund: U256::from(0),
        total_payment: U256::from(1_000_000_000_000_000_000u64), // 1 ETH
        native_token_symbol: "ETH".to_string(),
    };

    // Verify payment splits add up correctly
    let verified = distributor.verify_payment_split(&payment_data);
    assert!(verified, "Payment split should be valid");

    // Test invalid split (doesn't add up)
    let invalid_split = PaymentSplit {
        chain_id: 84532,
        host_earnings: U256::from(900_000_000_000_000_000u64),
        treasury_fee: U256::from(200_000_000_000_000_000u64), // Wrong: 0.2 ETH
        user_refund: U256::from(0),
        total_payment: U256::from(1_000_000_000_000_000_000u64),
        native_token_symbol: "ETH".to_string(),
    };

    let invalid_verified = distributor.verify_payment_split(&invalid_split);
    assert!(
        !invalid_verified,
        "Invalid payment split should fail verification"
    );
}

#[tokio::test]
async fn test_different_payment_tokens() {
    let registry = Arc::new(ChainRegistry::new());
    let config = PaymentConfig::default();

    let distributor = PaymentDistributor::new(registry.clone(), config);

    // Test native token payment (ETH)
    let native_payment = distributor
        .process_payment(
            84532,
            PaymentToken::Native,
            U256::from(1_000_000_000_000_000_000u64),
            500,
            1000,
            U256::from(1_000_000_000_000_000u64),
        )
        .await
        .unwrap();

    assert_eq!(native_payment.token_type, PaymentToken::Native);
    assert_eq!(native_payment.token_symbol, "ETH");

    // Test USDC payment
    let usdc_payment = distributor
        .process_payment(
            84532,
            PaymentToken::ERC20(test_usdc_address()),
            U256::from(1_000_000), // 1 USDC (6 decimals)
            500,
            1000,
            U256::from(1_000), // 0.001 USDC per token
        )
        .await
        .unwrap();

    assert!(matches!(usdc_payment.token_type, PaymentToken::ERC20(_)));
    assert_eq!(usdc_payment.token_symbol, "USDC");
}

#[tokio::test]
async fn test_chain_payment_statistics() {
    let registry = Arc::new(ChainRegistry::new());
    let config = PaymentConfig::default();

    let mut distributor = PaymentDistributor::new(registry.clone(), config);

    // Process several payments
    for i in 0..5 {
        distributor
            .record_payment(
                84532,
                U256::from((i + 1) * 100_000_000_000_000_000u64),
                U256::from(i * 10_000_000_000_000_000u64),
            )
            .await;
    }

    // Get statistics
    let stats = distributor.get_chain_statistics(84532).await;

    assert_eq!(stats.total_payments, 5);
    assert_eq!(stats.total_volume, U256::from(1_500_000_000_000_000_000u64)); // 1.5 ETH
    assert_eq!(stats.total_fees, U256::from(100_000_000_000_000_000u64)); // 0.1 ETH
    assert_eq!(stats.chain_id, 84532);
}

#[tokio::test]
async fn test_multi_chain_payment_tracking() {
    let registry = Arc::new(ChainRegistry::new());
    let config = PaymentConfig::default();

    let mut distributor = PaymentDistributor::new(registry.clone(), config);

    // Track payments across multiple chains
    distributor
        .record_payment(
            84532,
            U256::from(1_000_000_000_000_000_000u64),
            U256::from(100_000_000_000_000_000u64),
        )
        .await;
    distributor
        .record_payment(
            5611,
            U256::from(5_000_000_000_000_000_000u64),
            U256::from(500_000_000_000_000_000u64),
        )
        .await;

    // Get all chain stats
    let all_stats = distributor.get_all_chain_statistics().await;

    assert_eq!(all_stats.len(), 2);
    assert!(all_stats.iter().any(|s| s.chain_id == 84532));
    assert!(all_stats.iter().any(|s| s.chain_id == 5611));
}

// ============================================
// PRICE_PRECISION Tests (Sub-phase 2.1)
// Updated December 2025 for PRICE_PRECISION=1000
// ============================================

use fabstir_llm_node::contracts::pricing_constants::PRICE_PRECISION;

/// Test payment calculation with PRICE_PRECISION division
/// Formula: total_payment = (tokens_used * price_per_token) / PRICE_PRECISION
#[tokio::test]
async fn test_payment_split_with_price_precision() {
    let registry = Arc::new(ChainRegistry::new());
    let config = PaymentConfig::default();
    let distributor = PaymentDistributor::new(registry.clone(), config);

    // Test with PRICE_PRECISION pricing: $5/million tokens = 5000 with PRICE_PRECISION
    // Using USDC (6 decimals): deposit = 10_000_000 = $10
    let deposit = U256::from(10_000_000u64); // 10 USDC (6 decimals)
    let tokens_used = 1_000_000u64; // 1 million tokens
    let total_tokens = 2_000_000u64; // 2 million tokens max
    let price_per_token = U256::from(5000u64); // $5/million with PRICE_PRECISION

    let split = distributor
        .calculate_payment_split(84532, deposit, tokens_used, total_tokens, price_per_token)
        .await
        .unwrap();

    // NEW formula: total_payment = (tokens_used * price_per_token) / PRICE_PRECISION
    // = (1_000_000 * 5000) / 1000 = 5_000_000 USDC units = $5
    let expected_total = U256::from(5_000_000u64);
    let expected_host = expected_total * 90 / 100; // $4.50
    let expected_treasury = expected_total * 10 / 100; // $0.50
    let expected_refund = deposit - expected_total; // $5

    assert_eq!(
        split.total_payment, expected_total,
        "Total payment should be $5 (5_000_000 units)"
    );
    assert_eq!(
        split.host_earnings, expected_host,
        "Host earnings should be 90% = $4.50"
    );
    assert_eq!(
        split.treasury_fee, expected_treasury,
        "Treasury fee should be 10% = $0.50"
    );
    assert_eq!(
        split.user_refund, expected_refund,
        "User refund should be $5"
    );
}

/// Test refund calculation with PRICE_PRECISION division
/// Formula: amount_spent = (tokens_used * price_per_token) / PRICE_PRECISION
#[tokio::test]
async fn test_refund_with_price_precision() {
    let registry = Arc::new(ChainRegistry::new());
    let config = PaymentConfig::default();
    let distributor = PaymentDistributor::new(registry.clone(), config);

    // Test with sub-dollar pricing: $0.06/million tokens (budget model)
    // With PRICE_PRECISION: 0.06 * 1000 = 60
    let deposit = U256::from(1_000_000u64); // $1 USDC
    let tokens_used = 1_000_000u64; // 1 million tokens used
    let max_tokens = 10_000_000u64; // 10 million tokens max
    let price_per_token = U256::from(60u64); // $0.06/million with PRICE_PRECISION

    let refund = distributor.calculate_refund(deposit, tokens_used, max_tokens, price_per_token);

    // NEW formula: amount_spent = (tokens_used * price_per_token) / PRICE_PRECISION
    // = (1_000_000 * 60) / 1000 = 60_000 USDC units = $0.06
    let expected_spent = U256::from(60_000u64);
    let expected_refund = deposit - expected_spent; // $0.94

    assert_eq!(
        refund.amount_spent, expected_spent,
        "Amount spent should be $0.06 (60_000 units)"
    );
    assert_eq!(
        refund.refund_amount, expected_refund,
        "Refund should be $0.94 (940_000 units)"
    );
    assert_eq!(
        refund.tokens_unused,
        max_tokens - tokens_used,
        "Should have 9 million unused tokens"
    );
}

/// Test edge case: very small token amounts
#[tokio::test]
async fn test_price_precision_small_amounts() {
    let registry = Arc::new(ChainRegistry::new());
    let config = PaymentConfig::default();
    let distributor = PaymentDistributor::new(registry.clone(), config);

    // Very small usage: 100 tokens at minimum price
    let deposit = U256::from(1_000_000u64); // $1 USDC
    let tokens_used = 100u64;
    let max_tokens = 1_000_000u64;
    let price_per_token = U256::from(1u64); // MIN price with PRICE_PRECISION

    let refund = distributor.calculate_refund(deposit, tokens_used, max_tokens, price_per_token);

    // amount_spent = (100 * 1) / 1000 = 0 (rounds down)
    // This is expected behavior for very small amounts
    let expected_spent = U256::from(0u64);
    assert_eq!(
        refund.amount_spent, expected_spent,
        "Very small amounts should round to 0"
    );
    assert_eq!(
        refund.refund_amount, deposit,
        "Full refund when amount rounds to 0"
    );
}

/// Test edge case: large deposits and token amounts
#[tokio::test]
async fn test_price_precision_large_amounts() {
    let registry = Arc::new(ChainRegistry::new());
    let config = PaymentConfig::default();
    let distributor = PaymentDistributor::new(registry.clone(), config);

    // Large usage: 1 billion tokens at high price
    let deposit = U256::from(100_000_000_000u64); // $100,000 USDC
    let tokens_used = 1_000_000_000u64; // 1 billion tokens
    let max_tokens = 1_000_000_000u64;
    let price_per_token = U256::from(100_000u64); // $100/million with PRICE_PRECISION

    let refund = distributor.calculate_refund(deposit, tokens_used, max_tokens, price_per_token);

    // amount_spent = (1_000_000_000 * 100_000) / 1000 = 100_000_000_000
    // = $100,000 USDC
    let expected_spent = U256::from(100_000_000_000u64);
    assert_eq!(
        refund.amount_spent, expected_spent,
        "Large amount calculation should be correct"
    );
    assert_eq!(
        refund.refund_amount,
        U256::zero(),
        "No refund when full deposit used"
    );
}

/// Test precision loss handling (rounding behavior)
#[tokio::test]
async fn test_price_precision_rounding() {
    let registry = Arc::new(ChainRegistry::new());
    let config = PaymentConfig::default();
    let distributor = PaymentDistributor::new(registry.clone(), config);

    // Test that division rounds down (standard Solidity/Rust behavior)
    // 999 tokens at price 1 = (999 * 1) / 1000 = 0 (rounds down)
    let deposit = U256::from(1_000_000u64);
    let refund1 = distributor.calculate_refund(deposit, 999, 1_000_000, U256::from(1u64));
    assert_eq!(
        refund1.amount_spent,
        U256::from(0u64),
        "999/1000 should round to 0"
    );

    // 1000 tokens at price 1 = (1000 * 1) / 1000 = 1
    let refund2 = distributor.calculate_refund(deposit, 1000, 1_000_000, U256::from(1u64));
    assert_eq!(
        refund2.amount_spent,
        U256::from(1u64),
        "1000/1000 should equal 1"
    );

    // 1500 tokens at price 1 = (1500 * 1) / 1000 = 1 (rounds down)
    let refund3 = distributor.calculate_refund(deposit, 1500, 1_000_000, U256::from(1u64));
    assert_eq!(
        refund3.amount_spent,
        U256::from(1u64),
        "1500/1000 should round to 1"
    );
}

/// Test native token (ETH) pricing with PRICE_PRECISION
#[tokio::test]
async fn test_price_precision_native_token() {
    let registry = Arc::new(ChainRegistry::new());
    let config = PaymentConfig::default();
    let distributor = PaymentDistributor::new(registry.clone(), config);

    // Native token (ETH) with PRICE_PRECISION
    // Default native price: ~2,272,727,273 wei/million tokens
    let deposit = U256::from(1_000_000_000_000_000_000u64); // 1 ETH
    let tokens_used = 100_000u64; // 100k tokens
    let max_tokens = 1_000_000u64;
    let price_per_token = U256::from(2_272_727_273u64); // Default native price

    let refund = distributor.calculate_refund(deposit, tokens_used, max_tokens, price_per_token);

    // amount_spent = (100_000 * 2_272_727_273) / 1000 = 227,272,727,300 wei
    // ≈ 0.0002272 ETH
    let expected_spent = U256::from(227_272_727_300u64);
    assert_eq!(
        refund.amount_spent, expected_spent,
        "Native token calculation should include PRICE_PRECISION"
    );
}
