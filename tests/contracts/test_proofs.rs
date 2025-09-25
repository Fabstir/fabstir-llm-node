use ethers::prelude::*;
use fabstir_llm_node::contracts::{ProofConfig, ProofData, ProofStatus, ProofSubmitter};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_proof_submitter_creation() {
    let config = ProofConfig {
        proof_system_address: "0xCf7Ed3AccA5a467e9e704C703E8D87F634fB0Fc9"
            .parse()
            .unwrap(),
        ezkl_verifier_address: "0xDc64a140Aa3E981100a9becA4E685f962f0cF6C9"
            .parse()
            .unwrap(),
        proof_generation_timeout: Duration::from_secs(300),
        max_proof_size: 10 * 1024,                    // 10KB
        challenge_period: Duration::from_secs(86400), // 24 hours
        ..Default::default()
    };

    let web3_client = create_test_web3_client().await;
    let submitter = ProofSubmitter::new(config, web3_client)
        .await
        .expect("Failed to create proof submitter");

    // Should be ready to submit proofs
    assert!(submitter.is_ready());
}

#[tokio::test]
async fn test_proof_generation() {
    let config = ProofConfig::default();
    let web3_client = create_test_web3_client().await;

    let submitter = ProofSubmitter::new(config, web3_client)
        .await
        .expect("Failed to create proof submitter");

    // Generate proof for inference
    let job_id = U256::from(1);
    let model_commitment = vec![1u8; 32];
    let input_hash = vec![2u8; 32];
    let output_hash = vec![3u8; 32];

    let proof_data = submitter
        .generate_proof(job_id, model_commitment, input_hash, output_hash)
        .await
        .expect("Failed to generate proof");

    // Verify proof structure
    assert_eq!(proof_data.job_id, job_id);
    assert!(!proof_data.proof.is_empty());
    assert!(!proof_data.public_inputs.is_empty());
    assert!(!proof_data.verification_key.is_empty());
}

#[tokio::test]
async fn test_proof_submission() {
    let config = ProofConfig::default();
    let web3_client = create_test_web3_client().await;

    let mut submitter = ProofSubmitter::new(config, web3_client.clone())
        .await
        .expect("Failed to create proof submitter");

    // Set up wallet for submission
    submitter
        .set_wallet("0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
        .expect("Failed to set wallet");

    // Create a job and generate proof
    let job_id = create_test_job(&web3_client).await;
    let proof_data = generate_test_proof(job_id);

    // Submit proof
    let tx_hash = submitter
        .submit_proof(proof_data.clone())
        .await
        .expect("Failed to submit proof");

    // Wait for confirmation
    let receipt = web3_client
        .wait_for_confirmation(tx_hash)
        .await
        .expect("Failed to get receipt");

    assert_eq!(receipt.status.unwrap(), U64::from(1));

    // Verify proof was recorded
    let proof_status = submitter
        .get_proof_status(job_id)
        .await
        .expect("Failed to get proof status");

    assert_eq!(proof_status, ProofStatus::Submitted);
}

#[tokio::test]
async fn test_proof_verification_monitoring() {
    let config = ProofConfig::default();
    let web3_client = create_test_web3_client().await;

    let mut submitter = ProofSubmitter::new(config, web3_client.clone())
        .await
        .expect("Failed to create proof submitter");

    // Start monitoring
    let mut event_receiver = submitter.start_monitoring().await;

    // Submit and verify a proof
    let job_id = create_test_job(&web3_client).await;
    submit_and_verify_proof(&web3_client, job_id).await;

    // Should receive verification event
    let event = tokio::time::timeout(Duration::from_secs(2), event_receiver.recv())
        .await
        .expect("Timeout waiting for event")
        .expect("Channel closed");

    match event {
        ProofEvent::ProofVerified {
            job_id: id,
            is_valid,
        } => {
            assert_eq!(id, job_id);
            assert!(is_valid);
        }
        _ => panic!("Expected ProofVerified event"),
    }
}

#[tokio::test]
async fn test_challenge_detection() {
    let config = ProofConfig::default();
    let web3_client = create_test_web3_client().await;

    let mut submitter = ProofSubmitter::new(config, web3_client.clone())
        .await
        .expect("Failed to create proof submitter");

    let mut event_receiver = submitter.start_monitoring().await;

    // Submit proof and challenge it
    let job_id = create_test_job(&web3_client).await;
    submit_proof(&web3_client, job_id).await;
    challenge_proof(&web3_client, job_id, "Invalid output").await;

    // Should receive challenge event
    let event = tokio::time::timeout(Duration::from_secs(2), event_receiver.recv())
        .await
        .expect("Timeout waiting for event")
        .expect("Channel closed");

    match event {
        ProofEvent::ProofChallenged {
            job_id: id,
            challenger,
            reason,
        } => {
            assert_eq!(id, job_id);
            assert!(!challenger.is_zero());
            assert_eq!(reason, "Invalid output");
        }
        _ => panic!("Expected ProofChallenged event"),
    }
}

#[tokio::test]
async fn test_batch_proof_submission() {
    let config = ProofConfig {
        enable_batch_submission: true,
        batch_size: 5,
        ..Default::default()
    };

    let web3_client = create_test_web3_client().await;
    let mut submitter = ProofSubmitter::new(config, web3_client.clone())
        .await
        .expect("Failed to create proof submitter");

    submitter
        .set_wallet("0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
        .expect("Failed to set wallet");

    // Generate multiple proofs
    let mut proofs = Vec::new();
    for i in 0..5 {
        let job_id = U256::from(i + 1);
        let proof = generate_test_proof(job_id);
        proofs.push(proof);
    }

    // Submit batch
    let tx_hash = submitter
        .submit_proof_batch(proofs)
        .await
        .expect("Failed to submit batch");

    // Should complete in single transaction
    let receipt = web3_client
        .wait_for_confirmation(tx_hash)
        .await
        .expect("Failed to get receipt");

    assert_eq!(receipt.status.unwrap(), U64::from(1));

    // Gas should be less than 5 individual submissions
    assert!(receipt.gas_used.unwrap() < U256::from(1_000_000));
}

#[tokio::test]
async fn test_proof_storage_optimization() {
    let config = ProofConfig {
        use_proof_compression: true,
        store_proofs_on_ipfs: true,
        ..Default::default()
    };

    let web3_client = create_test_web3_client().await;
    let submitter = ProofSubmitter::new(config, web3_client)
        .await
        .expect("Failed to create proof submitter");

    // Generate large proof
    let job_id = U256::from(1);
    let large_proof = generate_large_test_proof(job_id);

    // Compress and store
    let stored_proof = submitter
        .prepare_proof_for_submission(large_proof.clone())
        .await
        .expect("Failed to prepare proof");

    // Should be compressed
    assert!(stored_proof.size < large_proof.proof.len());

    // Should have IPFS hash
    assert!(stored_proof.ipfs_hash.is_some());

    // On-chain data should be minimal
    assert!(stored_proof.on_chain_data.len() < 1024); // Less than 1KB
}

#[tokio::test]
async fn test_proof_timing_requirements() {
    let config = ProofConfig {
        max_proof_delay: Duration::from_secs(3600), // 1 hour
        ..Default::default()
    };

    let web3_client = create_test_web3_client().await;
    let submitter = ProofSubmitter::new(config, web3_client.clone())
        .await
        .expect("Failed to create proof submitter");

    // Create job
    let job_id = create_test_job(&web3_client).await;
    let job_timestamp = get_job_timestamp(&web3_client, job_id).await;

    // Check if still within proof window
    let can_submit = submitter
        .check_proof_deadline(job_id)
        .await
        .expect("Failed to check deadline");

    assert!(can_submit);

    // Simulate late submission
    let late_job_id = create_old_job(&web3_client, 2).await; // 2 hours old

    let can_submit_late = submitter
        .check_proof_deadline(late_job_id)
        .await
        .expect("Failed to check deadline");

    assert!(!can_submit_late);
}

#[tokio::test]
async fn test_proof_resubmission_on_failure() {
    let config = ProofConfig {
        max_resubmission_attempts: 3,
        resubmission_delay: Duration::from_millis(100),
        ..Default::default()
    };

    let web3_client = create_test_web3_client().await;
    let mut submitter = ProofSubmitter::new(config, web3_client.clone())
        .await
        .expect("Failed to create proof submitter");

    submitter
        .set_wallet("0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
        .expect("Failed to set wallet");

    // Inject network errors
    submitter.inject_error_rate(0.5); // 50% failure rate

    let job_id = create_test_job(&web3_client).await;
    let proof = generate_test_proof(job_id);

    // Should retry and eventually succeed
    let result = submitter.submit_proof_with_retry(proof).await;

    assert!(result.is_ok());

    // Check retry metrics
    let metrics = submitter.get_metrics();
    assert!(metrics.retry_count > 0);
    assert!(metrics.successful_submissions > 0);
}

#[tokio::test]
async fn test_proof_validation_before_submission() {
    let config = ProofConfig::default();
    let web3_client = create_test_web3_client().await;

    let submitter = ProofSubmitter::new(config, web3_client)
        .await
        .expect("Failed to create proof submitter");

    // Valid proof
    let valid_proof = generate_test_proof(U256::from(1));
    let is_valid = submitter
        .validate_proof(&valid_proof)
        .await
        .expect("Failed to validate proof");

    assert!(is_valid);

    // Invalid proof (empty)
    let invalid_proof = ProofData {
        job_id: U256::from(2),
        proof: vec![],
        public_inputs: vec![],
        verification_key: vec![],
        model_commitment: vec![],
        input_hash: vec![],
        output_hash: vec![],
    };

    let is_valid = submitter
        .validate_proof(&invalid_proof)
        .await
        .expect("Failed to validate proof");

    assert!(!is_valid);
}

#[tokio::test]
async fn test_challenge_response_submission() {
    let config = ProofConfig::default();
    let web3_client = create_test_web3_client().await;

    let mut submitter = ProofSubmitter::new(config, web3_client.clone())
        .await
        .expect("Failed to create proof submitter");

    submitter
        .set_wallet("0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
        .expect("Failed to set wallet");

    // Monitor for challenges
    let mut event_receiver = submitter.start_monitoring().await;

    // Create job, submit proof, and challenge it
    let job_id = create_test_job(&web3_client).await;
    submit_proof(&web3_client, job_id).await;
    let challenge_id = challenge_proof(&web3_client, job_id, "Invalid output").await;

    // Wait for challenge event
    let _ = event_receiver.recv().await;

    // Submit challenge response
    let response_data = generate_challenge_response(job_id, challenge_id);

    let tx_hash = submitter
        .submit_challenge_response(challenge_id, response_data)
        .await
        .expect("Failed to submit response");

    let receipt = web3_client
        .wait_for_confirmation(tx_hash)
        .await
        .expect("Failed to get receipt");

    assert_eq!(receipt.status.unwrap(), U64::from(1));
}

#[tokio::test]
async fn test_proof_fee_calculation() {
    let config = ProofConfig::default();
    let web3_client = create_test_web3_client().await;

    let submitter = ProofSubmitter::new(config, web3_client)
        .await
        .expect("Failed to create proof submitter");

    // Calculate verification fee
    let proof_size = 5 * 1024; // 5KB
    let fee = submitter
        .calculate_verification_fee(proof_size)
        .await
        .expect("Failed to calculate fee");

    // Fee should scale with proof size
    assert!(fee > U256::zero());

    // Larger proof should cost more
    let large_proof_size = 10 * 1024; // 10KB
    let large_fee = submitter
        .calculate_verification_fee(large_proof_size)
        .await
        .expect("Failed to calculate fee");

    assert!(large_fee > fee);
}

// Helper functions
async fn create_test_web3_client() -> Arc<Web3Client> {
    let config = Web3Config::default();
    Arc::new(
        Web3Client::new(config)
            .await
            .expect("Failed to create Web3 client"),
    )
}

async fn create_test_job(client: &Web3Client) -> U256 {
    // Implementation would create job via contract
    U256::from(1)
}

async fn create_old_job(client: &Web3Client, hours_ago: u64) -> U256 {
    // Implementation would create job with past timestamp
    U256::from(2)
}

async fn submit_proof(client: &Web3Client, job_id: U256) {
    // Implementation would submit proof
}

async fn submit_and_verify_proof(client: &Web3Client, job_id: U256) {
    // Implementation would submit and verify proof
}

async fn challenge_proof(client: &Web3Client, job_id: U256, reason: &str) -> U256 {
    // Implementation would challenge proof
    U256::from(1)
}

async fn get_job_timestamp(client: &Web3Client, job_id: U256) -> u64 {
    // Implementation would get job creation timestamp
    1234567890
}

fn generate_test_proof(job_id: U256) -> ProofData {
    ProofData {
        job_id,
        proof: vec![1u8; 256],
        public_inputs: vec![2u8; 64],
        verification_key: vec![3u8; 128],
        model_commitment: vec![4u8; 32],
        input_hash: vec![5u8; 32],
        output_hash: vec![6u8; 32],
    }
}

fn generate_large_test_proof(job_id: U256) -> ProofData {
    ProofData {
        job_id,
        proof: vec![1u8; 8192], // 8KB
        public_inputs: vec![2u8; 256],
        verification_key: vec![3u8; 512],
        model_commitment: vec![4u8; 32],
        input_hash: vec![5u8; 32],
        output_hash: vec![6u8; 32],
    }
}

fn generate_challenge_response(job_id: U256, challenge_id: U256) -> Vec<u8> {
    // Implementation would generate response data
    vec![7u8; 256]
}

use fabstir_llm_node::contracts::{ProofEvent, Web3Client, Web3Config};
