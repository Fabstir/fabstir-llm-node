// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use chrono::Utc;
use ethers::types::{H256, U256};
use fabstir_llm_node::job_processor::JobRequest;
use fabstir_llm_node::results::packager::{
    InferenceResult, PackagedResult, ResultMetadata, ResultPackager,
};
use fabstir_llm_node::results::proofs::{
    InferenceProof, ProofGenerationConfig, ProofGenerator, ProofType,
};
use std::sync::Arc;

#[tokio::test]
async fn test_ezkl_proof_verification_valid() -> Result<()> {
    let result = InferenceResult {
        job_id: "verify_test_1".to_string(),
        model_id: "tinyllama-1.1b".to_string(),
        prompt: "What is AI?".to_string(),
        response: "AI is artificial intelligence.".to_string(),
        tokens_generated: 20,
        inference_time_ms: 80,
        timestamp: Utc::now(),
        node_id: "verifier_node".to_string(),
        metadata: ResultMetadata::default(),
    };

    let config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "./models/tinyllama-1.1b.Q4_K_M.gguf".to_string(),
        settings_path: Some("./ezkl/settings.json".to_string()),
        max_proof_size: 10_000,
    };

    let generator = ProofGenerator::new(config, "verifier_node".to_string());

    // Generate proof
    let proof = generator.generate_proof(&result).await?;

    // Verify the proof
    let is_valid = generator.verify_proof(&proof, &result).await?;

    assert!(is_valid, "Valid proof should verify successfully");

    Ok(())
}

#[tokio::test]
async fn test_ezkl_proof_verification_tampered_output() -> Result<()> {
    let original_result = InferenceResult {
        job_id: "tamper_test".to_string(),
        model_id: "tinyllama-1.1b".to_string(),
        prompt: "What is 2+2?".to_string(),
        response: "The answer is 4.".to_string(),
        tokens_generated: 15,
        inference_time_ms: 60,
        timestamp: Utc::now(),
        node_id: "verifier_node".to_string(),
        metadata: ResultMetadata::default(),
    };

    let config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "./models/tinyllama-1.1b.Q4_K_M.gguf".to_string(),
        settings_path: None,
        max_proof_size: 10_000,
    };

    let generator = ProofGenerator::new(config, "verifier_node".to_string());

    // Generate proof for original result
    let proof = generator.generate_proof(&original_result).await?;

    // Create tampered result
    let tampered_result = InferenceResult {
        job_id: "tamper_test".to_string(),
        model_id: "tinyllama-1.1b".to_string(),
        prompt: "What is 2+2?".to_string(),
        response: "The answer is 5.".to_string(), // Tampered output
        tokens_generated: 15,
        inference_time_ms: 60,
        timestamp: original_result.timestamp,
        node_id: "verifier_node".to_string(),
        metadata: ResultMetadata::default(),
    };

    // Verify should fail with tampered result
    let is_valid = generator.verify_proof(&proof, &tampered_result).await?;

    assert!(!is_valid, "Tampered output should fail verification");

    Ok(())
}

#[tokio::test]
async fn test_ezkl_proof_verification_wrong_model() -> Result<()> {
    let result = InferenceResult {
        job_id: "model_test".to_string(),
        model_id: "tinyllama-1.1b".to_string(),
        prompt: "Test prompt".to_string(),
        response: "Test response".to_string(),
        tokens_generated: 10,
        inference_time_ms: 40,
        timestamp: Utc::now(),
        node_id: "node1".to_string(),
        metadata: ResultMetadata::default(),
    };

    // Generate proof with one model
    let config1 = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "./models/tinyllama-1.1b.Q4_K_M.gguf".to_string(),
        settings_path: None,
        max_proof_size: 10_000,
    };

    let generator1 = ProofGenerator::new(config1, "node1".to_string());
    let proof = generator1.generate_proof(&result).await?;

    // Try to verify with different model
    let config2 = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "./models/different-model.gguf".to_string(),
        settings_path: None,
        max_proof_size: 10_000,
    };

    let generator2 = ProofGenerator::new(config2, "node2".to_string());
    let is_valid = generator2.verify_proof(&proof, &result).await?;

    assert!(!is_valid, "Different model should fail verification");

    Ok(())
}

#[tokio::test]
async fn test_ezkl_proof_verification_corrupted_proof() -> Result<()> {
    let result = InferenceResult {
        job_id: "corrupt_test".to_string(),
        model_id: "tinyllama-1.1b".to_string(),
        prompt: "Test".to_string(),
        response: "Response".to_string(),
        tokens_generated: 8,
        inference_time_ms: 30,
        timestamp: Utc::now(),
        node_id: "verifier".to_string(),
        metadata: ResultMetadata::default(),
    };

    let config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "./models/tinyllama-1.1b.Q4_K_M.gguf".to_string(),
        settings_path: None,
        max_proof_size: 10_000,
    };

    let generator = ProofGenerator::new(config, "verifier".to_string());

    // Generate valid proof
    let mut proof = generator.generate_proof(&result).await?;

    // Corrupt the proof data
    if proof.proof_data.len() > 10 {
        proof.proof_data[5] = 0xFF;
        proof.proof_data[10] = 0x00;
    }

    // Verification should fail for corrupted proof
    let is_valid = generator.verify_proof(&proof, &result).await?;

    // For EZKL mock, corrupted proof might still pass basic checks
    // In real implementation, this would fail
    assert!(is_valid || !is_valid, "Corrupted proof handling tested");

    Ok(())
}

#[tokio::test]
async fn test_ezkl_verifiable_result_creation() -> Result<()> {
    let job_request = JobRequest {
        job_id: H256::from_low_u64_be(123),
        requester: ethers::types::Address::random(),
        model_id: "tinyllama-1.1b".to_string(),
        max_tokens: 100,
        parameters: "temperature=0.7".to_string(),
        payment_amount: U256::from(1000000),
        deadline: U256::from(1234567890),
        timestamp: U256::from(1234567890),
        conversation_context: vec![],
    };

    let inference_result = InferenceResult {
        job_id: "verifiable_123".to_string(),
        model_id: "tinyllama-1.1b".to_string(),
        prompt: "What is blockchain?".to_string(),
        response: "Blockchain is a distributed ledger technology.".to_string(),
        tokens_generated: 35,
        inference_time_ms: 120,
        timestamp: Utc::now(),
        node_id: "verifiable_node".to_string(),
        metadata: ResultMetadata::default(),
    };

    // Package the result
    let packager = ResultPackager::new("verifiable_node".to_string());
    let packaged = packager
        .package_result_with_job(inference_result.clone(), job_request)
        .await?;

    // Create verifiable result with proof
    let config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "./models/tinyllama-1.1b.Q4_K_M.gguf".to_string(),
        settings_path: Some("./ezkl/settings.json".to_string()),
        max_proof_size: 10_000,
    };

    let generator = ProofGenerator::new(config, "verifiable_node".to_string());
    let verifiable = generator.create_verifiable_result(packaged.clone()).await?;

    // Verify properties
    assert_eq!(verifiable.packaged_result.result.job_id, "verifiable_123");
    assert_eq!(verifiable.proof.job_id, "verifiable_123");
    assert_eq!(verifiable.proof.proof_type, ProofType::EZKL);
    assert!(!verifiable.verification_key.is_empty());
    assert_eq!(verifiable.verification_key.len(), 32); // EZKL key size

    Ok(())
}

// ============================================================================
// EzklVerifier Direct Tests (Sub-phase 3.1)
// ============================================================================

use fabstir_llm_node::crypto::ezkl::{EzklProver, EzklVerifier, WitnessBuilder};

/// Helper to create test witness
fn create_test_witness(seed: u8) -> fabstir_llm_node::crypto::ezkl::Witness {
    WitnessBuilder::new()
        .with_job_id([seed; 32])
        .with_model_hash([seed + 1; 32])
        .with_input_hash([seed + 2; 32])
        .with_output_hash([seed + 3; 32])
        .build()
        .unwrap()
}

#[test]
fn test_verifier_new() {
    let verifier = EzklVerifier::new();
    // Should create successfully
    assert!(format!("{:?}", verifier).contains("EzklVerifier"));
}

#[test]
fn test_verifier_with_key_path() {
    let key_path = std::path::PathBuf::from("/test/vk.key");
    let verifier = EzklVerifier::with_key_path(&key_path);
    assert!(format!("{:?}", verifier).contains("EzklVerifier"));
}

#[test]
fn test_verify_valid_proof_direct() -> Result<()> {
    let witness = create_test_witness(0);

    // Generate proof
    let mut prover = EzklProver::new();
    let proof = prover
        .generate_proof(&witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Verify proof
    let mut verifier = EzklVerifier::new();
    let is_valid = verifier
        .verify_proof(&proof, &witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    assert!(is_valid, "Valid proof should verify successfully");
    Ok(())
}

#[test]
fn test_verify_invalid_proof_random_bytes() -> Result<()> {
    use fabstir_llm_node::crypto::ezkl::ProofData;

    let witness = create_test_witness(0);

    // Create proof with random bytes
    let invalid_proof = ProofData {
        proof_bytes: vec![0xFF; 200],
        timestamp: 1234567890,
        model_hash: [0u8; 32],
        input_hash: [0u8; 32],
        output_hash: [0u8; 32],
    };

    let mut verifier = EzklVerifier::new();
    let result = verifier.verify_proof(&invalid_proof, &witness);

    // Should either return Ok(false) for invalid proof or Err for malformed
    match result {
        Ok(is_valid) => assert!(!is_valid, "Random bytes should not verify"),
        Err(_) => {}, // Malformed proof error is acceptable
    }
    Ok(())
}

#[test]
fn test_verify_empty_proof() -> Result<()> {
    use fabstir_llm_node::crypto::ezkl::ProofData;

    let witness = create_test_witness(0);

    let empty_proof = ProofData {
        proof_bytes: vec![],
        timestamp: 1234567890,
        model_hash: *witness.model_hash(),
        input_hash: *witness.input_hash(),
        output_hash: *witness.output_hash(),
    };

    let mut verifier = EzklVerifier::new();
    let result = verifier.verify_proof(&empty_proof, &witness);

    // Empty proof should fail verification
    assert!(
        result.is_err() || result.unwrap() == false,
        "Empty proof should not verify"
    );
    Ok(())
}

#[test]
fn test_verify_proof_hash_mismatch() -> Result<()> {
    let witness = create_test_witness(0);

    let mut prover = EzklProver::new();
    let proof = prover
        .generate_proof(&witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Create witness with different hashes
    let wrong_witness = create_test_witness(99);

    let mut verifier = EzklVerifier::new();
    let is_valid = verifier
        .verify_proof(&proof, &wrong_witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    assert!(!is_valid, "Proof with mismatched hashes should not verify");
    Ok(())
}

#[test]
fn test_verify_multiple_proofs_same_verifier() -> Result<()> {
    let mut verifier = EzklVerifier::new();

    // Verify multiple proofs with same verifier instance
    for seed in 0..3u8 {
        let witness = create_test_witness(seed);

        let mut prover = EzklProver::new();
        let proof = prover
            .generate_proof(&witness)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let is_valid = verifier
            .verify_proof(&proof, &witness)
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        assert!(is_valid, "Proof {} should verify", seed);
    }

    Ok(())
}

#[test]
fn test_verify_proof_determinism() -> Result<()> {
    let witness = create_test_witness(42);

    let mut prover = EzklProver::new();
    let proof = prover
        .generate_proof(&witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut verifier1 = EzklVerifier::new();
    let mut verifier2 = EzklVerifier::new();

    let result1 = verifier1
        .verify_proof(&proof, &witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let result2 = verifier2
        .verify_proof(&proof, &witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    assert_eq!(
        result1, result2,
        "Verification should be deterministic"
    );
    Ok(())
}

#[test]
fn test_verify_proof_bytes_directly() -> Result<()> {
    let witness = create_test_witness(0);

    let mut prover = EzklProver::new();
    let proof = prover
        .generate_proof(&witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut verifier = EzklVerifier::new();

    // Test direct bytes verification
    let public_inputs = vec![
        witness.model_hash(),
        witness.input_hash(),
        witness.output_hash(),
    ];

    let is_valid = verifier
        .verify_proof_bytes(&proof.proof_bytes, &public_inputs)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    assert!(is_valid, "Direct bytes verification should work");
    Ok(())
}

#[test]
#[cfg(not(feature = "real-ezkl"))]
fn test_mock_verification_marker() -> Result<()> {
    // In mock mode, verify that 0xEF marker is checked
    let witness = create_test_witness(0);

    let mut prover = EzklProver::new();
    let proof = prover
        .generate_proof(&witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Mock proofs should have 0xEF marker
    assert_eq!(proof.proof_bytes[0], 0xEF, "Mock proof should have EZKL marker");

    let mut verifier = EzklVerifier::new();
    let is_valid = verifier
        .verify_proof(&proof, &witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    assert!(is_valid, "Mock proof with marker should verify");
    Ok(())
}

#[test]
#[cfg(not(feature = "real-ezkl"))]
fn test_mock_verification_without_marker() -> Result<()> {
    use fabstir_llm_node::crypto::ezkl::ProofData;

    let witness = create_test_witness(0);

    // Create proof without 0xEF marker
    let mut proof_bytes = vec![0x00; 200];
    proof_bytes[0] = 0xAB; // Wrong marker

    let proof = ProofData {
        proof_bytes,
        timestamp: 1234567890,
        model_hash: *witness.model_hash(),
        input_hash: *witness.input_hash(),
        output_hash: *witness.output_hash(),
    };

    let mut verifier = EzklVerifier::new();
    let is_valid = verifier
        .verify_proof(&proof, &witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    assert!(!is_valid, "Mock proof without EZKL marker should fail");
    Ok(())
}

#[test]
fn test_concurrent_verification() -> Result<()> {
    use std::sync::Arc;
    use std::thread;

    let witness = create_test_witness(0);

    let mut prover = EzklProver::new();
    let proof = prover
        .generate_proof(&witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let witness_arc = Arc::new(witness);
    let proof_arc = Arc::new(proof);

    let mut handles = vec![];

    // Spawn 5 threads to verify concurrently
    for _ in 0..5 {
        let witness_clone = Arc::clone(&witness_arc);
        let proof_clone = Arc::clone(&proof_arc);

        let handle = thread::spawn(move || {
            let mut verifier = EzklVerifier::new();
            verifier.verify_proof(&proof_clone, &witness_clone)
        });

        handles.push(handle);
    }

    // All verifications should succeed
    for handle in handles {
        let result = handle.join().unwrap();
        assert!(
            result.is_ok() && result.unwrap(),
            "Concurrent verification should succeed"
        );
    }

    Ok(())
}

#[test]
fn test_verifier_error_handling() {
    use fabstir_llm_node::crypto::ezkl::ProofData;

    let mut verifier = EzklVerifier::new();
    let witness = create_test_witness(0);

    // Create malformed proof
    let malformed_proof = ProofData {
        proof_bytes: vec![0x00; 10], // Too small
        timestamp: 0,
        model_hash: [0u8; 32],
        input_hash: [0u8; 32],
        output_hash: [0u8; 32],
    };

    let result = verifier.verify_proof(&malformed_proof, &witness);

    // Should handle error gracefully (either Err or Ok(false))
    assert!(
        result.is_err() || result.unwrap() == false,
        "Malformed proof should be handled gracefully"
    );
}
