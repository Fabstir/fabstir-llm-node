// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! EZKL Availability and Capability Checks
//!
//! Provides functions to check if EZKL is available and what capabilities
//! are supported.

use serde::{Deserialize, Serialize};

/// EZKL Capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EzklCapabilities {
    /// Whether EZKL library is available
    pub available: bool,

    /// Whether using mock implementation
    pub is_mock: bool,

    /// Whether can generate proofs
    pub can_generate_proofs: bool,

    /// Whether can verify proofs
    pub can_verify_proofs: bool,

    /// Whether can compile circuits
    pub can_compile_circuits: bool,

    /// EZKL version (if available)
    pub version: Option<String>,
}

impl EzklCapabilities {
    /// Check EZKL capabilities based on feature flags
    pub fn check() -> Self {
        #[cfg(feature = "real-ezkl")]
        {
            Self {
                available: true,
                is_mock: false,
                can_generate_proofs: true,
                can_verify_proofs: true,
                can_compile_circuits: true,
                version: Some(crate::crypto::ezkl::SUPPORTED_EZKL_VERSION.to_string()),
            }
        }

        #[cfg(not(feature = "real-ezkl"))]
        {
            Self {
                available: false,
                is_mock: true,
                can_generate_proofs: true,   // Mock proofs
                can_verify_proofs: true,     // Mock verification
                can_compile_circuits: false, // No real compilation
                version: None,
            }
        }
    }
}

/// Check if real EZKL is available
///
/// Returns `true` if the `real-ezkl` feature is enabled, `false` otherwise.
pub fn is_ezkl_available() -> bool {
    #[cfg(feature = "real-ezkl")]
    {
        true
    }

    #[cfg(not(feature = "real-ezkl"))]
    {
        false
    }
}

/// Get EZKL version string
///
/// Returns the EZKL version if real EZKL is available, None otherwise.
pub fn get_ezkl_version() -> Option<String> {
    #[cfg(feature = "real-ezkl")]
    {
        Some(crate::crypto::ezkl::SUPPORTED_EZKL_VERSION.to_string())
    }

    #[cfg(not(feature = "real-ezkl"))]
    {
        None
    }
}

/// Initialize EZKL system
///
/// Performs any necessary initialization. Currently a no-op but
/// provides a hook for future initialization logic.
pub fn init_ezkl() -> anyhow::Result<()> {
    #[cfg(feature = "real-ezkl")]
    {
        // TODO: Add real EZKL initialization when needed
        // - Check library version
        // - Initialize any global state
        // - Validate system requirements
        tracing::info!(
            "🔐 Real EZKL initialized (v{})",
            crate::crypto::ezkl::SUPPORTED_EZKL_VERSION
        );
    }

    #[cfg(not(feature = "real-ezkl"))]
    {
        tracing::debug!("🔐 EZKL using mock implementation");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_capabilities() {
        let caps = EzklCapabilities::check();

        #[cfg(feature = "real-ezkl")]
        {
            assert!(caps.available);
            assert!(!caps.is_mock);
            assert!(caps.can_generate_proofs);
            assert!(caps.can_verify_proofs);
            assert!(caps.can_compile_circuits);
            assert!(caps.version.is_some());
        }

        #[cfg(not(feature = "real-ezkl"))]
        {
            assert!(!caps.available);
            assert!(caps.is_mock);
            assert!(caps.can_generate_proofs); // Mock proofs
            assert!(caps.can_verify_proofs); // Mock verification
            assert!(!caps.can_compile_circuits); // No real compilation
            assert!(caps.version.is_none());
        }
    }

    #[test]
    fn test_is_ezkl_available() {
        let available = is_ezkl_available();

        #[cfg(feature = "real-ezkl")]
        assert!(available);

        #[cfg(not(feature = "real-ezkl"))]
        assert!(!available);
    }

    #[test]
    fn test_get_ezkl_version() {
        let version = get_ezkl_version();

        #[cfg(feature = "real-ezkl")]
        {
            assert!(version.is_some());
            let ver = version.unwrap();
            assert!(ver.starts_with("22."));
        }

        #[cfg(not(feature = "real-ezkl"))]
        {
            assert!(version.is_none());
        }
    }

    #[test]
    fn test_init_ezkl_no_panic() {
        // Initialization should not panic
        let result = init_ezkl();
        assert!(result.is_ok());
    }

    #[test]
    fn test_capabilities_serialization() {
        let caps = EzklCapabilities::check();

        // Should be serializable to JSON
        let json = serde_json::to_string(&caps).unwrap();
        assert!(!json.is_empty());

        // Should be deserializable
        let deserialized: EzklCapabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(caps.available, deserialized.available);
        assert_eq!(caps.is_mock, deserialized.is_mock);
    }
}
