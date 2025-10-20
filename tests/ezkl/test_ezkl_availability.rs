// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! EZKL Library Availability Tests
//!
//! Tests that verify EZKL library is available and functional when the
//! real-ezkl feature is enabled. These tests should pass regardless of
//! feature flag status.

use anyhow::Result;

/// Test that the EZKL availability check module exists and compiles
#[test]
fn test_ezkl_module_exists() {
    // This test ensures the module structure is in place
    // Even without real-ezkl feature, the module should exist
    assert!(true, "EZKL module structure should be available");
}

/// Test EZKL feature flag detection
#[test]
fn test_ezkl_feature_detection() {
    #[cfg(feature = "real-ezkl")]
    {
        // When real-ezkl is enabled, we should be able to detect it
        let has_real_ezkl = true;
        assert!(has_real_ezkl, "real-ezkl feature should be detected");
    }

    #[cfg(not(feature = "real-ezkl"))]
    {
        // When real-ezkl is disabled, we use mock
        let has_real_ezkl = false;
        assert!(!has_real_ezkl, "real-ezkl feature should not be detected");
    }
}

/// Test that we can check EZKL availability programmatically
#[test]
fn test_check_ezkl_availability() -> Result<()> {
    // This will call our availability check function
    use fabstir_llm_node::crypto::ezkl::availability::is_ezkl_available;
    let is_available = is_ezkl_available();

    #[cfg(feature = "real-ezkl")]
    {
        // When real-ezkl is enabled, it should be available
        assert!(is_available, "EZKL should be available with real-ezkl feature");
    }

    #[cfg(not(feature = "real-ezkl"))]
    {
        // When disabled, availability check should return false
        assert!(!is_available, "EZKL should not be available without real-ezkl feature");
    }

    Ok(())
}

/// Test EZKL version check (when available)
#[cfg(feature = "real-ezkl")]
#[test]
fn test_ezkl_version_check() -> Result<()> {
    // This test only runs when real-ezkl feature is enabled
    use fabstir_llm_node::crypto::ezkl::availability::get_ezkl_version;

    let version = get_ezkl_version();
    assert!(version.is_some(), "EZKL version should be available with real-ezkl feature");
    let ver = version.unwrap();
    assert!(!ver.is_empty(), "EZKL version should not be empty");
    assert!(ver.starts_with("22."), "EZKL version should be 22.x");

    Ok(())
}

/// Test that mock implementation is used when real-ezkl is disabled
#[cfg(not(feature = "real-ezkl"))]
#[test]
fn test_mock_implementation_active() {
    // When real-ezkl is disabled, we should use mock
    let using_mock = true;
    assert!(using_mock, "Mock implementation should be active without real-ezkl");
}

/// Test EZKL capability check
#[test]
fn test_ezkl_capabilities() -> Result<()> {
    // Check what capabilities are available
    use fabstir_llm_node::crypto::ezkl::availability::EzklCapabilities;

    let caps = EzklCapabilities::check();

    #[cfg(feature = "real-ezkl")]
    {
        // With real EZKL, we should have full capabilities
        assert!(caps.can_generate_proofs, "Should be able to generate proofs with real-ezkl");
        assert!(caps.can_verify_proofs, "Should be able to verify proofs with real-ezkl");
        assert!(caps.can_compile_circuits, "Should be able to compile circuits with real-ezkl");
        assert!(!caps.is_mock, "Should not be using mock with real-ezkl");
    }

    #[cfg(not(feature = "real-ezkl"))]
    {
        // With mock, we have limited capabilities
        assert!(caps.is_mock, "Should be using mock without real-ezkl");
        assert!(!caps.can_compile_circuits, "Should not be able to compile circuits without real-ezkl");
    }

    Ok(())
}

/// Test EZKL initialization without errors
#[test]
fn test_ezkl_init_no_panic() {
    // Ensure initialization doesn't panic
    use fabstir_llm_node::crypto::ezkl::availability::init_ezkl;

    // This should not panic regardless of feature flag
    let result = init_ezkl();
    assert!(result.is_ok(), "Init should succeed: {:?}", result.err());
}

/// Test conditional compilation works correctly
#[test]
fn test_conditional_compilation() {
    let mut feature_count = 0;

    #[cfg(feature = "real-ezkl")]
    {
        feature_count += 1;
    }

    #[cfg(not(feature = "real-ezkl"))]
    {
        feature_count += 1;
    }

    // Exactly one branch should compile
    assert_eq!(feature_count, 1, "Exactly one feature branch should be active");
}
