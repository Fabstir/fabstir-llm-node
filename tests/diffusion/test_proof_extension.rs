// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for image generation proof extension (Phase 5.2)

use fabstir_llm_node::diffusion::billing::ImageContentHashes;

#[test]
fn test_image_content_hashes_creation() {
    let hashes = ImageContentHashes {
        prompt_hash: [1u8; 32],
        output_hash: [2u8; 32],
        safety_attestation_hash: [3u8; 32],
        seed: 42,
        generation_units: 1.0,
    };
    assert_eq!(hashes.seed, 42);
    assert!((hashes.generation_units - 1.0).abs() < 0.001);
}

#[test]
fn test_compute_data_hash_returns_32_bytes() {
    let hashes = ImageContentHashes {
        prompt_hash: [10u8; 32],
        output_hash: [20u8; 32],
        safety_attestation_hash: [30u8; 32],
        seed: 123,
        generation_units: 0.5,
    };
    let hash = hashes.compute_data_hash();
    assert_eq!(hash.len(), 32);
    assert!(hash.iter().any(|&b| b != 0));
}

#[test]
fn test_compute_data_hash_is_deterministic() {
    let hashes = ImageContentHashes {
        prompt_hash: [5u8; 32],
        output_hash: [6u8; 32],
        safety_attestation_hash: [7u8; 32],
        seed: 999,
        generation_units: 2.5,
    };
    let hash1 = hashes.compute_data_hash();
    let hash2 = hashes.compute_data_hash();
    assert_eq!(hash1, hash2);
}

#[test]
fn test_includes_prompt_hash() {
    let a = ImageContentHashes {
        prompt_hash: [0u8; 32],
        output_hash: [1u8; 32],
        safety_attestation_hash: [2u8; 32],
        seed: 1,
        generation_units: 1.0,
    };
    let b = ImageContentHashes {
        prompt_hash: [99u8; 32],
        output_hash: [1u8; 32],
        safety_attestation_hash: [2u8; 32],
        seed: 1,
        generation_units: 1.0,
    };
    assert_ne!(a.compute_data_hash(), b.compute_data_hash());
}

#[test]
fn test_includes_safety_attestation_hash() {
    let a = ImageContentHashes {
        prompt_hash: [1u8; 32],
        output_hash: [1u8; 32],
        safety_attestation_hash: [0u8; 32],
        seed: 1,
        generation_units: 1.0,
    };
    let b = ImageContentHashes {
        prompt_hash: [1u8; 32],
        output_hash: [1u8; 32],
        safety_attestation_hash: [99u8; 32],
        seed: 1,
        generation_units: 1.0,
    };
    assert_ne!(a.compute_data_hash(), b.compute_data_hash());
}

#[test]
fn test_to_witness_bytes_format() {
    let hashes = ImageContentHashes {
        prompt_hash: [8u8; 32],
        output_hash: [9u8; 32],
        safety_attestation_hash: [10u8; 32],
        seed: 42,
        generation_units: 1.5,
    };
    let bytes = hashes.to_witness_bytes();
    assert!(!bytes.is_empty());
    // Should contain all three 32-byte hashes + seed + units
    // Minimum size: 32*3 + 8 (seed) = 104 bytes
    assert!(bytes.len() >= 104);
}
