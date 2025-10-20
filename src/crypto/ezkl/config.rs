// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! EZKL Configuration Module
//!
//! Provides configuration for EZKL proof generation from environment variables.

use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;

/// EZKL Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EzklConfig {
    /// Enable real EZKL proofs (vs mock)
    pub enabled: bool,

    /// Path to proving key
    pub proving_key_path: PathBuf,

    /// Path to verification key
    pub verifying_key_path: PathBuf,

    /// Path to compiled circuit
    pub circuit_path: PathBuf,

    /// Maximum proof size in bytes
    pub max_proof_size: usize,

    /// Proof cache size (number of proofs to cache)
    pub cache_size: usize,
}

impl Default for EzklConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Default to mock
            proving_key_path: PathBuf::from("./keys/proving_key.bin"),
            verifying_key_path: PathBuf::from("./keys/verifying_key.bin"),
            circuit_path: PathBuf::from("./circuits/commitment.circuit"),
            max_proof_size: 10_000, // 10KB max
            cache_size: 100, // Cache 100 proofs
        }
    }
}

impl EzklConfig {
    /// Create configuration from environment variables
    ///
    /// Environment variables:
    /// - `ENABLE_REAL_EZKL`: true/false (default: false)
    /// - `EZKL_PROVING_KEY_PATH`: Path to proving key
    /// - `EZKL_VERIFYING_KEY_PATH`: Path to verification key
    /// - `EZKL_CIRCUIT_PATH`: Path to compiled circuit
    /// - `EZKL_MAX_PROOF_SIZE`: Maximum proof size in bytes
    /// - `EZKL_CACHE_SIZE`: Number of proofs to cache
    pub fn from_env() -> Self {
        Self {
            enabled: env::var("ENABLE_REAL_EZKL")
                .unwrap_or_else(|_| "false".to_string())
                .parse()
                .unwrap_or(false),
            proving_key_path: env::var("EZKL_PROVING_KEY_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("./keys/proving_key.bin")),
            verifying_key_path: env::var("EZKL_VERIFYING_KEY_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("./keys/verifying_key.bin")),
            circuit_path: env::var("EZKL_CIRCUIT_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("./circuits/commitment.circuit")),
            max_proof_size: env::var("EZKL_MAX_PROOF_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10_000),
            cache_size: env::var("EZKL_CACHE_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(100),
        }
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.max_proof_size == 0 {
            return Err("max_proof_size must be > 0".to_string());
        }

        if self.max_proof_size > 1_000_000 {
            return Err("max_proof_size too large (max 1MB)".to_string());
        }

        if self.cache_size > 10_000 {
            return Err("cache_size too large (max 10000)".to_string());
        }

        Ok(())
    }

    /// Check if real EZKL is enabled (both feature and config)
    pub fn is_real_ezkl_enabled(&self) -> bool {
        #[cfg(feature = "real-ezkl")]
        {
            self.enabled
        }

        #[cfg(not(feature = "real-ezkl"))]
        {
            false // Always false without feature flag
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = EzklConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.max_proof_size, 10_000);
        assert_eq!(config.cache_size, 100);
    }

    #[test]
    fn test_config_validation() {
        let mut config = EzklConfig::default();
        assert!(config.validate().is_ok());

        // Test invalid max_proof_size
        config.max_proof_size = 0;
        assert!(config.validate().is_err());

        config.max_proof_size = 2_000_000;
        assert!(config.validate().is_err());

        // Test invalid cache_size
        config.max_proof_size = 10_000; // Reset
        config.cache_size = 20_000;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_from_env_defaults() {
        // Clear any environment variables
        env::remove_var("ENABLE_REAL_EZKL");
        env::remove_var("EZKL_PROVING_KEY_PATH");
        env::remove_var("EZKL_MAX_PROOF_SIZE");

        let config = EzklConfig::from_env();
        assert!(!config.enabled);
        assert_eq!(config.proving_key_path, PathBuf::from("./keys/proving_key.bin"));
        assert_eq!(config.max_proof_size, 10_000);
    }

    #[test]
    fn test_from_env_with_values() {
        env::set_var("ENABLE_REAL_EZKL", "true");
        env::set_var("EZKL_MAX_PROOF_SIZE", "20000");
        env::set_var("EZKL_CACHE_SIZE", "200");

        let config = EzklConfig::from_env();
        assert!(config.enabled);
        assert_eq!(config.max_proof_size, 20_000);
        assert_eq!(config.cache_size, 200);

        // Cleanup
        env::remove_var("ENABLE_REAL_EZKL");
        env::remove_var("EZKL_MAX_PROOF_SIZE");
        env::remove_var("EZKL_CACHE_SIZE");
    }

    #[test]
    fn test_is_real_ezkl_enabled_with_feature() {
        let mut config = EzklConfig::default();

        // When config.enabled = false
        config.enabled = false;
        assert!(!config.is_real_ezkl_enabled());

        // When config.enabled = true
        config.enabled = true;
        #[cfg(feature = "real-ezkl")]
        assert!(config.is_real_ezkl_enabled());

        #[cfg(not(feature = "real-ezkl"))]
        assert!(!config.is_real_ezkl_enabled()); // Always false without feature
    }

    #[test]
    fn test_paths_are_pathbuf() {
        let config = EzklConfig::default();
        assert!(config.proving_key_path.to_str().is_some());
        assert!(config.verifying_key_path.to_str().is_some());
        assert!(config.circuit_path.to_str().is_some());
    }
}
