// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// src/ezkl/mod.rs - Example structure for Claude Code

pub mod batch_proofs;
pub mod integration;
pub mod proof_creation;
pub mod verification;

// Re-export main types for convenience
pub use integration::{
    CircuitConfig, EZKLConfig, EZKLError, EZKLIntegration, IntegrationStatus, ModelCircuit,
    ModelCompatibility, ProofArtifacts, ProofBackend, ProofSystem, ProvingKey, ResourceMetrics,
    VerifyingKey, Witness,
};

pub use proof_creation::{
    CompressionLevel, InferenceData, ModelInput, ModelOutput, PerformanceMetrics, ProofError,
    ProofFormat, ProofGenerator, ProofMetadata, ProofRequest, ProofResult, ProofStatus,
};

pub use batch_proofs::{
    AdaptiveMetrics, AggregatedProof, AggregationMethod, BatchError, BatchProofError,
    BatchProofGenerator, BatchProofRequest, BatchProofResult, BatchProofStatus, BatchProofStream,
    BatchStrategy, ChunkResult, ParallelismConfig, ProofEntry,
    ResourceMetrics as BatchResourceMetrics,
};

pub use verification::{
    BatchVerificationResult, ConstraintResult, OnChainVerifier, ProofData, ProofVerifier,
    PublicInputs, TrustLevel, VerificationError, VerificationMetrics, VerificationMode,
    VerificationRequest, VerificationResult, VerificationStatus,
};

// Common types used across modules
#[derive(Debug, Clone)]
pub struct ProofHash(pub String);

#[derive(Debug, Clone)]
pub struct CircuitHash(pub String);

// Utility functions
pub fn compute_proof_hash(data: &[u8]) -> ProofHash {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    ProofHash(format!("{:x}", hasher.finalize()))
}

pub fn compute_circuit_hash(circuit: &[u8]) -> CircuitHash {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(circuit);
    CircuitHash(format!("{}", hasher.finalize()))
}

// Version information
pub const EZKL_VERSION: &str = "0.1.0";
pub const PROOF_SYSTEM_VERSION: &str = "v1.0.0";

// Error types are already re-exported above

// Testing utilities (only available in test builds)
#[cfg(test)]
pub mod test_utils {
    use super::*;

    pub fn create_mock_proof() -> Vec<u8> {
        vec![1, 2, 3, 4, 5, 6, 7, 8]
    }

    pub fn create_mock_verifying_key() -> Vec<u8> {
        vec![9, 10, 11, 12, 13, 14, 15, 16]
    }

    pub fn create_mock_circuit() -> Vec<u8> {
        vec![17, 18, 19, 20, 21, 22, 23, 24]
    }
}
