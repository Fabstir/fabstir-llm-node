// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Embedding Model Manager (Sub-phase 3.2)
//!
//! This module provides a manager for loading and managing multiple ONNX embedding models.
//! Supports parallel model loading, default model selection, and model discovery.

use crate::embeddings::OnnxEmbeddingModel;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info, warn};

/// Configuration for loading an embedding model
#[derive(Debug, Clone)]
pub struct EmbeddingModelConfig {
    /// Model name (e.g., "all-MiniLM-L6-v2")
    pub name: String,
    /// Path to ONNX model file
    pub model_path: String,
    /// Path to tokenizer JSON file
    pub tokenizer_path: String,
    /// Expected embedding dimensions (must be 384)
    pub dimensions: usize,
}

/// Information about an available embedding model
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelInfo {
    /// Model name
    pub name: String,
    /// Embedding dimensions
    pub dimensions: usize,
    /// Whether model is loaded and available
    pub available: bool,
    /// Whether this is the default model
    pub is_default: bool,
}

/// Manager for ONNX embedding models
///
/// Provides:
/// - Parallel loading of multiple models
/// - Default model selection (first config)
/// - Model discovery via list_models()
/// - Thread-safe access via Arc wrappers
///
/// # Example
/// ```ignore
/// let configs = vec![
///     EmbeddingModelConfig {
///         name: "all-MiniLM-L6-v2".to_string(),
///         model_path: "./models/all-MiniLM-L6-v2-onnx/model.onnx".to_string(),
///         tokenizer_path: "./models/all-MiniLM-L6-v2-onnx/tokenizer.json".to_string(),
///         dimensions: 384,
///     }
/// ];
/// let manager = EmbeddingModelManager::new(configs).await?;
/// let model = manager.get_model(None).await?; // Get default model
/// let embedding = model.embed("Hello world").await?;
/// ```
#[derive(Debug, Clone)]
pub struct EmbeddingModelManager {
    /// Loaded models by name
    models: HashMap<String, Arc<OnnxEmbeddingModel>>,

    /// Name of the default model (first successfully loaded model)
    default_model: String,
}

impl EmbeddingModelManager {
    /// Creates a new embedding model manager with parallel model loading
    ///
    /// # Arguments
    /// - `configs`: Vector of model configurations to load
    ///
    /// # Returns
    /// - `Result<Self>`: Manager with successfully loaded models
    /// - Error if NO models load successfully
    ///
    /// # Behavior
    /// - Loads all models in parallel using tokio::spawn
    /// - First successfully loaded model becomes the default
    /// - Continues if some models fail (logs warnings)
    /// - Returns error only if ALL models fail to load
    ///
    /// # Example
    /// ```ignore
    /// let configs = vec![
    ///     EmbeddingModelConfig {
    ///         name: "all-MiniLM-L6-v2".to_string(),
    ///         model_path: "./models/all-MiniLM-L6-v2-onnx/model.onnx".to_string(),
    ///         tokenizer_path: "./models/all-MiniLM-L6-v2-onnx/tokenizer.json".to_string(),
    ///         dimensions: 384,
    ///     }
    /// ];
    /// let manager = EmbeddingModelManager::new(configs).await?;
    /// ```
    pub async fn new(configs: Vec<EmbeddingModelConfig>) -> Result<Self> {
        if configs.is_empty() {
            anyhow::bail!("No model configurations provided");
        }

        info!("Loading {} embedding models in parallel", configs.len());

        // Spawn parallel loading tasks
        let mut load_tasks = Vec::new();

        for config in configs {
            let task = tokio::spawn(async move {
                let model_name = config.name.clone();
                info!("Loading embedding model: {}", model_name);

                let result = OnnxEmbeddingModel::new(
                    model_name.clone(),
                    config.model_path,
                    config.tokenizer_path,
                )
                .await;

                match result {
                    Ok(model) => {
                        // Validate dimensions match config
                        if model.dimension() != config.dimensions {
                            warn!(
                                "Model {} dimension mismatch: expected {}, got {}",
                                model_name,
                                config.dimensions,
                                model.dimension()
                            );
                            return Err(anyhow::anyhow!(
                                "Model {} dimension mismatch: expected {}, got {}",
                                model_name,
                                config.dimensions,
                                model.dimension()
                            ));
                        }

                        info!(
                            "✓ Successfully loaded model: {} ({} dimensions)",
                            model_name,
                            model.dimension()
                        );
                        Ok((model_name, Arc::new(model)))
                    }
                    Err(e) => {
                        error!("✗ Failed to load model {}: {}", model_name, e);
                        Err(e)
                    }
                }
            });

            load_tasks.push(task);
        }

        // Wait for all tasks to complete and collect results
        let mut models = HashMap::new();
        let mut first_model_name: Option<String> = None;

        for task in load_tasks {
            match task.await {
                Ok(Ok((name, model))) => {
                    if first_model_name.is_none() {
                        first_model_name = Some(name.clone());
                    }
                    models.insert(name, model);
                }
                Ok(Err(e)) => {
                    warn!("Model loading failed: {}", e);
                }
                Err(e) => {
                    error!("Task join error: {}", e);
                }
            }
        }

        // Ensure at least one model loaded successfully
        if models.is_empty() {
            anyhow::bail!("No models loaded successfully");
        }

        let default_model = first_model_name.expect("Should have at least one model");

        info!(
            "Embedding model manager initialized: {} models loaded, default: {}",
            models.len(),
            default_model
        );

        Ok(Self {
            models,
            default_model,
        })
    }

    /// Gets a model by name, or the default model if name is None
    ///
    /// # Arguments
    /// - `name`: Optional model name. If None, returns default model.
    ///
    /// # Returns
    /// - `Result<Arc<OnnxEmbeddingModel>>`: Requested model
    /// - Error if model not found
    ///
    /// # Example
    /// ```ignore
    /// // Get default model
    /// let model = manager.get_model(None).await?;
    ///
    /// // Get specific model
    /// let model = manager.get_model(Some("all-MiniLM-L6-v2")).await?;
    /// ```
    pub async fn get_model(&self, name: Option<&str>) -> Result<Arc<OnnxEmbeddingModel>> {
        let model_name = name.unwrap_or(&self.default_model);

        self.models
            .get(model_name)
            .cloned()
            .context(format!("Model not found: {}", model_name))
    }

    /// Lists all available models
    ///
    /// # Returns
    /// - `Vec<ModelInfo>`: Information about all loaded models
    ///
    /// # Example
    /// ```ignore
    /// let models = manager.list_models();
    /// for model in models {
    ///     println!("{}: {} dimensions, default: {}",
    ///              model.name, model.dimensions, model.is_default);
    /// }
    /// ```
    pub fn list_models(&self) -> Vec<ModelInfo> {
        let mut models: Vec<ModelInfo> = self
            .models
            .iter()
            .map(|(name, model)| ModelInfo {
                name: name.clone(),
                dimensions: model.dimension(),
                available: true,
                is_default: name == &self.default_model,
            })
            .collect();

        // Sort by name for consistent ordering
        models.sort_by(|a, b| a.name.cmp(&b.name));

        models
    }

    /// Returns the name of the default model
    ///
    /// # Returns
    /// - `&str`: Name of the default model
    pub fn default_model_name(&self) -> &str {
        &self.default_model
    }

    /// Returns the number of loaded models
    pub fn model_count(&self) -> usize {
        self.models.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Comprehensive tests are in tests/embeddings/test_model_manager.rs
    // These are basic unit tests for internal functionality

    #[test]
    fn test_embedding_model_config_creation() {
        let config = EmbeddingModelConfig {
            name: "test-model".to_string(),
            model_path: "/path/to/model.onnx".to_string(),
            tokenizer_path: "/path/to/tokenizer.json".to_string(),
            dimensions: 384,
        };

        assert_eq!(config.name, "test-model");
        assert_eq!(config.dimensions, 384);
    }

    #[test]
    fn test_model_info_creation() {
        let info = ModelInfo {
            name: "test-model".to_string(),
            dimensions: 384,
            available: true,
            is_default: true,
        };

        assert_eq!(info.name, "test-model");
        assert_eq!(info.dimensions, 384);
        assert!(info.available);
        assert!(info.is_default);
    }
}
