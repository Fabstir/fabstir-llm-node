// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! EZKL Zero-Knowledge Proof Module
//!
//! This module provides integration with EZKL for generating commitment-based
//! zero-knowledge proofs. When the `real-ezkl` feature is disabled, it falls
//! back to mock implementations for development and testing.
//!
//! ## Feature Flags
//!
//! - `real-ezkl`: Enable real EZKL proof generation (requires EZKL library)
//!   - Default: OFF (uses mock implementation)
//!   - Enable with: `cargo build --features real-ezkl`
//!
//! ## Module Structure
//!
//! - `config`: Environment-based configuration
//! - `availability`: Library availability checks
//! - `circuit`: Circuit definitions for commitment proofs
//! - `witness`: Witness data generation from hashes
//! - `setup`: Key generation and circuit compilation
//! - `prover`: Proof generation (Phase 2.1)
//! - `error`: EZKL-specific error types (Phase 2.1)
//! - `key_manager`: Key loading and caching (Phase 2.2)
//! - `cache`: Proof caching with LRU eviction (Phase 2.2)
//! - `metrics`: Prometheus metrics (Phase 2.2)
//!
//! ## Usage
//!
//! ```ignore
//! use fabstir_llm_node::crypto::ezkl::availability::is_ezkl_available;
//!
//! if is_ezkl_available() {
//!     // Use real EZKL proofs
//! } else {
//!     // Use mock implementation
//! }
//! ```

pub mod availability;
pub mod cache;
pub mod circuit;
pub mod config;
pub mod error;
pub mod key_manager;
pub mod metrics;
pub mod prover;
pub mod setup;
pub mod verifier;
pub mod witness;

// Re-export commonly used types
pub use availability::{is_ezkl_available, EzklCapabilities};
pub use cache::{CacheStats, ProofCache};
pub use circuit::{CommitmentCircuit, CircuitMetadata};
pub use config::EzklConfig;
pub use error::{EzklError, EzklResult};
pub use key_manager::{KeyCacheStats, KeyManager};
pub use metrics::{global_metrics, EzklMetrics};
pub use prover::{generate_proof, generate_proof_from_circuit, EzklProver, ProofData};
pub use setup::{compile_circuit, generate_keys, keys_are_compatible, load_proving_key, load_verifying_key, ProvingKey, VerificationKey};
pub use verifier::{verify_proof as verify_ezkl_proof, verify_proof_bytes, EzklVerifier};
pub use witness::{Witness, WitnessBuilder};

/// Module version
pub const MODULE_VERSION: &str = "0.1.0";

/// Supported EZKL version
pub const SUPPORTED_EZKL_VERSION: &str = "22.3.0";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_version() {
        assert!(!MODULE_VERSION.is_empty());
        assert_eq!(MODULE_VERSION, "0.1.0");
    }

    #[test]
    fn test_supported_ezkl_version() {
        assert!(!SUPPORTED_EZKL_VERSION.is_empty());
        assert!(SUPPORTED_EZKL_VERSION.starts_with("22."));
    }

    #[test]
    fn test_module_exports() {
        // Verify that key types are exported
        // This will fail to compile if exports are missing
        let _config: Option<EzklConfig> = None;
        let _caps: Option<EzklCapabilities> = None;
    }
}
