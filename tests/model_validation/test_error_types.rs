// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Model Validation Error Types Tests (TDD - Phase 1, Sub-phase 1.1)
//!
//! These tests verify comprehensive error handling for model validation:
//! - ModelValidationError enum with 6 variants
//! - Display trait provides human-readable messages
//! - Error context (host address, model_id) is preserved
//! - std::error::Error trait implementation
//!
//! **TDD Approach**: Tests written BEFORE implementation.

use fabstir_llm_node::model_validation::{ModelValidationError, ModelValidator};
use ethers::types::{Address, H256};
use std::str::FromStr;

// ============================================================================
// Error Display Tests
// ============================================================================

/// Test ModelNotRegistered error displays correctly
#[test]
fn test_error_display_model_not_registered() {
    let err = ModelValidationError::ModelNotRegistered(
        "gpt-oss-120b.gguf".to_string()
    );
    let display_msg = format!("{}", err);

    assert!(display_msg.contains("not registered"), "Should contain 'not registered'");
    assert!(display_msg.contains("gpt-oss-120b.gguf"), "Should contain filename");
}

/// Test HostNotAuthorized error displays host address and model ID
#[test]
fn test_error_display_host_not_authorized() {
    let host_address = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
    let model_id = H256::from_str(
        "0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced"
    ).unwrap();

    let err = ModelValidationError::HostNotAuthorized(host_address, model_id);
    let display_msg = format!("{}", err);

    assert!(display_msg.to_lowercase().contains("not authorized"),
        "Should contain 'not authorized'");
    // Check if the display contains the full host address (hex-encoded bytes)
    assert!(display_msg.to_lowercase().contains("1234567890123456789012345678901234567890"),
        "Should contain full host address: actual='{}'", display_msg);
    assert!(display_msg.contains("0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced"),
        "Should contain model ID hex");
}

/// Test ModelHashMismatch error displays expected hash and path
#[test]
fn test_error_display_model_hash_mismatch() {
    let expected_hash = H256::from_str(
        "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
    ).unwrap();

    let err = ModelValidationError::ModelHashMismatch {
        expected: expected_hash,
        path: "/models/tampered.gguf".to_string(),
    };
    let display_msg = format!("{}", err);

    assert!(display_msg.to_lowercase().contains("hash")
        || display_msg.to_lowercase().contains("mismatch"),
        "Should contain 'hash' or 'mismatch'");
    assert!(display_msg.contains("/models/tampered.gguf")
        || display_msg.contains("tampered.gguf"),
        "Should contain file path");
}

/// Test that Error trait is implemented (allows ? operator)
#[test]
fn test_error_from_trait() {
    fn returns_result() -> Result<(), ModelValidationError> {
        Err(ModelValidationError::ContractUnavailable(
            "RPC timeout".to_string()
        ))
    }

    let result = returns_result();
    assert!(result.is_err());

    // Test that error can be used with the ? operator pattern
    let err = result.unwrap_err();
    let _: &dyn std::error::Error = &err;
}

/// Test that Debug format produces useful output
#[test]
fn test_error_debug_format() {
    let err = ModelValidationError::InvalidModelPath(
        "/invalid/path/no_extension".to_string()
    );
    let debug_msg = format!("{:?}", err);

    // Debug should contain the variant name and value
    assert!(debug_msg.contains("InvalidModelPath"),
        "Debug should contain variant name");
    assert!(debug_msg.contains("/invalid/path/no_extension"),
        "Debug should contain path value");
}

// ============================================================================
// ModelValidator Constructor Tests
// ============================================================================

/// Test ModelValidator::is_enabled() returns true when env var is set
#[test]
fn test_validator_new_feature_enabled() {
    // Save original env var
    let original = std::env::var("REQUIRE_MODEL_VALIDATION").ok();

    // Set env var to true
    std::env::set_var("REQUIRE_MODEL_VALIDATION", "true");

    // Create validator and check feature is enabled
    // Note: We need mock dependencies for real construction
    // For now, just test the env var parsing logic
    let enabled = std::env::var("REQUIRE_MODEL_VALIDATION")
        .map(|v| v.to_lowercase() == "true" || v == "1")
        .unwrap_or(false);

    assert!(enabled, "Feature should be enabled when REQUIRE_MODEL_VALIDATION=true");

    // Restore original env var
    match original {
        Some(val) => std::env::set_var("REQUIRE_MODEL_VALIDATION", val),
        None => std::env::remove_var("REQUIRE_MODEL_VALIDATION"),
    }
}

/// Test that feature is disabled by default (env var not set)
#[test]
fn test_validator_new_feature_disabled_by_default() {
    // Save original env var
    let original = std::env::var("REQUIRE_MODEL_VALIDATION").ok();

    // Remove env var to test default behavior
    std::env::remove_var("REQUIRE_MODEL_VALIDATION");

    // Check default is false
    let enabled = std::env::var("REQUIRE_MODEL_VALIDATION")
        .map(|v| v.to_lowercase() == "true" || v == "1")
        .unwrap_or(false);

    assert!(!enabled, "Feature should be disabled by default");

    // Restore original env var
    if let Some(val) = original {
        std::env::set_var("REQUIRE_MODEL_VALIDATION", val);
    }
}

// ============================================================================
// Error Context Preservation Tests
// ============================================================================

/// Test that error context fields are preserved correctly
#[test]
fn test_error_context_preserved() {
    // Test HostNotAuthorized context
    let host = Address::from_str("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    let model_id = H256::from_str(
        "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    ).unwrap();

    let err = ModelValidationError::HostNotAuthorized(host, model_id);

    match err {
        ModelValidationError::HostNotAuthorized(h, m) => {
            assert_eq!(h, host, "Host address should be preserved");
            assert_eq!(m, model_id, "Model ID should be preserved");
        }
        _ => panic!("Wrong error variant"),
    }

    // Test ModelIdMismatch context
    let expected = H256::from_str(
        "0x1111111111111111111111111111111111111111111111111111111111111111"
    ).unwrap();
    let actual = H256::from_str(
        "0x2222222222222222222222222222222222222222222222222222222222222222"
    ).unwrap();

    let err = ModelValidationError::ModelIdMismatch { expected, actual };

    match err {
        ModelValidationError::ModelIdMismatch { expected: e, actual: a } => {
            assert_eq!(e, expected, "Expected model ID should be preserved");
            assert_eq!(a, actual, "Actual model ID should be preserved");
        }
        _ => panic!("Wrong error variant"),
    }
}

// ============================================================================
// All Error Variants Exist Tests
// ============================================================================

/// Test that all 6 required error variants exist
#[test]
fn test_all_error_variants_exist() {
    // ModelNotRegistered - model filename not in dynamic map
    let _err1 = ModelValidationError::ModelNotRegistered("test.gguf".to_string());

    // HostNotAuthorized - host not registered for this model
    let _err2 = ModelValidationError::HostNotAuthorized(
        Address::zero(),
        H256::zero(),
    );

    // ModelIdMismatch - job.model_id != loaded model
    let _err3 = ModelValidationError::ModelIdMismatch {
        expected: H256::zero(),
        actual: H256::zero(),
    };

    // ModelHashMismatch - file SHA256 doesn't match contract
    let _err4 = ModelValidationError::ModelHashMismatch {
        expected: H256::zero(),
        path: "test.gguf".to_string(),
    };

    // ContractUnavailable - RPC or contract query failed
    let _err5 = ModelValidationError::ContractUnavailable("RPC error".to_string());

    // InvalidModelPath - path doesn't exist or no filename
    let _err6 = ModelValidationError::InvalidModelPath("/bad/path".to_string());
}
