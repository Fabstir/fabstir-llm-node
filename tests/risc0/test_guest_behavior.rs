//! Risc0 Guest Program Behavior Tests
//!
//! Tests for the Risc0 zkVM guest program that proves knowledge of hash commitments.
//!
//! Phase: 2.1 (TDD approach - tests written before implementation)
//! Status: Tests should FAIL until Phase 2.2 implementation complete
//!
//! The guest program should:
//! 1. Read 4x [u8; 32] hashes from host via env::read()
//! 2. Commit all 4 hashes to journal via env::commit()
//! 3. Maintain correct order: job_id, model_hash, input_hash, output_hash
//! 4. Handle serialization correctly (bincode format)

#[cfg(feature = "real-ezkl")]
use anyhow::Result;

#[cfg(feature = "real-ezkl")]
use risc0_zkvm::{default_prover, ExecutorEnv};

// Include the generated guest constants at module level
// This provides COMMITMENT_GUEST_ELF and COMMITMENT_GUEST_ID
#[cfg(feature = "real-ezkl")]
include!(concat!(env!("OUT_DIR"), "/methods.rs"));

/// Test that guest program can read four hash values from host
///
/// This test verifies the guest program correctly reads witness data
/// sent from the host via env::write() calls.
#[cfg(feature = "real-ezkl")]
#[test]
fn test_guest_reads_four_hashes() -> Result<()> {
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;

    // Create test witness data (4 hashes with distinct values for verification)
    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32]) // All zeros
        .with_model_hash([1u8; 32]) // All ones
        .with_input_hash([2u8; 32]) // All twos
        .with_output_hash([3u8; 32]) // All threes
        .build()?;

    // Build executor environment with witness data
    // Guest will read these values via env::read()
    let env = ExecutorEnv::builder()
        .write(witness.job_id())?
        .write(witness.model_hash())?
        .write(witness.input_hash())?
        .write(witness.output_hash())?
        .build()?;

    // Execute guest program
    // COMMITMENT_GUEST_ELF is available from the module-level include!
    let prover = default_prover();
    let prove_info = prover.prove(env, COMMITMENT_GUEST_ELF)?;
    let receipt = prove_info.receipt;

    // If we got here without panic, guest successfully read all 4 hashes
    assert!(receipt.journal.bytes.len() > 0, "Journal should contain data");

    Ok(())
}

/// Test that guest commits all values to journal
///
/// This test verifies the guest program writes all 4 hashes to the public
/// journal, which is what makes them verifiable by third parties.
#[cfg(feature = "real-ezkl")]
#[test]
fn test_guest_commits_to_journal() -> Result<()> {
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;

    // Create witness with distinct patterns for each hash
    let job_id = [0u8; 32];
    let model_hash = [1u8; 32];
    let input_hash = [2u8; 32];
    let output_hash = [3u8; 32];

    let witness = WitnessBuilder::new()
        .with_job_id(job_id)
        .with_model_hash(model_hash)
        .with_input_hash(input_hash)
        .with_output_hash(output_hash)
        .build()?;

    // Build executor environment
    let env = ExecutorEnv::builder()
        .write(witness.job_id())?
        .write(witness.model_hash())?
        .write(witness.input_hash())?
        .write(witness.output_hash())?
        .build()?;

    // Execute guest program
    let prover = default_prover();
    let prove_info = prover.prove(env, COMMITMENT_GUEST_ELF)?;
    let receipt = prove_info.receipt;

    // Decode journal to verify all 4 hashes were committed
    let journal_bytes = receipt.journal.bytes;

    // Journal should contain 4x 32-byte arrays = 128 bytes minimum
    // (actual size may be larger due to bincode encoding overhead)
    assert!(
        journal_bytes.len() >= 128,
        "Journal should contain at least 128 bytes (4x 32-byte hashes), got {}",
        journal_bytes.len()
    );

    Ok(())
}

/// Test that journal maintains correct order
///
/// This test verifies the guest program commits hashes in the expected order:
/// job_id, model_hash, input_hash, output_hash
#[cfg(feature = "real-ezkl")]
#[test]
fn test_guest_journal_order() -> Result<()> {
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;

    // Create witness with very distinct values to verify ordering
    let job_id = [0xAAu8; 32]; // 0xAA repeated
    let model_hash = [0xBBu8; 32]; // 0xBB repeated
    let input_hash = [0xCCu8; 32]; // 0xCC repeated
    let output_hash = [0xDDu8; 32]; // 0xDD repeated

    let witness = WitnessBuilder::new()
        .with_job_id(job_id)
        .with_model_hash(model_hash)
        .with_input_hash(input_hash)
        .with_output_hash(output_hash)
        .build()?;

    // Build executor environment
    let env = ExecutorEnv::builder()
        .write(witness.job_id())?
        .write(witness.model_hash())?
        .write(witness.input_hash())?
        .write(witness.output_hash())?
        .build()?;

    // Execute guest program
    let prover = default_prover();
    let prove_info = prover.prove(env, COMMITMENT_GUEST_ELF)?;
    let receipt = prove_info.receipt;

    // Decode journal and verify order
    let mut journal = receipt.journal.bytes.as_slice();

    // Read back the 4 committed hashes
    let j_job_id: [u8; 32] = bincode::deserialize_from(&mut journal)?;
    let j_model_hash: [u8; 32] = bincode::deserialize_from(&mut journal)?;
    let j_input_hash: [u8; 32] = bincode::deserialize_from(&mut journal)?;
    let j_output_hash: [u8; 32] = bincode::deserialize_from(&mut journal)?;

    // Verify order is correct
    assert_eq!(j_job_id, job_id, "job_id should be first");
    assert_eq!(j_model_hash, model_hash, "model_hash should be second");
    assert_eq!(j_input_hash, input_hash, "input_hash should be third");
    assert_eq!(j_output_hash, output_hash, "output_hash should be fourth");

    Ok(())
}

/// Test that guest handles serialization correctly
///
/// This test verifies the guest program uses proper bincode encoding
/// for the journal data, ensuring verifier can decode it correctly.
#[cfg(feature = "real-ezkl")]
#[test]
fn test_guest_handles_serialization() -> Result<()> {
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;

    // Use random-looking data to test serialization robustness
    let job_id = [
        0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
        0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x01, 0x02, 0x03, 0x04,
        0x05, 0x06, 0x07, 0x08,
    ];

    let model_hash = [0xFFu8; 32]; // All 0xFF
    let input_hash = [0x00u8; 32]; // All zeros
    let output_hash = {
        // Alternating pattern
        let mut hash = [0u8; 32];
        for i in 0..32 {
            hash[i] = if i % 2 == 0 { 0xAA } else { 0x55 };
        }
        hash
    };

    let witness = WitnessBuilder::new()
        .with_job_id(job_id)
        .with_model_hash(model_hash)
        .with_input_hash(input_hash)
        .with_output_hash(output_hash)
        .build()?;

    // Build executor environment
    let env = ExecutorEnv::builder()
        .write(witness.job_id())?
        .write(witness.model_hash())?
        .write(witness.input_hash())?
        .write(witness.output_hash())?
        .build()?;

    // Execute guest program
    let prover = default_prover();
    let prove_info = prover.prove(env, COMMITMENT_GUEST_ELF)?;
    let receipt = prove_info.receipt;

    // Verify we can deserialize all values correctly
    let mut journal = receipt.journal.bytes.as_slice();

    let j_job_id: [u8; 32] = bincode::deserialize_from(&mut journal)?;
    let j_model_hash: [u8; 32] = bincode::deserialize_from(&mut journal)?;
    let j_input_hash: [u8; 32] = bincode::deserialize_from(&mut journal)?;
    let j_output_hash: [u8; 32] = bincode::deserialize_from(&mut journal)?;

    // Verify exact match despite complex bit patterns
    assert_eq!(j_job_id, job_id, "job_id serialization mismatch");
    assert_eq!(
        j_model_hash, model_hash,
        "model_hash serialization mismatch"
    );
    assert_eq!(
        j_input_hash, input_hash,
        "input_hash serialization mismatch"
    );
    assert_eq!(
        j_output_hash, output_hash,
        "output_hash serialization mismatch"
    );

    Ok(())
}

/// Test guest program with real witness data
///
/// This test uses witness data similar to what will be generated
/// in production (computed from strings).
#[cfg(feature = "real-ezkl")]
#[test]
fn test_guest_with_real_witness_data() -> Result<()> {
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;

    // Create witness from strings (like production will)
    let witness = WitnessBuilder::new()
        .with_job_id_string("job_12345")
        .with_model_path("./models/tinyllama-1.1b.gguf")
        .with_input_string("What is the capital of France?")
        .with_output_string("The capital of France is Paris.")
        .build()?;

    // Build executor environment
    let env = ExecutorEnv::builder()
        .write(witness.job_id())?
        .write(witness.model_hash())?
        .write(witness.input_hash())?
        .write(witness.output_hash())?
        .build()?;

    // Execute guest program
    let prover = default_prover();
    let prove_info = prover.prove(env, COMMITMENT_GUEST_ELF)?;
    let receipt = prove_info.receipt;

    // Verify journal contains committed values
    let mut journal = receipt.journal.bytes.as_slice();

    let j_job_id: [u8; 32] = bincode::deserialize_from(&mut journal)?;
    let j_model_hash: [u8; 32] = bincode::deserialize_from(&mut journal)?;
    let j_input_hash: [u8; 32] = bincode::deserialize_from(&mut journal)?;
    let j_output_hash: [u8; 32] = bincode::deserialize_from(&mut journal)?;

    // Verify values match original witness
    assert_eq!(j_job_id, *witness.job_id());
    assert_eq!(j_model_hash, *witness.model_hash());
    assert_eq!(j_input_hash, *witness.input_hash());
    assert_eq!(j_output_hash, *witness.output_hash());

    Ok(())
}

/// Test guest execution produces valid receipt
///
/// This test verifies the guest program execution produces a receipt
/// that can be used for verification.
#[cfg(feature = "real-ezkl")]
#[test]
fn test_guest_produces_valid_receipt() -> Result<()> {
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;

    let witness = WitnessBuilder::new()
        .with_job_id([1u8; 32])
        .with_model_hash([2u8; 32])
        .with_input_hash([3u8; 32])
        .with_output_hash([4u8; 32])
        .build()?;

    let env = ExecutorEnv::builder()
        .write(witness.job_id())?
        .write(witness.model_hash())?
        .write(witness.input_hash())?
        .write(witness.output_hash())?
        .build()?;

    // Execute guest program
    let prover = default_prover();
    let prove_info = prover.prove(env, COMMITMENT_GUEST_ELF)?;
    let receipt = prove_info.receipt;

    // Receipt should have a journal
    assert!(!receipt.journal.bytes.is_empty(), "Receipt should have journal");

    // Receipt should be verifiable (we'll test actual verification in Phase 4)
    // For now, just ensure receipt structure is valid
    let _journal_len = receipt.journal.bytes.len();
    assert!(
        _journal_len > 0,
        "Journal should contain committed data"
    );

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
    // Real guest tests are only available with --features real-ezkl
    assert!(true, "Mock mode compiles successfully");
}

#[cfg(not(feature = "real-ezkl"))]
#[test]
fn test_mock_mode_documentation() {
    // Document that guest tests require real-ezkl feature
    println!("ℹ️  Guest behavior tests require --features real-ezkl");
    println!("ℹ️  Run: cargo test --features real-ezkl test_guest");
    assert!(true);
}
