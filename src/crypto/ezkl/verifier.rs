//! EZKL Proof Verification
//!
//! Handles verification of EZKL zero-knowledge proofs for commitment circuits.
//! Supports both real EZKL (with feature flag) and mock implementation.

use super::error::{EzklError, EzklResult};
use super::prover::ProofData;
use super::setup::{load_verifying_key, validate_verifying_key, VerificationKey};
use super::witness::Witness;
use std::path::Path;

/// EZKL proof verifier
pub struct EzklVerifier {
    /// Cached verification key
    verification_key: Option<VerificationKey>,
    /// Path to verification key file
    verification_key_path: Option<std::path::PathBuf>,
}

impl EzklVerifier {
    /// Create new verifier without preloaded keys
    pub fn new() -> Self {
        Self {
            verification_key: None,
            verification_key_path: None,
        }
    }

    /// Create new verifier with verification key path
    pub fn with_key_path(key_path: impl AsRef<Path>) -> Self {
        Self {
            verification_key: None,
            verification_key_path: Some(key_path.as_ref().to_path_buf()),
        }
    }

    /// Create new verifier with preloaded verification key
    pub fn with_key(verification_key: VerificationKey) -> EzklResult<Self> {
        validate_verifying_key(&verification_key)?;
        Ok(Self {
            verification_key: Some(verification_key),
            verification_key_path: None,
        })
    }

    /// Load verification key from configured path or provided path
    pub fn load_key(&mut self, key_path: Option<&Path>) -> EzklResult<&VerificationKey> {
        // If key already loaded, return it
        if self.verification_key.is_some() {
            return Ok(self.verification_key.as_ref().unwrap());
        }

        // Determine key path to use
        let path = key_path
            .or_else(|| self.verification_key_path.as_deref())
            .ok_or_else(|| {
                EzklError::config_error("No verification key path configured or provided")
            })?;

        // Load key from file
        tracing::info!("📖 Loading verification key from {:?}", path);
        let key = load_verifying_key(path).map_err(|e| EzklError::KeyLoadFailed {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;

        // Validate key
        validate_verifying_key(&key)?;

        // Cache key
        self.verification_key = Some(key);
        Ok(self.verification_key.as_ref().unwrap())
    }

    /// Verify proof from proof data and witness
    ///
    /// This is the main entry point for proof verification.
    /// It handles both mock and real EZKL implementations based on feature flags.
    pub fn verify_proof(&mut self, proof: &ProofData, witness: &Witness) -> EzklResult<bool> {
        tracing::debug!("🔍 Verifying EZKL proof");

        // Validate witness
        if !witness.is_valid() {
            return Err(EzklError::InvalidWitness {
                reason: "Witness validation failed".to_string(),
            });
        }

        // Validate proof data is not empty
        if proof.proof_bytes.is_empty() {
            return Err(EzklError::ProofVerificationFailed {
                reason: "Proof data is empty".to_string(),
            });
        }

        // Check proof size is reasonable (not too small or too large)
        if proof.proof_bytes.len() < 10 {
            return Err(EzklError::ProofVerificationFailed {
                reason: format!("Proof too small: {} bytes", proof.proof_bytes.len()),
            });
        }

        if proof.proof_bytes.len() > 100_000 {
            return Err(EzklError::ProofVerificationFailed {
                reason: format!("Proof too large: {} bytes", proof.proof_bytes.len()),
            });
        }

        // Verify hashes match between proof and witness
        if proof.model_hash != *witness.model_hash() {
            tracing::debug!("❌ Model hash mismatch");
            return Ok(false);
        }

        if proof.input_hash != *witness.input_hash() {
            tracing::debug!("❌ Input hash mismatch");
            return Ok(false);
        }

        if proof.output_hash != *witness.output_hash() {
            tracing::debug!("❌ Output hash mismatch");
            return Ok(false);
        }

        // Verify based on feature flag
        #[cfg(feature = "real-ezkl")]
        {
            self.verify_real_proof(proof, witness)
        }

        #[cfg(not(feature = "real-ezkl"))]
        {
            self.verify_mock_proof(proof, witness)
        }
    }

    /// Verify proof directly from bytes with public inputs
    ///
    /// This is a lower-level interface that takes proof bytes and public inputs directly.
    pub fn verify_proof_bytes(
        &mut self,
        proof_bytes: &[u8],
        public_inputs: &[&[u8; 32]],
    ) -> EzklResult<bool> {
        tracing::debug!("🔍 Verifying EZKL proof from bytes");

        // Validate inputs
        if proof_bytes.is_empty() {
            return Err(EzklError::ProofVerificationFailed {
                reason: "Proof bytes are empty".to_string(),
            });
        }

        if public_inputs.len() < 3 {
            return Err(EzklError::ProofVerificationFailed {
                reason: format!(
                    "Expected at least 3 public inputs, got {}",
                    public_inputs.len()
                ),
            });
        }

        // Verify based on feature flag
        #[cfg(feature = "real-ezkl")]
        {
            self.verify_real_proof_bytes(proof_bytes, public_inputs)
        }

        #[cfg(not(feature = "real-ezkl"))]
        {
            self.verify_mock_proof_bytes(proof_bytes, public_inputs)
        }
    }

    /// Verify mock proof (when real-ezkl feature is disabled)
    ///
    /// This checks the mock proof structure for testing and development.
    #[cfg(not(feature = "real-ezkl"))]
    fn verify_mock_proof(&self, proof: &ProofData, _witness: &Witness) -> EzklResult<bool> {
        tracing::debug!("🎭 Verifying mock EZKL proof");

        // Mock proof verification:
        // - Check proof has EZKL marker (0xEF)
        // - Check proof has reasonable size (>= 200 bytes)

        if proof.proof_bytes.len() < 200 {
            tracing::debug!("❌ Mock proof too small: {} bytes", proof.proof_bytes.len());
            return Ok(false);
        }

        if proof.proof_bytes[0] != 0xEF {
            tracing::debug!(
                "❌ Mock proof missing EZKL marker: 0x{:02X}",
                proof.proof_bytes[0]
            );
            return Ok(false);
        }

        tracing::info!("✅ Mock EZKL proof verified");
        Ok(true)
    }

    /// Verify mock proof from bytes
    #[cfg(not(feature = "real-ezkl"))]
    fn verify_mock_proof_bytes(&self, proof_bytes: &[u8], _public_inputs: &[&[u8; 32]]) -> EzklResult<bool> {
        tracing::debug!("🎭 Verifying mock EZKL proof from bytes");

        // Same checks as verify_mock_proof
        if proof_bytes.len() < 200 {
            return Ok(false);
        }

        if proof_bytes[0] != 0xEF {
            return Ok(false);
        }

        tracing::info!("✅ Mock EZKL proof bytes verified");
        Ok(true)
    }

    /// Verify real EZKL proof (when real-ezkl feature is enabled)
    ///
    /// This uses the actual EZKL library to verify SNARK proofs.
    #[cfg(feature = "real-ezkl")]
    fn verify_real_proof(&mut self, proof: &ProofData, witness: &Witness) -> EzklResult<bool> {
        tracing::info!("🔐 Verifying real EZKL proof");

        // Load verification key if not already loaded
        let _verification_key = self.load_key(None)?;

        // TODO: Implement real EZKL proof verification
        // This will be implemented when EZKL library is integrated
        // Steps:
        // 1. Extract public inputs from witness
        // 2. Call EZKL verification API with proof and public inputs
        // 3. Return verification result

        Err(EzklError::ProofVerificationFailed {
            reason: "Real EZKL proof verification not yet implemented. \
                     This requires nightly Rust and uncommenting EZKL dependencies in Cargo.toml"
                .to_string(),
        })
    }

    /// Verify real EZKL proof from bytes
    #[cfg(feature = "real-ezkl")]
    fn verify_real_proof_bytes(
        &mut self,
        proof_bytes: &[u8],
        public_inputs: &[&[u8; 32]],
    ) -> EzklResult<bool> {
        tracing::info!("🔐 Verifying real EZKL proof from bytes");

        // Load verification key if not already loaded
        let _verification_key = self.load_key(None)?;

        // TODO: Implement real EZKL proof verification from bytes
        Err(EzklError::ProofVerificationFailed {
            reason: "Real EZKL proof verification from bytes not yet implemented"
                .to_string(),
        })
    }
}

impl Default for EzklVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for EzklVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EzklVerifier")
            .field(
                "verification_key",
                &if self.verification_key.is_some() {
                    "Some(<cached>)"
                } else {
                    "None"
                },
            )
            .field("verification_key_path", &self.verification_key_path)
            .finish()
    }
}

/// Helper function to verify proof from proof data and witness (convenience function)
pub fn verify_proof(
    proof: &ProofData,
    witness: &Witness,
    verification_key_path: Option<&Path>,
) -> EzklResult<bool> {
    let mut verifier = if let Some(path) = verification_key_path {
        EzklVerifier::with_key_path(path)
    } else {
        EzklVerifier::new()
    };

    verifier.verify_proof(proof, witness)
}

/// Helper function to verify proof from bytes with public inputs
pub fn verify_proof_bytes(
    proof_bytes: &[u8],
    public_inputs: &[&[u8; 32]],
    verification_key_path: Option<&Path>,
) -> EzklResult<bool> {
    let mut verifier = if let Some(path) = verification_key_path {
        EzklVerifier::with_key_path(path)
    } else {
        EzklVerifier::new()
    };

    verifier.verify_proof_bytes(proof_bytes, public_inputs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::ezkl::WitnessBuilder;

    fn create_test_witness() -> Witness {
        WitnessBuilder::new()
            .with_job_id([0u8; 32])
            .with_model_hash([1u8; 32])
            .with_input_hash([2u8; 32])
            .with_output_hash([3u8; 32])
            .build()
            .unwrap()
    }

    #[test]
    fn test_verifier_new() {
        let verifier = EzklVerifier::new();
        assert!(verifier.verification_key.is_none());
        assert!(verifier.verification_key_path.is_none());
    }

    #[test]
    fn test_verifier_with_key_path() {
        let verifier = EzklVerifier::with_key_path("/test/vk.key");
        assert!(verifier.verification_key.is_none());
        assert!(verifier.verification_key_path.is_some());
    }

    #[test]
    #[cfg(not(feature = "real-ezkl"))]
    fn test_verify_mock_proof() -> EzklResult<()> {
        use crate::crypto::ezkl::EzklProver;

        let witness = create_test_witness();

        // Generate mock proof
        let mut prover = EzklProver::new();
        let proof = prover.generate_proof(&witness)?;

        // Verify proof
        let mut verifier = EzklVerifier::new();
        let is_valid = verifier.verify_proof(&proof, &witness)?;

        assert!(is_valid, "Mock proof should verify");
        Ok(())
    }

    #[test]
    fn test_verify_empty_proof() {
        let witness = create_test_witness();

        let empty_proof = ProofData {
            proof_bytes: vec![],
            timestamp: 1234567890,
            model_hash: *witness.model_hash(),
            input_hash: *witness.input_hash(),
            output_hash: *witness.output_hash(),
        };

        let mut verifier = EzklVerifier::new();
        let result = verifier.verify_proof(&empty_proof, &witness);

        assert!(result.is_err(), "Empty proof should error");
    }

    #[test]
    fn test_verify_hash_mismatch() -> EzklResult<()> {
        use crate::crypto::ezkl::EzklProver;

        let witness = create_test_witness();

        // Generate proof
        let mut prover = EzklProver::new();
        let proof = prover.generate_proof(&witness)?;

        // Create witness with different hashes
        let wrong_witness = WitnessBuilder::new()
            .with_job_id([99u8; 32])
            .with_model_hash([99u8; 32])
            .with_input_hash([99u8; 32])
            .with_output_hash([99u8; 32])
            .build()?;

        // Verify should fail
        let mut verifier = EzklVerifier::new();
        let is_valid = verifier.verify_proof(&proof, &wrong_witness)?;

        assert!(!is_valid, "Proof with wrong hashes should not verify");
        Ok(())
    }

    #[test]
    fn test_convenience_function() -> EzklResult<()> {
        use crate::crypto::ezkl::EzklProver;

        let witness = create_test_witness();

        let mut prover = EzklProver::new();
        let proof = prover.generate_proof(&witness)?;

        let is_valid = verify_proof(&proof, &witness, None)?;

        #[cfg(not(feature = "real-ezkl"))]
        assert!(is_valid, "Convenience function should work");

        Ok(())
    }

    #[test]
    fn test_verifier_debug_format() {
        let verifier = EzklVerifier::new();
        let debug_str = format!("{:?}", verifier);
        assert!(debug_str.contains("EzklVerifier"));
    }
}
