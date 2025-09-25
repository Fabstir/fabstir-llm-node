use anyhow::Result;
use ethers::types::H256;
use fabstir_llm_node::results::packager::{
    InferenceResult, PackagedResult, ResultMetadata, ResultPackager,
};
use fabstir_llm_node::results::proofs::{ProofGenerationConfig, ProofGenerator, ProofType};
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::timeout;

#[tokio::test]
async fn test_ezkl_proof_generation_basic() -> Result<()> {
    // Create test inference result
    let result = InferenceResult {
        job_id: "test_job_123".to_string(),
        model_id: "tinyllama-1.1b".to_string(),
        prompt: "What is 2+2?".to_string(),
        response: "The answer is 4.".to_string(),
        tokens_generated: 25,
        inference_time_ms: 150,
        timestamp: chrono::Utc::now(),
        node_id: "test_node_1".to_string(),
        metadata: ResultMetadata::default(),
    };

    // Create EZKL proof generator
    let config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "./models/tinyllama-1.1b.Q4_K_M.gguf".to_string(),
        settings_path: Some("./ezkl/settings.json".to_string()),
        max_proof_size: 10_000,
    };

    let generator = ProofGenerator::new(config, "test_node_1".to_string());

    // Generate proof
    let proof = generator.generate_proof(&result).await?;

    // Verify proof properties
    assert_eq!(proof.job_id, "test_job_123");
    assert_eq!(proof.proof_type, ProofType::EZKL);
    assert!(!proof.proof_data.is_empty());
    assert!(proof.proof_data.len() <= 10_000);
    assert!(!proof.model_hash.is_empty());
    assert!(!proof.input_hash.is_empty());
    assert!(!proof.output_hash.is_empty());
    assert_eq!(proof.prover_id, "test_node_1");

    Ok(())
}

#[tokio::test]
async fn test_ezkl_proof_generation_with_large_output() -> Result<()> {
    // Create result with large output
    let large_response = "x".repeat(10_000);
    let result = InferenceResult {
        job_id: "test_job_large".to_string(),
        model_id: "tinyllama-1.1b".to_string(),
        prompt: "Generate a large response".to_string(),
        response: large_response,
        tokens_generated: 5000,
        inference_time_ms: 2500,
        timestamp: chrono::Utc::now(),
        node_id: "test_node_2".to_string(),
        metadata: ResultMetadata::default(),
    };

    let config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "./models/tinyllama-1.1b.Q4_K_M.gguf".to_string(),
        settings_path: None,
        max_proof_size: 5_000,
    };

    let generator = ProofGenerator::new(config, "test_node_2".to_string());

    // Generate proof with size constraint
    let proof = generator.generate_proof(&result).await?;

    assert!(proof.proof_data.len() <= 5_000);
    assert_eq!(proof.proof_type, ProofType::EZKL);

    Ok(())
}

#[tokio::test]
async fn test_ezkl_proof_generation_timeout() -> Result<()> {
    let result = InferenceResult {
        job_id: "test_timeout".to_string(),
        model_id: "tinyllama-1.1b".to_string(),
        prompt: "Test prompt".to_string(),
        response: "Test response".to_string(),
        tokens_generated: 10,
        inference_time_ms: 50,
        timestamp: chrono::Utc::now(),
        node_id: "test_node_timeout".to_string(),
        metadata: ResultMetadata::default(),
    };

    let config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "./models/tinyllama-1.1b.Q4_K_M.gguf".to_string(),
        settings_path: Some("./ezkl/settings.json".to_string()),
        max_proof_size: 10_000,
    };

    let generator = ProofGenerator::new(config, "test_node_timeout".to_string());

    // Test with reasonable timeout (should succeed)
    let result_with_timeout =
        timeout(Duration::from_secs(5), generator.generate_proof(&result)).await;

    assert!(result_with_timeout.is_ok());

    Ok(())
}

#[tokio::test]
async fn test_ezkl_proof_determinism() -> Result<()> {
    let result = InferenceResult {
        job_id: "test_determinism".to_string(),
        model_id: "tinyllama-1.1b".to_string(),
        prompt: "What is the capital of France?".to_string(),
        response: "The capital of France is Paris.".to_string(),
        tokens_generated: 30,
        inference_time_ms: 100,
        timestamp: chrono::Utc::now(),
        node_id: "test_node".to_string(),
        metadata: ResultMetadata::default(),
    };

    let config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "./models/tinyllama-1.1b.Q4_K_M.gguf".to_string(),
        settings_path: None,
        max_proof_size: 10_000,
    };

    let generator = ProofGenerator::new(config.clone(), "test_node".to_string());

    // Generate two proofs for same input
    let proof1 = generator.generate_proof(&result).await?;
    let proof2 = generator.generate_proof(&result).await?;

    // Hashes should be identical for same input
    assert_eq!(proof1.model_hash, proof2.model_hash);
    assert_eq!(proof1.input_hash, proof2.input_hash);
    assert_eq!(proof1.output_hash, proof2.output_hash);

    Ok(())
}

#[tokio::test]
async fn test_ezkl_proof_with_invalid_model_path() -> Result<()> {
    let result = InferenceResult {
        job_id: "test_invalid".to_string(),
        model_id: "nonexistent".to_string(),
        prompt: "Test".to_string(),
        response: "Response".to_string(),
        tokens_generated: 5,
        inference_time_ms: 10,
        timestamp: chrono::Utc::now(),
        node_id: "test_node".to_string(),
        metadata: ResultMetadata::default(),
    };

    let config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "./models/nonexistent.gguf".to_string(),
        settings_path: None,
        max_proof_size: 10_000,
    };

    let generator = ProofGenerator::new(config, "test_node".to_string());

    // Should still generate proof even with invalid model path (using path hash)
    let proof = generator.generate_proof(&result).await?;
    assert!(!proof.proof_data.is_empty());

    Ok(())
}
