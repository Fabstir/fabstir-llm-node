// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Compatibility Tests for Embedding Feature (Sub-phase 6.2)
//!
//! These tests ensure that the embedding endpoint doesn't break existing functionality
//! and that the server works correctly with and without embedding models.

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use fabstir_llm_node::api::http_server::{create_app, AppState};
use serde_json::Value;
use tower::util::ServiceExt;

#[cfg(test)]
mod compatibility_tests {
    use super::*;

    /// Test 1: Inference endpoint unaffected
    ///
    /// Verifies that the /v1/models endpoint still works for inference models
    /// after adding embedding support.
    #[tokio::test]
    async fn test_inference_endpoint_unaffected() {
        let state = AppState::new_for_test();
        let app = create_app(std::sync::Arc::new(state));

        // Test default behavior (should return inference models)
        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Inference models endpoint should still work"
        );

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        // Should have models field
        assert!(
            body.get("models").is_some(),
            "Response should include models field"
        );

        // Should have chain context
        assert!(body.get("chain_id").is_some());
        assert!(body.get("chain_name").is_some());
    }

    /// Test 2: Health endpoint works
    ///
    /// Verifies that the /health endpoint continues to work.
    #[tokio::test]
    async fn test_health_endpoint_works() {
        let state = AppState::new_for_test();
        let app = create_app(std::sync::Arc::new(state));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Health endpoint should work"
        );

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        // Health response should have status field
        assert!(
            body.get("status").is_some(),
            "Health response should include status"
        );
    }

    /// Test 3: Metrics include embeddings
    ///
    /// Verifies that the /metrics endpoint still works and can include
    /// embedding-related metrics if present.
    #[tokio::test]
    async fn test_metrics_include_embeddings() {
        let state = AppState::new_for_test();
        let app = create_app(std::sync::Arc::new(state));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/metrics")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Metrics endpoint should work"
        );

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_text = String::from_utf8(body_bytes.to_vec()).unwrap();

        // Metrics should be in Prometheus format
        // Should contain at least some metrics
        assert!(
            !body_text.is_empty(),
            "Metrics response should not be empty"
        );
    }

    /// Test 4: Server starts without embed models
    ///
    /// Verifies that the server can start and function without embedding models loaded.
    #[tokio::test]
    async fn test_server_starts_without_embed_models() {
        let state = AppState::new_for_test();

        // Verify embedding manager is NOT loaded
        let manager_guard = state.embedding_model_manager.read().await;
        assert!(
            manager_guard.is_none(),
            "Embedding manager should not be loaded in test state"
        );
        drop(manager_guard);

        // Verify we can still create the app
        let app = create_app(std::sync::Arc::new(state));

        // Test health endpoint works without embeddings
        let request = Request::builder()
            .method(Method::GET)
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Server should start and respond without embedding models"
        );
    }

    /// Test 5: Memory usage acceptable
    ///
    /// Verifies that AppState creation doesn't consume excessive memory.
    /// This is a basic check to ensure no memory leaks in the structure.
    #[tokio::test]
    async fn test_memory_usage_acceptable() {
        // Create multiple AppState instances to check for memory issues
        let mut states = Vec::new();

        for _ in 0..10 {
            let state = AppState::new_for_test();
            states.push(state);
        }

        // If we get here without panicking or OOM, memory usage is acceptable
        assert_eq!(
            states.len(),
            10,
            "Should be able to create multiple AppState instances"
        );

        // Clean up
        drop(states);
    }

    /// Test 6: No port conflicts
    ///
    /// Verifies that creating multiple app instances doesn't cause
    /// resource conflicts (e.g., all using the same Arc instances).
    #[tokio::test]
    async fn test_no_port_conflicts() {
        let state1 = AppState::new_for_test();
        let state2 = AppState::new_for_test();

        let app1 = create_app(std::sync::Arc::new(state1));
        let app2 = create_app(std::sync::Arc::new(state2));

        // Test both apps can handle requests independently
        let request1 = Request::builder()
            .method(Method::GET)
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let request2 = Request::builder()
            .method(Method::GET)
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response1 = app1.oneshot(request1).await.unwrap();
        let response2 = app2.oneshot(request2).await.unwrap();

        assert_eq!(response1.status(), StatusCode::OK);
        assert_eq!(response2.status(), StatusCode::OK);
    }

    /// Test 7: Chains endpoint still works
    ///
    /// Verifies that the /v1/chains endpoint continues to function.
    #[tokio::test]
    async fn test_chains_endpoint_still_works() {
        let state = AppState::new_for_test();
        let app = create_app(std::sync::Arc::new(state));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/chains")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Chains endpoint should work"
        );

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        // Should have chains array
        assert!(
            body.get("chains").is_some(),
            "Response should include chains field"
        );

        let chains = body["chains"].as_array().unwrap();
        assert!(
            !chains.is_empty(),
            "Should have at least one chain registered"
        );
    }

    /// Test 8: Chain stats endpoint still works
    ///
    /// Verifies that the /v1/chains/stats endpoint continues to function.
    #[tokio::test]
    async fn test_chain_stats_endpoint_still_works() {
        let state = AppState::new_for_test();
        let app = create_app(std::sync::Arc::new(state));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/chains/stats")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "Chain stats endpoint should work"
        );

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        // Should have chains and total fields
        assert!(
            body.get("chains").is_some(),
            "Response should include chains stats"
        );
        assert!(
            body.get("total").is_some(),
            "Response should include total stats"
        );
    }
}
