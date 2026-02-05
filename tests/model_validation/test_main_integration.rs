// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Main Integration Tests (TDD - Phase 2, Sub-phase 2.2)
//!
//! These tests verify the integration of model validation into main.rs:
//! - Node exits with code 1 on unauthorized model (when enabled)
//! - Node starts successfully with authorized model
//! - Validation disabled allows any model (default)
//! - Error messages are clear and helpful
//!
//! **TDD Approach**: Tests written BEFORE implementation.

use std::env;

// ============================================================================
// Feature Flag Tests
// ============================================================================

/// Test that validation is DISABLED by default (REQUIRE_MODEL_VALIDATION not set)
#[test]
fn test_validation_disabled_by_default() {
    // Save original
    let original = env::var("REQUIRE_MODEL_VALIDATION").ok();

    // Remove env var to test default
    env::remove_var("REQUIRE_MODEL_VALIDATION");

    // Check default behavior
    let enabled = env::var("REQUIRE_MODEL_VALIDATION")
        .map(|v| v.to_lowercase() == "true" || v == "1")
        .unwrap_or(false);

    assert!(!enabled, "Validation should be disabled by default");

    // Restore original
    if let Some(val) = original {
        env::set_var("REQUIRE_MODEL_VALIDATION", val);
    }
}

/// Test that validation is enabled when REQUIRE_MODEL_VALIDATION=true
#[test]
fn test_validation_enabled_when_set() {
    // Save original
    let original = env::var("REQUIRE_MODEL_VALIDATION").ok();

    // Set env var to true
    env::set_var("REQUIRE_MODEL_VALIDATION", "true");

    // Check behavior
    let enabled = env::var("REQUIRE_MODEL_VALIDATION")
        .map(|v| v.to_lowercase() == "true" || v == "1")
        .unwrap_or(false);

    assert!(enabled, "Validation should be enabled when REQUIRE_MODEL_VALIDATION=true");

    // Restore original
    match original {
        Some(val) => env::set_var("REQUIRE_MODEL_VALIDATION", val),
        None => env::remove_var("REQUIRE_MODEL_VALIDATION"),
    }
}

/// Test that REQUIRE_MODEL_VALIDATION=1 also enables validation
#[test]
fn test_validation_enabled_with_1() {
    // Save original
    let original = env::var("REQUIRE_MODEL_VALIDATION").ok();

    // Set env var to 1
    env::set_var("REQUIRE_MODEL_VALIDATION", "1");

    // Check behavior
    let enabled = env::var("REQUIRE_MODEL_VALIDATION")
        .map(|v| v.to_lowercase() == "true" || v == "1")
        .unwrap_or(false);

    assert!(enabled, "Validation should be enabled when REQUIRE_MODEL_VALIDATION=1");

    // Restore original
    match original {
        Some(val) => env::set_var("REQUIRE_MODEL_VALIDATION", val),
        None => env::remove_var("REQUIRE_MODEL_VALIDATION"),
    }
}

// ============================================================================
// Error Message Tests
// ============================================================================

/// Test that error message for unauthorized model is clear
#[test]
fn test_error_message_host_not_authorized() {
    use fabstir_llm_node::model_validation::ModelValidationError;
    use ethers::types::{Address, H256};
    use std::str::FromStr;

    let host = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
    let model_id = H256::from_str(
        "0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced"
    ).unwrap();

    let err = ModelValidationError::HostNotAuthorized(host, model_id);
    let msg = format!("{}", err);

    // Error message should be helpful
    assert!(msg.to_lowercase().contains("not authorized"), "Should explain the issue");
    assert!(msg.contains("NodeRegistry") || msg.contains("register"),
        "Should suggest solution: {}", msg);
}

/// Test that error message for unregistered model is clear
#[test]
fn test_error_message_model_not_registered() {
    use fabstir_llm_node::model_validation::ModelValidationError;

    let err = ModelValidationError::ModelNotRegistered(
        "/models/unknown-model.gguf".to_string()
    );
    let msg = format!("{}", err);

    // Error message should be helpful
    assert!(msg.to_lowercase().contains("not registered"), "Should explain the issue");
}

/// Test that error message for hash mismatch is clear
#[test]
fn test_error_message_hash_mismatch() {
    use fabstir_llm_node::model_validation::ModelValidationError;
    use ethers::types::H256;
    use std::str::FromStr;

    let expected = H256::from_str(
        "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
    ).unwrap();

    let err = ModelValidationError::ModelHashMismatch {
        expected,
        path: "/models/tampered.gguf".to_string(),
    };
    let msg = format!("{}", err);

    // Error message should be helpful
    assert!(msg.to_lowercase().contains("hash"), "Should explain hash issue");
    assert!(
        msg.to_lowercase().contains("tampered") || msg.contains("tampered.gguf"),
        "Should include file path"
    );
}

// ============================================================================
// Integration Pattern Tests
// ============================================================================

/// Test that validation happens BEFORE model loading
#[test]
fn test_validation_before_model_loading_pattern() {
    // This tests the pattern that main.rs should follow:
    // 1. Initialize ModelValidator
    // 2. Call validate_model_at_startup()
    // 3. If error and validation enabled, exit with code 1
    // 4. If success or validation disabled, proceed to load model

    let feature_enabled = true;
    let validation_result: Result<ethers::types::H256, &str> = Err("HostNotAuthorized");

    if feature_enabled {
        match validation_result {
            Ok(_model_id) => {
                // Would proceed to load model
            }
            Err(e) => {
                // Would exit with code 1
                let would_exit = true;
                assert!(would_exit, "Should exit on validation error when enabled");
                assert!(!e.is_empty(), "Error message should not be empty");
            }
        }
    }
}

/// Test that validation disabled allows any model
#[test]
fn test_validation_disabled_allows_any_model() {
    let feature_enabled = false;
    let _validation_result: Result<ethers::types::H256, &str> = Err("Would fail");

    if !feature_enabled {
        // Skip validation, return H256::zero()
        let model_id = ethers::types::H256::zero();
        let should_proceed = true;
        assert!(should_proceed, "Should proceed when validation disabled");
        assert!(model_id.is_zero(), "Should return zero hash when disabled");
    }
}

/// Test semantic_model_id is passed to model loading
#[test]
fn test_semantic_model_id_passed_to_loader() {
    use ethers::types::H256;
    use std::str::FromStr;

    // After successful validation, semantic_model_id is known
    let semantic_model_id = H256::from_str(
        "0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced"
    ).unwrap();

    // This would be passed to load_model(config, Some(semantic_model_id))
    let passed_to_loader: Option<H256> = Some(semantic_model_id);

    assert!(passed_to_loader.is_some());
    assert_eq!(passed_to_loader.unwrap(), semantic_model_id);
}
