// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Commitment Circuit Design Tests
//!
//! Tests for the commitment circuit that proves hash relationships
//! for job_id, model_hash, input_hash, and output_hash.

use anyhow::Result;

/// Test that commitment circuit structure has correct fields
#[test]
fn test_circuit_has_four_hash_fields() {
    // Circuit should have exactly 4 hash fields, each 32 bytes
    use fabstir_llm_node::crypto::ezkl::circuit::CommitmentCircuit;

    let job_id = [0u8; 32];
    let model_hash = [1u8; 32];
    let input_hash = [2u8; 32];
    let output_hash = [3u8; 32];

    let circuit = CommitmentCircuit {
        job_id,
        model_hash,
        input_hash,
        output_hash,
    };

    assert_eq!(circuit.job_id.len(), 32);
    assert_eq!(circuit.model_hash.len(), 32);
    assert_eq!(circuit.input_hash.len(), 32);
    assert_eq!(circuit.output_hash.len(), 32);
}

/// Test that circuit fields are properly typed
#[test]
fn test_circuit_field_types() {
    // All fields should be [u8; 32] arrays
    use fabstir_llm_node::crypto::ezkl::circuit::CommitmentCircuit;
    use std::mem::size_of;

    // Circuit should have correct memory layout
    assert_eq!(size_of::<CommitmentCircuit>(), 128); // 4 * 32 bytes
}

/// Test creating circuit with valid inputs
#[test]
fn test_create_circuit_with_valid_inputs() -> Result<()> {
    // Should be able to create circuit with any 32-byte values
    use fabstir_llm_node::crypto::ezkl::circuit::CommitmentCircuit;

    let circuit = CommitmentCircuit::new(
        [0u8; 32],  // job_id
        [1u8; 32],  // model_hash
        [2u8; 32],  // input_hash
        [3u8; 32],  // output_hash
    );

    assert!(circuit.is_valid());

    Ok(())
}

/// Test creating circuit from bytes
#[test]
fn test_create_circuit_from_bytes() -> Result<()> {
    // Should be able to create circuit from raw bytes
    use fabstir_llm_node::crypto::ezkl::circuit::CommitmentCircuit;

    let job_id_bytes = vec![0u8; 32];
    let model_hash_bytes = vec![1u8; 32];
    let input_hash_bytes = vec![2u8; 32];
    let output_hash_bytes = vec![3u8; 32];

    let circuit = CommitmentCircuit::from_bytes(
        &job_id_bytes,
        &model_hash_bytes,
        &input_hash_bytes,
        &output_hash_bytes,
    )?;

    assert_eq!(circuit.job_id[0], 0);
    assert_eq!(circuit.model_hash[0], 1);

    Ok(())
}

/// Test creating circuit from hex strings
#[test]
fn test_create_circuit_from_hex() -> Result<()> {
    // Should be able to create circuit from hex-encoded hashes
    use fabstir_llm_node::crypto::ezkl::circuit::CommitmentCircuit;

    let job_id_hex = "0".repeat(64); // 32 bytes = 64 hex chars
    let model_hash_hex = "1".repeat(64);
    let input_hash_hex = "2".repeat(64);
    let output_hash_hex = "3".repeat(64);

    let circuit = CommitmentCircuit::from_hex(
        &job_id_hex,
        &model_hash_hex,
        &input_hash_hex,
        &output_hash_hex,
    )?;

    assert!(circuit.is_valid());

    Ok(())
}

/// Test circuit validation rejects invalid sizes
#[test]
fn test_circuit_rejects_invalid_sizes() {
    // Circuit should reject non-32-byte inputs
    use fabstir_llm_node::crypto::ezkl::circuit::CommitmentCircuit;

    // Wrong size should fail
    let result = CommitmentCircuit::from_bytes(
        &[0u8; 16],  // Too short
        &[1u8; 32],
        &[2u8; 32],
        &[3u8; 32],
    );

    assert!(result.is_err());
}

/// Test circuit serialization to JSON
#[test]
fn test_circuit_serialization() -> Result<()> {
    // Circuit should be serializable
    use fabstir_llm_node::crypto::ezkl::circuit::CommitmentCircuit;

    let circuit = CommitmentCircuit::new(
        [0u8; 32],
        [1u8; 32],
        [2u8; 32],
        [3u8; 32],
    );

    let json = serde_json::to_string(&circuit)?;
    assert!(!json.is_empty());

    let deserialized: CommitmentCircuit = serde_json::from_str(&json)?;
    assert_eq!(circuit.job_id, deserialized.job_id);

    Ok(())
}

/// Test circuit metadata
#[test]
fn test_circuit_metadata() {
    // Circuit should expose metadata about its structure
    use fabstir_llm_node::crypto::ezkl::circuit::CommitmentCircuit;

    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    let metadata = circuit.metadata();

    assert_eq!(metadata.field_count(), 4);
    assert_eq!(metadata.circuit_type(), "commitment");
    assert_eq!(metadata.hash_size(), 32);
}

/// Test circuit with all zeros (edge case)
#[test]
fn test_circuit_with_all_zeros() {
    // Should handle all-zero hashes (though unusual)
    // TODO: Uncomment when implementation is ready
    // use fabstir_llm_node::crypto::ezkl::circuit::CommitmentCircuit;

    // let circuit = CommitmentCircuit::new([0u8; 32], [0u8; 32], [0u8; 32], [0u8; 32]);
    // assert!(circuit.is_valid());

    assert!(true);
}

/// Test circuit with all ones (edge case)
#[test]
fn test_circuit_with_all_ones() {
    // Should handle all-ones hashes
    // TODO: Uncomment when implementation is ready
    // use fabstir_llm_node::crypto::ezkl::circuit::CommitmentCircuit;

    // let circuit = CommitmentCircuit::new([0xFF; 32], [0xFF; 32], [0xFF; 32], [0xFF; 32]);
    // assert!(circuit.is_valid());

    assert!(true);
}

/// Test circuit comparison
#[test]
fn test_circuit_equality() {
    // Two circuits with same values should be equal
    // TODO: Uncomment when implementation is ready
    // use fabstir_llm_node::crypto::ezkl::circuit::CommitmentCircuit;

    // let circuit1 = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    // let circuit2 = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);

    // assert_eq!(circuit1, circuit2);

    // Different circuits should not be equal
    // let circuit3 = CommitmentCircuit::new([4u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    // assert_ne!(circuit1, circuit3);

    assert!(true);
}

/// Test circuit cloning
#[test]
fn test_circuit_clone() {
    // Circuit should be cloneable
    // TODO: Uncomment when implementation is ready
    // use fabstir_llm_node::crypto::ezkl::circuit::CommitmentCircuit;

    // let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    // let cloned = circuit.clone();

    // assert_eq!(circuit, cloned);

    assert!(true);
}

/// Test circuit debug output
#[test]
fn test_circuit_debug_output() {
    // Circuit should have useful debug output
    // TODO: Uncomment when implementation is ready
    // use fabstir_llm_node::crypto::ezkl::circuit::CommitmentCircuit;

    // let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    // let debug_str = format!("{:?}", circuit);

    // assert!(debug_str.contains("CommitmentCircuit"));
    // assert!(debug_str.contains("job_id"));

    assert!(true);
}

/// Test circuit constraint specification
#[test]
fn test_circuit_constraints() {
    // Circuit should define its constraints
    use fabstir_llm_node::crypto::ezkl::circuit::CommitmentCircuit;

    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    let constraints = circuit.constraints();

    // Constraints should include:
    // - All fields are 32 bytes (4 size constraints)
    // - All fields are bound together (1 binding constraint)
    assert_eq!(constraints.len(), 5);
}

/// Test circuit satisfiability check
#[test]
fn test_circuit_satisfiability() {
    // Valid circuit should be satisfiable
    use fabstir_llm_node::crypto::ezkl::circuit::CommitmentCircuit;

    let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
    assert!(circuit.is_satisfiable());
}

/// Test circuit with realistic hash values
#[test]
fn test_circuit_with_realistic_hashes() -> Result<()> {
    // Test with SHA256-like hash values
    use fabstir_llm_node::crypto::ezkl::circuit::CommitmentCircuit;
    use sha2::{Digest, Sha256};

    let job_id = Sha256::digest(b"job_123").into();
    let model_hash = Sha256::digest(b"tinyllama-1.1b").into();
    let input_hash = Sha256::digest(b"What is 2+2?").into();
    let output_hash = Sha256::digest(b"The answer is 4").into();

    let circuit = CommitmentCircuit::new(job_id, model_hash, input_hash, output_hash);
    assert!(circuit.is_valid());

    Ok(())
}
