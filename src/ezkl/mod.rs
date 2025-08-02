// src/ezkl/mod.rs - Example structure for Claude Code

pub mod integration;
pub mod proof_creation;
pub mod batch_proofs;
pub mod verification;

// Re-export main types for convenience
pub use integration::{
    EZKLIntegration, EZKLConfig, ProofSystem, ModelCircuit,
    CircuitConfig, ProofBackend, ProvingKey, VerifyingKey,
    EZKLError, IntegrationStatus, ProofArtifacts,
    ModelCompatibility, Witness, ResourceMetrics
};

pub use proof_creation::{
    ProofGenerator, ProofRequest, ProofResult, ProofMetadata,
    ProofFormat, CompressionLevel, ProofError, ProofStatus,
    InferenceData, ModelInput, ModelOutput, PerformanceMetrics
};

pub use batch_proofs::{
    BatchProofGenerator, BatchProofRequest, BatchProofResult,
    BatchStrategy, AggregationMethod, ParallelismConfig,
    BatchProofStatus, BatchProofError, BatchProofStream,
    ChunkResult, AdaptiveMetrics, ResourceMetrics as BatchResourceMetrics,
    AggregatedProof, ProofEntry, BatchError
};

pub use verification::{
    ProofVerifier, VerificationRequest, VerificationResult,
    VerificationStatus, ProofData, PublicInputs,
    VerificationError, VerificationMode, TrustLevel,
    OnChainVerifier, VerificationMetrics, BatchVerificationResult,
    ConstraintResult
};

// Common types used across modules
#[derive(Debug, Clone)]
pub struct ProofHash(pub String);

#[derive(Debug, Clone)]
pub struct CircuitHash(pub String);

// Utility functions
pub fn compute_proof_hash(data: &[u8]) -> ProofHash {
    use sha2::{Sha256, Digest};
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