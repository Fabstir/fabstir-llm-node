// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Vision model manager for loading and managing OCR and Florence models

use std::sync::Arc;

use crate::vision::florence::FlorenceModel;
use crate::vision::ocr::PaddleOcrModel;

/// Configuration for loading vision models
#[derive(Debug, Clone)]
pub struct VisionModelConfig {
    /// Path to OCR model directory (optional)
    pub ocr_model_dir: Option<String>,
    /// Path to Florence model directory (optional)
    pub florence_model_dir: Option<String>,
}

impl Default for VisionModelConfig {
    fn default() -> Self {
        Self {
            ocr_model_dir: Some("./models/paddleocr-onnx".to_string()),
            florence_model_dir: Some("./models/florence-2-onnx".to_string()),
        }
    }
}

/// Information about a loaded vision model
#[derive(Debug, Clone)]
pub struct VisionModelInfo {
    /// Model name
    pub name: String,
    /// Model type (ocr, vision)
    pub model_type: String,
    /// Whether the model is available
    pub available: bool,
}

/// Manager for vision models (OCR and Florence-2)
///
/// Handles loading, caching, and providing access to vision models.
/// Both models run on CPU only to avoid GPU VRAM competition with LLM.
pub struct VisionModelManager {
    ocr_model: Option<Arc<PaddleOcrModel>>,
    florence_model: Option<Arc<FlorenceModel>>,
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

        Ok(Self {
            ocr_model,
            florence_model,
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

    /// Check if OCR is available
    pub fn has_ocr(&self) -> bool {
        self.ocr_model.is_some()
    }

    /// Check if Florence (image description) is available
    pub fn has_florence(&self) -> bool {
        self.florence_model.is_some()
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
}
