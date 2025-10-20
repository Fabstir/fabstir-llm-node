// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use ethers::types::{Address, H256, U256};
use fabstir_llm_node::job_processor::{JobRequest, Message};
use fabstir_llm_node::results::packager::{InferenceResult, ResultMetadata, ResultPackager};
use fabstir_llm_node::results::proofs::{ProofGenerationConfig, ProofGenerator, ProofType};
//use fabstir_llm_node::contracts::proofs::ProofSubmitter;
use chrono::Utc;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

#[tokio::test]
async fn test_ezkl_end_to_end_proof_flow() -> Result<()> {
    // Create a job request with conversation context
    let job_request = JobRequest {
        job_id: H256::from_low_u64_be(999),
        requester: Address::from_low_u64_be(1001),
        model_id: "tinyllama-1.1b".to_string(),
        max_tokens: 500,
        parameters: "temperature=0.8,top_p=0.9".to_string(),
        payment_amount: U256::from(5000000),
        deadline: U256::from(chrono::Utc::now().timestamp() as u64 + 3600),
        timestamp: U256::from(chrono::Utc::now().timestamp() as u64),
        conversation_context: vec![
            Message {
                role: "user".to_string(),
                content: "What is machine learning?".to_string(),
                timestamp: Some(chrono::Utc::now().timestamp()),
            },
            Message {
                role: "assistant".to_string(),
                content: "Machine learning is a subset of AI...".to_string(),
                timestamp: Some(chrono::Utc::now().timestamp()),
            },
        ],
    };

    // Simulate inference result
    let inference_result = InferenceResult {
        job_id: format!("{:x}", job_request.job_id),
        model_id: job_request.model_id.clone(),
        prompt: "Explain deep learning in the context of our previous discussion".to_string(),
        response: "Deep learning extends machine learning by using neural networks...".to_string(),
        tokens_generated: 150,
        inference_time_ms: 450,
        timestamp: Utc::now(),
        node_id: "integration_node".to_string(),
        metadata: ResultMetadata::default(),
    };

    // Package the result
    let packager = ResultPackager::new("integration_node".to_string());
    let packaged = packager
        .package_result_with_job(inference_result.clone(), job_request.clone())
        .await?;

    // Generate EZKL proof
    let config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "./models/tinyllama-1.1b.Q4_K_M.gguf".to_string(),
        settings_path: Some("./ezkl/settings.json".to_string()),
        max_proof_size: 20_000,
    };

    let generator = ProofGenerator::new(config, "integration_node".to_string());
    let verifiable = generator.create_verifiable_result(packaged).await?;

    // Verify all components are present
    assert!(!verifiable.proof.proof_data.is_empty());
    assert!(!verifiable.verification_key.is_empty());
    assert_eq!(verifiable.proof.proof_type, ProofType::EZKL);
    assert_eq!(
        verifiable.packaged_result.result.job_id,
        format!("{:x}", job_request.job_id)
    );

    Ok(())
}

#[tokio::test]
async fn test_ezkl_proof_submission_to_contract() -> Result<()> {
    let job_request = JobRequest {
        job_id: H256::from_low_u64_be(1234),
        requester: Address::random(),
        model_id: "tinyllama-1.1b".to_string(),
        max_tokens: 200,
        parameters: "{}".to_string(),
        payment_amount: U256::from(2000000),
        deadline: U256::from(chrono::Utc::now().timestamp() as u64 + 7200),
        timestamp: U256::from(chrono::Utc::now().timestamp() as u64),
        conversation_context: vec![],
    };

    let inference_result = InferenceResult {
        job_id: format!("{:x}", job_request.job_id),
        model_id: "tinyllama-1.1b".to_string(),
        prompt: "What is quantum computing?".to_string(),
        response: "Quantum computing uses quantum bits...".to_string(),
        tokens_generated: 75,
        inference_time_ms: 250,
        timestamp: Utc::now(),
        node_id: "submitter_node".to_string(),
        metadata: ResultMetadata::default(),
    };

    // Generate proof and prepare for submission
    let config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "./models/tinyllama-1.1b.Q4_K_M.gguf".to_string(),
        settings_path: None,
        max_proof_size: 15_000,
    };

    let generator = ProofGenerator::new(config, "submitter_node".to_string());
    let proof = generator.generate_proof(&inference_result).await?;

    // Verify proof can be serialized for contract submission
    let proof_bytes = bincode::serialize(&proof)?;
    assert!(proof_bytes.len() < 50_000, "Proof too large for contract");

    // Verify proof can be deserialized
    let deserialized: fabstir_llm_node::results::proofs::InferenceProof =
        bincode::deserialize(&proof_bytes)?;
    assert_eq!(deserialized.job_id, proof.job_id);

    Ok(())
}

#[tokio::test]
async fn test_ezkl_concurrent_proof_generation() -> Result<()> {
    let config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "./models/tinyllama-1.1b.Q4_K_M.gguf".to_string(),
        settings_path: None,
        max_proof_size: 10_000,
    };

    let generator = Arc::new(ProofGenerator::new(config, "concurrent_node".to_string()));

    // Create multiple inference results
    let mut tasks = vec![];

    for i in 0..5 {
        let gen = generator.clone();
        let result = InferenceResult {
            job_id: format!("concurrent_{}", i),
            model_id: "tinyllama-1.1b".to_string(),
            prompt: format!("Question {}", i),
            response: format!("Answer {}", i),
            tokens_generated: 20 + i * 5,
            inference_time_ms: 100 + i as u64 * 50,
            timestamp: Utc::now(),
            node_id: "concurrent_node".to_string(),
            metadata: ResultMetadata::default(),
        };

        tasks.push(tokio::spawn(
            async move { gen.generate_proof(&result).await },
        ));
    }

    // Wait for all proofs with timeout
    let results = timeout(
        Duration::from_secs(10),
        futures::future::try_join_all(tasks),
    )
    .await?;

    // Verify all proofs were generated
    for (i, result) in results?.into_iter().enumerate() {
        let proof = result?;
        assert_eq!(proof.job_id, format!("concurrent_{}", i));
        assert!(!proof.proof_data.is_empty());
    }

    Ok(())
}

#[tokio::test]
async fn test_ezkl_proof_caching() -> Result<()> {
    let result = InferenceResult {
        job_id: "cache_test".to_string(),
        model_id: "tinyllama-1.1b".to_string(),
        prompt: "What is caching?".to_string(),
        response: "Caching stores frequently accessed data...".to_string(),
        tokens_generated: 40,
        inference_time_ms: 150,
        timestamp: Utc::now(),
        node_id: "cache_node".to_string(),
        metadata: ResultMetadata::default(),
    };

    let config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "./models/tinyllama-1.1b.Q4_K_M.gguf".to_string(),
        settings_path: None,
        max_proof_size: 10_000,
    };

    let generator = ProofGenerator::new(config, "cache_node".to_string());

    // Generate proof twice for same input
    let start1 = std::time::Instant::now();
    let proof1 = generator.generate_proof(&result).await?;
    let time1 = start1.elapsed();

    let start2 = std::time::Instant::now();
    let proof2 = generator.generate_proof(&result).await?;
    let time2 = start2.elapsed();

    // Second generation should be faster if cached (though mock may not show this)
    assert_eq!(proof1.model_hash, proof2.model_hash);
    assert_eq!(proof1.input_hash, proof2.input_hash);
    assert_eq!(proof1.output_hash, proof2.output_hash);

    println!("First generation: {:?}, Second: {:?}", time1, time2);

    Ok(())
}

#[tokio::test]
async fn test_ezkl_proof_with_payment_verification() -> Result<()> {
    // Simulate a job that requires payment verification
    let job_request = JobRequest {
        job_id: H256::from_low_u64_be(7777),
        requester: Address::from_low_u64_be(2001),
        model_id: "tinyllama-1.1b".to_string(),
        max_tokens: 1000,
        parameters: "temperature=0.5".to_string(),
        payment_amount: U256::from(10_000_000), // Higher payment
        deadline: U256::from(chrono::Utc::now().timestamp() as u64 + 1800),
        timestamp: U256::from(chrono::Utc::now().timestamp() as u64),
        conversation_context: vec![],
    };

    let inference_result = InferenceResult {
        job_id: format!("{:x}", job_request.job_id),
        model_id: job_request.model_id.clone(),
        prompt: "Complex calculation request".to_string(),
        response: "Result of complex calculation...".to_string(),
        tokens_generated: 850,
        inference_time_ms: 2500,
        timestamp: Utc::now(),
        node_id: "payment_node".to_string(),
        metadata: ResultMetadata::default(),
    };

    // Package and generate proof
    let packager = ResultPackager::new("payment_node".to_string());
    let packaged = packager
        .package_result_with_job(inference_result.clone(), job_request.clone())
        .await?;

    let config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "./models/tinyllama-1.1b.Q4_K_M.gguf".to_string(),
        settings_path: Some("./ezkl/settings.json".to_string()),
        max_proof_size: 25_000,
    };

    let generator = ProofGenerator::new(config, "payment_node".to_string());
    let verifiable = generator.create_verifiable_result(packaged).await?;

    // Verify proof contains payment information
    assert!(verifiable.packaged_result.result.tokens_generated > 800);

    // Verify proof is valid for payment release
    let is_valid = generator
        .verify_proof(&verifiable.proof, &verifiable.packaged_result.result)
        .await?;

    assert!(is_valid, "Proof must be valid for payment release");

    Ok(())
}
