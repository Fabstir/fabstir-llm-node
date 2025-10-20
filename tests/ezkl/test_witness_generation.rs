// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Witness Data Generation Tests
//!
//! Tests for generating witness data from hash values for circuit proving.

use anyhow::Result;

/// Test creating witness from four hashes
#[test]
fn test_create_witness_from_hashes() -> Result<()> {
    // Should create witness from 4 hash values
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;

    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    assert!(!witness.is_empty());

    Ok(())
}

/// Test witness builder pattern
#[test]
fn test_witness_builder_pattern() -> Result<()> {
    // Builder pattern should allow flexible construction
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;

    let builder = WitnessBuilder::new();
    let builder = builder.with_job_id([0u8; 32]);
    let builder = builder.with_model_hash([1u8; 32]);
    let builder = builder.with_input_hash([2u8; 32]);
    let builder = builder.with_output_hash([3u8; 32]);

    let witness = builder.build()?;
    assert!(witness.is_valid());

    Ok(())
}

/// Test witness validation
#[test]
fn test_witness_validation() {
    // Witness should validate that all fields are present
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;

    // Missing field should fail
    let result = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        // Missing input_hash and output_hash
        .build();

    assert!(result.is_err());
}

/// Test witness from inference result
#[test]
#[cfg(feature = "inference")]
fn test_witness_from_inference_result() -> Result<()> {
    // Should create witness from InferenceResult
    use fabstir_llm_node::crypto::ezkl::witness::create_witness_from_result;
    use fabstir_llm_node::results::packager::{InferenceResult, ResultMetadata};
    use chrono::Utc;

    let result = InferenceResult {
        job_id: "test_job".to_string(),
        model_id: "tinyllama".to_string(),
        prompt: "Test prompt".to_string(),
        response: "Test response".to_string(),
        tokens_generated: 0,
        inference_time_ms: 0,
        timestamp: Utc::now(),
        node_id: "test_node".to_string(),
        metadata: ResultMetadata {
            temperature: 0.7,
            max_tokens: 100,
            top_p: 0.9,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
        },
    };

    let witness = create_witness_from_result(&result, "model_path.gguf")?;
    assert!(witness.is_valid());

    Ok(())
}

/// Test witness serialization
#[test]
fn test_witness_serialization() -> Result<()> {
    // Witness should be serializable to JSON
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;

    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    let json = serde_json::to_string(&witness)?;
    assert!(!json.is_empty());

    let deserialized = serde_json::from_str(&json)?;
    assert_eq!(witness, deserialized);

    Ok(())
}

/// Test witness to bytes conversion
#[test]
fn test_witness_to_bytes() -> Result<()> {
    // Witness should convert to byte representation
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;

    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    let bytes = witness.to_bytes();
    assert_eq!(bytes.len(), 128); // 4 * 32 bytes

    Ok(())
}

/// Test witness from bytes
#[test]
fn test_witness_from_bytes() -> Result<()> {
    // Should reconstruct witness from bytes
    use fabstir_llm_node::crypto::ezkl::witness::{Witness, WitnessBuilder};

    let original = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    let bytes = original.to_bytes();
    let reconstructed = Witness::from_bytes(&bytes)?;

    assert_eq!(original, reconstructed);

    Ok(())
}

/// Test witness with hash computation
#[test]
fn test_witness_with_computed_hashes() -> Result<()> {
    // Witness builder should compute hashes from strings
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;

    let witness = WitnessBuilder::new()
        .with_job_id_string("job_123")
        .with_model_path("./models/model.gguf")
        .with_input_string("What is 2+2?")
        .with_output_string("The answer is 4")
        .build()?;

    assert!(witness.is_valid());

    Ok(())
}

/// Test witness cloning
#[test]
fn test_witness_clone() -> Result<()> {
    // Witness should be cloneable
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;

    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    let cloned = witness.clone();
    assert_eq!(witness, cloned);

    Ok(())
}

/// Test witness field access
#[test]
fn test_witness_field_access() -> Result<()> {
    // Should be able to access individual fields
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;

    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    assert_eq!(witness.job_id()[0], 0);
    assert_eq!(witness.model_hash()[0], 1);
    assert_eq!(witness.input_hash()[0], 2);
    assert_eq!(witness.output_hash()[0], 3);

    Ok(())
}

/// Test witness debug output
#[test]
fn test_witness_debug_output() -> Result<()> {
    // Witness debug should be available (for development/debugging)
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;

    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    let debug_str = format!("{:?}", witness);
    // Should show struct name
    assert!(debug_str.contains("Witness"));
    assert!(!debug_str.is_empty());

    Ok(())
}

/// Test witness with empty job_id
#[test]
fn test_witness_with_empty_job_id() {
    // Should handle empty/zero job_id
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;

    let result = WitnessBuilder::new()
        .with_job_id([0u8; 32])  // All zeros
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build();

    // Should still be valid (though unusual)
    assert!(result.is_ok());
}

/// Test witness equality
#[test]
fn test_witness_equality() -> Result<()> {
    // Two witnesses with same data should be equal
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;

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

    assert_eq!(witness1, witness2);

    Ok(())
}

/// Test witness size
#[test]
fn test_witness_size() -> Result<()> {
    // Witness should have predictable size
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;
    use std::mem::size_of_val;

    let witness = WitnessBuilder::new()
        .with_job_id([0u8; 32])
        .with_model_hash([1u8; 32])
        .with_input_hash([2u8; 32])
        .with_output_hash([3u8; 32])
        .build()?;

    let size = size_of_val(&witness);
    assert!(size >= 128); // At least 4 * 32 bytes

    Ok(())
}

/// Test witness generation performance
#[tokio::test]
async fn test_witness_generation_performance() -> Result<()> {
    // Witness generation should be fast (< 5ms)
    use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;
    use std::time::Instant;

    let start = Instant::now();

    for _ in 0..1000 {
        let _ = WitnessBuilder::new()
            .with_job_id([0u8; 32])
            .with_model_hash([1u8; 32])
            .with_input_hash([2u8; 32])
            .with_output_hash([3u8; 32])
            .build()?;
    }

    let elapsed = start.elapsed();
    // 1000 witnesses in < 5ms total
    assert!(elapsed.as_millis() < 5);

    Ok(())
}
