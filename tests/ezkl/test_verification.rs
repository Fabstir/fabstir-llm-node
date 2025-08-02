use anyhow::Result;
use fabstir_llm_node::ezkl::{
    ProofVerifier, VerificationRequest, VerificationResult,
    VerificationStatus, VerifyingKey, ProofData, PublicInputs,
    VerificationError, VerificationMode, TrustLevel,
    OnChainVerifier, VerificationMetrics
};
use ethers::types::{Address, U256};
use std::str::FromStr;
use std::collections::HashMap;
use std::time::Duration;
use tokio;

async fn create_test_verifier() -> Result<ProofVerifier> {
    ProofVerifier::new_mock().await // Use mock for testing
}

fn create_test_proof_data() -> ProofData {
    ProofData {
        proof_bytes: vec![1, 2, 3, 4, 5, 6, 7, 8], // Mock proof
        public_inputs: PublicInputs {
            model_hash: "abc123def456".to_string(),
            input_hash: "input_hash_789".to_string(),
            output_hash: "output_hash_012".to_string(),
            timestamp: chrono::Utc::now().timestamp() as u64,
            node_id: "node_12345".to_string(),
        },
        proof_format: fabstir_llm_node::ezkl::ProofFormat::Standard,
        proof_system_version: "v1.0.0".to_string(),
        inner_proofs: vec![], // No inner proofs for basic test
    }
}

fn create_test_verifying_key() -> VerifyingKey {
    VerifyingKey {
        key_bytes: vec![9, 10, 11, 12, 13, 14, 15, 16], // Mock VK
        model_id: "llama-7b".to_string(),
        circuit_hash: "circuit_hash_345".to_string(),
        key_hash: "vk_hash_678".to_string(),
    }
}

// Helper function to create a default VerificationRequest
fn create_verification_request(
    proof: ProofData,
    vk: VerifyingKey,
    mode: VerificationMode,
    trust_level: TrustLevel,
) -> VerificationRequest {
    VerificationRequest {
        proof,
        verifying_key: vk,
        mode,
        trust_level,
        constraints: HashMap::new(),
        metadata: HashMap::new(),
        on_chain_verifier: None,
        max_proof_age: None,
    }
}

#[tokio::test]
async fn test_basic_verification() {
    let verifier = create_test_verifier().await.unwrap();
    let proof_data = create_test_proof_data();
    let vk = create_test_verifying_key();
    
    let request = create_verification_request(
        proof_data,
        vk,
        VerificationMode::Full,
        TrustLevel::Strict,
    );
    
    let result = verifier.verify_proof(request).await.unwrap();
    
    assert_eq!(result.status, VerificationStatus::Valid);
    assert!(result.is_valid);
    assert!(!result.error_message.is_some());
    assert!(result.verification_time_ms > 0);
    assert_eq!(result.trust_level, TrustLevel::Strict);
}

#[tokio::test]
async fn test_verification_modes() {
    let verifier = create_test_verifier().await.unwrap();
    let proof_data = create_test_proof_data();
    let vk = create_test_verifying_key();
    
    let modes = vec![
        VerificationMode::Full,
        VerificationMode::Fast,
        VerificationMode::Optimistic,
        VerificationMode::Batch,
    ];
    
    for mode in modes {
        let request = create_verification_request(
            proof_data.clone(),
            vk.clone(),
            mode.clone(),
            TrustLevel::Standard,
        );
        
        let result = verifier.verify_proof(request).await.unwrap();
        
        assert_eq!(result.mode, mode);
        assert!(result.is_valid);
        
        // Verify mode-specific properties
        match mode {
            VerificationMode::Fast => {
                assert!(result.verification_time_ms < 100); // Should be fast
            }
            VerificationMode::Optimistic => {
                assert!(result.confidence_score.is_some());
                assert!(result.confidence_score.unwrap() >= 0.95);
            }
            VerificationMode::Batch => {
                assert!(result.batch_compatible);
            }
            _ => {}
        }
    }
}

#[tokio::test]
async fn test_invalid_proof_detection() {
    let verifier = create_test_verifier().await.unwrap();
    let mut proof_data = create_test_proof_data();
    let vk = create_test_verifying_key();
    
    // Corrupt the proof
    proof_data.proof_bytes[0] = 255;
    proof_data.proof_bytes[1] = 254;
    
    let request = create_verification_request(
        proof_data,
        vk,
        VerificationMode::Full,
        TrustLevel::Strict,
    );
    
    let result = verifier.verify_proof(request).await.unwrap();
    
    assert_eq!(result.status, VerificationStatus::Invalid);
    assert!(!result.is_valid);
    assert!(result.error_message.is_some());
    assert!(result.error_message.as_ref().unwrap().contains("invalid") || 
            result.error_message.as_ref().unwrap().contains("failed"));
}

#[tokio::test]
async fn test_public_inputs_validation() {
    let verifier = create_test_verifier().await.unwrap();
    let proof_data = create_test_proof_data();
    let vk = create_test_verifying_key();
    
    // Test with mismatched model hash
    let mut bad_proof = proof_data.clone();
    bad_proof.public_inputs.model_hash = "wrong_hash".to_string();
    
    let request = create_verification_request(
        bad_proof,
        vk.clone(),
        VerificationMode::Full,
        TrustLevel::Standard,
    );
    
    let result = verifier.verify_proof(request).await.unwrap();
    
    assert_eq!(result.status, VerificationStatus::Invalid);
    assert!(result.error_message.as_ref().unwrap().contains("model_hash") || 
            result.error_message.as_ref().unwrap().contains("mismatch"));
}

#[tokio::test]
async fn test_batch_verification() {
    let verifier = create_test_verifier().await.unwrap();
    
    // Create multiple proofs
    let proofs: Vec<(ProofData, VerifyingKey)> = (0..10)
        .map(|i| {
            let mut proof = create_test_proof_data();
            proof.public_inputs.timestamp += i as u64;
            let vk = create_test_verifying_key();
            (proof, vk)
        })
        .collect();
    
    let result = verifier.verify_batch(proofs).await.unwrap();
    
    assert_eq!(result.total_proofs, 10);
    assert_eq!(result.valid_proofs, 10);
    assert_eq!(result.invalid_proofs, 0);
    assert!(result.batch_verification_time_ms > 0);
    assert!(result.avg_verification_time_ms > 0);
    assert!(result.batch_speedup > 1.0); // Batch should be faster per proof
}

#[tokio::test]
async fn test_on_chain_verification() {
    let verifier = create_test_verifier().await.unwrap();
    let proof_data = create_test_proof_data();
    let vk = create_test_verifying_key();
    
    // Create mock on-chain verifier
    let contract_address = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
    let on_chain_verifier = OnChainVerifier::new_mock(contract_address);
    
    let request = create_verification_request(
        proof_data,
        vk,
        VerificationMode::Full,
        TrustLevel::Strict,
    )
    .with_on_chain_verification(on_chain_verifier);
    
    let result = verifier.verify_proof(request).await.unwrap();
    
    assert!(result.is_valid);
    assert!(result.on_chain_verification.is_some());
    
    let on_chain_result = result.on_chain_verification.unwrap();
    assert!(on_chain_result.verified);
    assert!(on_chain_result.tx_hash.len() == 66); // 0x + 64 chars
    assert!(on_chain_result.gas_used > U256::zero());
    assert_eq!(on_chain_result.contract_address, contract_address);
}

#[tokio::test]
async fn test_recursive_proof_verification() {
    let verifier = create_test_verifier().await.unwrap();
    
    // Create a recursive proof (proof of proof)
    let inner_proof = create_test_proof_data();
    let mut recursive_proof = create_test_proof_data();
    recursive_proof.proof_format = fabstir_llm_node::ezkl::ProofFormat::Recursive;
    recursive_proof.inner_proofs = vec![inner_proof];
    
    let vk = create_test_verifying_key();
    
    let request = create_verification_request(
        recursive_proof,
        vk,
        VerificationMode::Full,
        TrustLevel::Standard,
    );
    
    let result = verifier.verify_proof(request).await.unwrap();
    
    assert!(result.is_valid);
    assert_eq!(result.recursion_depth, 1);
    assert!(result.inner_verification_results.is_some());
    assert_eq!(result.inner_verification_results.unwrap().len(), 1);
}

#[tokio::test]
async fn test_verification_with_constraints() {
    let verifier = create_test_verifier().await.unwrap();
    let proof_data = create_test_proof_data();
    let vk = create_test_verifying_key();
    
    // Add verification constraints
    let mut request = create_verification_request(
        proof_data,
        vk,
        VerificationMode::Full,
        TrustLevel::Standard,
    );
    
    request.add_constraint("max_output_length", "1000");
    request.add_constraint("min_confidence", "0.95");
    request.add_constraint("allowed_models", "llama-7b,gpt-3");
    
    let result = verifier.verify_proof(request).await.unwrap();
    
    assert!(result.is_valid);
    assert!(result.constraints_satisfied);
    assert_eq!(result.constraint_results.len(), 3);
    
    for (constraint, satisfied) in &result.constraint_results {
        assert!(satisfied, "Constraint {} not satisfied", constraint);
    }
}

#[tokio::test]
async fn test_verification_caching() {
    let verifier = create_test_verifier().await.unwrap();
    let proof_data = create_test_proof_data();
    let vk = create_test_verifying_key();
    
    let request = create_verification_request(
        proof_data.clone(),
        vk.clone(),
        VerificationMode::Full,
        TrustLevel::Standard,
    );
    
    // First verification
    let start = std::time::Instant::now();
    let result1 = verifier.verify_proof(request.clone()).await.unwrap();
    let first_time = start.elapsed();
    
    // Second verification (should be cached)
    let start = std::time::Instant::now();
    let result2 = verifier.verify_proof(request).await.unwrap();
    let cached_time = start.elapsed();
    
    assert_eq!(result1.is_valid, result2.is_valid);
    assert!(result2.from_cache);
    assert!(cached_time < first_time / 10); // Cached should be much faster
}

#[tokio::test]
async fn test_verification_metrics() {
    let verifier = create_test_verifier().await.unwrap();
    
    // Perform multiple verifications
    for i in 0..5 {
        let mut proof_data = create_test_proof_data();
        proof_data.public_inputs.timestamp += i;
        let vk = create_test_verifying_key();
        
        let request = create_verification_request(
            proof_data,
            vk,
            VerificationMode::Full,
            TrustLevel::Standard,
        );
        
        verifier.verify_proof(request).await.unwrap();
    }
    
    let metrics = verifier.get_metrics().await;
    
    assert_eq!(metrics.total_verifications, 5);
    assert!(metrics.successful_verifications >= 5);
    assert_eq!(metrics.failed_verifications, 0);
    assert!(metrics.avg_verification_time_ms > 0.0);
    assert!(metrics.cache_hit_rate >= 0.0);
    assert!(metrics.total_gas_used == U256::zero()); // No on-chain in this test
}

#[tokio::test]
async fn test_proof_expiry_validation() {
    let verifier = create_test_verifier().await.unwrap();
    let mut proof_data = create_test_proof_data();
    let vk = create_test_verifying_key();
    
    // Set timestamp to 1 hour ago
    proof_data.public_inputs.timestamp = 
        (chrono::Utc::now().timestamp() - 3600) as u64;
    
    let mut request = create_verification_request(
        proof_data,
        vk,
        VerificationMode::Full,
        TrustLevel::Strict,
    );
    
    // Set max age to 30 minutes
    request.set_max_proof_age(Duration::from_secs(1800));
    
    let result = verifier.verify_proof(request).await.unwrap();
    
    assert_eq!(result.status, VerificationStatus::Expired);
    assert!(!result.is_valid);
    assert!(result.error_message.as_ref().unwrap().contains("expired") || 
            result.error_message.as_ref().unwrap().contains("too old"));
}

#[tokio::test]
async fn test_cross_model_verification() {
    let verifier = create_test_verifier().await.unwrap();
    let proof_data = create_test_proof_data();
    let mut vk = create_test_verifying_key();
    
    // VK is for different model
    vk.model_id = "gpt-3".to_string();
    
    let request = create_verification_request(
        proof_data,
        vk,
        VerificationMode::Full,
        TrustLevel::Strict,
    );
    
    let result = verifier.verify_proof(request).await.unwrap();
    
    assert_eq!(result.status, VerificationStatus::Invalid);
    assert!(result.error_message.as_ref().unwrap().contains("model") || 
            result.error_message.as_ref().unwrap().contains("mismatch"));
}

#[tokio::test]
async fn test_verification_with_metadata() {
    let verifier = create_test_verifier().await.unwrap();
    let proof_data = create_test_proof_data();
    let vk = create_test_verifying_key();
    
    let mut request = create_verification_request(
        proof_data,
        vk,
        VerificationMode::Full,
        TrustLevel::Standard,
    );
    
    // Add metadata for verification context
    request.add_metadata("job_id", "job_12345");
    request.add_metadata("client_id", "client_abc");
    request.add_metadata("inference_type", "completion");
    
    let result = verifier.verify_proof(request).await.unwrap();
    
    assert!(result.is_valid);
    assert_eq!(result.metadata.get("job_id"), Some(&"job_12345".to_string()));
    assert_eq!(result.metadata.get("client_id"), Some(&"client_abc".to_string()));
}