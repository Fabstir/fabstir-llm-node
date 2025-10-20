// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! EZKL-Specific Error Types
//!
//! Error types for EZKL proof generation and verification operations.
//! These errors provide detailed context for debugging and user-facing messages.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during EZKL operations
#[derive(Debug, Error)]
pub enum EzklError {
    /// Proving key not found at specified path
    #[error("Proving key not found at {path:?}")]
    ProvingKeyNotFound { path: PathBuf },

    /// Verification key not found at specified path
    #[error("Verification key not found at {path:?}")]
    VerifyingKeyNotFound { path: PathBuf },

    /// Invalid proving key format
    #[error("Invalid proving key format: {reason}")]
    InvalidProvingKey { reason: String },

    /// Invalid verification key format
    #[error("Invalid verification key format: {reason}")]
    InvalidVerifyingKey { reason: String },

    /// Proving key is empty
    #[error("Proving key is empty")]
    EmptyProvingKey,

    /// Verification key is empty
    #[error("Verification key is empty")]
    EmptyVerifyingKey,

    /// Keys are incompatible (don't match the same circuit)
    #[error("Proving and verification keys are incompatible")]
    IncompatibleKeys,

    /// Circuit compilation failed
    #[error("Circuit compilation failed: {reason}")]
    CircuitCompilationFailed { reason: String },

    /// Circuit validation failed
    #[error("Circuit validation failed: {reason}")]
    CircuitValidationFailed { reason: String },

    /// Witness generation failed
    #[error("Witness generation failed: {reason}")]
    WitnessGenerationFailed { reason: String },

    /// Witness is invalid
    #[error("Invalid witness: {reason}")]
    InvalidWitness { reason: String },

    /// Proof generation failed
    #[error("Proof generation failed: {reason}")]
    ProofGenerationFailed { reason: String },

    /// Proof verification failed
    #[error("Proof verification failed: {reason}")]
    ProofVerificationFailed { reason: String },

    /// Proof is invalid
    #[error("Invalid proof: {reason}")]
    InvalidProof { reason: String },

    /// Proof timeout
    #[error("Proof generation timed out after {seconds}s")]
    ProofTimeout { seconds: u64 },

    /// Key loading failed
    #[error("Failed to load key from {path:?}: {reason}")]
    KeyLoadFailed { path: PathBuf, reason: String },

    /// Key saving failed
    #[error("Failed to save key to {path:?}: {reason}")]
    KeySaveFailed { path: PathBuf, reason: String },

    /// Circuit not compiled
    #[error("Circuit must be compiled before {operation}")]
    CircuitNotCompiled { operation: String },

    /// Feature not available
    #[error("EZKL feature not available: {feature}. Enable with --features real-ezkl")]
    FeatureNotAvailable { feature: String },

    /// EZKL library not available
    #[error("EZKL library is not available. Using mock implementation.")]
    EzklNotAvailable,

    /// Configuration error
    #[error("EZKL configuration error: {reason}")]
    ConfigError { reason: String },

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Other error (catch-all)
    #[error("EZKL error: {0}")]
    Other(String),
}

/// Result type for EZKL operations
pub type EzklResult<T> = Result<T, EzklError>;

impl EzklError {
    /// Create a ProofGenerationFailed error
    pub fn proof_generation_failed(reason: impl Into<String>) -> Self {
        Self::ProofGenerationFailed {
            reason: reason.into(),
        }
    }

    /// Create a ProofVerificationFailed error
    pub fn proof_verification_failed(reason: impl Into<String>) -> Self {
        Self::ProofVerificationFailed {
            reason: reason.into(),
        }
    }

    /// Create a CircuitCompilationFailed error
    pub fn circuit_compilation_failed(reason: impl Into<String>) -> Self {
        Self::CircuitCompilationFailed {
            reason: reason.into(),
        }
    }

    /// Create a WitnessGenerationFailed error
    pub fn witness_generation_failed(reason: impl Into<String>) -> Self {
        Self::WitnessGenerationFailed {
            reason: reason.into(),
        }
    }

    /// Create a ConfigError
    pub fn config_error(reason: impl Into<String>) -> Self {
        Self::ConfigError {
            reason: reason.into(),
        }
    }

    /// Check if this error is recoverable
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::ProofTimeout { .. }
                | Self::ConfigError { .. }
                | Self::Io(_)
                | Self::EzklNotAvailable
        )
    }

    /// Check if this error indicates a key problem
    pub fn is_key_error(&self) -> bool {
        matches!(
            self,
            Self::ProvingKeyNotFound { .. }
                | Self::VerifyingKeyNotFound { .. }
                | Self::InvalidProvingKey { .. }
                | Self::InvalidVerifyingKey { .. }
                | Self::EmptyProvingKey
                | Self::EmptyVerifyingKey
                | Self::IncompatibleKeys
                | Self::KeyLoadFailed { .. }
                | Self::KeySaveFailed { .. }
        )
    }

    /// Check if this error indicates a proof problem
    pub fn is_proof_error(&self) -> bool {
        matches!(
            self,
            Self::ProofGenerationFailed { .. }
                | Self::ProofVerificationFailed { .. }
                | Self::InvalidProof { .. }
                | Self::ProofTimeout { .. }
        )
    }

    /// Check if this error indicates a circuit problem
    pub fn is_circuit_error(&self) -> bool {
        matches!(
            self,
            Self::CircuitCompilationFailed { .. }
                | Self::CircuitValidationFailed { .. }
                | Self::CircuitNotCompiled { .. }
        )
    }

    /// Get user-friendly error message with suggestions
    pub fn user_message(&self) -> String {
        match self {
            Self::ProvingKeyNotFound { path } => {
                format!(
                    "Proving key not found at {:?}. Run ./scripts/generate_ezkl_keys.sh to generate keys.",
                    path
                )
            }
            Self::VerifyingKeyNotFound { path } => {
                format!(
                    "Verification key not found at {:?}. Run ./scripts/generate_ezkl_keys.sh to generate keys.",
                    path
                )
            }
            Self::InvalidProvingKey { reason } => {
                format!(
                    "Proving key is invalid: {}. Regenerate keys with ./scripts/generate_ezkl_keys.sh",
                    reason
                )
            }
            Self::InvalidVerifyingKey { reason } => {
                format!(
                    "Verification key is invalid: {}. Regenerate keys with ./scripts/generate_ezkl_keys.sh",
                    reason
                )
            }
            Self::ProofTimeout { seconds } => {
                format!(
                    "Proof generation timed out after {}s. Check system resources or increase timeout.",
                    seconds
                )
            }
            Self::EzklNotAvailable => {
                "EZKL library is not available. Using mock implementation. \
                 To enable real EZKL, build with: cargo build --features real-ezkl"
                    .to_string()
            }
            Self::FeatureNotAvailable { feature } => {
                format!(
                    "Feature '{}' requires real EZKL. Build with: cargo build --features real-ezkl",
                    feature
                )
            }
            _ => self.to_string(),
        }
    }
}

/// Convert anyhow::Error to EzklError
impl From<anyhow::Error> for EzklError {
    fn from(err: anyhow::Error) -> Self {
        Self::Other(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = EzklError::ProofGenerationFailed {
            reason: "test reason".to_string(),
        };
        assert!(err.to_string().contains("test reason"));
    }

    #[test]
    fn test_proof_generation_failed() {
        let err = EzklError::proof_generation_failed("test");
        assert!(err.to_string().contains("test"));
    }

    #[test]
    fn test_is_recoverable() {
        let recoverable = EzklError::ProofTimeout { seconds: 5 };
        assert!(recoverable.is_recoverable());

        let not_recoverable = EzklError::InvalidProvingKey {
            reason: "bad key".to_string(),
        };
        assert!(!not_recoverable.is_recoverable());
    }

    #[test]
    fn test_is_key_error() {
        let key_error = EzklError::ProvingKeyNotFound {
            path: PathBuf::from("/test"),
        };
        assert!(key_error.is_key_error());

        let not_key_error = EzklError::ProofGenerationFailed {
            reason: "test".to_string(),
        };
        assert!(!not_key_error.is_key_error());
    }

    #[test]
    fn test_is_proof_error() {
        let proof_error = EzklError::ProofGenerationFailed {
            reason: "test".to_string(),
        };
        assert!(proof_error.is_proof_error());

        let not_proof_error = EzklError::ProvingKeyNotFound {
            path: PathBuf::from("/test"),
        };
        assert!(!not_proof_error.is_proof_error());
    }

    #[test]
    fn test_is_circuit_error() {
        let circuit_error = EzklError::CircuitCompilationFailed {
            reason: "test".to_string(),
        };
        assert!(circuit_error.is_circuit_error());

        let not_circuit_error = EzklError::ProofGenerationFailed {
            reason: "test".to_string(),
        };
        assert!(!not_circuit_error.is_circuit_error());
    }

    #[test]
    fn test_user_message() {
        let err = EzklError::ProvingKeyNotFound {
            path: PathBuf::from("/test/key.bin"),
        };
        let msg = err.user_message();
        assert!(msg.contains("generate_ezkl_keys.sh"));
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let ezkl_err = EzklError::from(io_err);
        assert!(matches!(ezkl_err, EzklError::Io(_)));
    }

    #[test]
    fn test_from_anyhow_error() {
        let anyhow_err = anyhow::anyhow!("test error");
        let ezkl_err = EzklError::from(anyhow_err);
        assert!(matches!(ezkl_err, EzklError::Other(_)));
    }
}
