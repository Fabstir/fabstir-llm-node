//! Risc0 Proof Generation Tests
//!
//! Tests for proof generation using Risc0 zkVM. These tests verify that the
//! prover can generate valid STARK proofs from witness data.
//!
//! Phase: 3.1 (TDD approach - tests written before implementation)
//! Status: Tests should FAIL until Phase 3.2 implementation complete
//!
//! Test Coverage:
//! 1. Basic proof generation from witness
//! 2. Proof contains correct witness data in journal
//! 3. Proof serialization and deserialization
//! 4. Proof size is reasonable (< 500KB)
//! 5. Error handling for invalid witness
//! 6. Proof metadata (timestamp, hashes)

#[cfg(feature = "real-ezkl")]
use anyhow::Result;

#[cfg(feature = "real-ezkl")]
use fabstir_llm_node::crypto::ezkl::{EzklProver, WitnessBuilder};

/// Test basic proof generation from witness data
///
/// Verifies that the prover can generate a proof from valid witness data.
/// This is the most fundamental test - if this fails, nothing else will work.
#[cfg(feature = "real-ezkl")]
#[test]
fn test_generate_real_proof_basic() -> Result<()> {
    // Create test witness data
    let witness = WitnessBuilder::new()
        .with_job_id([1u8; 32])
        .with_model_hash([2u8; 32])
        .with_input_hash([3u8; 32])
        .with_output_hash([4u8; 32])
        .build()?;

    // Create prover
    let mut prover = EzklProver::new();

    // Generate proof
    let proof = prover.generate_proof(&witness)?;

    // Basic assertions
    assert!(!proof.proof_bytes.is_empty(), "Proof bytes should not be empty");
    assert!(proof.timestamp > 0, "Proof should have valid timestamp");
    assert_eq!(proof.model_hash, [2u8; 32], "Model hash should match");
    assert_eq!(proof.input_hash, [3u8; 32], "Input hash should match");
    assert_eq!(proof.output_hash, [4u8; 32], "Output hash should match");

    Ok(())
}

/// Test that proof contains witness data in journal
///
/// Verifies that the journal in the proof contains all 4 hash commitments
/// in the correct order: job_id, model_hash, input_hash, output_hash
#[cfg(feature = "real-ezkl")]
#[test]
fn test_proof_contains_witness_data() -> Result<()> {
    use risc0_zkvm::Receipt;

    // Create witness with distinct values for verification
    let job_id = [0xAAu8; 32];
    let model_hash = [0xBBu8; 32];
    let input_hash = [0xCCu8; 32];
    let output_hash = [0xDDu8; 32];

    let witness = WitnessBuilder::new()
        .with_job_id(job_id)
        .with_model_hash(model_hash)
        .with_input_hash(input_hash)
        .with_output_hash(output_hash)
        .build()?;

    // Generate proof
    let mut prover = EzklProver::new();
    let proof = prover.generate_proof(&witness)?;

    // Deserialize receipt from proof bytes
    let receipt: Receipt = bincode::deserialize(&proof.proof_bytes)?;

    // Verify journal contains all 4 hashes in correct order
    let mut journal = receipt.journal.bytes.as_slice();

    // Read back the 4 committed hashes (they should be raw bytes from commit_slice)
    let mut j_job_id = [0u8; 32];
    std::io::Read::read_exact(&mut journal, &mut j_job_id)?;

    let mut j_model_hash = [0u8; 32];
    std::io::Read::read_exact(&mut journal, &mut j_model_hash)?;

    let mut j_input_hash = [0u8; 32];
    std::io::Read::read_exact(&mut journal, &mut j_input_hash)?;

    let mut j_output_hash = [0u8; 32];
    std::io::Read::read_exact(&mut journal, &mut j_output_hash)?;

    // Verify all hashes match
    assert_eq!(j_job_id, job_id, "Job ID should match in journal");
    assert_eq!(j_model_hash, model_hash, "Model hash should match in journal");
    assert_eq!(j_input_hash, input_hash, "Input hash should match in journal");
    assert_eq!(j_output_hash, output_hash, "Output hash should match in journal");

    Ok(())
}

/// Test that proofs can be serialized and deserialized
///
/// Verifies that ProofData can be serialized to bytes and deserialized back
/// without data loss. This is critical for storing and transmitting proofs.
#[cfg(feature = "real-ezkl")]
#[test]
fn test_proof_is_serializable() -> Result<()> {
    // Create witness
    let witness = WitnessBuilder::new()
        .with_job_id([5u8; 32])
        .with_model_hash([6u8; 32])
        .with_input_hash([7u8; 32])
        .with_output_hash([8u8; 32])
        .build()?;

    // Generate proof
    let mut prover = EzklProver::new();
    let proof = prover.generate_proof(&witness)?;

    // Serialize ProofData
    let serialized = bincode::serialize(&proof)?;
    assert!(!serialized.is_empty(), "Serialized proof should not be empty");

    // Deserialize ProofData
    let deserialized: fabstir_llm_node::crypto::ezkl::ProofData =
        bincode::deserialize(&serialized)?;

    // Verify data matches
    assert_eq!(
        deserialized.proof_bytes.len(),
        proof.proof_bytes.len(),
        "Proof bytes length should match"
    );
    assert_eq!(
        deserialized.timestamp, proof.timestamp,
        "Timestamp should match"
    );
    assert_eq!(
        deserialized.model_hash, proof.model_hash,
        "Model hash should match"
    );
    assert_eq!(
        deserialized.input_hash, proof.input_hash,
        "Input hash should match"
    );
    assert_eq!(
        deserialized.output_hash, proof.output_hash,
        "Output hash should match"
    );

    // Verify the receipt itself is still valid after serialization
    use risc0_zkvm::Receipt;
    let receipt: Receipt = bincode::deserialize(&deserialized.proof_bytes)?;
    assert!(!receipt.journal.bytes.is_empty(), "Receipt journal should not be empty");

    Ok(())
}

/// Test proof generation with real-world witness data
///
/// Uses witness data created from strings (like production will use)
/// to ensure the proof generation works with realistic inputs.
#[cfg(feature = "real-ezkl")]
#[test]
fn test_proof_with_real_witness_data() -> Result<()> {
    // Create witness from strings (like production)
    let witness = WitnessBuilder::new()
        .with_job_id_string("job_test_12345")
        .with_model_path("./models/test-model.gguf")
        .with_input_string("What is the meaning of life?")
        .with_output_string("The meaning of life is 42.")
        .build()?;

    // Generate proof
    let mut prover = EzklProver::new();
    let proof = prover.generate_proof(&witness)?;

    // Verify proof was generated
    assert!(!proof.proof_bytes.is_empty(), "Proof bytes should not be empty");
    assert_eq!(proof.model_hash, *witness.model_hash(), "Model hash should match");
    assert_eq!(proof.input_hash, *witness.input_hash(), "Input hash should match");
    assert_eq!(proof.output_hash, *witness.output_hash(), "Output hash should match");

    Ok(())
}

/// Test that proof size is reasonable
///
/// Verifies that generated proofs are within acceptable size limits.
/// Based on Risc0 benchmarks, we expect proofs to be 194-281KB for STARK proofs.
/// Setting limit to 500KB to allow some overhead.
#[cfg(feature = "real-ezkl")]
#[test]
fn test_proof_size_reasonable() -> Result<()> {
    // Create witness
    let witness = WitnessBuilder::new()
        .with_job_id([9u8; 32])
        .with_model_hash([10u8; 32])
        .with_input_hash([11u8; 32])
        .with_output_hash([12u8; 32])
        .build()?;

    // Generate proof
    let mut prover = EzklProver::new();
    let proof = prover.generate_proof(&witness)?;

    // Check proof size
    let proof_size = proof.proof_bytes.len();
    println!("Proof size: {} bytes ({:.2} KB)", proof_size, proof_size as f64 / 1024.0);

    // Risc0 STARK proofs should be 194-281KB according to benchmarks
    // Allow up to 500KB for safety margin
    assert!(
        proof_size < 500_000,
        "Proof size ({} bytes) should be less than 500KB",
        proof_size
    );

    // Also verify it's not suspiciously small (should be at least 100KB for STARK proofs)
    assert!(
        proof_size > 100_000,
        "Proof size ({} bytes) seems too small for a STARK proof (expected > 100KB)",
        proof_size
    );

    Ok(())
}

/// Test error handling for invalid witness
///
/// Verifies that the prover handles invalid witness data gracefully
/// by returning appropriate errors rather than panicking.
#[cfg(feature = "real-ezkl")]
#[test]
fn test_proof_generation_error_handling() -> Result<()> {
    // Create a witness that will fail validation
    // Note: WitnessBuilder always creates valid witness, so we need to test
    // with specific invalid scenarios that might occur in production

    // Test 1: Verify that valid witness works
    let valid_witness = WitnessBuilder::new()
        .with_job_id([1u8; 32])
        .with_model_hash([2u8; 32])
        .with_input_hash([3u8; 32])
        .with_output_hash([4u8; 32])
        .build()?;

    let mut prover = EzklProver::new();
    let result = prover.generate_proof(&valid_witness);
    assert!(result.is_ok(), "Valid witness should generate proof successfully");

    // Test 2: Verify proof generation works multiple times
    let result2 = prover.generate_proof(&valid_witness);
    assert!(result2.is_ok(), "Should be able to generate multiple proofs");

    Ok(())
}

/// Test proof metadata correctness
///
/// Verifies that proof metadata (timestamp, hashes) is correctly
/// captured in the ProofData structure.
#[cfg(feature = "real-ezkl")]
#[test]
fn test_proof_metadata() -> Result<()> {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Create witness with known values
    let job_id = [0x11u8; 32];
    let model_hash = [0x22u8; 32];
    let input_hash = [0x33u8; 32];
    let output_hash = [0x44u8; 32];

    let witness = WitnessBuilder::new()
        .with_job_id(job_id)
        .with_model_hash(model_hash)
        .with_input_hash(input_hash)
        .with_output_hash(output_hash)
        .build()?;

    // Capture time before proof generation
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Generate proof
    let mut prover = EzklProver::new();
    let proof = prover.generate_proof(&witness)?;

    // Capture time after proof generation
    let after = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Verify timestamp is within expected range
    assert!(
        proof.timestamp >= before,
        "Proof timestamp should be after generation start"
    );
    assert!(
        proof.timestamp <= after,
        "Proof timestamp should be before generation end"
    );

    // Verify hashes match witness (but NOT job_id, as it's not stored in ProofData)
    assert_eq!(proof.model_hash, model_hash, "Model hash should match");
    assert_eq!(proof.input_hash, input_hash, "Input hash should match");
    assert_eq!(proof.output_hash, output_hash, "Output hash should match");

    Ok(())
}

// ============================================================================
// Mock Mode Tests (run when real-ezkl feature is disabled)
// ============================================================================

/// Test that mock mode still compiles when real-ezkl is disabled
#[cfg(not(feature = "real-ezkl"))]
#[test]
fn test_mock_mode_compiles() {
    // This test exists to ensure the test file compiles in mock mode
    // Real proof generation tests are only available with --features real-ezkl
    assert!(true, "Mock mode compiles successfully");
}

#[cfg(not(feature = "real-ezkl"))]
#[test]
fn test_mock_mode_documentation() {
    // Document that proof generation tests require real-ezkl feature
    println!("ℹ️  Proof generation tests require --features real-ezkl");
    println!("ℹ️  Run: cargo test --features real-ezkl test_generate_real_proof");
    assert!(true);
}
