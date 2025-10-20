// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::api::websocket::{
    messages::ProofData,
    proof_config::{ProofConfig, ProofMode},
};
use crate::results::packager::{InferenceResult, ResultMetadata};
use crate::results::proofs::{ProofGenerationConfig, ProofGenerator, ProofType};

/// Manager for generating and caching proofs for WebSocket responses
pub struct ProofManager {
    generator: ProofGenerator,
    cache: Arc<RwLock<HashMap<String, ProofData>>>,
    cache_order: Arc<RwLock<VecDeque<String>>>, // Track insertion order for LRU
    config: ProofConfig,
}

impl ProofManager {
    /// Create a new proof manager with default configuration
    pub fn new() -> Self {
        Self::with_config(ProofConfig::default())
    }

    /// Create a new proof manager with specific configuration
    pub fn with_config(config: ProofConfig) -> Self {
        let validated_config = config.clone().validate();

        // Convert ProofMode to ProofType
        let proof_type = match validated_config.get_mode() {
            ProofMode::EZKL => ProofType::EZKL,
            ProofMode::Risc0 => ProofType::Risc0,
            ProofMode::Simple => ProofType::Simple,
        };

        let gen_config = ProofGenerationConfig {
            proof_type,
            model_path: validated_config.model_path.clone(),
            settings_path: None,
            max_proof_size: 10_000,
        };

        let generator = ProofGenerator::new(gen_config, "websocket_node".to_string());

        // Create cache with configured size
        let cache = Arc::new(RwLock::new(HashMap::with_capacity(
            validated_config.cache_size,
        )));
        let cache_order = Arc::new(RwLock::new(VecDeque::with_capacity(
            validated_config.cache_size,
        )));

        Self {
            generator,
            cache,
            cache_order,
            config: validated_config,
        }
    }

    /// Create a new proof manager with custom generator
    pub fn new_with_generator(generator: ProofGenerator) -> Self {
        Self {
            generator,
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_order: Arc::new(RwLock::new(VecDeque::new())),
            config: ProofConfig::default(),
        }
    }

    /// Generate a proof for the given inference result (optional based on config)
    pub async fn generate_proof_optional(
        &self,
        model: &str,
        prompt: &str,
        output: &str,
    ) -> Result<Option<ProofData>> {
        if !self.config.enabled {
            return Ok(None);
        }

        self.generate_proof(model, prompt, output).await.map(Some)
    }

    /// Generate a proof for the given inference result
    pub async fn generate_proof(
        &self,
        model: &str,
        prompt: &str,
        output: &str,
    ) -> Result<ProofData> {
        // Create cache key from inputs
        let cache_key = format!("{}-{}-{}", model, prompt, output);

        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(proof) = cache.get(&cache_key) {
                debug!("Cache hit for key: {}", cache_key);
                // Update LRU order (move to back)
                {
                    let mut order = self.cache_order.write().await;
                    if let Some(pos) = order.iter().position(|k| k == &cache_key) {
                        order.remove(pos);
                        order.push_back(cache_key.clone());
                    } else {
                        // Key exists in cache but not in order - this shouldn't happen
                        debug!("Warning: Key {} in cache but not in order queue", cache_key);
                    }
                }
                return Ok(proof.clone());
            }
            debug!("Cache miss for key: {}", cache_key);
        }

        // Generate new proof
        debug!("Generating new proof for model: {}", model);

        // Create inference result for proof generation
        let result = InferenceResult {
            job_id: "websocket".to_string(),
            model_id: model.to_string(),
            prompt: prompt.to_string(),
            response: output.to_string(),
            tokens_generated: output.len() as u32 / 4, // Rough estimate
            inference_time_ms: 100,                    // Mock inference time
            timestamp: chrono::Utc::now(),
            node_id: "websocket_node".to_string(),
            metadata: ResultMetadata::default(),
        };

        // Generate EZKL proof
        match self.generator.generate_proof(&result).await {
            Ok(proof) => {
                // Convert proof to ProofData for WebSocket message
                let proof_hash = format!("{:x}", sha2::Sha256::digest(&proof.proof_data));
                let proof_data = ProofData {
                    hash: proof_hash,
                    proof_type: format!("{:?}", proof.proof_type).to_lowercase(),
                    model_hash: proof.model_hash,
                    input_hash: proof.input_hash,
                    output_hash: proof.output_hash,
                    timestamp: proof.timestamp.timestamp_millis() as u64,
                };

                // Cache the proof with LRU eviction
                {
                    let mut cache = self.cache.write().await;
                    let mut order = self.cache_order.write().await;

                    // Add to cache
                    cache.insert(cache_key.clone(), proof_data.clone());
                    order.push_back(cache_key.clone());

                    debug!(
                        "Cache size after insert: {}, max: {}",
                        cache.len(),
                        self.config.cache_size
                    );

                    // Limit cache size based on configuration
                    while cache.len() > self.config.cache_size {
                        debug!(
                            "Cache size {} exceeds max {}, evicting oldest entry",
                            cache.len(),
                            self.config.cache_size
                        );
                        // Remove oldest entry (front of queue)
                        if let Some(oldest_key) = order.pop_front() {
                            debug!("Evicting key: {}", oldest_key);
                            cache.remove(&oldest_key);
                        } else {
                            debug!("Warning - no key in order queue to evict!");
                            break;
                        }
                    }
                    debug!("Cache size after potential eviction: {}", cache.len());
                }

                Ok(proof_data)
            }
            Err(e) => {
                warn!("Failed to generate proof: {}", e);
                // Return a placeholder proof on error
                Ok(ProofData {
                    hash: "error".to_string(),
                    proof_type: "none".to_string(),
                    model_hash: "".to_string(),
                    input_hash: "".to_string(),
                    output_hash: "".to_string(),
                    timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                })
            }
        }
    }

    /// Clear the proof cache
    pub async fn clear_cache(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
        debug!("Proof cache cleared");
    }

    /// Get the current configuration (for testing)
    pub fn config(&self) -> &ProofConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_proof_manager_caching() {
        use crate::api::websocket::proof_config::ProofConfig;

        let config = ProofConfig {
            enabled: true,
            proof_type: "Simple".to_string(),
            model_path: "./models/test.gguf".to_string(),
            cache_size: 100,
            batch_size: 10,
        };
        let manager = ProofManager::with_config(config);

        // Generate proof
        let proof1 = manager
            .generate_proof("model", "prompt", "output")
            .await
            .unwrap();

        // Should return cached version
        let proof2 = manager
            .generate_proof("model", "prompt", "output")
            .await
            .unwrap();

        assert_eq!(proof1.hash, proof2.hash);
        assert_eq!(proof1.timestamp, proof2.timestamp);
    }
}
