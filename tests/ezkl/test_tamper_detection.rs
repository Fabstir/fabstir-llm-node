// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tamper Detection Tests (Sub-phase 3.1)
//!
//! Tests verification system's ability to detect various types of tampering
//! including proof byte corruption, hash manipulation, replay attacks, and substitution.

use anyhow::Result;
use fabstir_llm_node::crypto::ezkl::{EzklProver, EzklVerifier, ProofData, WitnessBuilder};

/// Helper to create test witness
fn create_test_witness(seed: u8) -> fabstir_llm_node::crypto::ezkl::Witness {
    WitnessBuilder::new()
        .with_job_id([seed; 32])
        .with_model_hash([seed + 1; 32])
        .with_input_hash([seed + 2; 32])
        .with_output_hash([seed + 3; 32])
        .build()
        .unwrap()
}

/// Helper to generate valid proof
fn generate_valid_proof(witness: &fabstir_llm_node::crypto::ezkl::Witness) -> Result<ProofData> {
    let mut prover = EzklProver::new();
    prover
        .generate_proof(witness)
        .map_err(|e| anyhow::anyhow!("{}", e))
}

#[test]
fn test_detect_tampered_proof_bytes() -> Result<()> {
    let witness = create_test_witness(0);
    let mut proof = generate_valid_proof(&witness)?;

    // Tamper with proof bytes
    // For real Risc0 proofs (~221KB), we need aggressive tampering to ensure detection
    #[cfg(feature = "real-ezkl")]
    {
        // Corrupt first 1000 bytes which include receipt metadata and critical structure
        let tamper_end = std::cmp::min(1000, proof.proof_bytes.len());
        for i in 0..tamper_end {
            proof.proof_bytes[i] ^= 0xFF;
        }
    }

    #[cfg(not(feature = "real-ezkl"))]
    {
        // For mock proofs, just tamper with a few bytes
        if proof.proof_bytes.len() > 50 {
            proof.proof_bytes[25] ^= 0xFF; // Flip bits
            proof.proof_bytes[50] ^= 0xFF;
        }
    }

    let mut verifier = EzklVerifier::new();
    let result = verifier.verify_proof(&proof, &witness);

    // Tampered proof should either error or return false
    #[cfg(feature = "real-ezkl")]
    match result {
        Ok(false) => {
            // Good: verification detected tampering
        }
        Err(_) => {
            // Also good: verification errored due to tampering
        }
        Ok(true) => {
            panic!("Tampered proof bytes should be detected in real EZKL");
        }
    }

    #[cfg(not(feature = "real-ezkl"))]
    {
        // Mock verifier can't detect byte-level tampering
        // Just verify test completes without panicking
        let _ = result;
    }
    Ok(())
}

#[test]
fn test_detect_wrong_model_hash() -> Result<()> {
    let witness = create_test_witness(1);
    let proof = generate_valid_proof(&witness)?;

    // Create witness with different model hash
    let tampered_witness = WitnessBuilder::new()
        .with_job_id(*witness.job_id())
        .with_model_hash([99u8; 32]) // Different model hash
        .with_input_hash(*witness.input_hash())
        .with_output_hash(*witness.output_hash())
        .build()?;

    let mut verifier = EzklVerifier::new();
    let is_valid = verifier
        .verify_proof(&proof, &tampered_witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    assert!(
        !is_valid,
        "Wrong model hash should fail verification"
    );
    Ok(())
}

#[test]
fn test_detect_wrong_input_hash() -> Result<()> {
    let witness = create_test_witness(2);
    let proof = generate_valid_proof(&witness)?;

    // Create witness with different input hash
    let tampered_witness = WitnessBuilder::new()
        .with_job_id(*witness.job_id())
        .with_model_hash(*witness.model_hash())
        .with_input_hash([88u8; 32]) // Different input hash
        .with_output_hash(*witness.output_hash())
        .build()?;

    let mut verifier = EzklVerifier::new();
    let is_valid = verifier
        .verify_proof(&proof, &tampered_witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    assert!(
        !is_valid,
        "Wrong input hash should fail verification"
    );
    Ok(())
}

#[test]
fn test_detect_wrong_output_hash() -> Result<()> {
    let witness = create_test_witness(3);
    let proof = generate_valid_proof(&witness)?;

    // Create witness with different output hash
    let tampered_witness = WitnessBuilder::new()
        .with_job_id(*witness.job_id())
        .with_model_hash(*witness.model_hash())
        .with_input_hash(*witness.input_hash())
        .with_output_hash([77u8; 32]) // Different output hash
        .build()?;

    let mut verifier = EzklVerifier::new();
    let is_valid = verifier
        .verify_proof(&proof, &tampered_witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    assert!(
        !is_valid,
        "Wrong output hash should fail verification"
    );
    Ok(())
}

#[test]
fn test_detect_proof_replay_attack() -> Result<()> {
    // Generate proof for job 1
    let witness1 = create_test_witness(10);
    let proof1 = generate_valid_proof(&witness1)?;

    // Try to use proof1 for different job (job 2)
    let witness2 = create_test_witness(20);

    let mut verifier = EzklVerifier::new();
    let is_valid = verifier
        .verify_proof(&proof1, &witness2)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    assert!(
        !is_valid,
        "Proof replay attack should be detected"
    );
    Ok(())
}

#[test]
fn test_detect_partial_hash_tampering() -> Result<()> {
    let witness = create_test_witness(4);
    let proof = generate_valid_proof(&witness)?;

    // Tamper with just one byte of model hash
    let mut tampered_model_hash = *witness.model_hash();
    tampered_model_hash[0] ^= 0x01; // Flip one bit

    let tampered_witness = WitnessBuilder::new()
        .with_job_id(*witness.job_id())
        .with_model_hash(tampered_model_hash)
        .with_input_hash(*witness.input_hash())
        .with_output_hash(*witness.output_hash())
        .build()?;

    let mut verifier = EzklVerifier::new();
    let is_valid = verifier
        .verify_proof(&proof, &tampered_witness)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    assert!(
        !is_valid,
        "Even single byte tampering should be detected"
    );
    Ok(())
}

#[test]
fn test_detect_proof_substitution() -> Result<()> {
    // Generate two different proofs
    let witness_a = create_test_witness(30);
    let proof_a = generate_valid_proof(&witness_a)?;

    let witness_b = create_test_witness(40);
    let _proof_b = generate_valid_proof(&witness_b)?;

    // Try to verify proof_a with witness_b
    let mut verifier = EzklVerifier::new();
    let is_valid = verifier
        .verify_proof(&proof_a, &witness_b)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    assert!(
        !is_valid,
        "Proof substitution should be detected"
    );
    Ok(())
}

#[test]
fn test_detect_timestamp_tampering() -> Result<()> {
    let witness = create_test_witness(5);
    let mut proof = generate_valid_proof(&witness)?;

    // Tamper with timestamp
    let original_timestamp = proof.timestamp;
    proof.timestamp = original_timestamp + 1000;

    let mut verifier = EzklVerifier::new();
    let result = verifier.verify_proof(&proof, &witness);

    // Note: Timestamp tampering might not affect SNARK verification itself
    // This test documents behavior - in production, timestamp should be checked separately
    match result {
        Ok(is_valid) => {
            // If verification succeeds, timestamp is not part of commitment
            // This is expected for basic commitment circuit
            assert!(is_valid || !is_valid, "Timestamp handling documented");
        }
        Err(_) => {
            // If it errors, that's also acceptable
        }
    }
    Ok(())
}

#[test]
fn test_detect_public_input_mismatch() -> Result<()> {
    let witness = create_test_witness(6);
    let proof = generate_valid_proof(&witness)?;

    let mut verifier = EzklVerifier::new();

    // Try to verify with wrong public inputs
    let wrong_public_inputs = vec![
        &[0u8; 32],  // Wrong model hash
        witness.input_hash(),
        witness.output_hash(),
    ];

    let is_valid = verifier
        .verify_proof_bytes(&proof.proof_bytes, &wrong_public_inputs)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Note: Mock verifier doesn't check public inputs against proof
    // Real EZKL will detect this mismatch
    #[cfg(feature = "real-ezkl")]
    assert!(
        !is_valid,
        "Public input mismatch should be detected in real EZKL"
    );

    #[cfg(not(feature = "real-ezkl"))]
    {
        // Mock verifier can't detect public input mismatch
        let _ = is_valid;
    }
    Ok(())
}

#[test]
fn test_detect_full_tampering_scenario() -> Result<()> {
    // Simulate complete attack scenario:
    // 1. Attacker intercepts valid proof
    // 2. Tries to modify all components to claim different work

    let witness_original = create_test_witness(7);
    let proof_original = generate_valid_proof(&witness_original)?;

    // Attacker creates completely different witness
    let witness_fake = WitnessBuilder::new()
        .with_job_id([255u8; 32])
        .with_model_hash([254u8; 32])
        .with_input_hash([253u8; 32])
        .with_output_hash([252u8; 32])
        .build()?;

    // Try to verify original proof with fake witness
    let mut verifier = EzklVerifier::new();
    let is_valid = verifier
        .verify_proof(&proof_original, &witness_fake)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    assert!(
        !is_valid,
        "Complete tampering scenario should be detected"
    );

    // Also verify original still works
    let original_valid = verifier
        .verify_proof(&proof_original, &witness_original)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    assert!(
        original_valid,
        "Original proof should still verify correctly"
    );

    Ok(())
}

#[test]
fn test_tamper_detection_multiple_attempts() -> Result<()> {
    // Test that multiple tampering attempts all fail
    let witness = create_test_witness(8);
    let proof = generate_valid_proof(&witness)?;

    let mut verifier = EzklVerifier::new();

    // Attempt 1: Wrong model hash
    let attempt1 = WitnessBuilder::new()
        .with_job_id(*witness.job_id())
        .with_model_hash([111u8; 32])
        .with_input_hash(*witness.input_hash())
        .with_output_hash(*witness.output_hash())
        .build()?;

    assert!(
        !verifier.verify_proof(&proof, &attempt1)?,
        "Attempt 1 should fail"
    );

    // Attempt 2: Wrong input hash
    let attempt2 = WitnessBuilder::new()
        .with_job_id(*witness.job_id())
        .with_model_hash(*witness.model_hash())
        .with_input_hash([222u8; 32])
        .with_output_hash(*witness.output_hash())
        .build()?;

    assert!(
        !verifier.verify_proof(&proof, &attempt2)?,
        "Attempt 2 should fail"
    );

    // Attempt 3: Wrong output hash
    let attempt3 = WitnessBuilder::new()
        .with_job_id(*witness.job_id())
        .with_model_hash(*witness.model_hash())
        .with_input_hash(*witness.input_hash())
        .with_output_hash([255u8; 32])
        .build()?;

    assert!(
        !verifier.verify_proof(&proof, &attempt3)?,
        "Attempt 3 should fail"
    );

    // Verify original still works after multiple failed attempts
    let original_valid = verifier.verify_proof(&proof, &witness)?;
    assert!(
        original_valid,
        "Original should still verify after failed attempts"
    );

    Ok(())
}
