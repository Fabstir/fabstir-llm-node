// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Circuit Setup and Key Generation
//!
//! Handles circuit compilation and proving/verification key generation.
//!
//! ## Usage
//!
//! ```ignore
//! use fabstir_llm_node::crypto::ezkl::setup::{compile_circuit, generate_keys};
//!
//! // Compile circuit
//! let circuit = CommitmentCircuit::new(...);
//! let compiled = compile_circuit(&circuit)?;
//!
//! // Generate keys (one-time setup)
//! let (proving_key, verifying_key) = generate_keys(&compiled)?;
//! ```

use super::circuit::CommitmentCircuit;
use anyhow::{anyhow, Result};
use std::fs;
use std::path::Path;

/// Compiled circuit data
#[derive(Debug, Clone)]
pub struct CompiledCircuit {
    pub circuit: CommitmentCircuit,
    pub compiled_data: Vec<u8>,
}

/// Proving key
#[derive(Debug, Clone)]
pub struct ProvingKey {
    pub key_data: Vec<u8>,
}

/// Verification key
#[derive(Debug, Clone)]
pub struct VerificationKey {
    pub key_data: Vec<u8>,
}

/// Compile circuit for proving
///
/// In mock mode, this just validates the circuit.
/// With real EZKL, this would compile to a proving circuit.
pub fn compile_circuit(circuit: &CommitmentCircuit) -> Result<CompiledCircuit> {
    #[cfg(feature = "real-ezkl")]
    {
        // TODO: Real EZKL circuit compilation
        // This would use the actual EZKL library to compile the circuit
        return Err(anyhow!("Real EZKL compilation not yet implemented"));
    }

    #[cfg(not(feature = "real-ezkl"))]
    {
        // Mock compilation: validate and serialize
        if !circuit.is_valid() {
            return Err(anyhow!("Circuit validation failed"));
        }

        // Mock compiled data: just serialize the circuit
        let compiled_data = serde_json::to_vec(circuit)?;

        Ok(CompiledCircuit {
            circuit: circuit.clone(),
            compiled_data,
        })
    }
}

/// Generate proving and verification keys
///
/// In mock mode, this generates placeholder keys.
/// With real EZKL, this would generate actual cryptographic keys.
pub fn generate_keys(compiled: &CompiledCircuit) -> Result<(ProvingKey, VerificationKey)> {
    #[cfg(feature = "real-ezkl")]
    {
        // TODO: Real EZKL key generation
        // This would use the actual EZKL library to generate keys
        return Err(anyhow!("Real EZKL key generation not yet implemented"));
    }

    #[cfg(not(feature = "real-ezkl"))]
    {
        // Mock key generation
        // In production, these would be large cryptographic keys

        // Mock proving key (in reality: ~100-300 MB)
        let proving_key = ProvingKey {
            key_data: vec![0xAA; 1000], // Placeholder
        };

        // Mock verification key (in reality: ~10-50 MB)
        let verifying_key = VerificationKey {
            key_data: vec![0xBB; 500], // Placeholder
        };

        Ok((proving_key, verifying_key))
    }
}

/// Save proving key to file
pub fn save_proving_key(key: &ProvingKey, path: &Path) -> Result<()> {
    // Create parent directory if it doesn't exist
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, &key.key_data)?;
    tracing::info!(
        "📝 Saved proving key to {:?} ({} bytes)",
        path,
        key.key_data.len()
    );
    Ok(())
}

/// Save verification key to file
pub fn save_verifying_key(key: &VerificationKey, path: &Path) -> Result<()> {
    // Create parent directory if it doesn't exist
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, &key.key_data)?;
    tracing::info!(
        "📝 Saved verification key to {:?} ({} bytes)",
        path,
        key.key_data.len()
    );
    Ok(())
}

/// Load proving key from file
pub fn load_proving_key(path: &Path) -> Result<ProvingKey> {
    if !path.exists() {
        return Err(anyhow!("Proving key not found at {:?}", path));
    }

    let key_data = fs::read(path)?;
    tracing::info!(
        "📖 Loaded proving key from {:?} ({} bytes)",
        path,
        key_data.len()
    );

    Ok(ProvingKey { key_data })
}

/// Load verification key from file
pub fn load_verifying_key(path: &Path) -> Result<VerificationKey> {
    if !path.exists() {
        return Err(anyhow!("Verification key not found at {:?}", path));
    }

    let key_data = fs::read(path)?;
    tracing::info!(
        "📖 Loaded verification key from {:?} ({} bytes)",
        path,
        key_data.len()
    );

    Ok(VerificationKey { key_data })
}

/// Validate proving key
pub fn validate_proving_key(key: &ProvingKey) -> Result<()> {
    if key.key_data.is_empty() {
        return Err(anyhow!("Proving key is empty"));
    }

    #[cfg(not(feature = "real-ezkl"))]
    {
        // Mock validation: check placeholder format
        if key.key_data[0] != 0xAA {
            return Err(anyhow!("Invalid mock proving key format"));
        }
    }

    Ok(())
}

/// Validate verification key
pub fn validate_verifying_key(key: &VerificationKey) -> Result<()> {
    if key.key_data.is_empty() {
        return Err(anyhow!("Verification key is empty"));
    }

    #[cfg(not(feature = "real-ezkl"))]
    {
        // Mock validation: check placeholder format
        if key.key_data[0] != 0xBB {
            return Err(anyhow!("Invalid mock verification key format"));
        }
    }

    Ok(())
}

/// Check if keys are compatible (match the same circuit)
pub fn keys_are_compatible(proving_key: &ProvingKey, verifying_key: &VerificationKey) -> bool {
    #[cfg(not(feature = "real-ezkl"))]
    {
        // Mock check: both should be non-empty with correct markers
        !proving_key.key_data.is_empty()
            && !verifying_key.key_data.is_empty()
            && proving_key.key_data[0] == 0xAA
            && verifying_key.key_data[0] == 0xBB
    }

    #[cfg(feature = "real-ezkl")]
    {
        // Real check would verify cryptographic relationship
        // TODO: Implement real key compatibility check
        !proving_key.key_data.is_empty() && !verifying_key.key_data.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // Note: Setup tests are only for EZKL (SNARK proofs with circuit compilation and key generation)
    // Risc0 uses transparent setup (no keys, no circuit compilation), so these tests don't apply

    #[test]
    #[cfg(not(feature = "real-ezkl"))]
    fn test_compile_circuit() -> Result<()> {
        let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
        let compiled = compile_circuit(&circuit)?;
        assert!(!compiled.compiled_data.is_empty());
        Ok(())
    }

    #[test]
    #[cfg(not(feature = "real-ezkl"))]
    fn test_generate_keys() -> Result<()> {
        let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
        let compiled = compile_circuit(&circuit)?;
        let (proving_key, verifying_key) = generate_keys(&compiled)?;

        assert!(!proving_key.key_data.is_empty());
        assert!(!verifying_key.key_data.is_empty());
        Ok(())
    }

    #[test]
    #[cfg(not(feature = "real-ezkl"))]
    fn test_save_and_load_proving_key() -> Result<()> {
        let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
        let compiled = compile_circuit(&circuit)?;
        let (proving_key, _) = generate_keys(&compiled)?;

        let temp_dir = TempDir::new()?;
        let key_path = temp_dir.path().join("proving_key.bin");

        save_proving_key(&proving_key, &key_path)?;
        assert!(key_path.exists());

        let loaded_key = load_proving_key(&key_path)?;
        assert_eq!(proving_key.key_data, loaded_key.key_data);
        Ok(())
    }

    #[test]
    #[cfg(not(feature = "real-ezkl"))]
    fn test_save_and_load_verifying_key() -> Result<()> {
        let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
        let compiled = compile_circuit(&circuit)?;
        let (_, verifying_key) = generate_keys(&compiled)?;

        let temp_dir = TempDir::new()?;
        let key_path = temp_dir.path().join("verifying_key.bin");

        save_verifying_key(&verifying_key, &key_path)?;
        assert!(key_path.exists());

        let loaded_key = load_verifying_key(&key_path)?;
        assert_eq!(verifying_key.key_data, loaded_key.key_data);
        Ok(())
    }

    #[test]
    #[cfg(not(feature = "real-ezkl"))]
    fn test_validate_keys() -> Result<()> {
        let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
        let compiled = compile_circuit(&circuit)?;
        let (proving_key, verifying_key) = generate_keys(&compiled)?;

        assert!(validate_proving_key(&proving_key).is_ok());
        assert!(validate_verifying_key(&verifying_key).is_ok());
        Ok(())
    }

    #[test]
    #[cfg(not(feature = "real-ezkl"))]
    fn test_keys_compatibility() -> Result<()> {
        let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
        let compiled = compile_circuit(&circuit)?;
        let (proving_key, verifying_key) = generate_keys(&compiled)?;

        assert!(keys_are_compatible(&proving_key, &verifying_key));
        Ok(())
    }

    #[test]
    fn test_load_nonexistent_key() {
        let result = load_proving_key(Path::new("/nonexistent/key.bin"));
        assert!(result.is_err());
    }
}
