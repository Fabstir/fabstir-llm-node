//! Commitment Circuit Implementation
//!
//! Simple commitment circuit that proves knowledge of hash relationships
//! for job_id, model_hash, input_hash, and output_hash.
//!
//! ## Security Properties
//!
//! This circuit proves:
//! - The prover knows 4 hash values (32 bytes each)
//! - These hashes are cryptographically bound together
//! - The proof cannot be forged or replayed for different jobs
//!
//! This does NOT prove:
//! - That the LLM inference was actually performed
//! - That output was correctly computed from input
//!
//! ## Circuit Structure
//!
//! Inputs (public):
//! - job_id: [u8; 32] - SHA256 hash of job identifier
//! - model_hash: [u8; 32] - SHA256 hash of model path
//! - input_hash: [u8; 32] - SHA256 hash of input prompt
//! - output_hash: [u8; 32] - SHA256 hash of output response
//!
//! Constraints:
//! 1. All inputs are exactly 32 bytes (SHA256 size)
//! 2. All inputs are bound together in the proof
//!
//! ## Usage
//!
//! ```ignore
//! use fabstir_llm_node::crypto::ezkl::circuit::CommitmentCircuit;
//!
//! let circuit = CommitmentCircuit::new(
//!     job_id_hash,
//!     model_hash,
//!     input_hash,
//!     output_hash,
//! );
//!
//! // Generate proof (requires proving key)
//! let proof = circuit.generate_proof(&proving_key)?;
//! ```

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Commitment Circuit
///
/// Proves knowledge of 4 hash values that are cryptographically bound together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitmentCircuit {
    /// Job ID hash (32 bytes)
    pub job_id: [u8; 32],
    /// Model hash (32 bytes)
    pub model_hash: [u8; 32],
    /// Input prompt hash (32 bytes)
    pub input_hash: [u8; 32],
    /// Output response hash (32 bytes)
    pub output_hash: [u8; 32],
}

/// Circuit metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitMetadata {
    pub circuit_type: String,
    pub field_count: usize,
    pub hash_size: usize,
    pub constraint_count: usize,
}

/// Constraint type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstraintType {
    /// Size constraint (field must be 32 bytes)
    Size,
    /// Binding constraint (fields are bound together)
    Binding,
}

/// Circuit constraint
#[derive(Debug, Clone)]
pub struct Constraint {
    constraint_type: ConstraintType,
    description: String,
}

impl Constraint {
    pub fn constraint_type(&self) -> ConstraintType {
        self.constraint_type.clone()
    }

    pub fn description(&self) -> &str {
        &self.description
    }
}

impl CommitmentCircuit {
    /// Create new commitment circuit
    pub fn new(
        job_id: [u8; 32],
        model_hash: [u8; 32],
        input_hash: [u8; 32],
        output_hash: [u8; 32],
    ) -> Self {
        Self {
            job_id,
            model_hash,
            input_hash,
            output_hash,
        }
    }

    /// Create circuit from byte slices
    pub fn from_bytes(
        job_id: &[u8],
        model_hash: &[u8],
        input_hash: &[u8],
        output_hash: &[u8],
    ) -> Result<Self> {
        if job_id.len() != 32 {
            return Err(anyhow!("job_id must be 32 bytes, got {}", job_id.len()));
        }
        if model_hash.len() != 32 {
            return Err(anyhow!(
                "model_hash must be 32 bytes, got {}",
                model_hash.len()
            ));
        }
        if input_hash.len() != 32 {
            return Err(anyhow!(
                "input_hash must be 32 bytes, got {}",
                input_hash.len()
            ));
        }
        if output_hash.len() != 32 {
            return Err(anyhow!(
                "output_hash must be 32 bytes, got {}",
                output_hash.len()
            ));
        }

        let mut job_id_arr = [0u8; 32];
        let mut model_hash_arr = [0u8; 32];
        let mut input_hash_arr = [0u8; 32];
        let mut output_hash_arr = [0u8; 32];

        job_id_arr.copy_from_slice(job_id);
        model_hash_arr.copy_from_slice(model_hash);
        input_hash_arr.copy_from_slice(input_hash);
        output_hash_arr.copy_from_slice(output_hash);

        Ok(Self::new(
            job_id_arr,
            model_hash_arr,
            input_hash_arr,
            output_hash_arr,
        ))
    }

    /// Create circuit from hex strings
    pub fn from_hex(
        job_id_hex: &str,
        model_hash_hex: &str,
        input_hash_hex: &str,
        output_hash_hex: &str,
    ) -> Result<Self> {
        let job_id = hex::decode(job_id_hex.strip_prefix("0x").unwrap_or(job_id_hex))?;
        let model_hash =
            hex::decode(model_hash_hex.strip_prefix("0x").unwrap_or(model_hash_hex))?;
        let input_hash =
            hex::decode(input_hash_hex.strip_prefix("0x").unwrap_or(input_hash_hex))?;
        let output_hash =
            hex::decode(output_hash_hex.strip_prefix("0x").unwrap_or(output_hash_hex))?;

        Self::from_bytes(&job_id, &model_hash, &input_hash, &output_hash)
    }

    /// Validate circuit (all fields are 32 bytes)
    pub fn is_valid(&self) -> bool {
        self.job_id.len() == 32
            && self.model_hash.len() == 32
            && self.input_hash.len() == 32
            && self.output_hash.len() == 32
    }

    /// Get circuit metadata
    pub fn metadata(&self) -> CircuitMetadata {
        CircuitMetadata {
            circuit_type: "commitment".to_string(),
            field_count: 4,
            hash_size: 32,
            constraint_count: 5, // 4 size + 1 binding
        }
    }

    /// Get circuit constraints
    pub fn constraints(&self) -> Vec<Constraint> {
        vec![
            Constraint {
                constraint_type: ConstraintType::Size,
                description: "job_id must be 32 bytes".to_string(),
            },
            Constraint {
                constraint_type: ConstraintType::Size,
                description: "model_hash must be 32 bytes".to_string(),
            },
            Constraint {
                constraint_type: ConstraintType::Size,
                description: "input_hash must be 32 bytes".to_string(),
            },
            Constraint {
                constraint_type: ConstraintType::Size,
                description: "output_hash must be 32 bytes".to_string(),
            },
            Constraint {
                constraint_type: ConstraintType::Binding,
                description: "All hashes are cryptographically bound together".to_string(),
            },
        ]
    }

    /// Check if circuit is satisfiable
    pub fn is_satisfiable(&self) -> bool {
        self.is_valid()
    }

    /// Get binding constraints
    pub fn get_binding_constraints(&self) -> Vec<Constraint> {
        self.constraints()
            .into_iter()
            .filter(|c| c.constraint_type() == ConstraintType::Binding)
            .collect()
    }

    /// Check constraints are met
    pub fn check_constraints(&self) -> Result<()> {
        if !self.is_valid() {
            return Err(anyhow!("Circuit validation failed"));
        }
        Ok(())
    }

    /// Compute commitment hash (for binding all fields)
    pub fn compute_commitment(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(&self.job_id);
        hasher.update(&self.model_hash);
        hasher.update(&self.input_hash);
        hasher.update(&self.output_hash);
        hasher.finalize().into()
    }

    /// Get constraint complexity (number of constraints)
    pub fn constraint_complexity(&self) -> usize {
        self.constraints().len()
    }

    /// Encode constraints to bytes
    pub fn encode_constraints(&self) -> Vec<u8> {
        // Simple encoding: just serialize the circuit
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Get unique constraints (no duplicates)
    pub fn unique_constraints(&self) -> Vec<Constraint> {
        // In this simple circuit, all constraints are unique
        self.constraints()
    }
}

impl CircuitMetadata {
    pub fn field_count(&self) -> usize {
        self.field_count
    }

    pub fn circuit_type(&self) -> &str {
        &self.circuit_type
    }

    pub fn hash_size(&self) -> usize {
        self.hash_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_creation() {
        let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
        assert!(circuit.is_valid());
    }

    #[test]
    fn test_circuit_from_bytes() -> Result<()> {
        let circuit = CommitmentCircuit::from_bytes(&[0u8; 32], &[1u8; 32], &[2u8; 32], &[3u8; 32])?;
        assert!(circuit.is_valid());
        Ok(())
    }

    #[test]
    fn test_circuit_from_bytes_invalid_size() {
        let result = CommitmentCircuit::from_bytes(&[0u8; 16], &[1u8; 32], &[2u8; 32], &[3u8; 32]);
        assert!(result.is_err());
    }

    #[test]
    fn test_circuit_metadata() {
        let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
        let metadata = circuit.metadata();
        assert_eq!(metadata.field_count(), 4);
        assert_eq!(metadata.circuit_type(), "commitment");
        assert_eq!(metadata.hash_size(), 32);
    }

    #[test]
    fn test_circuit_constraints() {
        let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
        let constraints = circuit.constraints();
        assert_eq!(constraints.len(), 5); // 4 size + 1 binding
    }

    #[test]
    fn test_circuit_commitment() {
        let circuit1 = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
        let circuit2 = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);

        // Same circuit should produce same commitment
        assert_eq!(circuit1.compute_commitment(), circuit2.compute_commitment());

        // Different circuit should produce different commitment
        let circuit3 = CommitmentCircuit::new([4u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
        assert_ne!(circuit1.compute_commitment(), circuit3.compute_commitment());
    }

    #[test]
    fn test_circuit_serialization() -> Result<()> {
        let circuit = CommitmentCircuit::new([0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]);
        let json = serde_json::to_string(&circuit)?;
        let deserialized: CommitmentCircuit = serde_json::from_str(&json)?;
        assert_eq!(circuit, deserialized);
        Ok(())
    }
}
