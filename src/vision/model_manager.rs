// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Vision model manager for loading and managing OCR, Florence, and VLM models

use std::sync::Arc;

use crate::vision::florence::FlorenceModel;
use crate::vision::ocr::PaddleOcrModel;
use crate::vision::vlm_client::VlmClient;

/// Configuration for loading vision models
#[derive(Debug, Clone)]
pub struct VisionModelConfig {
    /// Path to OCR model directory (optional)
    pub ocr_model_dir: Option<String>,
    /// Path to Florence model directory (optional)
    pub florence_model_dir: Option<String>,
    /// VLM sidecar endpoint URL (optional, e.g. "http://localhost:8081")
    pub vlm_endpoint: Option<String>,
    /// VLM model name (optional, defaults to "qwen3-vl")
    pub vlm_model_name: Option<String>,
}

impl Default for VisionModelConfig {
    fn default() -> Self {
        Self {
            // Use English PP-OCRv5 models for better English text recognition
            ocr_model_dir: Some("./models/paddleocr-english-onnx".to_string()),
            florence_model_dir: Some("./models/florence-2-onnx".to_string()),
            vlm_endpoint: None,
            vlm_model_name: None,
        }
    }
}

/// Information about a loaded vision model
#[derive(Debug, Clone, serde::Serialize)]
pub struct VisionModelInfo {
    /// Model name
    pub name: String,
    /// Model type (ocr, vision)
    pub model_type: String,
    /// Whether the model is available
    pub available: bool,
}

/// Manager for vision models (OCR, Florence-2, and optional VLM sidecar)
///
/// Handles loading, caching, and providing access to vision models.
/// ONNX models run on CPU only. VLM sidecar runs on GPU via separate process.
pub struct VisionModelManager {
    ocr_model: Option<Arc<PaddleOcrModel>>,
    florence_model: Option<Arc<FlorenceModel>>,
    vlm_client: Option<Arc<VlmClient>>,
}

impl VisionModelManager {
    /// Create a new VisionModelManager with the given configuration
    ///
    /// Models are loaded lazily - missing model directories are handled gracefully.
    pub async fn new(config: VisionModelConfig) -> anyhow::Result<Self> {
        let ocr_model = if let Some(ref dir) = config.ocr_model_dir {
            match PaddleOcrModel::new(dir).await {
                Ok(model) => {
                    tracing::info!("✅ PaddleOCR model loaded from {}", dir);
                    Some(Arc::new(model))
                }
                Err(e) => {
                    tracing::warn!("⚠️ Failed to load OCR model from {}: {}", dir, e);
                    None
                }
            }
        } else {
            None
        };

        let florence_model = if let Some(ref dir) = config.florence_model_dir {
            match FlorenceModel::new(dir).await {
                Ok(model) => {
                    tracing::info!("✅ Florence-2 model loaded from {}", dir);
                    Some(Arc::new(model))
                }
                Err(e) => {
                    tracing::warn!("⚠️ Failed to load Florence model from {}: {}", dir, e);
                    None
                }
            }
        } else {
            None
        };

        let vlm_client = if let Some(ref endpoint) = config.vlm_endpoint {
            let model_name = config.vlm_model_name.as_deref().unwrap_or("qwen3-vl");
            match VlmClient::new(endpoint, model_name) {
                Ok(client) => {
                    tracing::info!("✅ VLM client configured: {}", endpoint);
                    Some(Arc::new(client))
                }
                Err(e) => {
                    tracing::warn!("⚠️ Failed to create VLM client: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            ocr_model,
            florence_model,
            vlm_client,
        })
    }

    /// Get the OCR model if available
    pub fn get_ocr_model(&self) -> Option<Arc<PaddleOcrModel>> {
        self.ocr_model.clone()
    }

    /// Get the Florence model if available
    pub fn get_florence_model(&self) -> Option<Arc<FlorenceModel>> {
        self.florence_model.clone()
    }

    /// Get the VLM client if available
    pub fn get_vlm_client(&self) -> Option<Arc<VlmClient>> {
        self.vlm_client.clone()
    }

    /// Check if OCR is available
    pub fn has_ocr(&self) -> bool {
        self.ocr_model.is_some()
    }

    /// Check if Florence (image description) is available
    pub fn has_florence(&self) -> bool {
        self.florence_model.is_some()
    }

    /// Check if VLM sidecar is configured
    pub fn has_vlm(&self) -> bool {
        self.vlm_client.is_some()
    }

    /// List all available vision models
    pub fn list_models(&self) -> Vec<VisionModelInfo> {
        let mut models = Vec::new();

        models.push(VisionModelInfo {
            name: "paddleocr".to_string(),
            model_type: "ocr".to_string(),
            available: self.ocr_model.is_some(),
        });

        models.push(VisionModelInfo {
            name: "florence-2".to_string(),
            model_type: "vision".to_string(),
            available: self.florence_model.is_some(),
        });

        if let Some(ref client) = self.vlm_client {
            models.push(VisionModelInfo {
                name: client.model_name().to_string(),
                model_type: "vlm".to_string(),
                available: true,
            });
        }

        models
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = VisionModelConfig::default();
        assert!(config.ocr_model_dir.is_some());
        assert!(config.florence_model_dir.is_some());
        assert!(config.vlm_endpoint.is_none());
        assert!(config.vlm_model_name.is_none());
    }

    #[test]
    fn test_vision_model_info() {
        let info = VisionModelInfo {
            name: "test".to_string(),
            model_type: "ocr".to_string(),
            available: true,
        };
        assert_eq!(info.name, "test");
        assert!(info.available);
    }

    #[test]
    fn test_config_with_vlm() {
        let config = VisionModelConfig {
            ocr_model_dir: None,
            florence_model_dir: None,
            vlm_endpoint: Some("http://localhost:8081".to_string()),
            vlm_model_name: Some("qwen3-vl-8b".to_string()),
        };
        assert!(config.vlm_endpoint.is_some());
        assert_eq!(config.vlm_model_name.as_deref(), Some("qwen3-vl-8b"));
    }

    #[test]
    fn test_config_without_vlm() {
        let config = VisionModelConfig {
            ocr_model_dir: None,
            florence_model_dir: None,
            vlm_endpoint: None,
            vlm_model_name: None,
        };
        assert!(config.vlm_endpoint.is_none());
    }

    #[tokio::test]
    async fn test_has_vlm() {
        // No ONNX models, no VLM
        let config = VisionModelConfig {
            ocr_model_dir: None,
            florence_model_dir: None,
            vlm_endpoint: None,
            vlm_model_name: None,
        };
        let manager = VisionModelManager::new(config).await.unwrap();
        assert!(!manager.has_vlm());

        // With VLM endpoint
        let config_vlm = VisionModelConfig {
            ocr_model_dir: None,
            florence_model_dir: None,
            vlm_endpoint: Some("http://localhost:8081".to_string()),
            vlm_model_name: Some("test-vlm".to_string()),
        };
        let manager_vlm = VisionModelManager::new(config_vlm).await.unwrap();
        assert!(manager_vlm.has_vlm());
    }

    #[tokio::test]
    async fn test_list_models_includes_vlm() {
        let config = VisionModelConfig {
            ocr_model_dir: None,
            florence_model_dir: None,
            vlm_endpoint: Some("http://localhost:8081".to_string()),
            vlm_model_name: Some("qwen3-vl".to_string()),
        };
        let manager = VisionModelManager::new(config).await.unwrap();
        let models = manager.list_models();
        // Should have paddleocr, florence-2, and qwen3-vl
        assert_eq!(models.len(), 3);
        let vlm_model = models.iter().find(|m| m.model_type == "vlm").unwrap();
        assert_eq!(vlm_model.name, "qwen3-vl");
        assert!(vlm_model.available);
    }

    #[test]
    fn test_vlm_env_vars_optional() {
        // Default config has no VLM - simulates absent env vars
        let config = VisionModelConfig::default();
        assert!(config.vlm_endpoint.is_none());
        assert!(config.vlm_model_name.is_none());
    }

    #[test]
    fn test_vlm_config_from_env() {
        // Simulates VLM_ENDPOINT and VLM_MODEL_NAME env vars being set
        let config = VisionModelConfig {
            ocr_model_dir: None,
            florence_model_dir: None,
            vlm_endpoint: Some("http://vlm-sidecar:8081".to_string()),
            vlm_model_name: Some("qwen3-vl-8b".to_string()),
        };
        assert_eq!(
            config.vlm_endpoint.as_deref(),
            Some("http://vlm-sidecar:8081")
        );
        assert_eq!(config.vlm_model_name.as_deref(), Some("qwen3-vl-8b"));
    }
}
