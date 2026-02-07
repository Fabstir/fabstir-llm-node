// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Model Discovery Endpoint tests for GET /v1/models (Sub-phase 5.1)
//!
//! These tests verify that:
//! - The /v1/models endpoint returns embedding models when ?type=embedding
//! - The endpoint returns inference models when ?type=inference or no type
//! - Default model is correctly marked
//! - Model dimensions are included in response
//! - Model availability status is accurate
//! - Empty array returned when no models loaded
//! - Chain context is included in response
//! - Query parameter filtering works correctly

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use fabstir_llm_node::{
    api::{
        handlers::{ModelInfo as InferenceModelInfo, ModelsResponse},
        http_server::{create_app, AppState},
    },
    embeddings::{EmbeddingModelConfig, EmbeddingModelManager},
};
use serde_json::Value;
use std::sync::Arc;
use tower::util::ServiceExt;

const MODEL_PATH: &str = "/workspace/models/all-MiniLM-L6-v2-onnx/model.onnx";
const TOKENIZER_PATH: &str = "/workspace/models/all-MiniLM-L6-v2-onnx/tokenizer.json";

/// Helper: Create test AppState with embedding model
async fn setup_state_with_embeddings() -> AppState {
    let configs = vec![EmbeddingModelConfig {
        name: "all-MiniLM-L6-v2".to_string(),
        model_path: MODEL_PATH.to_string(),
        tokenizer_path: TOKENIZER_PATH.to_string(),
        dimensions: 384,
    }];

    let manager = EmbeddingModelManager::new(configs)
        .await
        .expect("Failed to create embedding model manager");

    let mut state = AppState::new_for_test();
    *state.embedding_model_manager.write().await = Some(Arc::new(manager));
    state
}

/// Helper: Create test AppState without embedding model
fn setup_state_without_embeddings() -> AppState {
    AppState::new_for_test()
}

#[cfg(test)]
mod models_endpoint_tests {
    use super::*;

    /// Test 1: List embedding models with ?type=embedding
    ///
    /// Verifies that GET /v1/models?type=embedding returns embedding models.
    #[tokio::test]
    async fn test_list_embedding_models() {
        let state = setup_state_with_embeddings().await;
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models?type=embedding")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Should return 200 OK for embedding models"
        );

        // Parse response body
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        // Verify models array exists
        assert!(
            body.get("models").is_some(),
            "Response should have models field"
        );
        let models = body["models"].as_array().unwrap();
        assert_eq!(models.len(), 1, "Should return 1 embedding model");

        // Verify model structure
        let model = &models[0];
        assert_eq!(model["name"], "all-MiniLM-L6-v2");
        assert_eq!(model["dimensions"], 384);
        assert_eq!(model["available"], true);
        assert_eq!(model["is_default"], true);
    }

    /// Test 2: List inference models with ?type=inference
    ///
    /// Verifies that GET /v1/models?type=inference returns inference models.
    #[tokio::test]
    async fn test_list_inference_models() {
        let state = setup_state_with_embeddings().await;
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models?type=inference")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Should return 200 OK for inference models"
        );

        // Parse response body
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        // Verify models array exists (may be empty if no inference models loaded)
        assert!(
            body.get("models").is_some(),
            "Response should have models field"
        );

        // Verify inference models don't have embedding-specific fields
        let models = body["models"].as_array().unwrap();
        if !models.is_empty() {
            let model = &models[0];
            assert!(
                model.get("dimensions").is_none(),
                "Inference models should not have dimensions field"
            );
            assert!(
                model.get("is_default").is_none(),
                "Inference models should not have is_default field"
            );
        }
    }

    /// Test 3: Default model is marked
    ///
    /// Verifies that the default embedding model has is_default=true.
    #[tokio::test]
    async fn test_default_model_marked() {
        let state = setup_state_with_embeddings().await;
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models?type=embedding")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        let models = body["models"].as_array().unwrap();

        // Find default model
        let default_models: Vec<&Value> =
            models.iter().filter(|m| m["is_default"] == true).collect();

        assert_eq!(
            default_models.len(),
            1,
            "Exactly one model should be marked as default"
        );
        assert_eq!(default_models[0]["name"], "all-MiniLM-L6-v2");
    }

    /// Test 4: Model dimensions are included
    ///
    /// Verifies that embedding models include dimensions field.
    #[tokio::test]
    async fn test_model_dimensions_included() {
        let state = setup_state_with_embeddings().await;
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models?type=embedding")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        let models = body["models"].as_array().unwrap();
        assert!(!models.is_empty(), "Should have at least one model");

        for model in models {
            assert!(
                model.get("dimensions").is_some(),
                "Each embedding model should have dimensions field"
            );
            assert_eq!(
                model["dimensions"].as_u64().unwrap(),
                384,
                "Model should have 384 dimensions"
            );
        }
    }

    /// Test 5: Model availability status
    ///
    /// Verifies that models include accurate availability status.
    #[tokio::test]
    async fn test_model_availability_status() {
        let state = setup_state_with_embeddings().await;
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models?type=embedding")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        let models = body["models"].as_array().unwrap();
        assert!(!models.is_empty(), "Should have at least one model");

        for model in models {
            assert!(
                model.get("available").is_some(),
                "Each model should have available field"
            );
            assert_eq!(
                model["available"].as_bool().unwrap(),
                true,
                "Loaded models should be marked as available"
            );
        }
    }

    /// Test 6: No models returns empty array
    ///
    /// Verifies that when no embedding models are loaded, returns empty array (not error).
    #[tokio::test]
    async fn test_no_models_returns_empty_array() {
        let state = setup_state_without_embeddings();
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models?type=embedding")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Should return 200 OK even when no models loaded"
        );

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        let models = body["models"].as_array().unwrap();
        assert_eq!(
            models.len(),
            0,
            "Should return empty array when no models loaded"
        );
    }

    /// Test 7: Chain context is included
    ///
    /// Verifies that response includes chain_id and chain_name.
    #[tokio::test]
    async fn test_chain_context_included() {
        let state = setup_state_with_embeddings().await;
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models?type=embedding")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        // Verify chain context
        assert!(
            body.get("chain_id").is_some(),
            "Response should include chain_id"
        );
        assert!(
            body.get("chain_name").is_some(),
            "Response should include chain_name"
        );

        assert_eq!(
            body["chain_id"].as_u64().unwrap(),
            84532,
            "Should default to Base Sepolia"
        );
        assert_eq!(
            body["chain_name"].as_str().unwrap(),
            "Base Sepolia",
            "Should include chain name"
        );
    }

    /// Test 8: Query parameter type filtering works
    ///
    /// Verifies that the type parameter correctly filters model types.
    #[tokio::test]
    async fn test_query_param_type_filtering() {
        let state = setup_state_with_embeddings().await;
        let app = create_app(Arc::new(state));

        // Test without type parameter (should default to inference)
        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        // Without type param, should return inference models (which don't have dimensions)
        let models = body["models"].as_array().unwrap();
        if !models.is_empty() {
            assert!(
                models[0].get("dimensions").is_none(),
                "Default (no type param) should return inference models"
            );
        }
    }
}
