//! Risc0 Proof Verification Tests
//!
//! Tests for real Risc0 STARK proof verification. These tests follow TDD approach -
//! written before implementation, they should fail until Phase 4.2 is complete.
//!
//! Test Coverage:
//! - Valid proof verification
//! - Invalid/tampered proof detection
//! - Wrong image ID detection
//! - Journal mismatch detection
//! - Deserialization failure handling
//! - Verification performance

#![cfg(feature = "real-ezkl")]

use fabstir_llm_node::crypto::ezkl::{EzklProver, EzklVerifier, Witness, WitnessBuilder};

/// Helper: Create test witness with given seed
fn create_test_witness(seed: u8) -> Witness {
    WitnessBuilder::new()
        .with_job_id([seed; 32])
        .with_model_hash([seed + 1; 32])
        .with_input_hash([seed + 2; 32])
        .with_output_hash([seed + 3; 32])
        .build()
        .unwrap()
}

#[test]
fn test_verify_valid_proof() {
    // Generate a real proof
    let witness = create_test_witness(0);
    let mut prover = EzklProver::new();
    let proof = prover
        .generate_proof(&witness)
        .expect("Proof generation should succeed");

    // Verify the proof
    let mut verifier = EzklVerifier::new();
    let is_valid = verifier
        .verify_proof(&proof, &witness)
        .expect("Verification should not error");

    assert!(is_valid, "Valid proof should verify successfully");
}

#[test]
fn test_verify_invalid_proof_tampered_bytes() {
    // Generate a real proof
    let witness = create_test_witness(1);
    let mut prover = EzklProver::new();
    let mut proof = prover
        .generate_proof(&witness)
        .expect("Proof generation should succeed");

    // Tamper with proof bytes (flip some bits in the middle)
    let tamper_pos = proof.proof_bytes.len() / 2;
    proof.proof_bytes[tamper_pos] ^= 0xFF;
    proof.proof_bytes[tamper_pos + 1] ^= 0xFF;

    // Verification should fail (either error or return false)
    let mut verifier = EzklVerifier::new();
    let result = verifier.verify_proof(&proof, &witness);

    // Tampered proof should either error or return false
    match result {
        Ok(false) => {
            // Good: verification detected tampering
        }
        Err(_) => {
            // Also good: verification errored due to tampering
        }
        Ok(true) => {
            panic!("Tampered proof should not verify as valid!");
        }
    }
}

#[test]
fn test_verify_wrong_image_id() {
    // This test verifies that proofs generated with a different guest program
    // (different image ID) cannot be used to verify against our commitment guest
    //
    // Note: This is more of a conceptual test. In practice, tampering with the
    // proof bytes to have a different image ID will cause deserialization or
    // cryptographic verification to fail.

    let witness = create_test_witness(2);
    let mut prover = EzklProver::new();
    let mut proof = prover
        .generate_proof(&witness)
        .expect("Proof generation should succeed");

    // Tamper with proof bytes to simulate wrong image ID
    // (In practice, we can't easily create a proof with a different guest program,
    // so we simulate by corrupting the proof structure)
    if proof.proof_bytes.len() > 100 {
        // Corrupt bytes that likely contain the image ID in the receipt
        for i in 50..60 {
            proof.proof_bytes[i] ^= 0xFF;
        }
    }

    let mut verifier = EzklVerifier::new();
    let result = verifier.verify_proof(&proof, &witness);

    // Should fail due to image ID mismatch
    match result {
        Ok(false) => {
            // Good: verification detected wrong image ID
        }
        Err(_) => {
            // Also good: verification errored due to wrong image ID
        }
        Ok(true) => {
            panic!("Proof with wrong image ID should not verify!");
        }
    }
}

#[test]
fn test_verify_journal_mismatch() {
    // Generate proof for one witness
    let witness1 = create_test_witness(3);
    let mut prover = EzklProver::new();
    let proof = prover
        .generate_proof(&witness1)
        .expect("Proof generation should succeed");

    // Try to verify with different witness
    let witness2 = create_test_witness(99);

    let mut verifier = EzklVerifier::new();
    let is_valid = verifier
        .verify_proof(&proof, &witness2)
        .expect("Verification should not error (just return false)");

    assert!(
        !is_valid,
        "Proof should not verify with mismatched witness"
    );
}

#[test]
fn test_verify_deserialization_failure() {
    let witness = create_test_witness(4);

    // Create corrupted proof bytes (not a valid receipt)
    let corrupted_proof = fabstir_llm_node::crypto::ezkl::prover::ProofData {
        proof_bytes: vec![0xDE, 0xAD, 0xBE, 0xEF], // Invalid receipt bytes
        timestamp: 1234567890,
        model_hash: *witness.model_hash(),
        input_hash: *witness.input_hash(),
        output_hash: *witness.output_hash(),
    };

    let mut verifier = EzklVerifier::new();
    let result = verifier.verify_proof(&corrupted_proof, &witness);

    // Should error due to deserialization failure
    assert!(
        result.is_err(),
        "Corrupted proof bytes should cause verification error"
    );

    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("deserialize") || err_msg.contains("Proof too small"),
        "Error should mention deserialization or size: {}",
        err_msg
    );
}

#[test]
fn test_verify_output_hash_tampering() {
    // This is a critical security test: verify that tampering with output hash
    // is detected. This protects against hosts lying about their inference results.

    let witness = create_test_witness(5);
    let mut prover = EzklProver::new();
    let mut proof = prover
        .generate_proof(&witness)
        .expect("Proof generation should succeed");

    // Tamper with output hash in proof metadata
    proof.output_hash[0] ^= 0xFF;
    proof.output_hash[1] ^= 0xFF;

    let mut verifier = EzklVerifier::new();
    let is_valid = verifier
        .verify_proof(&proof, &witness)
        .expect("Verification should not error");

    assert!(
        !is_valid,
        "Proof with tampered output hash should not verify"
    );
}

#[test]
fn test_verification_performance() {
    // Verify that verification is fast (< 1 second per proof)
    // This is important for node throughput and settlement speed

    let witness = create_test_witness(6);
    let mut prover = EzklProver::new();
    let proof = prover
        .generate_proof(&witness)
        .expect("Proof generation should succeed");

    let mut verifier = EzklVerifier::new();

    let start = std::time::Instant::now();
    let is_valid = verifier
        .verify_proof(&proof, &witness)
        .expect("Verification should succeed");
    let elapsed = start.elapsed();

    assert!(is_valid, "Proof should be valid");
    assert!(
        elapsed.as_secs() < 1,
        "Verification should take < 1 second, took {:?}",
        elapsed
    );

    println!("✅ Verification time: {:?}", elapsed);
}

#[test]
fn test_verify_multiple_proofs_sequentially() {
    // Test verifying multiple proofs in sequence
    // Ensures verifier state doesn't cause issues

    let mut verifier = EzklVerifier::new();

    for seed in 0..3u8 {
        let witness = create_test_witness(seed);
        let mut prover = EzklProver::new();
        let proof = prover
            .generate_proof(&witness)
            .expect("Proof generation should succeed");

        let is_valid = verifier
            .verify_proof(&proof, &witness)
            .expect("Verification should succeed");

        assert!(is_valid, "Proof {} should verify", seed);
    }
}

#[test]
fn test_verify_with_real_witness_data() {
    // Test with witness built from string hashes (more realistic)
    let witness = WitnessBuilder::new()
        .with_job_id_string("job-verification-test-001")
        .with_model_path("model-llama-3-8b")
        .with_input_string("User input: What is Rust?")
        .with_output_string("Rust is a systems programming language...")
        .build()
        .expect("Witness should build");

    let mut prover = EzklProver::new();
    let proof = prover
        .generate_proof(&witness)
        .expect("Proof generation should succeed");

    let mut verifier = EzklVerifier::new();
    let is_valid = verifier
        .verify_proof(&proof, &witness)
        .expect("Verification should succeed");

    assert!(is_valid, "Proof with real witness data should verify");
}

#[test]
fn test_verify_proof_size_validation() {
    // Test that proof size validation works correctly
    let witness = create_test_witness(7);

    // Create proof with size that's too small
    let tiny_proof = fabstir_llm_node::crypto::ezkl::prover::ProofData {
        proof_bytes: vec![0x01, 0x02], // Only 2 bytes
        timestamp: 1234567890,
        model_hash: *witness.model_hash(),
        input_hash: *witness.input_hash(),
        output_hash: *witness.output_hash(),
    };

    let mut verifier = EzklVerifier::new();
    let result = verifier.verify_proof(&tiny_proof, &witness);

    assert!(result.is_err(), "Tiny proof should be rejected");

    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("too small") || err_msg.contains("Proof too small"),
        "Error should mention size: {}",
        err_msg
    );
}

#[test]
fn test_verify_cryptographic_properties() {
    // This test verifies that the proof actually provides cryptographic guarantees
    // by ensuring that even identical witness data in different proofs are
    // cryptographically independent

    let witness = create_test_witness(8);

    // Generate two proofs for the same witness
    let mut prover = EzklProver::new();
    let proof1 = prover
        .generate_proof(&witness)
        .expect("First proof generation should succeed");
    let proof2 = prover
        .generate_proof(&witness)
        .expect("Second proof generation should succeed");

    // Proofs should be different (non-deterministic due to randomness)
    // Note: Risc0 proofs may or may not be deterministic depending on version
    // This is more of an observation than a hard requirement
    println!(
        "Proof 1 size: {} bytes, Proof 2 size: {} bytes",
        proof1.proof_bytes.len(),
        proof2.proof_bytes.len()
    );

    // Both proofs should verify
    let mut verifier = EzklVerifier::new();

    let is_valid1 = verifier
        .verify_proof(&proof1, &witness)
        .expect("First verification should succeed");
    let is_valid2 = verifier
        .verify_proof(&proof2, &witness)
        .expect("Second verification should succeed");

    assert!(is_valid1, "First proof should verify");
    assert!(is_valid2, "Second proof should verify");
}
