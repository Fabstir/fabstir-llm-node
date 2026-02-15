// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::{anyhow, Result};
use ethers::prelude::*;
use ethers::utils::keccak256;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tracing::{debug, error, info};

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

// ============================================================================
// Model ID Calculation (standalone utility)
// ============================================================================

/// Calculate model ID from HuggingFace repo and filename
///
/// Model IDs are keccak256("{repo}/{filename}") - same as contract
pub fn calculate_model_id(huggingface_repo: &str, file_name: &str) -> H256 {
    let input = format!("{}/{}", huggingface_repo, file_name);
    let hash = keccak256(input.as_bytes());
    H256::from_slice(&hash)
}

pub struct ModelRegistryClient {
    contract: Arc<ModelRegistry<Provider<Http>>>,
    node_registry: Option<Arc<NodeRegistryWithModels<Provider<Http>>>>,
}

impl ModelRegistryClient {
    pub async fn new(
        provider: Arc<Provider<Http>>,
        model_registry_address: Address,
        node_registry_address: Option<Address>,
    ) -> Result<Self> {
        // Verify ModelRegistry contract exists
        let code = provider
            .get_code(model_registry_address, None)
            .await
            .map_err(|e| anyhow!("Failed to check ModelRegistry contract: {}", e))?;

        if code == ethers::types::Bytes::from(vec![]) {
            return Err(anyhow!(
                "ModelRegistry contract not deployed at address: {:?}",
                model_registry_address
            ));
        }

        let contract = Arc::new(ModelRegistry::new(model_registry_address, provider.clone()));

        let node_registry = if let Some(addr) = node_registry_address {
            // Verify NodeRegistryWithModels contract exists
            let node_code = provider
                .get_code(addr, None)
                .await
                .map_err(|e| anyhow!("Failed to check NodeRegistryWithModels contract: {}", e))?;

            if node_code == ethers::types::Bytes::from(vec![]) {
                return Err(anyhow!(
                    "NodeRegistryWithModels contract not deployed at address: {:?}",
                    addr
                ));
            }

            Some(Arc::new(NodeRegistryWithModels::new(
                addr,
                provider.clone(),
            )))
        } else {
            None
        };

        Ok(Self {
            contract,
            node_registry,
        })
    }

    /// Get model ID from HuggingFace repo and filename
    pub fn get_model_id(&self, huggingface_repo: &str, file_name: &str) -> H256 {
        calculate_model_id(huggingface_repo, file_name)
    }

    /// Check if a model is approved
    pub async fn is_model_approved(&self, model_id: H256) -> Result<bool> {
        debug!("Checking if model {:?} is approved", model_id);

        // Call the actual contract
        let method = self
            .contract
            .method::<_, bool>("isModelApproved", model_id)
            .map_err(|e| anyhow!("Failed to create method call: {}", e))?;

        let approved = method
            .call()
            .await
            .map_err(|e| anyhow!("Failed to check model approval: {}", e))?;

        Ok(approved)
    }

    /// Get model details from registry
    pub async fn get_model_details(&self, model_id: H256) -> Result<ModelInfo> {
        debug!("Getting details for model {:?}", model_id);

        // Call the actual contract to get model details
        let method = self
            .contract
            .method::<_, (String, String, H256, u8, bool, u64)>("getModel", model_id)
            .map_err(|e| anyhow!("Failed to create method call: {}", e))?;

        let model_data = method
            .call()
            .await
            .map_err(|e| anyhow!("Failed to get model details: {}", e))?;

        Ok(ModelInfo {
            huggingface_repo: model_data.0,
            file_name: model_data.1,
            sha256_hash: model_data.2,
            approval_tier: model_data.3,
            active: model_data.4,
            timestamp: model_data.5,
        })
    }

    /// Get all approved model IDs
    pub async fn get_all_approved_models(&self) -> Result<Vec<H256>> {
        info!("Getting all approved models");

        // Call the actual contract to get all model IDs
        let method = self
            .contract
            .method::<_, Vec<H256>>("getAllModels", ())
            .map_err(|e| anyhow!("Failed to create method call: {}", e))?;

        let model_ids = method
            .call()
            .await
            .map_err(|e| anyhow!("Failed to get all models: {}", e))?;

        // Filter to only approved models
        let mut approved = Vec::new();
        for id in model_ids {
            if self.is_model_approved(id).await? {
                approved.push(id);
            }
        }

        Ok(approved)
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
            error!(
                "Model hash mismatch! Expected: {}, Got: {}",
                expected_hash, calculated_hash
            );
        }

        Ok(matches)
    }

    /// Validate models for node registration (queries contract dynamically)
    ///
    /// This function queries the ModelRegistry contract to get all approved models,
    /// builds a filename→model_id map, and validates each model path against it.
    /// No hardcoded model list - any model registered on-chain is supported.
    pub async fn validate_models_for_registration(
        &self,
        model_paths: &[String],
    ) -> Result<Vec<H256>> {
        info!(
            "Validating {} models for registration (querying contract)",
            model_paths.len()
        );

        // Step 1: Get all approved models from contract
        let all_model_ids = self.get_all_approved_models().await?;
        info!("Found {} approved models on-chain", all_model_ids.len());

        // Step 2: Build filename → (model_id, sha256_hash) map from contract
        let mut filename_map: std::collections::HashMap<String, (H256, H256)> =
            std::collections::HashMap::new();

        for model_id in &all_model_ids {
            match self.get_model_details(*model_id).await {
                Ok(info) => {
                    debug!("  {} → 0x{}", info.file_name, hex::encode(&model_id.0[..8]));
                    filename_map.insert(info.file_name.clone(), (*model_id, info.sha256_hash));
                }
                Err(e) => {
                    debug!(
                        "Could not get details for model 0x{}: {}",
                        hex::encode(&model_id.0),
                        e
                    );
                }
            }
        }

        // Step 3: Validate each model path against dynamic map
        let mut validated_ids = Vec::new();

        for path_str in model_paths {
            let file_name = Path::new(path_str)
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| anyhow!("Invalid model path: {}", path_str))?;

            // Lookup in dynamic map (from contract)
            let (model_id, sha256_hash) = filename_map.get(file_name).ok_or_else(|| {
                anyhow!(
                    "Model '{}' is not registered in ModelRegistry. \
                     Only models approved on-chain can be used. \
                     Found {} approved models: {:?}",
                    file_name,
                    filename_map.len(),
                    filename_map.keys().collect::<Vec<_>>()
                )
            })?;

            // Verify file hash if it exists
            let path = Path::new(path_str);
            if path.exists() {
                let expected_hash = format!("{:x}", sha256_hash);
                if !self.verify_model_hash(path, &expected_hash).await? {
                    return Err(anyhow!(
                        "Model {} failed hash verification against on-chain SHA256",
                        file_name
                    ));
                }
            } else {
                debug!(
                    "Model file {} not found locally, skipping hash check",
                    path_str
                );
            }

            validated_ids.push(*model_id);
            info!(
                "Model {} validated successfully with ID 0x{}",
                file_name,
                hex::encode(&model_id.0[..8])
            );
        }

        Ok(validated_ids)
    }

    /// Find hosts that support a specific model
    pub async fn find_hosts_for_model(&self, model_id: H256) -> Result<Vec<Address>> {
        if let Some(registry) = &self.node_registry {
            debug!("Finding hosts for model {:?}", model_id);

            // Call the actual contract
            let method = registry
                .method::<_, Vec<Address>>("getNodesForModel", model_id)
                .map_err(|e| anyhow!("Failed to create method call: {}", e))?;

            let addresses = method
                .call()
                .await
                .map_err(|e| anyhow!("Failed to get nodes for model: {}", e))?;

            Ok(addresses)
        } else {
            Err(anyhow!("NodeRegistryWithModels not configured"))
        }
    }

    /// Check if a specific node supports a model
    ///
    /// Calls NodeRegistry.nodeSupportsModel(nodeAddress, modelId) to verify
    /// host authorization for a specific model.
    ///
    /// # Arguments
    /// * `node_address` - The host/node wallet address
    /// * `model_id` - The model ID (H256 hash)
    ///
    /// # Returns
    /// * `Ok(true)` if the node is authorized for the model
    /// * `Ok(false)` if the node is NOT authorized
    /// * `Err` if the contract query fails
    pub async fn node_supports_model(&self, node_address: Address, model_id: H256) -> Result<bool> {
        if let Some(registry) = &self.node_registry {
            debug!(
                "Checking if node {:?} supports model {:?}",
                node_address, model_id
            );

            // Call nodeSupportsModel(address nodeAddress, bytes32 modelId) -> bool
            let method = registry
                .method::<_, bool>("nodeSupportsModel", (node_address, model_id))
                .map_err(|e| anyhow!("Failed to create nodeSupportsModel call: {}", e))?;

            let supports = method
                .call()
                .await
                .map_err(|e| anyhow!("Failed to query nodeSupportsModel: {}", e))?;

            Ok(supports)
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
        // Test that calculate_model_id produces deterministic keccak256 hashes
        // Model ID = keccak256("{repo}/{filename}")

        // TinyVicuna - expected ID from API_REFERENCE.md
        let vicuna_id = calculate_model_id(
            "CohereForAI/TinyVicuna-1B-32k-GGUF",
            "tiny-vicuna-1b.q4_k_m.gguf",
        );
        let expected_vicuna = H256::from_slice(
            &hex::decode("0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced")
                .unwrap(),
        );
        assert_eq!(vicuna_id, expected_vicuna);

        // TinyLlama - expected ID from API_REFERENCE.md
        let llama_id = calculate_model_id(
            "TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF",
            "tinyllama-1b.Q4_K_M.gguf",
        );
        let expected_llama = H256::from_slice(
            &hex::decode("14843424179fbcb9aeb7fd446fa97143300609757bd49ffb3ec7fb2f75aed1ca")
                .unwrap(),
        );
        assert_eq!(llama_id, expected_llama);

        // GPT-OSS-20B - expected ID from API_REFERENCE.md
        let gpt_id = calculate_model_id(
            "bartowski/openai_gpt-oss-20b-GGUF",
            "openai_gpt-oss-20b-MXFP4.gguf",
        );
        let expected_gpt = H256::from_slice(
            &hex::decode("7583557c14f71d2bf21d48ffb7cde9329f9494090869d2d311ea481b26e7e06c")
                .unwrap(),
        );
        assert_eq!(gpt_id, expected_gpt);
    }

    #[test]
    fn test_model_id_is_keccak256() {
        // Verify the calculation matches keccak256("{repo}/{filename}")
        use ethers::utils::keccak256;

        let repo = "CohereForAI/TinyVicuna-1B-32k-GGUF";
        let filename = "tiny-vicuna-1b.q4_k_m.gguf";
        let input = format!("{}/{}", repo, filename);

        let expected = H256::from_slice(&keccak256(input.as_bytes()));
        let actual = calculate_model_id(repo, filename);

        assert_eq!(actual, expected);
    }

    #[test]
    fn test_model_id_deterministic() {
        // Same inputs should always produce same output
        let id1 = calculate_model_id("test/repo", "model.gguf");
        let id2 = calculate_model_id("test/repo", "model.gguf");
        assert_eq!(id1, id2);

        // Different inputs should produce different outputs
        let id3 = calculate_model_id("test/repo", "other.gguf");
        assert_ne!(id1, id3);
    }
}
