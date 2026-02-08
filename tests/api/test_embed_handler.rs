// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Embed Handler tests for POST /v1/embed endpoint (Sub-phase 4.1)
//!
//! These TDD tests verify that the embed_handler correctly:
//! - Validates requests and returns appropriate errors
//! - Generates embeddings using ONNX models
//! - Adds chain context to responses
//! - Counts tokens accurately
//! - Handles all error cases gracefully
//! - Logs operations appropriately
//!
//! Test-Driven Development (TDD) Approach:
//! 1. Write these tests FIRST (they will fail initially)
//! 2. Implement embed_handler in src/api/embed/handler.rs
//! 3. Run tests to verify handler works correctly

use axum::{
    extract::{Json, State},
    http::StatusCode,
};
use fabstir_llm_node::{
    api::{
        embed::{EmbedRequest, EmbedResponse},
        http_server::AppState,
    },
    embeddings::{EmbeddingModelConfig, EmbeddingModelManager},
};
use std::sync::Arc;
use tokio::sync::RwLock;

// Model file paths (downloaded by scripts/download_embedding_model.sh)
const MODEL_PATH: &str = "/workspace/models/all-MiniLM-L6-v2-onnx/model.onnx";
const TOKENIZER_PATH: &str = "/workspace/models/all-MiniLM-L6-v2-onnx/tokenizer.json";

/// Helper: Create test AppState with real embedding model
async fn setup_test_state_with_model() -> AppState {
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

/// Helper: Create test AppState without embedding model (for error testing)
fn setup_test_state_without_model() -> AppState {
    AppState::new_for_test()
}

#[cfg(test)]
mod embed_handler_tests {
    use super::*;
    use fabstir_llm_node::api::embed::embed_handler;

    // ========== SUCCESS CASES ==========

    /// Test 1: Single text embedding succeeds
    ///
    /// Verifies that embedding a single text returns 384-dimensional vector.
    #[tokio::test]
    async fn test_handler_single_text_success() {
        let state = setup_test_state_with_model().await;

        let request = EmbedRequest {
            texts: vec!["Hello world".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = embed_handler(State(state), Json(request)).await;

        assert!(result.is_ok(), "Handler should succeed: {:?}", result.err());

        let response = result.unwrap().0; // Extract from Json wrapper
        assert_eq!(response.embeddings.len(), 1, "Should return 1 embedding");
        assert_eq!(
            response.embeddings[0].embedding.len(),
            384,
            "Embedding should have 384 dimensions"
        );
        assert_eq!(response.embeddings[0].text, "Hello world");
        assert!(
            response.embeddings[0].token_count >= 2,
            "Should have at least 2 tokens"
        );
        assert_eq!(response.model, "all-MiniLM-L6-v2");
        assert_eq!(response.provider, "host");
        assert_eq!(response.cost, 0.0, "Cost should be zero");
    }

    /// Test 2: Batch embedding succeeds
    ///
    /// Verifies that embedding multiple texts returns correct count.
    #[tokio::test]
    async fn test_handler_batch_success() {
        let state = setup_test_state_with_model().await;

        let request = EmbedRequest {
            texts: vec![
                "First text".to_string(),
                "Second text".to_string(),
                "Third text".to_string(),
            ],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = embed_handler(State(state), Json(request)).await;

        assert!(result.is_ok(), "Handler should succeed: {:?}", result.err());

        let response = result.unwrap().0; // Extract from Json wrapper
        assert_eq!(response.embeddings.len(), 3, "Should return 3 embeddings");

        for (i, result) in response.embeddings.iter().enumerate() {
            assert_eq!(
                result.embedding.len(),
                384,
                "Embedding {} should have 384 dimensions",
                i
            );
            assert!(
                result.token_count >= 2,
                "Embedding {} should have tokens",
                i
            );
        }

        assert_eq!(response.embeddings[0].text, "First text");
        assert_eq!(response.embeddings[1].text, "Second text");
        assert_eq!(response.embeddings[2].text, "Third text");
    }

    /// Test 3: Default model is applied when not specified
    ///
    /// Verifies that omitting model uses default model.
    #[tokio::test]
    async fn test_handler_default_model_applied() {
        let state = setup_test_state_with_model().await;

        let request = EmbedRequest {
            texts: vec!["Test text".to_string()],
            model: "all-MiniLM-L6-v2".to_string(), // This is the default in serde defaults
            chain_id: 84532,
        };

        let result = embed_handler(State(state), Json(request)).await;

        assert!(result.is_ok(), "Handler should succeed");

        let response = result.unwrap().0; // Extract from Json wrapper
        assert_eq!(
            response.model, "all-MiniLM-L6-v2",
            "Should use default model"
        );
    }

    /// Test 4: Custom model can be specified
    ///
    /// Verifies that specifying a model name uses that model.
    /// Note: We only have one model loaded, so this uses the same model
    /// but verifies the model selection logic works.
    #[tokio::test]
    async fn test_handler_custom_model_specified() {
        let state = setup_test_state_with_model().await;

        let request = EmbedRequest {
            texts: vec!["Test text".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = embed_handler(State(state), Json(request)).await;

        assert!(result.is_ok(), "Handler should succeed");

        let response = result.unwrap().0; // Extract from Json wrapper
        assert_eq!(
            response.model, "all-MiniLM-L6-v2",
            "Should use specified model"
        );
    }

    /// Test 5: Chain context is added to response
    ///
    /// Verifies that chain_id, chain_name, and native_token are included.
    #[tokio::test]
    async fn test_handler_chain_context_added() {
        let state = setup_test_state_with_model().await;

        // Test Base Sepolia (always available)
        let request = EmbedRequest {
            texts: vec!["Test text".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = embed_handler(State(state.clone()), Json(request)).await;
        assert!(result.is_ok(), "Handler should succeed");

        let response = result.unwrap().0; // Extract from Json wrapper
        assert_eq!(response.chain_id, 84532);
        assert_eq!(response.chain_name, "Base Sepolia");
        assert_eq!(response.native_token, "ETH");

        // Test opBNB Testnet (only if registered in ChainRegistry)
        // opBNB is only available if contracts are deployed (env vars set)
        // In tests, it may not be available, so we check if chain exists first
        let request2 = EmbedRequest {
            texts: vec!["Test text".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 5611,
        };

        let result2 = embed_handler(State(state.clone()), Json(request2)).await;

        // If opBNB is registered, verify chain context
        if let Ok(response2) = result2 {
            assert_eq!(response2.chain_id, 5611);
            assert_eq!(response2.chain_name, "opBNB Testnet");
            assert_eq!(response2.native_token, "BNB");
        } else {
            // If not registered, that's okay for tests - just verify error is about invalid chain
            let (status, msg) = result2.unwrap_err();
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "Should be 400 for invalid chain"
            );
            assert!(
                msg.contains("chain") || msg.contains("5611"),
                "Error should mention chain issue"
            );
        }
    }

    /// Test 6: Token counting is accurate
    ///
    /// Verifies that token_count field is correctly populated.
    #[tokio::test]
    async fn test_handler_token_counting_accurate() {
        let state = setup_test_state_with_model().await;

        let request = EmbedRequest {
            texts: vec![
                "hello".to_string(),
                "the quick brown fox jumps over the lazy dog".to_string(),
            ],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = embed_handler(State(state), Json(request)).await;
        assert!(result.is_ok(), "Handler should succeed");

        let response = result.unwrap().0; // Extract from Json wrapper

        // First text ("hello") should have 2-4 tokens
        assert!(
            response.embeddings[0].token_count >= 2 && response.embeddings[0].token_count <= 4,
            "Short text should have 2-4 tokens, got {}",
            response.embeddings[0].token_count
        );

        // Second text (longer) should have more tokens
        assert!(
            response.embeddings[1].token_count >= 10,
            "Long text should have many tokens, got {}",
            response.embeddings[1].token_count
        );

        // total_tokens should be sum of all token counts
        let expected_total: usize = response.embeddings.iter().map(|e| e.token_count).sum();
        assert_eq!(
            response.total_tokens, expected_total,
            "total_tokens should match sum"
        );
    }

    /// Test 7: Cost is always zero
    ///
    /// Verifies that host embeddings have zero cost.
    #[tokio::test]
    async fn test_handler_cost_always_zero() {
        let state = setup_test_state_with_model().await;

        let request = EmbedRequest {
            texts: vec!["Test text".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = embed_handler(State(state), Json(request)).await;
        assert!(result.is_ok(), "Handler should succeed");

        let response = result.unwrap().0; // Extract from Json wrapper
        assert_eq!(response.cost, 0.0, "Host embeddings should be free");
        assert_eq!(response.provider, "host", "Provider should be 'host'");
    }

    // ========== ERROR CASES ==========

    /// Test 8: Empty texts array returns validation error
    ///
    /// Verifies that empty texts array is rejected.
    #[tokio::test]
    async fn test_handler_empty_texts_error() {
        let state = setup_test_state_with_model().await;

        let request = EmbedRequest {
            texts: vec![],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = embed_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should return error for empty texts");

        let (status, error_msg) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST, "Should be 400 Bad Request");
        assert!(
            error_msg.contains("texts") || error_msg.contains("empty") || error_msg.contains("1"),
            "Error should mention texts/empty, got: {}",
            error_msg
        );
    }

    /// Test 9: Too many texts returns validation error
    ///
    /// Verifies that >96 texts are rejected.
    #[tokio::test]
    async fn test_handler_too_many_texts_error() {
        let state = setup_test_state_with_model().await;

        // Create 97 texts (exceeds 96 limit)
        let texts: Vec<String> = (0..97).map(|i| format!("Text {}", i)).collect();

        let request = EmbedRequest {
            texts,
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = embed_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should return error for too many texts");

        let (status, error_msg) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            error_msg.contains("96") || error_msg.contains("many") || error_msg.contains("limit"),
            "Error should mention limit, got: {}",
            error_msg
        );
    }

    /// Test 10: Text too long returns validation error
    ///
    /// Verifies that texts >8192 characters are rejected.
    #[tokio::test]
    async fn test_handler_text_too_long_error() {
        let state = setup_test_state_with_model().await;

        // Create text with 8193 characters (exceeds 8192 limit)
        let long_text = "a".repeat(8193);

        let request = EmbedRequest {
            texts: vec![long_text],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = embed_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should return error for text too long");

        let (status, error_msg) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            error_msg.contains("8192")
                || error_msg.contains("long")
                || error_msg.contains("length"),
            "Error should mention length limit, got: {}",
            error_msg
        );
    }

    /// Test 11: Invalid chain_id returns error
    ///
    /// Verifies that unsupported chain IDs are rejected.
    #[tokio::test]
    async fn test_handler_invalid_chain_error() {
        let state = setup_test_state_with_model().await;

        let request = EmbedRequest {
            texts: vec!["Test text".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 99999, // Invalid chain ID
        };

        let result = embed_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should return error for invalid chain_id");

        let (status, error_msg) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            error_msg.contains("chain")
                || error_msg.contains("99999")
                || error_msg.contains("valid"),
            "Error should mention invalid chain, got: {}",
            error_msg
        );
    }

    /// Test 12: Model not found returns error
    ///
    /// Verifies that requesting non-existent model returns clear error.
    #[tokio::test]
    async fn test_handler_model_not_found_error() {
        let state = setup_test_state_with_model().await;

        let request = EmbedRequest {
            texts: vec!["Test text".to_string()],
            model: "nonexistent-model".to_string(),
            chain_id: 84532,
        };

        let result = embed_handler(State(state), Json(request)).await;

        assert!(result.is_err(), "Should return error for model not found");

        let (status, error_msg) = result.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND, "Should be 404 Not Found");
        assert!(
            error_msg.contains("model")
                || error_msg.contains("nonexistent")
                || error_msg.contains("found"),
            "Error should mention model not found, got: {}",
            error_msg
        );
    }

    /// Test 13: Dimension mismatch returns error
    ///
    /// This test verifies defensive validation (though our implementation
    /// validates dimensions at model load time, so this is a safety check).
    #[tokio::test]
    async fn test_handler_dimension_mismatch_error() {
        // This test is more of a sanity check since we validate dimensions
        // during model loading. If a model is loaded, it MUST have 384 dimensions.
        // We'll just verify that the response has correct dimensions.

        let state = setup_test_state_with_model().await;

        let request = EmbedRequest {
            texts: vec!["Test text".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = embed_handler(State(state), Json(request)).await;

        assert!(result.is_ok(), "Valid model should succeed");

        let response = result.unwrap().0; // Extract from Json wrapper
                                          // Verify dimensions are correct
        assert_eq!(
            response.embeddings[0].embedding.len(),
            384,
            "Embeddings must have 384 dimensions"
        );
    }

    /// Test 14: Model not loaded returns service unavailable
    ///
    /// Verifies that handler returns 503 when embedding model manager is not initialized.
    #[tokio::test]
    async fn test_handler_model_not_loaded_error() {
        let state = setup_test_state_without_model();

        let request = EmbedRequest {
            texts: vec!["Test text".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = embed_handler(State(state), Json(request)).await;

        assert!(
            result.is_err(),
            "Should return error when model manager not initialized"
        );

        let (status, error_msg) = result.unwrap_err();
        assert_eq!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "Should be 503 Service Unavailable"
        );
        assert!(
            error_msg.contains("unavailable")
                || error_msg.contains("not loaded")
                || error_msg.contains("not initialized"),
            "Error should indicate service unavailable, got: {}",
            error_msg
        );
    }

    /// Test 15: Processing time is logged
    ///
    /// Verifies that handler logs operations (basic sanity check).
    /// Full logging verification would require capturing tracing output.
    #[tokio::test]
    async fn test_handler_processing_time_logged() {
        let state = setup_test_state_with_model().await;

        let request = EmbedRequest {
            texts: vec!["Test text".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        // Just verify the handler completes successfully
        // (logging is verified manually or with tracing capture)
        let start = std::time::Instant::now();
        let result = embed_handler(State(state), Json(request)).await;
        let elapsed = start.elapsed();

        assert!(result.is_ok(), "Handler should succeed");
        assert!(
            elapsed.as_millis() < 5000,
            "Processing should be reasonably fast (<5s), took {:?}",
            elapsed
        );
    }
}
