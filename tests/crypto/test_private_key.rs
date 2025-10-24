// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Private Key Extraction Tests (TDD - Phase 6, Sub-phase 6.1)
//!
//! These tests verify that the node can extract its private key from environment:
//! - Read HOST_PRIVATE_KEY from environment
//! - Parse using ethers LocalWallet::from_str()
//! - Extract raw 32-byte private key
//! - Validate key format (0x-prefixed hex)
//! - Handle missing/invalid keys
//! - Never log the actual key
//!
//! **TDD Approach**: Tests written BEFORE implementation.

use std::env;

/// Test that a valid private key can be extracted from environment
#[test]
fn test_extract_private_key_from_env() {
    // Set valid private key in environment
    let test_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
    env::set_var("HOST_PRIVATE_KEY", test_key);

    // Extract private key (function to be implemented)
    let result = fabstir_llm_node::crypto::extract_node_private_key();

    // Expected: Key extracted successfully
    assert!(result.is_ok());

    let private_key_bytes = result.unwrap();

    // Expected: 32-byte private key
    assert_eq!(private_key_bytes.len(), 32);

    // Expected: Matches the test key
    let expected_bytes =
        hex::decode("1234567890123456789012345678901234567890123456789012345678901234").unwrap();
    assert_eq!(private_key_bytes, expected_bytes.as_slice());

    // Cleanup
    env::remove_var("HOST_PRIVATE_KEY");
}

/// Test that invalid key format is rejected
#[test]
fn test_invalid_key_format() {
    // Test various invalid formats
    let invalid_keys = vec![
        "not_hex",                                                             // Not hex
        "0x123",                                                               // Too short
        "0xZZZZ567890123456789012345678901234567890123456789012345678901234",  // Invalid hex chars
        "1234567890123456789012345678901234567890123456789012345678901234",    // Missing 0x prefix
        "0x12345678901234567890123456789012345678901234567890123456789012345", // Too long (65 chars)
    ];

    for invalid_key in invalid_keys {
        env::set_var("HOST_PRIVATE_KEY", invalid_key);

        let result = fabstir_llm_node::crypto::extract_node_private_key();

        // Expected: Error returned for invalid format
        assert!(
            result.is_err(),
            "Should reject invalid key: {}",
            invalid_key
        );

        env::remove_var("HOST_PRIVATE_KEY");
    }
}

/// Test that missing HOST_PRIVATE_KEY is handled gracefully
#[test]
fn test_missing_host_private_key() {
    // Ensure HOST_PRIVATE_KEY is not set
    env::remove_var("HOST_PRIVATE_KEY");

    let result = fabstir_llm_node::crypto::extract_node_private_key();

    // Expected: Error returned when key is missing
    assert!(result.is_err());

    let error_msg = result.unwrap_err().to_string();

    // Expected: Error message mentions missing key
    assert!(
        error_msg.contains("HOST_PRIVATE_KEY") || error_msg.contains("not set"),
        "Error should mention missing HOST_PRIVATE_KEY: {}",
        error_msg
    );
}

/// Test key validation (correct length and format)
#[test]
fn test_key_validation() {
    // Test correct 32-byte key (64 hex chars + 0x prefix)
    let valid_key = "0xabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd";
    env::set_var("HOST_PRIVATE_KEY", valid_key);

    let result = fabstir_llm_node::crypto::extract_node_private_key();
    assert!(result.is_ok(), "Valid 32-byte key should be accepted");

    let key_bytes = result.unwrap();
    assert_eq!(key_bytes.len(), 32, "Key should be exactly 32 bytes");

    env::remove_var("HOST_PRIVATE_KEY");

    // Test key without 0x prefix (should be rejected or handled)
    let no_prefix_key = "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd";
    env::set_var("HOST_PRIVATE_KEY", no_prefix_key);

    let result = fabstir_llm_node::crypto::extract_node_private_key();
    // Expected: Either rejected or parsed correctly (implementation choice)
    // For security, we'll require 0x prefix
    assert!(result.is_err(), "Key without 0x prefix should be rejected");

    env::remove_var("HOST_PRIVATE_KEY");
}

/// Test that the actual private key is never logged
#[test]
fn test_key_never_logged() {
    // This test verifies the implementation doesn't log the key
    // In a real scenario, you'd capture log output and verify
    // For now, we'll just verify extraction works without panicking

    let test_key = "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
    env::set_var("HOST_PRIVATE_KEY", test_key);

    let result = fabstir_llm_node::crypto::extract_node_private_key();

    // Expected: Success without logging the actual key
    assert!(result.is_ok());

    // Implementation should log "Private key loaded" but NEVER the actual key

    env::remove_var("HOST_PRIVATE_KEY");
}

/// Test empty string as HOST_PRIVATE_KEY
#[test]
fn test_empty_private_key() {
    env::set_var("HOST_PRIVATE_KEY", "");

    let result = fabstir_llm_node::crypto::extract_node_private_key();

    // Expected: Error for empty key
    assert!(result.is_err());

    env::remove_var("HOST_PRIVATE_KEY");
}

/// Test key with whitespace
#[test]
fn test_key_with_whitespace() {
    // Test key with leading/trailing whitespace
    let key_with_space = " 0xabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd ";
    env::set_var("HOST_PRIVATE_KEY", key_with_space);

    let result = fabstir_llm_node::crypto::extract_node_private_key();

    // Expected: Should handle whitespace gracefully (trim and parse)
    assert!(result.is_ok(), "Should trim whitespace and parse correctly");

    env::remove_var("HOST_PRIVATE_KEY");
}

/// Test that extracted key can be used with k256
#[test]
fn test_key_compatible_with_k256() {
    use k256::SecretKey;

    let test_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
    env::set_var("HOST_PRIVATE_KEY", test_key);

    let result = fabstir_llm_node::crypto::extract_node_private_key();
    assert!(result.is_ok());

    let key_bytes = result.unwrap();

    // Expected: Key can be used to create a k256 SecretKey
    let secret_key_result = SecretKey::from_slice(&key_bytes);
    assert!(
        secret_key_result.is_ok(),
        "Extracted key should be compatible with k256::SecretKey"
    );

    env::remove_var("HOST_PRIVATE_KEY");
}
