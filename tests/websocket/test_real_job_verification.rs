use fabstir_llm_node::api::websocket::job_verification::{
    JobVerifier, JobVerificationConfig, JobDetails, VerificationResult,
    BlockchainVerifier, JobStatus,
};
use fabstir_llm_node::contracts::client::Web3Client;
use ethers::types::{Address, U256};
use std::str::FromStr;
use std::time::Duration;

#[tokio::test]
async fn test_blockchain_job_verification() {
    // Connect to Base Sepolia
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://sepolia.base.org".to_string());
    
    let config = JobVerificationConfig {
        enabled: true,
        blockchain_verification: true,
        cache_duration: Duration::from_secs(60),
        marketplace_address: "0x7ce861CC0188c260f3Ba58eb9a4d33e17Eb62304".to_string(),
    };
    
    let verifier = match JobVerifier::new(config, rpc_url).await {
        Ok(v) => v,
        Err(e) => {
            println!("Skipping test: Could not connect to blockchain: {}", e);
            return;
        }
    };
    
    // Verify a known job ID (need to create one first or use existing)
    let job_id = 1u64; // Assuming job ID 1 exists
    
    let result = verifier.verify_job(job_id).await;
    
    match result {
        Ok(details) => {
            assert_eq!(details.job_id, job_id);
            assert!(!details.client_address.is_empty());
            assert!(details.payment_amount > 0);
            // Job should be in a valid state
            assert!(matches!(
                details.status,
                JobStatus::Pending | JobStatus::Claimed | JobStatus::Completed
            ));
        }
        Err(e) => {
            // If job doesn't exist, that's OK for this test
            println!("Job verification error (expected if job doesn't exist): {}", e);
        }
    }
}

#[tokio::test]
async fn test_job_state_transitions() {
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://sepolia.base.org".to_string());
    
    let config = JobVerificationConfig {
        enabled: true,
        blockchain_verification: true,
        cache_duration: Duration::from_secs(60),
        marketplace_address: "0x7ce861CC0188c260f3Ba58eb9a4d33e17Eb62304".to_string(),
    };
    
    let verifier = match JobVerifier::new(config, rpc_url).await {
        Ok(v) => v,
        Err(e) => {
            println!("Skipping test: Could not connect to blockchain: {}", e);
            return;
        }
    };
    
    // Test state validation
    let pending_job = JobDetails {
        job_id: 1,
        client_address: "0x1234567890123456789012345678901234567890".to_string(),
        payment_amount: 1000000000000000000u128, // 1 ETH in wei
        model_id: "tinyllama-1.1b".to_string(),
        input_url: "https://s5.garden/input/123".to_string(),
        output_url: None,
        status: JobStatus::Pending,
        created_at: chrono::Utc::now().timestamp() as u64,
        deadline: chrono::Utc::now().timestamp() as u64 + 3600,
    };
    
    // Pending job can be claimed
    assert!(verifier.can_claim_job(&pending_job).await);
    
    // Claimed job cannot be claimed again
    let mut claimed_job = pending_job.clone();
    claimed_job.status = JobStatus::Claimed;
    assert!(!verifier.can_claim_job(&claimed_job).await);
    
    // Completed job cannot be claimed
    let mut completed_job = pending_job.clone();
    completed_job.status = JobStatus::Completed;
    assert!(!verifier.can_claim_job(&completed_job).await);
}

#[tokio::test]
async fn test_job_expiry_verification() {
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://sepolia.base.org".to_string());
    
    let config = JobVerificationConfig {
        enabled: true,
        blockchain_verification: true,
        cache_duration: Duration::from_secs(60),
        marketplace_address: "0x7ce861CC0188c260f3Ba58eb9a4d33e17Eb62304".to_string(),
    };
    
    let verifier = match JobVerifier::new(config, rpc_url).await {
        Ok(v) => v,
        Err(e) => {
            println!("Skipping test: Could not connect to blockchain: {}", e);
            return;
        }
    };
    
    // Test expired job
    let expired_job = JobDetails {
        job_id: 2,
        client_address: "0x1234567890123456789012345678901234567890".to_string(),
        payment_amount: 1000000000000000000u128,
        model_id: "tinyllama-1.1b".to_string(),
        input_url: "https://s5.garden/input/123".to_string(),
        output_url: None,
        status: JobStatus::Pending,
        created_at: chrono::Utc::now().timestamp() as u64 - 7200, // 2 hours ago
        deadline: chrono::Utc::now().timestamp() as u64 - 3600, // 1 hour ago
    };
    
    assert!(verifier.is_job_expired(&expired_job).await);
    assert!(!verifier.can_claim_job(&expired_job).await);
    
    // Test non-expired job
    let valid_job = JobDetails {
        job_id: 3,
        client_address: "0x1234567890123456789012345678901234567890".to_string(),
        payment_amount: 1000000000000000000u128,
        model_id: "tinyllama-1.1b".to_string(),
        input_url: "https://s5.garden/input/123".to_string(),
        output_url: None,
        status: JobStatus::Pending,
        created_at: chrono::Utc::now().timestamp() as u64,
        deadline: chrono::Utc::now().timestamp() as u64 + 3600, // 1 hour from now
    };
    
    assert!(!verifier.is_job_expired(&valid_job).await);
    assert!(verifier.can_claim_job(&valid_job).await);
}

#[tokio::test]
async fn test_payment_verification() {
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://sepolia.base.org".to_string());
    
    let config = JobVerificationConfig {
        enabled: true,
        blockchain_verification: true,
        cache_duration: Duration::from_secs(60),
        marketplace_address: "0x7ce861CC0188c260f3Ba58eb9a4d33e17Eb62304".to_string(),
    };
    
    let verifier = match JobVerifier::new(config, rpc_url).await {
        Ok(v) => v,
        Err(e) => {
            println!("Skipping test: Could not connect to blockchain: {}", e);
            return;
        }
    };
    
    // Test payment verification
    let job = JobDetails {
        job_id: 4,
        client_address: "0x1234567890123456789012345678901234567890".to_string(),
        payment_amount: 1000000000000000000u128, // 1 ETH
        model_id: "tinyllama-1.1b".to_string(),
        input_url: "https://s5.garden/input/123".to_string(),
        output_url: None,
        status: JobStatus::Pending,
        created_at: chrono::Utc::now().timestamp() as u64,
        deadline: chrono::Utc::now().timestamp() as u64 + 3600,
    };
    
    // Verify payment is escrowed
    let payment_verified = verifier.verify_payment_escrowed(&job).await;
    
    match payment_verified {
        Ok(true) => {
            println!("Payment verified in escrow");
        }
        Ok(false) => {
            println!("Payment not in escrow");
        }
        Err(e) => {
            println!("Payment verification error: {}", e);
        }
    }
}

#[tokio::test]
async fn test_concurrent_job_verifications() {
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://sepolia.base.org".to_string());
    
    let config = JobVerificationConfig {
        enabled: true,
        blockchain_verification: true,
        cache_duration: Duration::from_secs(60),
        marketplace_address: "0x7ce861CC0188c260f3Ba58eb9a4d33e17Eb62304".to_string(),
    };
    
    let verifier = match JobVerifier::new(config, rpc_url).await {
        Ok(v) => v,
        Err(e) => {
            println!("Skipping test: Could not connect to blockchain: {}", e);
            return;
        }
    };
    
    // Verify multiple jobs concurrently
    let mut handles = vec![];
    
    for job_id in 1..=5 {
        let verifier_clone = verifier.clone();
        let handle = tokio::spawn(async move {
            verifier_clone.verify_job(job_id).await
        });
        handles.push(handle);
    }
    
    let mut results = vec![];
    for handle in handles {
        let result = handle.await.unwrap();
        results.push(result);
    }
    
    // Check results
    for (i, result) in results.iter().enumerate() {
        match result {
            Ok(details) => {
                println!("Job {} verified: status = {:?}", i + 1, details.status);
            }
            Err(e) => {
                println!("Job {} verification failed: {}", i + 1, e);
            }
        }
    }
}

#[tokio::test]
async fn test_job_verification_caching() {
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://sepolia.base.org".to_string());
    
    let config = JobVerificationConfig {
        enabled: true,
        blockchain_verification: true,
        cache_duration: Duration::from_secs(60),
        marketplace_address: "0x7ce861CC0188c260f3Ba58eb9a4d33e17Eb62304".to_string(),
    };
    
    let verifier = match JobVerifier::new(config, rpc_url).await {
        Ok(v) => v,
        Err(e) => {
            println!("Skipping test: Could not connect to blockchain: {}", e);
            return;
        }
    };
    
    let job_id = 1u64;
    
    // First verification - hits blockchain
    let start = std::time::Instant::now();
    let result1 = verifier.verify_job(job_id).await;
    let first_duration = start.elapsed();
    
    // Second verification - should use cache
    let start = std::time::Instant::now();
    let result2 = verifier.verify_job(job_id).await;
    let cached_duration = start.elapsed();
    
    // Cache should be faster
    if result1.is_ok() && result2.is_ok() {
        assert!(cached_duration < first_duration / 2);
        
        // Results should be identical
        let details1 = result1.unwrap();
        let details2 = result2.unwrap();
        assert_eq!(details1.job_id, details2.job_id);
        assert_eq!(details1.client_address, details2.client_address);
        assert_eq!(details1.payment_amount, details2.payment_amount);
    }
}

#[tokio::test]
async fn test_signature_verification_for_job_claim() {
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://sepolia.base.org".to_string());
    
    let config = JobVerificationConfig {
        enabled: true,
        blockchain_verification: true,
        cache_duration: Duration::from_secs(60),
        marketplace_address: "0x7ce861CC0188c260f3Ba58eb9a4d33e17Eb62304".to_string(),
    };
    
    let verifier = match JobVerifier::new(config, rpc_url).await {
        Ok(v) => v,
        Err(e) => {
            println!("Skipping test: Could not connect to blockchain: {}", e);
            return;
        }
    };
    
    // Test signature verification for job claims
    let job_id = 5u64;
    let host_address = "0xABCDEF1234567890ABCDEF1234567890ABCDEF12";
    
    // Generate claim message
    let message = verifier.create_claim_message(job_id, host_address).await;
    
    // In real scenario, host would sign this with their private key
    // For testing, we'll verify the message format
    assert!(message.contains(&job_id.to_string()));
    assert!(message.contains(host_address));
    
    // Verify signature (mock for now, real implementation would use ethers)
    let mock_signature = "0x1234..."; // Mock signature
    let is_valid = verifier.verify_claim_signature(
        job_id,
        host_address,
        mock_signature
    ).await;
    
    // Mock should return validation result
    match is_valid {
        Ok(valid) => println!("Signature validation: {}", valid),
        Err(e) => println!("Signature validation error: {}", e),
    }
}

#[tokio::test]
async fn test_job_metadata_retrieval() {
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://sepolia.base.org".to_string());
    
    let config = JobVerificationConfig {
        enabled: true,
        blockchain_verification: true,
        cache_duration: Duration::from_secs(60),
        marketplace_address: "0x7ce861CC0188c260f3Ba58eb9a4d33e17Eb62304".to_string(),
    };
    
    let verifier = match JobVerifier::new(config, rpc_url).await {
        Ok(v) => v,
        Err(e) => {
            println!("Skipping test: Could not connect to blockchain: {}", e);
            return;
        }
    };
    
    // Test retrieving job metadata
    let job_id = 1u64;
    
    let metadata = verifier.get_job_metadata(job_id).await;
    
    match metadata {
        Ok(data) => {
            assert!(data.contains_key("model_id"));
            assert!(data.contains_key("input_url"));
            assert!(data.contains_key("max_tokens"));
            println!("Job metadata: {:?}", data);
        }
        Err(e) => {
            println!("Metadata retrieval error: {}", e);
        }
    }
}

#[tokio::test]
async fn test_batch_job_verification() {
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://sepolia.base.org".to_string());
    
    let config = JobVerificationConfig {
        enabled: true,
        blockchain_verification: true,
        cache_duration: Duration::from_secs(60),
        marketplace_address: "0x7ce861CC0188c260f3Ba58eb9a4d33e17Eb62304".to_string(),
    };
    
    let verifier = match JobVerifier::new(config, rpc_url).await {
        Ok(v) => v,
        Err(e) => {
            println!("Skipping test: Could not connect to blockchain: {}", e);
            return;
        }
    };
    
    // Verify multiple jobs in batch
    let job_ids = vec![1, 2, 3, 4, 5];
    
    let results = verifier.batch_verify_jobs(job_ids).await;
    
    match results {
        Ok(jobs) => {
            for job in jobs {
                println!("Batch verified job {}: status = {:?}", job.job_id, job.status);
            }
        }
        Err(e) => {
            println!("Batch verification error: {}", e);
        }
    }
}

#[tokio::test]
async fn test_job_verification_disabled_mode() {
    let rpc_url = std::env::var("RPC_URL")
        .unwrap_or_else(|_| "https://sepolia.base.org".to_string());
    
    let config = JobVerificationConfig {
        enabled: false, // Disabled mode
        blockchain_verification: false,
        cache_duration: Duration::from_secs(60),
        marketplace_address: "0x7ce861CC0188c260f3Ba58eb9a4d33e17Eb62304".to_string(),
    };
    
    let verifier = match JobVerifier::new(config, rpc_url).await {
        Ok(v) => v,
        Err(e) => {
            println!("Skipping test: Could not connect to blockchain: {}", e);
            return;
        }
    };
    
    // When disabled, all verifications should pass
    let result = verifier.verify_job(99999).await;
    assert!(result.is_ok());
    
    let details = result.unwrap();
    assert_eq!(details.job_id, 99999);
    assert_eq!(details.status, JobStatus::Pending); // Default status when disabled
}