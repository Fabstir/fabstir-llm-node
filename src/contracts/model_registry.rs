use ethers::prelude::*;
use ethers::utils::keccak256;
use std::sync::Arc;
use anyhow::{Result, anyhow};
use tracing::{info, debug, error};
use serde::{Serialize, Deserialize};
use sha2::{Sha256, Digest};
use tokio::io::AsyncReadExt;
use std::path::Path;

use crate::contracts::types::{ModelRegistry, NodeRegistryWithModels};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub huggingface_repo: String,
    pub file_name: String,
    pub sha256_hash: H256,
    pub approval_tier: u8,
    pub active: bool,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub struct ApprovedModels {
    pub tiny_vicuna: ModelSpec,
    pub tiny_llama: ModelSpec,
}

#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub repo: String,
    pub file: String,
    pub sha256: String,
    pub id: H256,
}

impl Default for ApprovedModels {
    fn default() -> Self {
        let tiny_vicuna = ModelSpec {
            repo: "CohereForAI/TinyVicuna-1B-32k-GGUF".to_string(),
            file: "tiny-vicuna-1b.q4_k_m.gguf".to_string(),
            sha256: "329d002bc20d4e7baae25df802c9678b5a4340b3ce91f23e6a0644975e95935f".to_string(),
            id: H256::zero(), // Will be calculated
        };

        let tiny_llama = ModelSpec {
            repo: "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF".to_string(),
            file: "tinyllama-1b.Q4_K_M.gguf".to_string(),
            sha256: "45b71fe98efe5f530b825dce6f5049d738e9c16869f10be4370ab81a9912d4a6".to_string(),
            id: H256::zero(), // Will be calculated
        };

        let mut models = Self {
            tiny_vicuna,
            tiny_llama,
        };

        // Calculate model IDs
        models.tiny_vicuna.id = Self::calculate_model_id(&models.tiny_vicuna.repo, &models.tiny_vicuna.file);
        models.tiny_llama.id = Self::calculate_model_id(&models.tiny_llama.repo, &models.tiny_llama.file);

        models
    }
}

impl ApprovedModels {
    pub fn calculate_model_id(huggingface_repo: &str, file_name: &str) -> H256 {
        let input = format!("{}/{}", huggingface_repo, file_name);
        let hash = keccak256(input.as_bytes());
        H256::from_slice(&hash)
    }

    pub fn get_all_ids(&self) -> Vec<H256> {
        vec![self.tiny_vicuna.id, self.tiny_llama.id]
    }

    pub fn get_spec_by_file(&self, file_name: &str) -> Option<&ModelSpec> {
        if file_name == self.tiny_vicuna.file {
            Some(&self.tiny_vicuna)
        } else if file_name == self.tiny_llama.file {
            Some(&self.tiny_llama)
        } else {
            None
        }
    }
}

pub struct ModelRegistryClient {
    contract: Arc<ModelRegistry<Provider<Http>>>,
    node_registry: Option<Arc<NodeRegistryWithModels<Provider<Http>>>>,
    approved_models: ApprovedModels,
}

impl ModelRegistryClient {
    pub async fn new(
        provider: Arc<Provider<Http>>,
        model_registry_address: Address,
        node_registry_address: Option<Address>,
    ) -> Result<Self> {
        let contract = Arc::new(ModelRegistry::new(model_registry_address, provider.clone()));

        let node_registry = if let Some(addr) = node_registry_address {
            Some(Arc::new(NodeRegistryWithModels::new(addr, provider.clone())))
        } else {
            None
        };

        Ok(Self {
            contract,
            node_registry,
            approved_models: ApprovedModels::default(),
        })
    }

    /// Get model ID from HuggingFace repo and filename
    pub fn get_model_id(&self, huggingface_repo: &str, file_name: &str) -> H256 {
        ApprovedModels::calculate_model_id(huggingface_repo, file_name)
    }

    /// Check if a model is approved
    pub async fn is_model_approved(&self, model_id: H256) -> Result<bool> {
        debug!("Checking if model {:?} is approved", model_id);

        // For MVP, check against our known approved models
        let is_approved = self.approved_models.get_all_ids().contains(&model_id);

        // Optional: Also check on-chain (commented out for mock)
        // let approved = self.contract.is_model_approved(model_id).call().await?;

        Ok(is_approved)
    }

    /// Get model details from registry
    pub async fn get_model_details(&self, model_id: H256) -> Result<ModelInfo> {
        debug!("Getting details for model {:?}", model_id);

        // For MVP, return mock data for approved models
        if model_id == self.approved_models.tiny_vicuna.id {
            Ok(ModelInfo {
                huggingface_repo: self.approved_models.tiny_vicuna.repo.clone(),
                file_name: self.approved_models.tiny_vicuna.file.clone(),
                sha256_hash: H256::from_slice(&hex::decode(&self.approved_models.tiny_vicuna.sha256)?),
                approval_tier: 1,
                active: true,
                timestamp: 1735000000, // Mock timestamp
            })
        } else if model_id == self.approved_models.tiny_llama.id {
            Ok(ModelInfo {
                huggingface_repo: self.approved_models.tiny_llama.repo.clone(),
                file_name: self.approved_models.tiny_llama.file.clone(),
                sha256_hash: H256::from_slice(&hex::decode(&self.approved_models.tiny_llama.sha256)?),
                approval_tier: 1,
                active: true,
                timestamp: 1735000000, // Mock timestamp
            })
        } else {
            Err(anyhow!("Model not found in registry"))
        }
    }

    /// Get all approved model IDs
    pub async fn get_all_approved_models(&self) -> Result<Vec<H256>> {
        info!("Getting all approved models");

        // For MVP, return our two approved models
        Ok(self.approved_models.get_all_ids())
    }

    /// Verify model file integrity
    pub async fn verify_model_hash(&self, file_path: &Path, expected_hash: &str) -> Result<bool> {
        info!("Verifying model hash for {:?}", file_path);

        if !file_path.exists() {
            return Err(anyhow!("Model file does not exist"));
        }

        // Calculate SHA256 hash of file
        let mut file = tokio::fs::File::open(file_path).await?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0; 8192];

        loop {
            let n = file.read(&mut buffer).await?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }

        let calculated_hash = format!("{:x}", hasher.finalize());
        let matches = calculated_hash == expected_hash;

        if matches {
            info!("Model hash verification successful");
        } else {
            error!("Model hash mismatch! Expected: {}, Got: {}", expected_hash, calculated_hash);
        }

        Ok(matches)
    }

    /// Validate models for node registration
    pub async fn validate_models_for_registration(&self, model_paths: &[String]) -> Result<Vec<H256>> {
        info!("Validating {} models for registration", model_paths.len());

        let mut validated_ids = Vec::new();

        for path_str in model_paths {
            let file_name = Path::new(path_str)
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| anyhow!("Invalid model path: {}", path_str))?;

            // Find the model spec for this file
            let spec = self.approved_models.get_spec_by_file(file_name)
                .ok_or_else(|| anyhow!("Model {} is not in approved list", file_name))?;

            // Verify the model is approved
            if !self.is_model_approved(spec.id).await? {
                return Err(anyhow!("Model {} is not approved", file_name));
            }

            // Verify file hash if it exists
            let path = Path::new(path_str);
            if path.exists() {
                if !self.verify_model_hash(path, &spec.sha256).await? {
                    return Err(anyhow!("Model {} failed hash verification", file_name));
                }
            } else {
                debug!("Model file {} not found locally, skipping hash check", path_str);
            }

            validated_ids.push(spec.id);
            info!("Model {} validated successfully with ID {:?}", file_name, spec.id);
        }

        Ok(validated_ids)
    }

    /// Get the approved models specifications
    pub fn get_approved_models(&self) -> &ApprovedModels {
        &self.approved_models
    }

    /// Find hosts that support a specific model
    pub async fn find_hosts_for_model(&self, model_id: H256) -> Result<Vec<Address>> {
        if let Some(registry) = &self.node_registry {
            debug!("Finding hosts for model {:?}", model_id);

            // Mock implementation - would call contract in production
            // let addresses = registry.get_nodes_for_model(model_id).call().await?;

            // For testing, return empty vec
            Ok(Vec::new())
        } else {
            Err(anyhow!("NodeRegistryWithModels not configured"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_id_calculation() {
        let approved = ApprovedModels::default();

        // Test TinyVicuna ID calculation
        let vicuna_id = ApprovedModels::calculate_model_id(
            "CohereForAI/TinyVicuna-1B-32k-GGUF",
            "tiny-vicuna-1b.q4_k_m.gguf"
        );
        assert_eq!(vicuna_id, approved.tiny_vicuna.id);

        // Test TinyLlama ID calculation
        let llama_id = ApprovedModels::calculate_model_id(
            "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF",
            "tinyllama-1b.Q4_K_M.gguf"
        );
        assert_eq!(llama_id, approved.tiny_llama.id);
    }

    #[test]
    fn test_approved_models_lookup() {
        let approved = ApprovedModels::default();

        // Test finding by filename
        let spec = approved.get_spec_by_file("tiny-vicuna-1b.q4_k_m.gguf");
        assert!(spec.is_some());
        assert_eq!(spec.unwrap().repo, "CohereForAI/TinyVicuna-1B-32k-GGUF");

        // Test non-existent file
        let spec = approved.get_spec_by_file("unknown-model.gguf");
        assert!(spec.is_none());
    }
}