// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Model Manager tests for multi-model embedding support (Sub-phase 3.2)
//!
//! These TDD tests verify that the EmbeddingModelManager correctly:
//! - Loads multiple models in parallel
//! - Provides default model selection
//! - Lists available models
//! - Handles partial failures gracefully
//! - Provides thread-safe concurrent access
//!
//! Test-Driven Development (TDD) Approach:
//! 1. Write these tests FIRST (they will fail initially)
//! 2. Implement EmbeddingModelManager in src/embeddings/model_manager.rs
//! 3. Run tests to verify multi-model management works correctly

use fabstir_llm_node::embeddings::{EmbeddingModelConfig, EmbeddingModelManager, ModelInfo};

// Model file paths (downloaded by scripts/download_embedding_model.sh)
const MODEL_PATH: &str = "/workspace/models/all-MiniLM-L6-v2-onnx/model.onnx";
const TOKENIZER_PATH: &str = "/workspace/models/all-MiniLM-L6-v2-onnx/tokenizer.json";

#[cfg(test)]
mod model_manager_tests {
    use super::*;

    /// Helper: Create a valid config for all-MiniLM-L6-v2
    fn create_valid_config(name: &str) -> EmbeddingModelConfig {
        EmbeddingModelConfig {
            name: name.to_string(),
            model_path: MODEL_PATH.to_string(),
            tokenizer_path: TOKENIZER_PATH.to_string(),
            dimensions: 384,
        }
    }

    /// Test 1: Manager loads single model successfully
    ///
    /// Verifies that the manager can load a single embedding model.
    #[tokio::test]
    async fn test_manager_loads_single_model() {
        let configs = vec![create_valid_config("all-MiniLM-L6-v2")];

        let result = EmbeddingModelManager::new(configs).await;

        assert!(
            result.is_ok(),
            "Failed to create manager with single model: {:?}",
            result.err()
        );

        let manager = result.unwrap();

        // Verify default model is set
        assert_eq!(manager.default_model_name(), "all-MiniLM-L6-v2");

        // Verify we can get the model
        let model_result = manager.get_model(None).await;
        assert!(
            model_result.is_ok(),
            "Failed to get default model: {:?}",
            model_result.err()
        );

        let model = model_result.unwrap();
        assert_eq!(model.model_name(), "all-MiniLM-L6-v2");
        assert_eq!(model.dimension(), 384);
    }

    /// Test 2: Manager loads multiple models successfully
    ///
    /// Verifies that the manager can load multiple models.
    /// Note: We only have one physical model, so we'll create multiple configs
    /// pointing to the same files but with different names.
    #[tokio::test]
    async fn test_manager_loads_multiple_models() {
        let configs = vec![
            create_valid_config("model-1"),
            create_valid_config("model-2"),
            create_valid_config("model-3"),
        ];

        let result = EmbeddingModelManager::new(configs).await;

        assert!(
            result.is_ok(),
            "Failed to create manager with multiple models: {:?}",
            result.err()
        );

        let manager = result.unwrap();

        // First config should be default
        assert_eq!(manager.default_model_name(), "model-1");

        // Verify we can get each model by name
        for name in &["model-1", "model-2", "model-3"] {
            let model_result = manager.get_model(Some(name)).await;
            assert!(
                model_result.is_ok(),
                "Failed to get model {}: {:?}",
                name,
                model_result.err()
            );
        }
    }

    /// Test 3: Get default model works
    ///
    /// Verifies that calling get_model(None) returns the default model.
    #[tokio::test]
    async fn test_get_default_model() {
        let configs = vec![
            create_valid_config("first-model"),
            create_valid_config("second-model"),
        ];

        let manager = EmbeddingModelManager::new(configs)
            .await
            .expect("Failed to create manager");

        // Get default model (should be first-model)
        let default_model = manager
            .get_model(None)
            .await
            .expect("Failed to get default model");

        assert_eq!(default_model.model_name(), "first-model");

        // Verify default_model_name() matches
        assert_eq!(manager.default_model_name(), "first-model");
    }

    /// Test 4: Get model by name works
    ///
    /// Verifies that calling get_model(Some(name)) returns the correct model.
    #[tokio::test]
    async fn test_get_model_by_name() {
        let configs = vec![
            create_valid_config("alpha"),
            create_valid_config("beta"),
            create_valid_config("gamma"),
        ];

        let manager = EmbeddingModelManager::new(configs)
            .await
            .expect("Failed to create manager");

        // Get specific models by name
        let alpha = manager
            .get_model(Some("alpha"))
            .await
            .expect("Failed to get alpha model");
        assert_eq!(alpha.model_name(), "alpha");

        let beta = manager
            .get_model(Some("beta"))
            .await
            .expect("Failed to get beta model");
        assert_eq!(beta.model_name(), "beta");

        let gamma = manager
            .get_model(Some("gamma"))
            .await
            .expect("Failed to get gamma model");
        assert_eq!(gamma.model_name(), "gamma");
    }

    /// Test 5: Get nonexistent model returns error
    ///
    /// Verifies that requesting a model that doesn't exist returns a clear error.
    #[tokio::test]
    async fn test_get_nonexistent_model_error() {
        let configs = vec![create_valid_config("existing-model")];

        let manager = EmbeddingModelManager::new(configs)
            .await
            .expect("Failed to create manager");

        // Try to get a model that doesn't exist
        let result = manager.get_model(Some("nonexistent-model")).await;

        assert!(
            result.is_err(),
            "Getting nonexistent model should return error"
        );

        let error = result.unwrap_err();
        let error_msg = error.to_string();

        // Error message should mention the model name
        assert!(
            error_msg.contains("nonexistent-model") || error_msg.contains("not found"),
            "Error message should mention model name, got: {}",
            error_msg
        );
    }

    /// Test 6: List all models returns correct information
    ///
    /// Verifies that list_models() returns complete information about all models.
    #[tokio::test]
    async fn test_list_all_models() {
        let configs = vec![
            create_valid_config("model-a"),
            create_valid_config("model-b"),
            create_valid_config("model-c"),
        ];

        let manager = EmbeddingModelManager::new(configs)
            .await
            .expect("Failed to create manager");

        let models = manager.list_models();

        // Should have 3 models
        assert_eq!(models.len(), 3, "Should have 3 models");

        // Verify each model info
        let model_names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
        assert!(model_names.contains(&"model-a"), "Should contain model-a");
        assert!(model_names.contains(&"model-b"), "Should contain model-b");
        assert!(model_names.contains(&"model-c"), "Should contain model-c");

        // All models should have 384 dimensions
        for model_info in &models {
            assert_eq!(
                model_info.dimensions, 384,
                "Model {} should have 384 dimensions",
                model_info.name
            );
        }

        // All models should be available (successfully loaded)
        for model_info in &models {
            assert!(
                model_info.available,
                "Model {} should be available",
                model_info.name
            );
        }

        // First model should be marked as default
        let default_models: Vec<&ModelInfo> =
            models.iter().filter(|m| m.is_default).collect();
        assert_eq!(
            default_models.len(),
            1,
            "Exactly one model should be marked as default"
        );
        assert_eq!(
            default_models[0].name, "model-a",
            "First model should be default"
        );
    }

    /// Test 7: Models load in parallel (performance test)
    ///
    /// Verifies that multiple models are loaded concurrently, not sequentially.
    #[tokio::test]
    async fn test_parallel_model_loading() {
        let configs = vec![
            create_valid_config("parallel-1"),
            create_valid_config("parallel-2"),
            create_valid_config("parallel-3"),
        ];

        let start = std::time::Instant::now();

        let result = EmbeddingModelManager::new(configs).await;

        let elapsed = start.elapsed();

        assert!(
            result.is_ok(),
            "Failed to create manager: {:?}",
            result.err()
        );

        // If loading were sequential, 3 models would take ~3x the time of 1 model
        // With parallel loading, it should be closer to 1x time
        // We'll just verify it completes in reasonable time (<10 seconds)
        assert!(
            elapsed.as_secs() < 10,
            "Parallel loading should complete quickly, took {:?}",
            elapsed
        );

        let manager = result.unwrap();
        let models = manager.list_models();
        assert_eq!(models.len(), 3, "Should have loaded 3 models");
    }

    /// Test 8: Partial load failure is acceptable
    ///
    /// Verifies that if some models fail to load, the manager still works
    /// with the models that succeeded.
    #[tokio::test]
    async fn test_partial_load_failure_acceptable() {
        let configs = vec![
            // Valid model
            create_valid_config("valid-model"),
            // Invalid model (nonexistent path)
            EmbeddingModelConfig {
                name: "invalid-model".to_string(),
                model_path: "/nonexistent/path/model.onnx".to_string(),
                tokenizer_path: "/nonexistent/path/tokenizer.json".to_string(),
                dimensions: 384,
            },
        ];

        let result = EmbeddingModelManager::new(configs).await;

        // Manager should still be created (1 model loaded successfully)
        assert!(
            result.is_ok(),
            "Manager should be created even if some models fail: {:?}",
            result.err()
        );

        let manager = result.unwrap();

        // Should have the valid model
        let valid_model = manager.get_model(Some("valid-model")).await;
        assert!(
            valid_model.is_ok(),
            "Valid model should be accessible: {:?}",
            valid_model.err()
        );

        // Invalid model should not be accessible
        let invalid_model = manager.get_model(Some("invalid-model")).await;
        assert!(
            invalid_model.is_err(),
            "Invalid model should not be accessible"
        );

        // list_models() should only show the valid model
        let models = manager.list_models();
        assert_eq!(
            models.len(),
            1,
            "Should only have 1 successfully loaded model"
        );
        assert_eq!(models[0].name, "valid-model");
        assert!(models[0].available);
    }

    /// Test 9: All models fail returns error
    ///
    /// Verifies that if NO models load successfully, an error is returned.
    #[tokio::test]
    async fn test_all_models_fail_returns_error() {
        let configs = vec![
            EmbeddingModelConfig {
                name: "invalid-1".to_string(),
                model_path: "/nonexistent/path1/model.onnx".to_string(),
                tokenizer_path: "/nonexistent/path1/tokenizer.json".to_string(),
                dimensions: 384,
            },
            EmbeddingModelConfig {
                name: "invalid-2".to_string(),
                model_path: "/nonexistent/path2/model.onnx".to_string(),
                tokenizer_path: "/nonexistent/path2/tokenizer.json".to_string(),
                dimensions: 384,
            },
        ];

        let result = EmbeddingModelManager::new(configs).await;

        assert!(
            result.is_err(),
            "Manager creation should fail if no models load successfully"
        );

        let error = result.unwrap_err();
        let error_msg = error.to_string();

        // Error message should indicate no models loaded
        assert!(
            error_msg.contains("no models") || error_msg.contains("No models"),
            "Error message should indicate no models loaded, got: {}",
            error_msg
        );
    }
}
