use super::packager::{InferenceResult, PackagedResult};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceProof {
    pub job_id: String,
    pub model_hash: String,
    pub input_hash: String,
    pub output_hash: String,
    pub proof_data: Vec<u8>,
    pub proof_type: ProofType,
    pub timestamp: DateTime<Utc>,
    pub prover_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProofType {
    EZKL,
    Risc0,
    Simple, // For testing
}

#[derive(Debug, Clone)]
pub struct ProofGenerationConfig {
    pub proof_type: ProofType,
    pub model_path: String,
    pub settings_path: Option<String>,
    pub max_proof_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiableResult {
    pub packaged_result: PackagedResult,
    pub proof: InferenceProof,
    pub verification_key: Vec<u8>,
}

#[derive(Clone)]
pub struct ProofGenerator {
    config: ProofGenerationConfig,
    node_id: String,
}

impl ProofGenerator {
    pub fn new(config: ProofGenerationConfig, node_id: String) -> Self {
        Self { config, node_id }
    }

    pub async fn generate_proof(&self, result: &InferenceResult) -> Result<InferenceProof> {
        // Hash model, input, and output
        let model_hash = self.compute_data_hash(self.config.model_path.as_bytes());
        let input_hash = self.compute_data_hash(result.prompt.as_bytes());
        let output_hash = self.compute_data_hash(result.response.as_bytes());

        // Generate proof based on type
        let proof_data = match self.config.proof_type {
            ProofType::Simple => {
                // Simple proof: concatenate hashes and sign
                let mut combined = Vec::new();
                combined.extend_from_slice(model_hash.as_bytes());
                combined.extend_from_slice(input_hash.as_bytes());
                combined.extend_from_slice(output_hash.as_bytes());

                // Hash the combined data as proof
                let proof_hash = self.compute_data_hash(&combined);
                proof_hash.into_bytes()
            }
            ProofType::EZKL => {
                // Simulate EZKL proof generation
                let mut proof = vec![0xEF; 200]; // Mock EZKL proof header
                proof.extend_from_slice(model_hash.as_bytes());
                proof.extend_from_slice(input_hash.as_bytes());
                proof.extend_from_slice(output_hash.as_bytes());

                // Ensure we don't exceed max size
                if proof.len() > self.config.max_proof_size {
                    proof.truncate(self.config.max_proof_size);
                }
                proof
            }
            ProofType::Risc0 => {
                // Simulate Risc0 proof generation
                let mut proof = vec![0xAB; 150]; // Mock Risc0 proof
                proof.extend_from_slice(&[0xCD; 50]);
                proof
            }
        };

        Ok(InferenceProof {
            job_id: result.job_id.clone(),
            model_hash,
            input_hash,
            output_hash,
            proof_data,
            proof_type: self.config.proof_type.clone(),
            timestamp: Utc::now(),
            prover_id: self.node_id.clone(),
        })
    }

    pub async fn create_verifiable_result(
        &self,
        packaged_result: PackagedResult,
    ) -> Result<VerifiableResult> {
        let proof = self.generate_proof(&packaged_result.result).await?;

        // Generate verification key (mock for now)
        let verification_key = match self.config.proof_type {
            ProofType::Simple => vec![0x01, 0x02, 0x03, 0x04],
            ProofType::EZKL => vec![0xEF; 32],
            ProofType::Risc0 => vec![0xAB; 64],
        };

        Ok(VerifiableResult {
            packaged_result,
            proof,
            verification_key,
        })
    }

    pub async fn verify_proof(
        &self,
        proof: &InferenceProof,
        result: &InferenceResult,
    ) -> Result<bool> {
        // Recompute hashes
        let model_hash = self.compute_data_hash(self.config.model_path.as_bytes());
        let input_hash = self.compute_data_hash(result.prompt.as_bytes());
        let output_hash = self.compute_data_hash(result.response.as_bytes());

        // Check if hashes match
        if proof.model_hash != model_hash
            || proof.input_hash != input_hash
            || proof.output_hash != output_hash
        {
            return Ok(false);
        }

        // Verify based on proof type
        match proof.proof_type {
            ProofType::Simple => {
                // For simple proof, just verify the structure
                Ok(!proof.proof_data.is_empty())
            }
            ProofType::EZKL => {
                // Mock EZKL verification
                Ok(proof.proof_data.len() >= 200 && proof.proof_data[0] == 0xEF)
            }
            ProofType::Risc0 => {
                // Mock Risc0 verification
                Ok(proof.proof_data.len() == 200 && proof.proof_data[0] == 0xAB)
            }
        }
    }

    pub fn compute_model_hash(&self, model_path: &Path) -> Result<String> {
        // In a real implementation, we'd read and hash the model file
        // For now, just hash the path
        let hash = self.compute_data_hash(model_path.to_string_lossy().as_bytes());
        Ok(hash)
    }

    pub fn compute_data_hash(&self, data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        let result = hasher.finalize();
        format!("{:x}", result)
    }
}
