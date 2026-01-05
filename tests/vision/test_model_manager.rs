// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Vision Model Manager tests (Sub-phase 5.1)
//!
//! These TDD tests verify that the VisionModelManager correctly:
//! - Loads OCR and Florence models
//! - Provides model availability checks
//! - Lists available models
//! - Handles missing models gracefully
//!
//! Test-Driven Development (TDD) Approach:
//! 1. Write these tests FIRST (they will fail initially)
//! 2. Implement VisionModelManager in src/vision/model_manager.rs
//! 3. Run tests to verify vision model management works correctly

use fabstir_llm_node::vision::{VisionModelConfig, VisionModelInfo, VisionModelManager};

// Model paths (downloaded by download scripts)
const OCR_MODEL_DIR: &str = "/workspace/models/paddleocr-onnx";
const FLORENCE_MODEL_DIR: &str = "/workspace/models/florence-2-onnx";

#[cfg(test)]
mod model_manager_tests {
    use super::*;

    // =============================================================================
    // VisionModelConfig Tests
    // =============================================================================

    /// Test 1: Default config has expected paths
    #[test]
    fn test_default_config_has_expected_paths() {
        let config = VisionModelConfig::default();

        assert!(config.ocr_model_dir.is_some());
        assert!(config.florence_model_dir.is_some());

        // Check default paths
        assert!(config
            .ocr_model_dir
            .as_ref()
            .unwrap()
            .contains("paddleocr"));
        assert!(config
            .florence_model_dir
            .as_ref()
            .unwrap()
            .contains("florence"));
    }

    /// Test 2: Config can be created with custom paths
    #[test]
    fn test_config_with_custom_paths() {
        let config = VisionModelConfig {
            ocr_model_dir: Some("/custom/ocr".to_string()),
            florence_model_dir: Some("/custom/florence".to_string()),
        };

        assert_eq!(config.ocr_model_dir, Some("/custom/ocr".to_string()));
        assert_eq!(
            config.florence_model_dir,
            Some("/custom/florence".to_string())
        );
    }

    /// Test 3: Config can disable specific models
    #[test]
    fn test_config_disable_models() {
        // OCR only
        let ocr_only = VisionModelConfig {
            ocr_model_dir: Some("/path/to/ocr".to_string()),
            florence_model_dir: None,
        };
        assert!(ocr_only.ocr_model_dir.is_some());
        assert!(ocr_only.florence_model_dir.is_none());

        // Florence only
        let florence_only = VisionModelConfig {
            ocr_model_dir: None,
            florence_model_dir: Some("/path/to/florence".to_string()),
        };
        assert!(florence_only.ocr_model_dir.is_none());
        assert!(florence_only.florence_model_dir.is_some());

        // Neither
        let none = VisionModelConfig {
            ocr_model_dir: None,
            florence_model_dir: None,
        };
        assert!(none.ocr_model_dir.is_none());
        assert!(none.florence_model_dir.is_none());
    }

    // =============================================================================
    // VisionModelInfo Tests
    // =============================================================================

    /// Test 4: VisionModelInfo creation
    #[test]
    fn test_vision_model_info_creation() {
        let info = VisionModelInfo {
            name: "paddleocr".to_string(),
            model_type: "ocr".to_string(),
            available: true,
        };

        assert_eq!(info.name, "paddleocr");
        assert_eq!(info.model_type, "ocr");
        assert!(info.available);
    }

    /// Test 5: VisionModelInfo clone
    #[test]
    fn test_vision_model_info_clone() {
        let info = VisionModelInfo {
            name: "florence-2".to_string(),
            model_type: "vision".to_string(),
            available: false,
        };

        let cloned = info.clone();
        assert_eq!(cloned.name, "florence-2");
        assert_eq!(cloned.model_type, "vision");
        assert!(!cloned.available);
    }

    // =============================================================================
    // VisionModelManager Tests - Without Models (Unit Tests)
    // =============================================================================

    /// Test 6: Manager initializes with no models configured
    #[tokio::test]
    async fn test_manager_initializes_with_no_models() {
        let config = VisionModelConfig {
            ocr_model_dir: None,
            florence_model_dir: None,
        };

        let result = VisionModelManager::new(config).await;

        assert!(result.is_ok(), "Manager should initialize even with no models");

        let manager = result.unwrap();
        assert!(!manager.has_ocr());
        assert!(!manager.has_florence());
    }

    /// Test 7: Manager gracefully handles missing OCR directory
    #[tokio::test]
    async fn test_manager_handles_missing_ocr_directory() {
        let config = VisionModelConfig {
            ocr_model_dir: Some("/nonexistent/ocr/path".to_string()),
            florence_model_dir: None,
        };

        let result = VisionModelManager::new(config).await;

        // Should not error, just not have the model available
        assert!(result.is_ok(), "Manager should handle missing directories gracefully");

        let manager = result.unwrap();
        assert!(!manager.has_ocr(), "OCR should not be available with missing directory");
    }

    /// Test 8: Manager gracefully handles missing Florence directory
    #[tokio::test]
    async fn test_manager_handles_missing_florence_directory() {
        let config = VisionModelConfig {
            ocr_model_dir: None,
            florence_model_dir: Some("/nonexistent/florence/path".to_string()),
        };

        let result = VisionModelManager::new(config).await;

        // Should not error, just not have the model available
        assert!(result.is_ok(), "Manager should handle missing directories gracefully");

        let manager = result.unwrap();
        assert!(
            !manager.has_florence(),
            "Florence should not be available with missing directory"
        );
    }

    /// Test 9: Manager gracefully handles both missing directories
    #[tokio::test]
    async fn test_manager_handles_both_missing_directories() {
        let config = VisionModelConfig {
            ocr_model_dir: Some("/nonexistent/ocr".to_string()),
            florence_model_dir: Some("/nonexistent/florence".to_string()),
        };

        let result = VisionModelManager::new(config).await;

        assert!(result.is_ok(), "Manager should handle missing directories gracefully");

        let manager = result.unwrap();
        assert!(!manager.has_ocr());
        assert!(!manager.has_florence());
    }

    /// Test 10: list_models with no models returns expected info
    #[tokio::test]
    async fn test_list_models_with_no_models() {
        let config = VisionModelConfig {
            ocr_model_dir: None,
            florence_model_dir: None,
        };

        let manager = VisionModelManager::new(config)
            .await
            .expect("Manager should initialize");

        let models = manager.list_models();

        // Should still list both model types, but as unavailable
        assert_eq!(models.len(), 2);

        let ocr_info = models.iter().find(|m| m.model_type == "ocr");
        assert!(ocr_info.is_some());
        assert!(!ocr_info.unwrap().available);

        let florence_info = models.iter().find(|m| m.model_type == "vision");
        assert!(florence_info.is_some());
        assert!(!florence_info.unwrap().available);
    }

    /// Test 11: get_ocr_model returns None when not loaded
    #[tokio::test]
    async fn test_get_ocr_model_returns_none_when_not_loaded() {
        let config = VisionModelConfig {
            ocr_model_dir: None,
            florence_model_dir: None,
        };

        let manager = VisionModelManager::new(config)
            .await
            .expect("Manager should initialize");

        assert!(manager.get_ocr_model().is_none());
    }

    /// Test 12: get_florence_model returns None when not loaded
    #[tokio::test]
    async fn test_get_florence_model_returns_none_when_not_loaded() {
        let config = VisionModelConfig {
            ocr_model_dir: None,
            florence_model_dir: None,
        };

        let manager = VisionModelManager::new(config)
            .await
            .expect("Manager should initialize");

        assert!(manager.get_florence_model().is_none());
    }

    // =============================================================================
    // VisionModelManager Tests - With Models (Integration Tests)
    // =============================================================================

    /// Test 13: Manager loads OCR model successfully
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_manager_loads_ocr_model() {
        let config = VisionModelConfig {
            ocr_model_dir: Some(OCR_MODEL_DIR.to_string()),
            florence_model_dir: None,
        };

        let result = VisionModelManager::new(config).await;

        assert!(result.is_ok(), "Failed to create manager: {:?}", result.err());

        let manager = result.unwrap();
        assert!(manager.has_ocr(), "OCR should be available");
        assert!(!manager.has_florence(), "Florence should not be available");

        let ocr_model = manager.get_ocr_model();
        assert!(ocr_model.is_some(), "get_ocr_model should return model");
        assert!(ocr_model.unwrap().is_ready(), "OCR model should be ready");
    }

    /// Test 14: Manager loads Florence model successfully
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_manager_loads_florence_model() {
        let config = VisionModelConfig {
            ocr_model_dir: None,
            florence_model_dir: Some(FLORENCE_MODEL_DIR.to_string()),
        };

        let result = VisionModelManager::new(config).await;

        assert!(result.is_ok(), "Failed to create manager: {:?}", result.err());

        let manager = result.unwrap();
        assert!(!manager.has_ocr(), "OCR should not be available");
        assert!(manager.has_florence(), "Florence should be available");

        let florence_model = manager.get_florence_model();
        assert!(florence_model.is_some(), "get_florence_model should return model");
        assert!(
            florence_model.unwrap().is_ready(),
            "Florence model should be ready"
        );
    }

    /// Test 15: Manager loads both models successfully
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_manager_loads_both_models() {
        let config = VisionModelConfig {
            ocr_model_dir: Some(OCR_MODEL_DIR.to_string()),
            florence_model_dir: Some(FLORENCE_MODEL_DIR.to_string()),
        };

        let result = VisionModelManager::new(config).await;

        assert!(result.is_ok(), "Failed to create manager: {:?}", result.err());

        let manager = result.unwrap();
        assert!(manager.has_ocr(), "OCR should be available");
        assert!(manager.has_florence(), "Florence should be available");

        // Verify both models
        assert!(manager.get_ocr_model().is_some());
        assert!(manager.get_florence_model().is_some());
    }

    /// Test 16: list_models with both models loaded
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_list_models_with_both_loaded() {
        let config = VisionModelConfig {
            ocr_model_dir: Some(OCR_MODEL_DIR.to_string()),
            florence_model_dir: Some(FLORENCE_MODEL_DIR.to_string()),
        };

        let manager = VisionModelManager::new(config)
            .await
            .expect("Manager should initialize");

        let models = manager.list_models();

        assert_eq!(models.len(), 2);

        let ocr_info = models.iter().find(|m| m.name == "paddleocr");
        assert!(ocr_info.is_some());
        assert!(ocr_info.unwrap().available);

        let florence_info = models.iter().find(|m| m.name == "florence-2");
        assert!(florence_info.is_some());
        assert!(florence_info.unwrap().available);
    }

    /// Test 17: list_models with only OCR loaded
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_list_models_ocr_only() {
        let config = VisionModelConfig {
            ocr_model_dir: Some(OCR_MODEL_DIR.to_string()),
            florence_model_dir: None,
        };

        let manager = VisionModelManager::new(config)
            .await
            .expect("Manager should initialize");

        let models = manager.list_models();

        let ocr_info = models.iter().find(|m| m.name == "paddleocr");
        let florence_info = models.iter().find(|m| m.name == "florence-2");

        assert!(ocr_info.unwrap().available);
        assert!(!florence_info.unwrap().available);
    }

    /// Test 18: Default config loads models from expected paths
    #[tokio::test]
    #[ignore] // Requires model files in default locations
    async fn test_default_config_loads_models() {
        let config = VisionModelConfig::default();
        let manager = VisionModelManager::new(config)
            .await
            .expect("Manager should initialize with default config");

        // With models in default locations, both should load
        assert!(manager.has_ocr(), "OCR should be available with default config");
        assert!(
            manager.has_florence(),
            "Florence should be available with default config"
        );
    }

    /// Test 19: Manager can be cloned via Arc
    #[tokio::test]
    #[ignore] // Requires model files
    async fn test_manager_arc_clone() {
        use std::sync::Arc;

        let config = VisionModelConfig {
            ocr_model_dir: Some(OCR_MODEL_DIR.to_string()),
            florence_model_dir: None,
        };

        let manager = Arc::new(
            VisionModelManager::new(config)
                .await
                .expect("Manager should initialize"),
        );

        // Clone the Arc
        let manager_clone = Arc::clone(&manager);

        // Both should see OCR as available
        assert!(manager.has_ocr());
        assert!(manager_clone.has_ocr());

        // Model instances should be the same (via Arc)
        let model1 = manager.get_ocr_model();
        let model2 = manager_clone.get_ocr_model();

        assert!(model1.is_some());
        assert!(model2.is_some());
    }
}
