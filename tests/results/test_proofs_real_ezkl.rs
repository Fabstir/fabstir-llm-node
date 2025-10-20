// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Integration Tests for Real EZKL Proofs with Results System
//!
//! Tests the integration between the proof generation system
//! and the inference results packaging.

use anyhow::Result;
use fabstir_llm_node::crypto::ezkl::{CommitmentCircuit, WitnessBuilder};
use fabstir_llm_node::results::packager::InferenceResult;
use std::time::SystemTime;

/// Helper function to create test InferenceResult
fn create_test_result() -> InferenceResult {
    InferenceResult {
        job_id: "test_job_123".to_string(),
        prompt: "What is 2+2?".to_string(),
        response: "The answer is 4".to_string(),
        model: "test-model-7b".to_string(),
        timestamp: SystemTime::now(),
        tokens_generated: 10,
        generation_time_ms: 500,
        metadata: std::collections::HashMap::new(),
    }
}

#[test]
fn test_witness_from_inference_result() -> Result<()> {
    // Test creating witness from InferenceResult
    let result = create_test_result();

    let witness = WitnessBuilder::new()
        .with_job_id_string(&result.job_id)
        .with_model_path(&result.model)
        .with_input_string(&result.prompt)
        .with_output_string(&result.response)
        .build()?;

    assert!(witness.is_valid());
    assert!(!witness.is_empty());

    // Verify witness fields match result data
    // (We can't directly compare hashes, but we can verify they're non-zero)
    assert_ne!(witness.job_id(), &[0u8; 32]);
    assert_ne!(witness.model_hash(), &[0u8; 32]);
    assert_ne!(witness.input_hash(), &[0u8; 32]);
    assert_ne!(witness.output_hash(), &[0u8; 32]);

    Ok(())
}

#[test]
fn test_circuit_from_inference_result() -> Result<()> {
    // Test creating circuit from InferenceResult
    let result = create_test_result();

    // Compute hashes
    use sha2::{Digest, Sha256};
    let job_id_hash: [u8; 32] = Sha256::digest(result.job_id.as_bytes()).into();
    let model_hash: [u8; 32] = Sha256::digest(result.model.as_bytes()).into();
    let input_hash: [u8; 32] = Sha256::digest(result.prompt.as_bytes()).into();
    let output_hash: [u8; 32] = Sha256::digest(result.response.as_bytes()).into();

    let circuit = CommitmentCircuit::new(job_id_hash, model_hash, input_hash, output_hash);

    assert!(circuit.is_valid());
    assert_eq!(circuit.public_inputs().len(), 4);

    Ok(())
}

#[test]
fn test_proof_generation_integration() {
    // Test proof generation integration with ProofGenerator
    // TODO: Create ProofGenerator instance
    // TODO: Create InferenceResult
    // TODO: Call generate_proof()
    // TODO: Verify proof structure
    // TODO: Verify proof contains correct hashes
}

#[test]
fn test_proof_verification_integration() {
    // Test proof verification integration with ProofGenerator
    // TODO: Generate proof from InferenceResult
    // TODO: Call verify_proof()
    // TODO: Verify returns true for valid proof
    // TODO: Modify result and verify returns false
}

#[test]
fn test_proof_type_ezkl_selected() {
    // Test that EZKL proof type is correctly selected
    // TODO: Create ProofGenerator with EZKL config
    // TODO: Verify proof_type is ProofType::EZKL
    // TODO: Generate proof
    // TODO: Verify proof uses EZKL implementation
}

#[test]
fn test_proof_with_mock_fallback() {
    // Test that mock fallback works when real-ezkl is disabled
    #[cfg(not(feature = "real-ezkl"))]
    {
        // TODO: Create ProofGenerator
        // TODO: Generate proof (should use mock)
        // TODO: Verify proof has mock marker (0xEF)
    }
}

#[test]
#[cfg(feature = "real-ezkl")]
fn test_proof_with_real_ezkl() {
    // Test that real EZKL is used when feature is enabled
    // TODO: Create ProofGenerator with real EZKL
    // TODO: Generate proof
    // TODO: Verify proof structure is real SNARK
    // TODO: Verify proof size is 2-10 KB (not 200 bytes mock)
}

#[test]
fn test_hash_computation_consistency() -> Result<()> {
    // Test that hash computation is consistent between witness and circuit
    let result = create_test_result();

    // Create witness
    let witness = WitnessBuilder::new()
        .with_job_id_string(&result.job_id)
        .with_model_path(&result.model)
        .with_input_string(&result.prompt)
        .with_output_string(&result.response)
        .build()?;

    // Create circuit with same hashes
    let circuit = CommitmentCircuit::new(
        *witness.job_id(),
        *witness.model_hash(),
        *witness.input_hash(),
        *witness.output_hash(),
    );

    // Verify circuit uses same data
    assert_eq!(circuit.job_id, *witness.job_id());
    assert_eq!(circuit.model_hash, *witness.model_hash());
    assert_eq!(circuit.input_hash, *witness.input_hash());
    assert_eq!(circuit.output_hash, *witness.output_hash());

    Ok(())
}

#[test]
fn test_proof_metadata_includes_hashes() {
    // Test that proof metadata includes all hash commitments
    // TODO: Generate proof
    // TODO: Verify proof includes model_hash
    // TODO: Verify proof includes input_hash
    // TODO: Verify proof includes output_hash
    // TODO: Verify proof includes timestamp
}

#[test]
fn test_proof_rejects_tampered_result() {
    // Test that verification fails if result is tampered with
    // TODO: Generate proof for original result
    // TODO: Modify result.response
    // TODO: Call verify_proof() with modified result
    // TODO: Verify returns false
}

#[test]
fn test_proof_rejects_wrong_model() {
    // Test that verification fails if model is changed
    // TODO: Generate proof with model_a
    // TODO: Verify with model_b
    // TODO: Verify returns false
}

#[test]
fn test_proof_rejects_wrong_input() {
    // Test that verification fails if input is changed
    // TODO: Generate proof with input_a
    // TODO: Verify with input_b
    // TODO: Verify returns false
}

#[test]
fn test_proof_timestamp_validation() {
    // Test that proof timestamp is within acceptable range
    // TODO: Generate proof
    // TODO: Verify timestamp is recent (within last 5 minutes)
    // TODO: Verify timestamp is not in future
}

#[test]
fn test_multiple_results_different_proofs() -> Result<()> {
    // Test that different results produce different proofs
    let result1 = create_test_result();
    let mut result2 = create_test_result();
    result2.response = "The answer is 5".to_string(); // Different response

    let witness1 = WitnessBuilder::new()
        .with_job_id_string(&result1.job_id)
        .with_model_path(&result1.model)
        .with_input_string(&result1.prompt)
        .with_output_string(&result1.response)
        .build()?;

    let witness2 = WitnessBuilder::new()
        .with_job_id_string(&result2.job_id)
        .with_model_path(&result2.model)
        .with_input_string(&result2.prompt)
        .with_output_string(&result2.response)
        .build()?;

    // Witnesses should be different
    assert_ne!(witness1, witness2);
    assert_ne!(witness1.output_hash(), witness2.output_hash());

    // TODO: Verify proofs are different when prover is implemented
    Ok(())
}

#[test]
fn test_proof_caching_with_results() {
    // Test that proof caching works correctly
    // TODO: Generate proof for result1
    // TODO: Generate proof for result1 again (should hit cache)
    // TODO: Verify cache hit metrics incremented
    // TODO: Verify proofs are identical
}

#[test]
fn test_proof_generation_error_propagation() {
    // Test that proof generation errors are properly propagated
    // TODO: Create invalid result (empty fields)
    // TODO: Attempt to generate proof
    // TODO: Verify error is returned
    // TODO: Verify error message is helpful
}

#[test]
fn test_proof_size_matches_expectations() {
    // Test that proof size matches expected values
    // TODO: Generate proof
    // TODO: For mock: verify ~200 bytes
    // TODO: For real EZKL: verify 2-10 KB
    // TODO: Verify proof is not empty
}

#[test]
fn test_batch_proof_generation() {
    // Test generating proofs for multiple results
    // TODO: Create 10 different results
    // TODO: Generate proof for each
    // TODO: Verify all proofs are valid
    // TODO: Verify proofs are different
}

#[test]
fn test_concurrent_proof_generation_with_results() {
    // Test concurrent proof generation with multiple results
    // TODO: Create 5 different results
    // TODO: Generate proofs concurrently
    // TODO: Verify all proofs complete successfully
    // TODO: Verify no race conditions or data corruption
}

#[test]
fn test_proof_performance_with_real_data() {
    // Test proof generation performance with realistic data
    // TODO: Create result with 1000+ token response
    // TODO: Measure proof generation time
    // TODO: Verify < 100ms (p95 target)
}

#[test]
fn test_proof_verification_performance() {
    // Test proof verification performance
    // TODO: Generate proof
    // TODO: Measure verification time
    // TODO: Verify < 10ms (target)
}

#[test]
fn test_proof_with_metadata() -> Result<()> {
    // Test proof generation with additional metadata
    let mut result = create_test_result();
    result.metadata.insert("gpu".to_string(), "RTX 3090".to_string());
    result
        .metadata
        .insert("cuda_version".to_string(), "12.0".to_string());

    // Metadata should not affect hash computation
    let witness = WitnessBuilder::new()
        .with_job_id_string(&result.job_id)
        .with_model_path(&result.model)
        .with_input_string(&result.prompt)
        .with_output_string(&result.response)
        .build()?;

    assert!(witness.is_valid());
    // Metadata is not part of proof commitment
    // TODO: Verify proof can be generated regardless of metadata

    Ok(())
}

#[test]
fn test_proof_serialization_for_storage() {
    // Test that proof can be serialized for database storage
    // TODO: Generate proof
    // TODO: Serialize to JSON
    // TODO: Deserialize from JSON
    // TODO: Verify proof is still valid
}

#[test]
fn test_proof_serialization_for_network() {
    // Test that proof can be serialized for network transmission
    // TODO: Generate proof
    // TODO: Serialize to bytes
    // TODO: Deserialize from bytes
    // TODO: Verify proof is still valid
}

#[test]
fn test_proof_compatibility_across_versions() {
    // Test that proofs remain compatible across versions
    // TODO: Generate proof with current version
    // TODO: Verify proof includes version metadata
    // TODO: Test that older versions can verify (if applicable)
}
