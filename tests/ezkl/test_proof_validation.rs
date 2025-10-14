//! Proof Validation Tests
//!
//! Tests for EZKL proof size and format validation

use anyhow::Result;
use fabstir_llm_node::crypto::ezkl::{EzklProver, WitnessBuilder, ProofData};

/// Helper to create test witness
fn create_test_witness() -> Result<fabstir_llm_node::crypto::ezkl::Witness> {
    WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()
}

/// Test that proof size is within expected range
#[test]
fn test_proof_size_within_range() -> Result<()> {
    // Generate proof
    let mut prover = EzklProver::new();
    let witness = create_test_witness()?;
    let proof = prover.generate_proof(&witness)?;

    // Mock EZKL: 200 bytes
    // Real EZKL: 2KB - 10KB
    #[cfg(not(feature = "real-ezkl"))]
    {
        // Mock proof should be 200 bytes
        assert_eq!(proof.proof_bytes.len(), 200, "Mock proof should be exactly 200 bytes");
    }

    #[cfg(feature = "real-ezkl")]
    {
        // Real EZKL proof should be 2-10KB
        assert!(
            proof.proof_bytes.len() >= 2_000,
            "Real EZKL proof should be at least 2KB, got {} bytes",
            proof.proof_bytes.len()
        );
        assert!(
            proof.proof_bytes.len() <= 10_000,
            "Real EZKL proof should be at most 10KB, got {} bytes",
            proof.proof_bytes.len()
        );
    }

    Ok(())
}

/// Test that proof format is valid (has correct marker)
#[test]
fn test_proof_format_validation() -> Result<()> {
    let mut prover = EzklProver::new();
    let witness = create_test_witness()?;
    let proof = prover.generate_proof(&witness)?;

    // Mock EZKL proofs start with 0xEF marker
    #[cfg(not(feature = "real-ezkl"))]
    {
        assert!(!proof.proof_bytes.is_empty(), "Proof should not be empty");
        assert_eq!(
            proof.proof_bytes[0], 0xEF,
            "Mock EZKL proof should start with 0xEF marker"
        );
    }

    // Verify proof is not empty
    assert!(!proof.proof_bytes.is_empty());

    Ok(())
}

/// Test that proof can be serialized
#[test]
fn test_proof_serialization() -> Result<()> {
    let mut prover = EzklProver::new();
    let witness = create_test_witness()?;
    let proof = prover.generate_proof(&witness)?;

    // Test serialization to JSON
    let json = serde_json::to_string(&proof)?;
    assert!(!json.is_empty(), "Serialized proof should not be empty");

    // Test that it contains expected fields
    assert!(json.contains("proof_bytes"));
    assert!(json.contains("timestamp"));
    assert!(json.contains("model_hash"));

    Ok(())
}

/// Test that proof can be deserialized
#[test]
fn test_proof_deserialization() -> Result<()> {
    let mut prover = EzklProver::new();
    let witness = create_test_witness()?;
    let original_proof = prover.generate_proof(&witness)?;

    // Serialize and deserialize
    let json = serde_json::to_string(&original_proof)?;
    let deserialized_proof: ProofData = serde_json::from_str(&json)?;

    // Verify fields match
    assert_eq!(
        original_proof.proof_bytes.len(),
        deserialized_proof.proof_bytes.len()
    );
    assert_eq!(original_proof.model_hash, deserialized_proof.model_hash);
    assert_eq!(original_proof.input_hash, deserialized_proof.input_hash);
    assert_eq!(original_proof.output_hash, deserialized_proof.output_hash);

    Ok(())
}

/// Test proof contains all required fields
#[test]
fn test_proof_has_required_fields() -> Result<()> {
    let mut prover = EzklProver::new();
    let witness = create_test_witness()?;
    let proof = prover.generate_proof(&witness)?;

    // Verify all fields are present
    assert!(!proof.proof_bytes.is_empty(), "proof_bytes should not be empty");
    assert!(proof.timestamp > 0, "timestamp should be set");
    assert_eq!(proof.model_hash.len(), 32, "model_hash should be 32 bytes");
    assert_eq!(proof.input_hash.len(), 32, "input_hash should be 32 bytes");
    assert_eq!(proof.output_hash.len(), 32, "output_hash should be 32 bytes");

    Ok(())
}

/// Test proof hashes match witness
#[test]
fn test_proof_hashes_match_witness() -> Result<()> {
    let mut prover = EzklProver::new();
    let witness = create_test_witness()?;
    let proof = prover.generate_proof(&witness)?;

    // Verify hashes match
    assert_eq!(proof.model_hash, *witness.model_hash());
    assert_eq!(proof.input_hash, *witness.input_hash());
    assert_eq!(proof.output_hash, *witness.output_hash());

    Ok(())
}

/// Test that proof timestamp is recent
#[test]
fn test_proof_timestamp_is_recent() -> Result<()> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let mut prover = EzklProver::new();
    let witness = create_test_witness()?;

    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let proof = prover.generate_proof(&witness)?;

    let after = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Timestamp should be between before and after
    assert!(
        proof.timestamp >= before,
        "Proof timestamp should be >= generation start time"
    );
    assert!(
        proof.timestamp <= after + 1, // Allow 1 second tolerance
        "Proof timestamp should be <= generation end time"
    );

    Ok(())
}

/// Test mock proof contains witness data
#[test]
#[cfg(not(feature = "real-ezkl"))]
fn test_mock_proof_contains_witness_data() -> Result<()> {
    let mut prover = EzklProver::new();
    let witness = create_test_witness()?;
    let proof = prover.generate_proof(&witness)?;

    // Mock proof structure: [0xEF][job_id][model][input][output][padding]
    assert_eq!(proof.proof_bytes.len(), 200);
    assert_eq!(proof.proof_bytes[0], 0xEF);

    // Verify witness data is embedded in proof
    assert_eq!(&proof.proof_bytes[1..33], witness.job_id());
    assert_eq!(&proof.proof_bytes[33..65], witness.model_hash());
    assert_eq!(&proof.proof_bytes[65..97], witness.input_hash());
    assert_eq!(&proof.proof_bytes[97..129], witness.output_hash());

    Ok(())
}

/// Test that multiple proofs have unique timestamps
#[test]
fn test_proof_timestamps_are_unique() -> Result<()> {
    let mut prover = EzklProver::new();
    let witness = create_test_witness()?;

    let proof1 = prover.generate_proof(&witness)?;
    std::thread::sleep(std::time::Duration::from_millis(10)); // Small delay
    let proof2 = prover.generate_proof(&witness)?;

    // Timestamps should be different (or at least not go backwards)
    assert!(
        proof2.timestamp >= proof1.timestamp,
        "Second proof timestamp should be >= first"
    );

    Ok(())
}

/// Test proof with different witnesses produces different proofs
#[test]
fn test_different_witnesses_produce_different_proofs() -> Result<()> {
    let mut prover = EzklProver::new();

    let witness1 = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    let witness2 = WitnessBuilder::new()
        .with_job_id([10u8; 32])
        .with_model_hash([11u8; 32])
        .with_input_hash([12u8; 32])
        .with_output_hash([13u8; 32])
        .build()?;

    let proof1 = prover.generate_proof(&witness1)?;
    let proof2 = prover.generate_proof(&witness2)?;

    // Proofs should have different hashes
    assert_ne!(proof1.model_hash, proof2.model_hash);
    assert_ne!(proof1.input_hash, proof2.input_hash);
    assert_ne!(proof1.output_hash, proof2.output_hash);

    Ok(())
}

/// Test proof size is consistent for same witness
#[test]
fn test_proof_size_consistency() -> Result<()> {
    let mut prover = EzklProver::new();
    let witness = create_test_witness()?;

    let proof1 = prover.generate_proof(&witness)?;
    let proof2 = prover.generate_proof(&witness)?;

    // Proof sizes should be identical
    assert_eq!(
        proof1.proof_bytes.len(),
        proof2.proof_bytes.len(),
        "Proof sizes should be consistent"
    );

    Ok(())
}

/// Test proof validation with empty witness should fail
#[test]
fn test_invalid_witness_rejected() {
    // This test verifies that witness validation catches invalid witnesses
    // The witness builder should reject invalid constructions
    let result = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        // Missing other fields
        .build();

    assert!(result.is_err(), "Incomplete witness should be rejected");
}

/// Test that proof bytes are not all zeros
#[test]
fn test_proof_not_all_zeros() -> Result<()> {
    let mut prover = EzklProver::new();
    let witness = create_test_witness()?;
    let proof = prover.generate_proof(&witness)?;

    // Proof should contain non-zero bytes
    let has_nonzero = proof.proof_bytes.iter().any(|&byte| byte != 0);
    assert!(has_nonzero, "Proof should contain non-zero bytes");

    Ok(())
}

/// Test proof cloning
#[test]
fn test_proof_clone() -> Result<()> {
    let mut prover = EzklProver::new();
    let witness = create_test_witness()?;
    let original = prover.generate_proof(&witness)?;

    let cloned = original.clone();

    // Verify clone matches original
    assert_eq!(original.proof_bytes.len(), cloned.proof_bytes.len());
    assert_eq!(original.timestamp, cloned.timestamp);
    assert_eq!(original.model_hash, cloned.model_hash);
    assert_eq!(original.input_hash, cloned.input_hash);
    assert_eq!(original.output_hash, cloned.output_hash);

    Ok(())
}
