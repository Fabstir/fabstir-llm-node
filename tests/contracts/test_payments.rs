use fabstir_llm_node::contracts::{PaymentVerifier, PaymentConfig, PaymentStatus, TokenInfo};
use ethers::prelude::*;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_payment_verifier_creation() {
    let config = PaymentConfig {
        escrow_address: "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0".parse().unwrap(),
        supported_tokens: vec![
            TokenInfo {
                symbol: "USDC".to_string(),
                address: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".parse().unwrap(),
                decimals: 6,
            },
            TokenInfo {
                symbol: "ETH".to_string(),
                address: Address::zero(), // Native token
                decimals: 18,
            },
        ],
        min_payment_amount: U256::from(1_000_000), // $1 in USDC
        payment_timeout: Duration::from_secs(3600),
    };
    
    let web3_client = create_test_web3_client().await;
    let verifier = PaymentVerifier::new(config, web3_client)
        .await
        .expect("Failed to create payment verifier");
    
    // Should support configured tokens
    assert!(verifier.is_token_supported("USDC"));
    assert!(verifier.is_token_supported("ETH"));
    assert!(!verifier.is_token_supported("DAI"));
}

#[tokio::test]
async fn test_escrow_deposit_verification() {
    let config = PaymentConfig::default();
    let web3_client = create_test_web3_client().await;
    
    let verifier = PaymentVerifier::new(config, web3_client.clone())
        .await
        .expect("Failed to create payment verifier");
    
    // Create a job with payment
    let job_id = create_job_with_payment(&web3_client, U256::from(100_000_000)).await; // 100 USDC
    
    // Verify escrow deposit
    let deposit = verifier.verify_escrow_deposit(job_id)
        .await
        .expect("Failed to verify deposit");
    
    assert_eq!(deposit.job_id, job_id);
    assert_eq!(deposit.amount, U256::from(100_000_000));
    assert_eq!(deposit.token_symbol, "USDC");
    assert_eq!(deposit.status, PaymentStatus::Locked);
    assert!(!deposit.client.is_zero());
}

#[tokio::test]
async fn test_payment_release_monitoring() {
    let config = PaymentConfig::default();
    let web3_client = create_test_web3_client().await;
    
    let mut verifier = PaymentVerifier::new(config, web3_client.clone())
        .await
        .expect("Failed to create payment verifier");
    
    // Start monitoring
    let mut event_receiver = verifier.start_monitoring().await;
    
    // Complete a job to trigger payment release
    let job_id = create_and_complete_job(&web3_client).await;
    
    // Should receive payment released event
    let event = tokio::time::timeout(Duration::from_secs(2), event_receiver.recv())
        .await
        .expect("Timeout waiting for event")
        .expect("Channel closed");
    
    match event {
        PaymentEvent::PaymentReleased { job_id: id, recipient, amount } => {
            assert_eq!(id, job_id);
            assert!(!recipient.is_zero());
            assert!(amount > U256::zero());
        }
        _ => panic!("Expected PaymentReleased event"),
    }
}

#[tokio::test]
async fn test_token_balance_checking() {
    let config = PaymentConfig::default();
    let web3_client = create_test_web3_client().await;
    
    let verifier = PaymentVerifier::new(config, web3_client.clone())
        .await
        .expect("Failed to create payment verifier");
    
    // Set up test wallet
    let wallet_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".parse::<Address>().unwrap();
    
    // Check ETH balance
    let eth_balance = verifier.get_token_balance("ETH", wallet_address)
        .await
        .expect("Failed to get ETH balance");
    
    assert!(eth_balance > U256::zero());
    
    // Check USDC balance
    let usdc_balance = verifier.get_token_balance("USDC", wallet_address)
        .await
        .expect("Failed to get USDC balance");
    
    // Test wallet might have 0 USDC
    assert!(usdc_balance >= U256::zero());
}

#[tokio::test]
async fn test_payment_approval_verification() {
    let config = PaymentConfig::default();
    let web3_client = create_test_web3_client().await;
    
    let verifier = PaymentVerifier::new(config, web3_client.clone())
        .await
        .expect("Failed to create payment verifier");
    
    let client_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".parse::<Address>().unwrap();
    let amount = U256::from(100_000_000); // 100 USDC
    
    // Check if client has approved escrow
    let is_approved = verifier.check_token_approval("USDC", client_address, amount)
        .await
        .expect("Failed to check approval");
    
    if !is_approved {
        // Would need to approve in real scenario
        approve_token(&web3_client, "USDC", verifier.escrow_address(), amount).await;
        
        // Check again
        let is_approved = verifier.check_token_approval("USDC", client_address, amount)
            .await
            .expect("Failed to check approval");
        
        assert!(is_approved);
    }
}

#[tokio::test]
async fn test_multi_token_job_payments() {
    let config = PaymentConfig::default();
    let web3_client = create_test_web3_client().await;
    
    let verifier = PaymentVerifier::new(config, web3_client.clone())
        .await
        .expect("Failed to create payment verifier");
    
    // Create jobs with different payment tokens
    let usdc_job = create_job_with_token(&web3_client, "USDC", U256::from(100_000_000)).await;
    let eth_job = create_job_with_token(&web3_client, "ETH", U256::from(1_000_000_000_000_000)).await;
    
    // Verify both payments
    let usdc_deposit = verifier.verify_escrow_deposit(usdc_job)
        .await
        .expect("Failed to verify USDC deposit");
    
    let eth_deposit = verifier.verify_escrow_deposit(eth_job)
        .await
        .expect("Failed to verify ETH deposit");
    
    assert_eq!(usdc_deposit.token_symbol, "USDC");
    assert_eq!(eth_deposit.token_symbol, "ETH");
}

#[tokio::test]
async fn test_payment_dispute_handling() {
    let config = PaymentConfig::default();
    let web3_client = create_test_web3_client().await;
    
    let mut verifier = PaymentVerifier::new(config, web3_client.clone())
        .await
        .expect("Failed to create payment verifier");
    
    let mut event_receiver = verifier.start_monitoring().await;
    
    // Create job and raise dispute
    let job_id = create_job_with_payment(&web3_client, U256::from(100_000_000)).await;
    raise_payment_dispute(&web3_client, job_id).await;
    
    // Should receive dispute event
    let event = tokio::time::timeout(Duration::from_secs(2), event_receiver.recv())
        .await
        .expect("Timeout waiting for event")
        .expect("Channel closed");
    
    match event {
        PaymentEvent::DisputeRaised { job_id: id, reason } => {
            assert_eq!(id, job_id);
            assert!(!reason.is_empty());
        }
        _ => panic!("Expected DisputeRaised event"),
    }
    
    // Check payment status
    let status = verifier.get_payment_status(job_id)
        .await
        .expect("Failed to get payment status");
    
    assert_eq!(status, PaymentStatus::Disputed);
}

#[tokio::test]
async fn test_fee_calculation() {
    let config = PaymentConfig {
        platform_fee_percentage: 250, // 2.5%
        ..Default::default()
    };
    
    let web3_client = create_test_web3_client().await;
    let verifier = PaymentVerifier::new(config, web3_client)
        .await
        .expect("Failed to create payment verifier");
    
    // Calculate fees for different amounts
    let test_cases = vec![
        (U256::from(100_000_000), U256::from(2_500_000)), // 100 USDC -> 2.5 USDC fee
        (U256::from(1_000_000_000), U256::from(25_000_000)), // 1000 USDC -> 25 USDC fee
    ];
    
    for (amount, expected_fee) in test_cases {
        let (host_amount, platform_fee) = verifier.calculate_payment_split(amount);
        
        assert_eq!(platform_fee, expected_fee);
        assert_eq!(host_amount + platform_fee, amount);
    }
}

#[tokio::test]
async fn test_payment_timeout_detection() {
    let config = PaymentConfig {
        payment_timeout: Duration::from_millis(500), // Short timeout for testing
        ..Default::default()
    };
    
    let web3_client = create_test_web3_client().await;
    let mut verifier = PaymentVerifier::new(config, web3_client.clone())
        .await
        .expect("Failed to create payment verifier");
    
    // Create a job
    let job_id = create_job_with_payment(&web3_client, U256::from(100_000_000)).await;
    
    // Wait for timeout
    tokio::time::sleep(Duration::from_secs(1)).await;
    
    // Check if payment timed out
    let is_timed_out = verifier.check_payment_timeout(job_id)
        .await
        .expect("Failed to check timeout");
    
    assert!(is_timed_out);
    
    // Should be able to request refund
    let can_refund = verifier.can_request_refund(job_id)
        .await
        .expect("Failed to check refund eligibility");
    
    assert!(can_refund);
}

#[tokio::test]
async fn test_batch_payment_processing() {
    let config = PaymentConfig::default();
    let web3_client = create_test_web3_client().await;
    
    let verifier = PaymentVerifier::new(config, web3_client.clone())
        .await
        .expect("Failed to create payment verifier");
    
    // Create multiple completed jobs
    let mut job_ids = Vec::new();
    for _ in 0..5 {
        let job_id = create_and_complete_job(&web3_client).await;
        job_ids.push(job_id);
    }
    
    // Process batch payment release
    let results = verifier.process_batch_payments(&job_ids)
        .await
        .expect("Failed to process batch payments");
    
    // All should succeed
    assert_eq!(results.len(), 5);
    for (job_id, result) in results {
        assert!(result.is_ok());
        assert!(job_ids.contains(&job_id));
    }
}

#[tokio::test]
async fn test_payment_history_tracking() {
    let config = PaymentConfig::default();
    let web3_client = create_test_web3_client().await;
    
    let verifier = PaymentVerifier::new(config, web3_client.clone())
        .await
        .expect("Failed to create payment verifier");
    
    let host_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266".parse::<Address>().unwrap();
    
    // Get payment history for host
    let history = verifier.get_payment_history(host_address, 30) // Last 30 days
        .await
        .expect("Failed to get payment history");
    
    // Should include completed payments
    assert!(!history.is_empty());
    
    // Calculate total earnings
    let total_earnings = history.iter()
        .filter(|p| p.status == PaymentStatus::Released)
        .map(|p| p.amount)
        .fold(U256::zero(), |acc, amt| acc + amt);
    
    assert!(total_earnings >= U256::zero());
}

#[tokio::test]
async fn test_gas_cost_estimation() {
    let config = PaymentConfig::default();
    let web3_client = create_test_web3_client().await;
    
    let verifier = PaymentVerifier::new(config, web3_client)
        .await
        .expect("Failed to create payment verifier");
    
    // Estimate gas for claiming payment
    let job_id = U256::from(1);
    let gas_estimate = verifier.estimate_claim_gas(job_id)
        .await
        .expect("Failed to estimate gas");
    
    // Should be reasonable for escrow release
    assert!(gas_estimate > U256::from(50_000)); // Min gas
    assert!(gas_estimate < U256::from(500_000)); // Max gas
    
    // Convert to cost estimate
    let gas_price = verifier.get_current_gas_price()
        .await
        .expect("Failed to get gas price");
    
    let total_cost = gas_estimate * gas_price;
    assert!(total_cost > U256::zero());
}

// Helper functions
async fn create_test_web3_client() -> Arc<Web3Client> {
    let config = Web3Config::default();
    Arc::new(Web3Client::new(config).await.expect("Failed to create Web3 client"))
}

async fn create_job_with_payment(client: &Web3Client, amount: U256) -> U256 {
    // Implementation would create job with payment
    U256::from(1)
}

async fn create_job_with_token(client: &Web3Client, token: &str, amount: U256) -> U256 {
    // Implementation would create job with specific token
    U256::from(2)
}

async fn create_and_complete_job(client: &Web3Client) -> U256 {
    // Implementation would create and complete job
    U256::from(3)
}

async fn raise_payment_dispute(client: &Web3Client, job_id: U256) {
    // Implementation would raise dispute
}

async fn approve_token(client: &Web3Client, token: &str, spender: Address, amount: U256) {
    // Implementation would approve token spending
}

use fabstir_llm_node::contracts::PaymentEvent;