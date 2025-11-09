// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Route Registration tests for /v1/embed endpoint (Sub-phase 4.2)
//!
//! These tests verify that:
//! - The /v1/embed route is properly registered
//! - The route accepts POST requests
//! - The route rejects non-POST requests (e.g., GET)
//! - Server starts successfully with embeddings
//! - Server still starts if embeddings fail to load
//! - Embedding manager is accessible from AppState

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use fabstir_llm_node::{
    api::http_server::{create_app, AppState},
    embeddings::{EmbeddingModelConfig, EmbeddingModelManager},
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::util::ServiceExt; // for `oneshot` and `ready`

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
mod route_registration_tests {
    use super::*;
    use tower::util::ServiceExt;

    /// Test 1: Embed route is registered
    ///
    /// Verifies that the /v1/embed route exists in the router.
    #[tokio::test]
    async fn test_embed_route_registered() {
        let state = setup_state_with_embeddings().await;
        let app = create_app(Arc::new(state));

        // Create a POST request to /v1/embed
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/embed")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"texts": ["test"], "model": "all-MiniLM-L6-v2", "chainId": 84532}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Route should exist (not 404 Not Found for missing route)
        // It should return 200 OK since we have valid request and model loaded
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Route should be registered and return 200 OK"
        );
    }

    /// Test 2: Embed route accepts POST requests
    ///
    /// Verifies that POST /v1/embed works correctly.
    #[tokio::test]
    async fn test_embed_route_accepts_post() {
        let state = setup_state_with_embeddings().await;
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/embed")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"texts": ["Hello world"], "model": "all-MiniLM-L6-v2", "chainId": 84532}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "POST requests should be accepted"
        );
    }

    /// Test 3: Embed route rejects GET requests
    ///
    /// Verifies that GET /v1/embed returns Method Not Allowed.
    #[tokio::test]
    async fn test_embed_route_rejects_get() {
        let state = setup_state_with_embeddings().await;
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/embed")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Should return 405 Method Not Allowed (route exists but wrong method)
        assert_eq!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "GET requests should be rejected with 405"
        );
    }

    /// Test 4: Server starts with embeddings
    ///
    /// Verifies that the server can create an app with embeddings loaded.
    #[tokio::test]
    async fn test_server_starts_with_embeddings() {
        let state = setup_state_with_embeddings().await;

        // Verify embedding manager is loaded
        let manager_guard = state.embedding_model_manager.read().await;
        assert!(
            manager_guard.is_some(),
            "Embedding manager should be loaded"
        );
        drop(manager_guard); // Release the lock before moving state

        // Verify we can create the app
        let app = create_app(Arc::new(state));

        // Verify the app is a valid Router (type check)
        let _router: Router = app;
        // If we get here, the app was created successfully
    }

    /// Test 5: Server starts without embeddings
    ///
    /// Verifies that the server can start even if embeddings are not loaded.
    /// The /v1/embed endpoint should return 503 Service Unavailable.
    #[tokio::test]
    async fn test_server_starts_without_embeddings() {
        let state = setup_state_without_embeddings();

        // Verify embedding manager is NOT loaded
        let manager_guard = state.embedding_model_manager.read().await;
        assert!(
            manager_guard.is_none(),
            "Embedding manager should not be loaded"
        );
        drop(manager_guard); // Release the lock before moving state

        // Verify we can still create the app (graceful degradation)
        let app = create_app(Arc::new(state));

        // Try to use the embed endpoint - should return 503
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/embed")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"texts": ["test"], "model": "all-MiniLM-L6-v2", "chainId": 84532}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "Should return 503 when embeddings not loaded"
        );
    }

    /// Test 6: Embedding manager is accessible from AppState
    ///
    /// Verifies that handlers can access the embedding manager through AppState.
    #[tokio::test]
    async fn test_embedding_manager_accessible() {
        let state = setup_state_with_embeddings().await;

        // Access embedding manager from AppState
        let manager_guard = state.embedding_model_manager.read().await;
        assert!(
            manager_guard.is_some(),
            "Embedding manager should be accessible"
        );

        let manager = manager_guard.as_ref().unwrap();

        // Verify we can get a model from the manager
        let model_result = manager.get_model(Some("all-MiniLM-L6-v2")).await;
        assert!(
            model_result.is_ok(),
            "Should be able to get model from manager"
        );

        let model = model_result.unwrap();
        assert_eq!(model.model_name(), "all-MiniLM-L6-v2");
        assert_eq!(model.dimension(), 384);

        // Verify we can list models
        let models = manager.list_models();
        assert_eq!(models.len(), 1, "Should have 1 model loaded");
        assert_eq!(models[0].name, "all-MiniLM-L6-v2");
        assert!(models[0].available);
        assert!(models[0].is_default);
    }
}
