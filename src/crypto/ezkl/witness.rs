//! Witness Data Builder
//!
//! Generates witness data from hash values for circuit proving.
//!
//! ## Usage
//!
//! ```ignore
//! use fabstir_llm_node::crypto::ezkl::witness::WitnessBuilder;
//!
//! let witness = WitnessBuilder::new()
//!     .with_job_id([0u8; 32])
//!     .with_model_hash([1u8; 32])
//!     .with_input_hash([2u8; 32])
//!     .with_output_hash([3u8; 32])
//!     .build()?;
//! ```

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Witness data for circuit proving
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Witness {
    job_id: [u8; 32],
    model_hash: [u8; 32],
    input_hash: [u8; 32],
    output_hash: [u8; 32],
}

/// Builder for creating witness data
#[derive(Debug, Default)]
pub struct WitnessBuilder {
    job_id: Option<[u8; 32]>,
    model_hash: Option<[u8; 32]>,
    input_hash: Option<[u8; 32]>,
    output_hash: Option<[u8; 32]>,
}

impl Witness {
    /// Check if witness is empty
    pub fn is_empty(&self) -> bool {
        false // Witness always has 4 fields
    }

    /// Check if witness is valid
    pub fn is_valid(&self) -> bool {
        true // If constructed, it's valid
    }

    /// Get job_id field
    pub fn job_id(&self) -> &[u8; 32] {
        &self.job_id
    }

    /// Get model_hash field
    pub fn model_hash(&self) -> &[u8; 32] {
        &self.model_hash
    }

    /// Get input_hash field
    pub fn input_hash(&self) -> &[u8; 32] {
        &self.input_hash
    }

    /// Get output_hash field
    pub fn output_hash(&self) -> &[u8; 32] {
        &self.output_hash
    }

    /// Convert witness to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(128);
        bytes.extend_from_slice(&self.job_id);
        bytes.extend_from_slice(&self.model_hash);
        bytes.extend_from_slice(&self.input_hash);
        bytes.extend_from_slice(&self.output_hash);
        bytes
    }

    /// Create witness from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 128 {
            return Err(anyhow!(
                "Witness bytes must be 128 bytes, got {}",
                bytes.len()
            ));
        }

        let mut job_id = [0u8; 32];
        let mut model_hash = [0u8; 32];
        let mut input_hash = [0u8; 32];
        let mut output_hash = [0u8; 32];

        job_id.copy_from_slice(&bytes[0..32]);
        model_hash.copy_from_slice(&bytes[32..64]);
        input_hash.copy_from_slice(&bytes[64..96]);
        output_hash.copy_from_slice(&bytes[96..128]);

        Ok(Self {
            job_id,
            model_hash,
            input_hash,
            output_hash,
        })
    }
}

impl WitnessBuilder {
    /// Create new witness builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set job_id field
    pub fn with_job_id(mut self, job_id: [u8; 32]) -> Self {
        self.job_id = Some(job_id);
        self
    }

    /// Set job_id from string (computes hash)
    pub fn with_job_id_string(mut self, job_id: &str) -> Self {
        let hash = Sha256::digest(job_id.as_bytes());
        self.job_id = Some(hash.into());
        self
    }

    /// Set model_hash field
    pub fn with_model_hash(mut self, model_hash: [u8; 32]) -> Self {
        self.model_hash = Some(model_hash);
        self
    }

    /// Set model_hash from path (computes hash)
    pub fn with_model_path(mut self, model_path: &str) -> Self {
        let hash = Sha256::digest(model_path.as_bytes());
        self.model_hash = Some(hash.into());
        self
    }

    /// Set input_hash field
    pub fn with_input_hash(mut self, input_hash: [u8; 32]) -> Self {
        self.input_hash = Some(input_hash);
        self
    }

    /// Set input_hash from string (computes hash)
    pub fn with_input_string(mut self, input: &str) -> Self {
        let hash = Sha256::digest(input.as_bytes());
        self.input_hash = Some(hash.into());
        self
    }

    /// Set output_hash field
    pub fn with_output_hash(mut self, output_hash: [u8; 32]) -> Self {
        self.output_hash = Some(output_hash);
        self
    }

    /// Set output_hash from string (computes hash)
    pub fn with_output_string(mut self, output: &str) -> Self {
        let hash = Sha256::digest(output.as_bytes());
        self.output_hash = Some(hash.into());
        self
    }

    /// Build witness (validates all fields are present)
    pub fn build(self) -> Result<Witness> {
        let job_id = self
            .job_id
            .ok_or_else(|| anyhow!("job_id is required"))?;
        let model_hash = self
            .model_hash
            .ok_or_else(|| anyhow!("model_hash is required"))?;
        let input_hash = self
            .input_hash
            .ok_or_else(|| anyhow!("input_hash is required"))?;
        let output_hash = self
            .output_hash
            .ok_or_else(|| anyhow!("output_hash is required"))?;

        Ok(Witness {
            job_id,
            model_hash,
            input_hash,
            output_hash,
        })
    }
}

/// Create witness from InferenceResult
#[cfg(feature = "inference")]
pub fn create_witness_from_result(
    result: &crate::results::packager::InferenceResult,
    model_path: &str,
) -> Result<Witness> {
    WitnessBuilder::new()
        .with_job_id_string(&result.job_id)
        .with_model_path(model_path)
        .with_input_string(&result.prompt)
        .with_output_string(&result.response)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_witness_builder() -> Result<()> {
        let witness = WitnessBuilder::new()
            .with_job_id([0u8; 32])
            .with_model_hash([1u8; 32])
            .with_input_hash([2u8; 32])
            .with_output_hash([3u8; 32])
            .build()?;

        assert!(witness.is_valid());
        assert!(!witness.is_empty());
        Ok(())
    }

    #[test]
    fn test_witness_builder_missing_field() {
        let result = WitnessBuilder::new()
            .with_job_id([0u8; 32])
            .with_model_hash([1u8; 32])
            // Missing input_hash and output_hash
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_witness_builder_with_strings() -> Result<()> {
        let witness = WitnessBuilder::new()
            .with_job_id_string("job_123")
            .with_model_path("./models/model.gguf")
            .with_input_string("What is 2+2?")
            .with_output_string("The answer is 4")
            .build()?;

        assert!(witness.is_valid());
        Ok(())
    }

    #[test]
    fn test_witness_serialization() -> Result<()> {
        let witness = WitnessBuilder::new()
            .with_job_id([0u8; 32])
            .with_model_hash([1u8; 32])
            .with_input_hash([2u8; 32])
            .with_output_hash([3u8; 32])
            .build()?;

        let json = serde_json::to_string(&witness)?;
        let deserialized: Witness = serde_json::from_str(&json)?;
        assert_eq!(witness, deserialized);
        Ok(())
    }

    #[test]
    fn test_witness_to_from_bytes() -> Result<()> {
        let witness = WitnessBuilder::new()
            .with_job_id([0u8; 32])
            .with_model_hash([1u8; 32])
            .with_input_hash([2u8; 32])
            .with_output_hash([3u8; 32])
            .build()?;

        let bytes = witness.to_bytes();
        assert_eq!(bytes.len(), 128);

        let reconstructed = Witness::from_bytes(&bytes)?;
        assert_eq!(witness, reconstructed);
        Ok(())
    }

    #[test]
    fn test_witness_from_bytes_invalid_size() {
        let result = Witness::from_bytes(&[0u8; 64]); // Wrong size
        assert!(result.is_err());
    }

    #[test]
    fn test_witness_field_access() -> Result<()> {
        let witness = WitnessBuilder::new()
            .with_job_id([0u8; 32])
            .with_model_hash([1u8; 32])
            .with_input_hash([2u8; 32])
            .with_output_hash([3u8; 32])
            .build()?;

        assert_eq!(witness.job_id()[0], 0);
        assert_eq!(witness.model_hash()[0], 1);
        assert_eq!(witness.input_hash()[0], 2);
        assert_eq!(witness.output_hash()[0], 3);
        Ok(())
    }

    #[test]
    fn test_witness_clone() -> Result<()> {
        let witness = WitnessBuilder::new()
            .with_job_id([0u8; 32])
            .with_model_hash([1u8; 32])
            .with_input_hash([2u8; 32])
            .with_output_hash([3u8; 32])
            .build()?;

        let cloned = witness.clone();
        assert_eq!(witness, cloned);
        Ok(())
    }
}
