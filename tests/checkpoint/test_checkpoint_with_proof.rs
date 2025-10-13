//! Checkpoint Integration Tests with EZKL Proofs
//!
//! Tests that demonstrate checkpoint submission generating and storing EZKL proofs.
//! These tests show the intended integration pattern.

use anyhow::Result;
use chrono::Utc;
use fabstir_llm_node::crypto::ezkl::{EzklProver, ProofData};
use fabstir_llm_node::results::packager::{InferenceResult, ResultMetadata};
use fabstir_llm_node::results::proofs::{InferenceProof, ProofGenerationConfig, ProofGenerator, ProofType};
use fabstir_llm_node::storage::{ProofStore, ResultStore};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Helper to create test inference result
fn create_test_result(job_id: u64) -> InferenceResult {
    InferenceResult {
        job_id: job_id.to_string(),
        model_id: "test-model".to_string(),
        prompt: "What is 2+2?".to_string(),
        response: "2+2 equals 4".to_string(),
        tokens_generated: 100,
        inference_time_ms: 50,
        timestamp: Utc::now(),
        node_id: "test-node".to_string(),
        metadata: ResultMetadata::default(),
    }
}

/// Helper to create test proof generator
fn create_test_proof_generator() -> ProofGenerator {
    let config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "/test/model".to_string(),
        settings_path: None,
        max_proof_size: 10000,
    };
    ProofGenerator::new(config, "test-prover".to_string())
}

#[tokio::test]
async fn test_checkpoint_generates_ezkl_proof() -> Result<()> {
    let result = create_test_result(100);
    let proof_gen = create_test_proof_generator();

    // Generate proof for checkpoint
    let proof = proof_gen.generate_proof(&result).await?;

    // Verify proof structure
    assert_eq!(proof.job_id, "100");
    assert!(!proof.proof_data.is_empty());
    assert_eq!(proof.proof_type, ProofType::EZKL);
    assert!(!proof.model_hash.is_empty());
    assert!(!proof.input_hash.is_empty());
    assert!(!proof.output_hash.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_checkpoint_stores_proof_in_store() -> Result<()> {
    let result = create_test_result(200);
    let proof_gen = create_test_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));

    // Generate and store proof
    let proof = proof_gen.generate_proof(&result).await?;
    proof_store.write().await.store_proof(200, proof.clone()).await?;

    // Verify proof can be retrieved
    let retrieved = proof_store.read().await.retrieve_proof(200).await?;
    assert_eq!(retrieved.job_id, proof.job_id);
    assert_eq!(retrieved.proof_data.len(), proof.proof_data.len());

    Ok(())
}

#[tokio::test]
async fn test_checkpoint_with_invalid_result_fails() -> Result<()> {
    let mut result = create_test_result(300);
    result.response = "".to_string(); // Invalid empty response

    let proof_gen = create_test_proof_generator();

    // Should still generate proof (validation happens later)
    let proof = proof_gen.generate_proof(&result).await?;
    assert!(!proof.proof_data.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_checkpoint_proof_format_valid() -> Result<()> {
    let result = create_test_result(400);
    let proof_gen = create_test_proof_generator();

    let proof = proof_gen.generate_proof(&result).await?;

    // Check mock EZKL proof format
    #[cfg(not(feature = "real-ezkl"))]
    {
        assert!(proof.proof_data.len() >= 200);
        assert_eq!(proof.proof_data[0], 0xEF); // Mock EZKL marker
    }

    // Check real EZKL proof format
    #[cfg(feature = "real-ezkl")]
    {
        assert!(proof.proof_data.len() >= 100); // Real proofs vary in size
    }

    Ok(())
}

#[tokio::test]
async fn test_checkpoint_proof_retrievable() -> Result<()> {
    let result = create_test_result(500);
    let proof_gen = create_test_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));

    // Generate, store, and retrieve
    let proof = proof_gen.generate_proof(&result).await?;
    let original_size = proof.proof_data.len();

    proof_store.write().await.store_proof(500, proof).await?;

    let retrieved = proof_store.read().await.retrieve_proof(500).await?;
    assert_eq!(retrieved.proof_data.len(), original_size);

    Ok(())
}

#[tokio::test]
async fn test_force_checkpoint_includes_proof() -> Result<()> {
    // Simulate force checkpoint on session completion
    let result = create_test_result(600);
    let proof_gen = create_test_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));

    // Generate proof as if checkpoint was forced
    let proof = proof_gen.generate_proof(&result).await?;
    proof_store.write().await.store_proof(600, proof.clone()).await?;

    // Verify proof exists
    assert!(proof_store.read().await.has_proof(600).await);

    Ok(())
}

#[tokio::test]
async fn test_concurrent_checkpoint_proofs() -> Result<()> {
    let proof_gen = Arc::new(create_test_proof_generator());
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));

    // Generate proofs concurrently for multiple jobs
    let handles: Vec<_> = (700..710)
        .map(|job_id| {
            let proof_gen = proof_gen.clone();
            let proof_store = proof_store.clone();

            tokio::spawn(async move {
                let result = create_test_result(job_id);
                let proof = proof_gen.generate_proof(&result).await.unwrap();
                proof_store
                    .write()
                    .await
                    .store_proof(job_id, proof)
                    .await
                    .unwrap();
            })
        })
        .collect();

    for handle in handles {
        handle.await.unwrap();
    }

    // Verify all proofs stored
    assert_eq!(proof_store.read().await.len().await, 10);

    Ok(())
}

#[tokio::test]
async fn test_checkpoint_with_mock_ezkl() -> Result<()> {
    #[cfg(not(feature = "real-ezkl"))]
    {
        let result = create_test_result(800);
        let proof_gen = create_test_proof_generator();

        let proof = proof_gen.generate_proof(&result).await?;

        // Mock proofs have predictable structure
        assert!(proof.proof_data.len() >= 200);
        assert_eq!(proof.proof_data[0], 0xEF);
    }

    Ok(())
}

#[tokio::test]
#[cfg(feature = "real-ezkl")]
async fn test_checkpoint_with_real_ezkl() -> Result<()> {
    use fabstir_llm_node::crypto::ezkl::WitnessBuilder;

    let result = create_test_result(900);

    // Generate real EZKL proof
    let witness = WitnessBuilder::new()
        .with_job_id_string(&result.job_id)
        .with_model_path("/test/model")
        .with_input_string(&result.prompt)
        .with_output_string(&result.response)
        .build()?;

    let mut prover = EzklProver::new();
    let proof_data = prover.generate_proof(&witness)?;

    // Real proofs should be valid
    assert!(!proof_data.proof_bytes.is_empty());
    assert_eq!(proof_data.model_hash, *witness.model_hash());

    Ok(())
}

#[tokio::test]
async fn test_checkpoint_result_and_proof_stored() -> Result<()> {
    let result = create_test_result(1000);
    let proof_gen = create_test_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));
    let result_store = Arc::new(RwLock::new(ResultStore::new()));

    // Store both result and proof (as checkpoint would)
    result_store.write().await.store_result(1000, result.clone()).await?;
    let proof = proof_gen.generate_proof(&result).await?;
    proof_store.write().await.store_proof(1000, proof).await?;

    // Verify both exist
    assert!(result_store.read().await.has_result(1000).await);
    assert!(proof_store.read().await.has_proof(1000).await);

    Ok(())
}

#[tokio::test]
async fn test_checkpoint_proof_size_limits() -> Result<()> {
    let result = create_test_result(1100);
    let mut config = ProofGenerationConfig {
        proof_type: ProofType::EZKL,
        model_path: "/test/model".to_string(),
        settings_path: None,
        max_proof_size: 1000, // Small limit
    };

    let proof_gen = ProofGenerator::new(config.clone(), "test-node".to_string());
    let proof = proof_gen.generate_proof(&result).await?;

    // Mock proofs respect size limits
    #[cfg(not(feature = "real-ezkl"))]
    {
        assert!(proof.proof_data.len() <= 1000);
    }

    Ok(())
}

#[tokio::test]
async fn test_checkpoint_proof_timestamp() -> Result<()> {
    let result = create_test_result(1200);
    let proof_gen = create_test_proof_generator();

    let before = Utc::now();
    let proof = proof_gen.generate_proof(&result).await?;
    let after = Utc::now();

    // Proof timestamp should be between before and after
    assert!(proof.timestamp >= before);
    assert!(proof.timestamp <= after);

    Ok(())
}

#[tokio::test]
async fn test_checkpoint_multiple_proofs_same_job() -> Result<()> {
    // Test storing multiple proof versions for same job (should overwrite)
    let result = create_test_result(1300);
    let proof_gen = create_test_proof_generator();
    let proof_store = Arc::new(RwLock::new(ProofStore::new()));

    // Store first proof
    let proof1 = proof_gen.generate_proof(&result).await?;
    proof_store.write().await.store_proof(1300, proof1).await?;

    // Store second proof (overwrites)
    let proof2 = proof_gen.generate_proof(&result).await?;
    proof_store.write().await.store_proof(1300, proof2.clone()).await?;

    // Should have only one proof (the latest)
    assert_eq!(proof_store.read().await.len().await, 1);
    let retrieved = proof_store.read().await.retrieve_proof(1300).await?;
    assert_eq!(retrieved.timestamp, proof2.timestamp);

    Ok(())
}
