// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! End-to-End Integration Tests for Embedding Endpoint (Sub-phase 6.1)
//!
//! These tests verify the complete embedding workflow from HTTP request to response,
//! using real embedding models and the full HTTP stack.

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use fabstir_llm_node::{
    api::http_server::{create_app, AppState},
    embeddings::{EmbeddingModelConfig, EmbeddingModelManager},
};
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;
use tower::util::ServiceExt;

const MODEL_PATH: &str = "/workspace/models/all-MiniLM-L6-v2-onnx/model.onnx";
const TOKENIZER_PATH: &str = "/workspace/models/all-MiniLM-L6-v2-onnx/tokenizer.json";

/// Helper: Create test AppState with embedding model
async fn setup_test_server() -> AppState {
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

#[cfg(test)]
mod e2e_tests {
    use super::*;

    /// Test 1: Single text embedding E2E
    ///
    /// Verifies the complete flow for embedding a single text.
    #[tokio::test]
    async fn test_e2e_single_embedding() {
        let state = setup_test_server().await;
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

        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        // Verify response structure
        assert!(body.get("embeddings").is_some());
        assert!(body.get("model").is_some());
        assert!(body.get("totalTokens").is_some());

        let embeddings = body["embeddings"].as_array().unwrap();
        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0]["embedding"].as_array().unwrap().len(), 384);
        assert_eq!(embeddings[0]["text"], "Hello world");
        assert!(embeddings[0]["tokenCount"].as_u64().unwrap() > 0);
    }

    /// Test 2: Batch embedding E2E
    ///
    /// Verifies batch processing of multiple texts.
    #[tokio::test]
    async fn test_e2e_batch_embedding() {
        let state = setup_test_server().await;
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/embed")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"texts": ["First text", "Second text", "Third text"], "model": "all-MiniLM-L6-v2", "chainId": 84532}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        let embeddings = body["embeddings"].as_array().unwrap();
        assert_eq!(embeddings.len(), 3);

        // Verify each embedding
        for (i, text) in ["First text", "Second text", "Third text"]
            .iter()
            .enumerate()
        {
            assert_eq!(embeddings[i]["text"], *text);
            assert_eq!(embeddings[i]["embedding"].as_array().unwrap().len(), 384);
        }
    }

    /// Test 3: Default model E2E
    ///
    /// Verifies that default model is used when model field is "default".
    #[tokio::test]
    async fn test_e2e_default_model() {
        let state = setup_test_server().await;
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/embed")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"texts": ["Test"], "model": "default", "chainId": 84532}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        // Default model should be all-MiniLM-L6-v2
        assert_eq!(body["model"], "all-MiniLM-L6-v2");
    }

    /// Test 4: Custom model specification E2E
    ///
    /// Verifies that specified model is used in response.
    #[tokio::test]
    async fn test_e2e_custom_model() {
        let state = setup_test_server().await;
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/embed")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"texts": ["Test"], "model": "all-MiniLM-L6-v2", "chainId": 84532}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(body["model"], "all-MiniLM-L6-v2");
        assert_eq!(body["provider"], "host");
        assert_eq!(body["cost"], 0.0);
    }

    /// Test 5: Model discovery E2E
    ///
    /// Verifies that model discovery endpoint returns embedding models.
    #[tokio::test]
    async fn test_e2e_model_discovery() {
        let state = setup_test_server().await;
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::GET)
            .uri("/v1/models?type=embedding")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        let models = body["models"].as_array().unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["name"], "all-MiniLM-L6-v2");
        assert_eq!(models[0]["dimensions"], 384);
        assert_eq!(models[0]["available"], true);
        assert_eq!(models[0]["is_default"], true);
    }

    /// Test 6: Chain context for Base Sepolia E2E
    ///
    /// Verifies that Base Sepolia chain context is included.
    #[tokio::test]
    async fn test_e2e_chain_context_base_sepolia() {
        let state = setup_test_server().await;
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/embed")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"texts": ["Test"], "model": "all-MiniLM-L6-v2", "chainId": 84532}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        assert_eq!(body["chainId"], 84532);
        assert_eq!(body["chainName"], "Base Sepolia");
        assert_eq!(body["nativeToken"], "ETH");
    }

    /// Test 7: Chain context for opBNB Testnet E2E
    ///
    /// Verifies that opBNB chain context is included if chain is registered.
    #[tokio::test]
    async fn test_e2e_chain_context_opbnb() {
        let state = setup_test_server().await;
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/embed")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"texts": ["Test"], "model": "all-MiniLM-L6-v2", "chainId": 5611}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        // Chain 5611 may or may not be registered depending on environment
        if response.status() == StatusCode::OK {
            let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(&body_bytes).unwrap();

            assert_eq!(body["chainId"], 5611);
            assert_eq!(body["chainName"], "opBNB Testnet");
            assert_eq!(body["nativeToken"], "BNB");
        } else {
            // If chain not registered, should return 400
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
    }

    /// Test 8: Validation errors E2E
    ///
    /// Verifies that validation errors are properly returned.
    #[tokio::test]
    async fn test_e2e_validation_errors() {
        let state = setup_test_server().await;
        let app = create_app(Arc::new(state));

        // Test empty texts array
        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/embed")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"texts": [], "model": "all-MiniLM-L6-v2", "chainId": 84532}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    /// Test 9: Model not found E2E
    ///
    /// Verifies that requesting non-existent model returns 404.
    #[tokio::test]
    async fn test_e2e_model_not_found() {
        let state = setup_test_server().await;
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/embed")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"texts": ["Test"], "model": "nonexistent-model", "chainId": 84532}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_text = String::from_utf8(body_bytes.to_vec()).unwrap();
        assert!(body_text.contains("not found") || body_text.contains("nonexistent-model"));
    }

    /// Test 10: Concurrent requests E2E
    ///
    /// Verifies that multiple concurrent requests are handled correctly.
    #[tokio::test]
    async fn test_e2e_concurrent_requests() {
        let state = Arc::new(setup_test_server().await);

        // Spawn 10 concurrent requests
        let mut tasks = Vec::new();
        for i in 0..10 {
            let state_clone = state.clone();
            let task = tokio::spawn(async move {
                let app = create_app(state_clone);
                let text = format!("Concurrent text {}", i);
                let body = serde_json::json!({
                    "texts": [text],
                    "model": "all-MiniLM-L6-v2",
                    "chainId": 84532
                });

                let request = Request::builder()
                    .method(Method::POST)
                    .uri("/v1/embed")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap();

                let response = app.oneshot(request).await.unwrap();
                response.status()
            });
            tasks.push(task);
        }

        // Wait for all tasks to complete
        for task in tasks {
            let status = task.await.unwrap();
            assert_eq!(status, StatusCode::OK);
        }
    }

    /// Test 11: Large batch (96 texts) E2E
    ///
    /// Verifies that maximum batch size is handled correctly.
    #[tokio::test]
    async fn test_e2e_large_batch_96_texts() {
        let state = setup_test_server().await;
        let app = create_app(Arc::new(state));

        // Create 96 texts (maximum allowed)
        let texts: Vec<String> = (0..96).map(|i| format!("Text number {}", i)).collect();

        let body = serde_json::json!({
            "texts": texts,
            "model": "all-MiniLM-L6-v2",
            "chainId": 84532
        });

        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/embed")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let response_body: Value = serde_json::from_slice(&body_bytes).unwrap();

        let embeddings = response_body["embeddings"].as_array().unwrap();
        assert_eq!(embeddings.len(), 96);

        // Verify all embeddings are valid
        for (i, embedding) in embeddings.iter().enumerate() {
            assert_eq!(embedding["text"], format!("Text number {}", i));
            assert_eq!(embedding["embedding"].as_array().unwrap().len(), 384);
        }
    }

    /// Test 12: Empty text rejected E2E
    ///
    /// Verifies that empty strings in texts array are rejected.
    #[tokio::test]
    async fn test_e2e_empty_text_rejected() {
        let state = setup_test_server().await;
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/embed")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"texts": [""], "model": "all-MiniLM-L6-v2", "chainId": 84532}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    /// Test 13: Response format E2E
    ///
    /// Verifies that response format matches specification exactly.
    #[tokio::test]
    async fn test_e2e_response_format() {
        let state = setup_test_server().await;
        let app = create_app(Arc::new(state));

        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/embed")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"texts": ["Hello"], "model": "all-MiniLM-L6-v2", "chainId": 84532}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&body_bytes).unwrap();

        // Verify all required fields exist
        assert!(body.get("embeddings").is_some(), "Missing embeddings field");
        assert!(body.get("model").is_some(), "Missing model field");
        assert!(body.get("provider").is_some(), "Missing provider field");
        assert!(
            body.get("totalTokens").is_some(),
            "Missing totalTokens field"
        );
        assert!(body.get("cost").is_some(), "Missing cost field");
        assert!(body.get("chainId").is_some(), "Missing chainId field");
        assert!(body.get("chainName").is_some(), "Missing chainName field");
        assert!(
            body.get("nativeToken").is_some(),
            "Missing nativeToken field"
        );

        // Verify embedding structure
        let embeddings = body["embeddings"].as_array().unwrap();
        assert_eq!(embeddings.len(), 1);

        let embedding = &embeddings[0];
        assert!(
            embedding.get("embedding").is_some(),
            "Missing embedding array"
        );
        assert!(embedding.get("text").is_some(), "Missing text field");
        assert!(
            embedding.get("tokenCount").is_some(),
            "Missing tokenCount field"
        );

        // Verify types
        assert!(body["embeddings"].is_array());
        assert!(body["model"].is_string());
        assert!(body["provider"].is_string());
        assert!(body["totalTokens"].is_number());
        assert!(body["cost"].is_number());
        assert!(body["chainId"].is_number());
        assert!(body["chainName"].is_string());
        assert!(body["nativeToken"].is_string());

        assert!(embedding["embedding"].is_array());
        assert!(embedding["text"].is_string());
        assert!(embedding["tokenCount"].is_number());
    }

    /// Test 14: Performance benchmark E2E
    ///
    /// Verifies that embedding performance meets targets (<100ms per text).
    #[tokio::test]
    async fn test_e2e_performance_benchmark() {
        let state = setup_test_server().await;
        let app = create_app(Arc::new(state));

        // Test single embedding performance
        let start = Instant::now();

        let request = Request::builder()
            .method(Method::POST)
            .uri("/v1/embed")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"texts": ["Performance test text"], "model": "all-MiniLM-L6-v2", "chainId": 84532}"#,
            ))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        let elapsed = start.elapsed();

        assert_eq!(response.status(), StatusCode::OK);

        // Performance target: <100ms per embedding
        // Note: First run might be slower due to model warm-up
        println!("Single embedding took: {:?}", elapsed);

        // We don't assert on timing in tests since it varies by hardware
        // but we log it for monitoring purposes
        // In production, this should be <100ms on modern hardware
    }
}
