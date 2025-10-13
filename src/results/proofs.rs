use super::packager::{InferenceResult, PackagedResult};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

// EZKL integration (Phase 2.1, Phase 3.1)
use crate::crypto::ezkl::{EzklProver, EzklVerifier, ProofData, WitnessBuilder};

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
                // Real EZKL proof generation (Phase 2.1)
                #[cfg(feature = "real-ezkl")]
                {
                    // Use real EZKL prover
                    let witness = WitnessBuilder::new()
                        .with_job_id_string(&result.job_id)
                        .with_model_path(&self.config.model_path)
                        .with_input_string(&result.prompt)
                        .with_output_string(&result.response)
                        .build()
                        .map_err(|e| anyhow::anyhow!("Failed to build EZKL witness: {}", e))?;

                    let mut prover = EzklProver::new();
                    let proof_data = prover
                        .generate_proof(&witness)
                        .map_err(|e| anyhow::anyhow!("Failed to generate EZKL proof: {}", e))?;

                    tracing::info!(
                        "✅ Generated real EZKL proof ({} bytes)",
                        proof_data.proof_bytes.len()
                    );

                    proof_data.proof_bytes
                }
                #[cfg(not(feature = "real-ezkl"))]
                {
                    // Mock EZKL proof generation (for development)
                    let mut proof = vec![0xEF; 200]; // Mock EZKL proof header
                    proof.extend_from_slice(model_hash.as_bytes());
                    proof.extend_from_slice(input_hash.as_bytes());
                    proof.extend_from_slice(output_hash.as_bytes());

                    // Ensure we don't exceed max size
                    if proof.len() > self.config.max_proof_size {
                        proof.truncate(self.config.max_proof_size);
                    }

                    tracing::debug!("🎭 Generated mock EZKL proof ({} bytes)", proof.len());

                    proof
                }
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
                // EZKL verification (Phase 3.1 - real verification)
                #[cfg(feature = "real-ezkl")]
                {
                    // Convert string hashes back to bytes for witness
                    let model_hash_bytes = hex_str_to_bytes32(&proof.model_hash)?;
                    let input_hash_bytes = hex_str_to_bytes32(&proof.input_hash)?;
                    let output_hash_bytes = hex_str_to_bytes32(&proof.output_hash)?;
                    let job_id_bytes = hex_str_to_bytes32(&proof.job_id)?;

                    // Reconstruct witness from proof
                    let witness = WitnessBuilder::new()
                        .with_job_id(job_id_bytes)
                        .with_model_hash(model_hash_bytes)
                        .with_input_hash(input_hash_bytes)
                        .with_output_hash(output_hash_bytes)
                        .build()
                        .map_err(|e| anyhow::anyhow!("Failed to build witness: {}", e))?;

                    // Create ProofData from InferenceProof
                    let proof_data = ProofData {
                        proof_bytes: proof.proof_data.clone(),
                        timestamp: proof.timestamp.timestamp() as u64,
                        model_hash: model_hash_bytes,
                        input_hash: input_hash_bytes,
                        output_hash: output_hash_bytes,
                    };

                    // Verify using real EZKL verifier
                    let mut verifier = EzklVerifier::new();
                    let is_valid = verifier
                        .verify_proof(&proof_data, &witness)
                        .map_err(|e| anyhow::anyhow!("EZKL verification failed: {}", e))?;

                    tracing::debug!("🔍 Real EZKL proof verification: {}", is_valid);
                    Ok(is_valid)
                }
                #[cfg(not(feature = "real-ezkl"))]
                {
                    // Mock EZKL verification
                    Ok(proof.proof_data.len() >= 200 && proof.proof_data[0] == 0xEF)
                }
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

/// Helper function to convert hex string to 32-byte array
fn hex_str_to_bytes32(hex_str: &str) -> Result<[u8; 32]> {
    // Remove "0x" prefix if present
    let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);

    // Handle case where string might be a hash (64 chars) or other format
    if hex_str.len() == 64 {
        // Standard SHA256 hex string
        let mut bytes = [0u8; 32];
        for i in 0..32 {
            bytes[i] = u8::from_str_radix(&hex_str[i * 2..i * 2 + 2], 16)
                .map_err(|e| anyhow::anyhow!("Invalid hex string: {}", e))?;
        }
        Ok(bytes)
    } else {
        // For job_id or other strings, hash them to get 32 bytes
        let mut hasher = Sha256::new();
        hasher.update(hex_str.as_bytes());
        let result = hasher.finalize();
        Ok(result.into())
    }
}
