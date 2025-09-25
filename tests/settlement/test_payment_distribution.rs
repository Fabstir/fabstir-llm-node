use fabstir_llm_node::settlement::{
    payment_distribution::{
        PaymentDistributor, PaymentConfig, PaymentSplit,
        ChainPaymentStats, PaymentToken, RefundCalculation,
    },
    types::{SettlementError, SettlementStatus},
};
use fabstir_llm_node::config::chains::ChainRegistry;
use ethers::types::{U256, Address};
use std::sync::Arc;
use std::str::FromStr;

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
    let deposit = U256::from(1_000_000_000_000_000_000u64); // 1 ETH
    let tokens_used = 1000;
    let total_tokens = 2000;
    let price_per_token = U256::from(1_000_000_000_000_000u64); // 0.001 ETH per token

    let split = distributor.calculate_payment_split(
        84532, // Base Sepolia
        deposit,
        tokens_used,
        total_tokens,
        price_per_token,
    ).await.unwrap();

    // Host should get 90% of payment (configurable via env)
    let expected_payment = price_per_token * tokens_used;
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
    let deposit = U256::from(10_000_000_000_000_000_000u64); // 10 BNB
    let tokens_used = 500;
    let total_tokens = 1000;
    let price_per_token = U256::from(10_000_000_000_000_000u64); // 0.01 BNB per token

    let split = distributor.calculate_payment_split(
        5611, // opBNB Testnet
        deposit,
        tokens_used,
        total_tokens,
        price_per_token,
    ).await.unwrap();

    // Verify payment calculation
    let expected_payment = price_per_token * tokens_used;
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

    // Scenario 1: Partial usage
    let deposit = U256::from(1_000_000_000_000_000_000u64); // 1 ETH
    let tokens_used = 300;
    let max_tokens = 1000;
    let price_per_token = U256::from(1_000_000_000_000_000u64); // 0.001 ETH

    let refund = distributor.calculate_refund(
        deposit,
        tokens_used,
        max_tokens,
        price_per_token,
    );

    let expected_cost = price_per_token * tokens_used;
    assert_eq!(refund.refund_amount, deposit - expected_cost);
    assert_eq!(refund.tokens_unused, max_tokens - tokens_used);

    // Scenario 2: Full usage
    let refund_full = distributor.calculate_refund(
        deposit,
        max_tokens, // All tokens used
        max_tokens,
        price_per_token,
    );

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
        treasury_fee: U256::from(100_000_000_000_000_000u64),   // 0.1 ETH
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
    assert!(!invalid_verified, "Invalid payment split should fail verification");
}

#[tokio::test]
async fn test_different_payment_tokens() {
    let registry = Arc::new(ChainRegistry::new());
    let config = PaymentConfig::default();

    let distributor = PaymentDistributor::new(registry.clone(), config);

    // Test native token payment (ETH)
    let native_payment = distributor.process_payment(
        84532,
        PaymentToken::Native,
        U256::from(1_000_000_000_000_000_000u64),
        500,
        1000,
        U256::from(1_000_000_000_000_000u64),
    ).await.unwrap();

    assert_eq!(native_payment.token_type, PaymentToken::Native);
    assert_eq!(native_payment.token_symbol, "ETH");

    // Test USDC payment
    let usdc_payment = distributor.process_payment(
        84532,
        PaymentToken::ERC20(test_usdc_address()),
        U256::from(1_000_000), // 1 USDC (6 decimals)
        500,
        1000,
        U256::from(1_000), // 0.001 USDC per token
    ).await.unwrap();

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
        distributor.record_payment(
            84532,
            U256::from((i + 1) * 100_000_000_000_000_000u64),
            U256::from(i * 10_000_000_000_000_000u64),
        ).await;
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
    distributor.record_payment(84532, U256::from(1_000_000_000_000_000_000u64), U256::from(100_000_000_000_000_000u64)).await;
    distributor.record_payment(5611, U256::from(5_000_000_000_000_000_000u64), U256::from(500_000_000_000_000_000u64)).await;

    // Get all chain stats
    let all_stats = distributor.get_all_chain_statistics().await;

    assert_eq!(all_stats.len(), 2);
    assert!(all_stats.iter().any(|s| s.chain_id == 84532));
    assert!(all_stats.iter().any(|s| s.chain_id == 5611));
}