// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! POST /v1/embed HTTP handler (Sub-phase 4.1)
//!
//! This module implements the HTTP handler for the embedding endpoint.
//! Integrates ONNX embedding models with the Axum HTTP server.

use crate::api::embed::{EmbedRequest, EmbedResponse, EmbeddingResult};
use crate::api::http_server::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use tracing::{debug, error, info};

/// POST /v1/embed handler
///
/// Generates embeddings for input texts using ONNX Runtime.
///
/// # Request Body
/// ```json
/// {
///   "texts": ["text1", "text2", ...],
///   "model": "all-MiniLM-L6-v2",  // optional, defaults to default model
///   "chainId": 84532               // optional, defaults to Base Sepolia
/// }
/// ```
///
/// # Response Body
/// ```json
/// {
///   "embeddings": [
///     {
///       "embedding": [0.1, 0.2, ...],
///       "text": "text1",
///       "tokenCount": 2
///     }
///   ],
///   "model": "all-MiniLM-L6-v2",
///   "provider": "host",
///   "totalTokens": 2,
///   "cost": 0.0,
///   "chainId": 84532,
///   "chainName": "Base Sepolia",
///   "nativeToken": "ETH"
/// }
/// ```
///
/// # Error Responses
/// - 400 Bad Request: Invalid request (empty texts, too many, too long, invalid chain)
/// - 404 Not Found: Model not found
/// - 503 Service Unavailable: Embedding model manager not loaded
/// - 500 Internal Server Error: Inference failed
pub async fn embed_handler(
    State(state): State<AppState>,
    Json(request): Json<EmbedRequest>,
) -> Result<Json<EmbedResponse>, (StatusCode, String)> {
    let start_time = std::time::Instant::now();

    // Log request received
    info!(
        "Embedding request received: {} texts, model={}, chain_id={}",
        request.texts.len(),
        request.model,
        request.chain_id
    );

    // Step 1: Validate request
    if let Err(e) = request.validate() {
        error!("Request validation failed: {}", e);
        return Err((StatusCode::BAD_REQUEST, format!("Validation error: {}", e)));
    }

    // Step 2: Get chain context from registry
    let chain = state
        .chain_registry
        .get_chain(request.chain_id)
        .ok_or_else(|| {
            error!("Invalid chain_id: {}", request.chain_id);
            (
                StatusCode::BAD_REQUEST,
                format!(
                    "Invalid chain_id: {}. Supported chains: 84532 (Base Sepolia), 5611 (opBNB Testnet)",
                    request.chain_id
                ),
            )
        })?;

    let chain_name = chain.name.clone();
    let native_token = chain.native_token.symbol.clone();

    debug!(
        "Chain context: {} (chain_id={}), native_token={}",
        chain_name, request.chain_id, native_token
    );

    // Step 3: Get embedding model manager from AppState
    let manager_guard = state.embedding_model_manager.read().await;
    let manager = manager_guard.as_ref().ok_or_else(|| {
        error!("Embedding model manager not initialized");
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "Embedding service not available. Model manager not initialized.".to_string(),
        )
    })?;

    // Step 4: Select model (default or specified)
    let model_name = &request.model;
    let model = manager.get_model(Some(model_name)).await.map_err(|e| {
        error!("Model not found: {} - {}", model_name, e);

        // Get available models for error message
        let available = manager.list_models();
        let available_names: Vec<String> = available.iter().map(|m| m.name.clone()).collect();

        (
            StatusCode::NOT_FOUND,
            format!(
                "Model '{}' not found. Available models: {}",
                model_name,
                available_names.join(", ")
            ),
        )
    })?;

    debug!("Using model: {} ({} dimensions)", model_name, model.dimension());

    // Step 5: Validate model dimensions (defensive check)
    if model.dimension() != 384 {
        error!(
            "Model dimension mismatch: expected 384, got {}",
            model.dimension()
        );
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Model dimension mismatch: expected 384, got {}",
                model.dimension()
            ),
        ));
    }

    // Step 6: Generate embeddings via ONNX
    let embeddings_vec = model.embed_batch(&request.texts).await.map_err(|e| {
        error!("Embedding generation failed: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Embedding generation failed: {}", e),
        )
    })?;

    debug!(
        "Generated {} embeddings, each with {} dimensions",
        embeddings_vec.len(),
        embeddings_vec.first().map(|v| v.len()).unwrap_or(0)
    );

    // Step 7: Count tokens for each text
    let mut embedding_results = Vec::with_capacity(request.texts.len());

    for (text, embedding) in request.texts.iter().zip(embeddings_vec.iter()) {
        let token_count = model.count_tokens(text).await.map_err(|e| {
            error!("Token counting failed for text: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Token counting failed: {}", e),
            )
        })?;

        embedding_results.push(EmbeddingResult {
            embedding: embedding.clone(),
            text: text.clone(),
            token_count,
        });
    }

    // Step 8: Build response
    let total_tokens: usize = embedding_results.iter().map(|e| e.token_count).sum();

    let response = EmbedResponse {
        embeddings: embedding_results,
        model: model_name.clone(),
        provider: "host".to_string(),
        total_tokens,
        cost: 0.0, // Host embeddings are zero-cost
        chain_id: request.chain_id,
        chain_name,
        native_token,
    };

    // Log success
    let elapsed = start_time.elapsed();
    info!(
        "Embedding request completed: {} embeddings, {} total tokens, {:?} elapsed",
        response.embeddings.len(),
        response.total_tokens,
        elapsed
    );

    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embeddings::{EmbeddingModelConfig, EmbeddingModelManager};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    const MODEL_PATH: &str = "/workspace/models/all-MiniLM-L6-v2-onnx/model.onnx";
    const TOKENIZER_PATH: &str = "/workspace/models/all-MiniLM-L6-v2-onnx/tokenizer.json";

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

    #[tokio::test]
    async fn test_handler_with_real_model() {
        let state = setup_test_state_with_model().await;

        let request = EmbedRequest {
            texts: vec!["Hello world".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = embed_handler(State(state), Json(request)).await;
        assert!(result.is_ok(), "Handler should succeed: {:?}", result.err());

        let response = result.unwrap().0;  // Extract from Json wrapper
        assert_eq!(response.embeddings.len(), 1);
        assert_eq!(response.embeddings[0].embedding.len(), 384);
        assert_eq!(response.model, "all-MiniLM-L6-v2");
        assert_eq!(response.provider, "host");
        assert_eq!(response.cost, 0.0);
        assert_eq!(response.chain_id, 84532);
        assert_eq!(response.chain_name, "Base Sepolia");
        assert_eq!(response.native_token, "ETH");
    }

    #[tokio::test]
    async fn test_handler_without_model_manager() {
        let state = AppState::new_for_test(); // No model manager

        let request = EmbedRequest {
            texts: vec!["Test".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = embed_handler(State(state), Json(request)).await;
        assert!(result.is_err(), "Should fail without model manager");

        let (status, _msg) = result.unwrap_err();
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }
}
