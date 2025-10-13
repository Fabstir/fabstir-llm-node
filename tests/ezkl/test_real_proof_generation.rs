//! Real EZKL Proof Generation Tests
//!
//! Tests for real EZKL proof generation functionality.
//! These tests verify that the proof generation works correctly
//! with both mock and real EZKL implementations.

use anyhow::Result;
use fabstir_llm_node::crypto::ezkl::{
    CommitmentCircuit, ProvingKey, VerificationKey, WitnessBuilder,
};

#[test]
fn test_proof_structure_validation() -> Result<()> {
    // Test that generated proof has valid structure
    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    assert!(witness.is_valid());
    assert!(!witness.is_empty());

    // Proof structure will be validated in prover module
    // TODO: Add real proof structure validation when prover is implemented
    Ok(())
}

#[test]
fn test_proof_generation_with_valid_inputs() -> Result<()> {
    // Test proof generation with various valid inputs
    let test_cases = vec![
        ([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]),
        ([255u8; 32], [254u8; 32], [253u8; 32], [252u8; 32]),
        ([42u8; 32], [43u8; 32], [44u8; 32], [45u8; 32]),
    ];

    for (job_id, model, input, output) in test_cases {
        let witness = WitnessBuilder::new()
            .with_job_id(job_id)
            .with_model_hash(model)
            .with_input_hash(input)
            .with_output_hash(output)
            .build()?;

        assert!(witness.is_valid());
        assert_eq!(witness.job_id(), &job_id);
        assert_eq!(witness.model_hash(), &model);
        assert_eq!(witness.input_hash(), &input);
        assert_eq!(witness.output_hash(), &output);
    }

    // TODO: Add real proof generation when prover is implemented
    Ok(())
}

#[test]
fn test_proof_generation_error_handling() {
    // Test that proof generation handles errors gracefully
    // TODO: Test key loading errors
    // TODO: Test invalid witness errors
    // TODO: Test circuit compilation errors
}

#[test]
fn test_proof_determinism() -> Result<()> {
    // Test that same input produces consistent proof structure
    let witness1 = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    let witness2 = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    // Witnesses should be identical for same inputs
    assert_eq!(witness1, witness2);
    assert_eq!(witness1.to_bytes(), witness2.to_bytes());

    // TODO: Test proof determinism when prover is implemented
    // Note: SNARK proofs may have randomness, so we test structure not exact bytes
    Ok(())
}

#[test]
fn test_proof_size_validation() {
    // Test that proof size is within acceptable limits
    // TODO: Mock proofs are ~200 bytes
    // TODO: Real SNARK proofs should be 2-10 KB
    // TODO: Verify proof data is not empty
}

#[test]
fn test_proof_metadata() {
    // Test that proof includes correct metadata
    // TODO: Test timestamp is included
    // TODO: Test proof type marker (EZKL)
    // TODO: Test hash commitments are included
}

#[test]
fn test_concurrent_proof_generation() {
    // Test that multiple proofs can be generated in parallel
    // TODO: Generate 10+ proofs concurrently
    // TODO: Verify all proofs are valid
    // TODO: Verify no race conditions
}

#[test]
#[cfg(not(feature = "real-ezkl"))]
fn test_mock_proof_generation() -> Result<()> {
    // Test mock proof generation (when real-ezkl feature is disabled)
    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);

    assert!(circuit.is_valid());
    assert_eq!(circuit.public_inputs().len(), 4);

    // Mock proof should have recognizable marker
    // TODO: Verify mock proof format when integrated with ProofGenerator
    Ok(())
}

#[test]
#[cfg(feature = "real-ezkl")]
fn test_real_proof_generation() -> Result<()> {
    // Test real EZKL proof generation (when real-ezkl feature is enabled)
    // TODO: Load proving key from test fixtures
    // TODO: Create witness from test data
    // TODO: Generate real SNARK proof
    // TODO: Verify proof structure
    // TODO: Verify proof size is 2-10 KB
    Ok(())
}

#[test]
fn test_proof_with_string_inputs() -> Result<()> {
    // Test proof generation using string inputs (common use case)
    let witness = WitnessBuilder::new()
        .with_job_id_string("job_12345")
        .with_model_path("./models/llama-7b.gguf")
        .with_input_string("What is 2+2?")
        .with_output_string("The answer is 4")
        .build()?;

    assert!(witness.is_valid());
    assert_eq!(witness.job_id().len(), 32); // SHA256 hash
    assert_eq!(witness.model_hash().len(), 32);
    assert_eq!(witness.input_hash().len(), 32);
    assert_eq!(witness.output_hash().len(), 32);

    // TODO: Generate proof from witness when prover is implemented
    Ok(())
}

#[test]
fn test_proof_generation_timeout() {
    // Test that proof generation completes within timeout
    // TODO: Set 5 second timeout
    // TODO: Generate proof
    // TODO: Verify completes within timeout (target: < 100ms)
}

#[test]
fn test_proof_with_realistic_data() -> Result<()> {
    // Test proof generation with realistic LLM data
    let job_id = "fabstir_job_1234567890";
    let model_path = "./models/llama-2-7b-chat.Q4_K_M.gguf";
    let prompt = "Explain quantum computing in simple terms.";
    let response = "Quantum computing uses quantum mechanics principles like superposition \
                     and entanglement to perform computations that would be impossible \
                     or take too long on classical computers...";

    let witness = WitnessBuilder::new()
        .with_job_id_string(job_id)
        .with_model_path(model_path)
        .with_input_string(prompt)
        .with_output_string(response)
        .build()?;

    assert!(witness.is_valid());

    // Verify hashes are different (no collisions for different data)
    assert_ne!(witness.job_id(), witness.model_hash());
    assert_ne!(witness.input_hash(), witness.output_hash());

    // TODO: Generate and verify proof when prover is implemented
    Ok(())
}

#[test]
fn test_proof_generation_with_empty_strings() {
    // Test that proof generation handles empty strings gracefully
    let witness = WitnessBuilder::new()
        .with_job_id_string("")
        .with_model_path("")
        .with_input_string("")
        .with_output_string("")
        .build();

    // Should succeed (SHA256 of empty string is valid)
    assert!(witness.is_ok());

    // TODO: Verify proof can be generated from empty strings
}

#[test]
fn test_proof_generation_with_large_inputs() -> Result<()> {
    // Test proof generation with large text inputs (typical LLM responses)
    let large_response = "a".repeat(10000); // 10KB response

    let witness = WitnessBuilder::new()
        .with_job_id_string("large_job")
        .with_model_path("./models/large-model.gguf")
        .with_input_string("Write a long essay")
        .with_output_string(&large_response)
        .build()?;

    assert!(witness.is_valid());
    // Hash should still be 32 bytes regardless of input size
    assert_eq!(witness.output_hash().len(), 32);

    // TODO: Verify proof size is independent of input data size
    // Proof should still be 2-10 KB even for 10KB input
    Ok(())
}

#[test]
fn test_proof_witness_serialization() -> Result<()> {
    // Test that witness can be serialized for proof generation
    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    // Test binary serialization
    let bytes = witness.to_bytes();
    assert_eq!(bytes.len(), 128); // 4 * 32 bytes

    // Test JSON serialization
    let json = serde_json::to_string(&witness)?;
    assert!(!json.is_empty());

    // Verify round-trip
    let deserialized: fabstir_llm_node::crypto::ezkl::Witness = serde_json::from_str(&json)?;
    assert_eq!(witness, deserialized);

    Ok(())
}

#[test]
fn test_proof_circuit_compilation() -> Result<()> {
    // Test that circuit compiles successfully
    use fabstir_llm_node::crypto::ezkl::compile_circuit;

    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    let compiled = compile_circuit(&circuit)?;

    assert!(!compiled.compiled_data.is_empty());
    assert_eq!(compiled.circuit, circuit);

    // TODO: Test real circuit compilation with EZKL library
    Ok(())
}

#[test]
fn test_proof_key_compatibility() -> Result<()> {
    // Test that proving and verification keys are compatible
    use fabstir_llm_node::crypto::ezkl::{compile_circuit, generate_keys, keys_are_compatible};

    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    let compiled = compile_circuit(&circuit)?;
    let (proving_key, verifying_key) = generate_keys(&compiled)?;

    assert!(keys_are_compatible(&proving_key, &verifying_key));

    // TODO: Test that incompatible keys are detected
    Ok(())
}

#[test]
fn test_proof_generation_performance() {
    // Test that proof generation meets performance targets
    // TODO: Generate 10 proofs
    // TODO: Measure average time
    // TODO: Verify p50 < 50ms, p95 < 100ms (for mock)
    // TODO: For real EZKL, verify < 100ms (will be implemented in Phase 2.2)
}

#[test]
fn test_proof_memory_usage() {
    // Test that proof generation doesn't leak memory
    // TODO: Generate 100 proofs in loop
    // TODO: Measure memory usage
    // TODO: Verify no significant memory growth
}

#[test]
fn test_proof_error_messages() {
    // Test that error messages are helpful for debugging
    // TODO: Test error when proving key missing
    // TODO: Test error when witness invalid
    // TODO: Test error when circuit compilation fails
    // TODO: Verify error messages include context
}
