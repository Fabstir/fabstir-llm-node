// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! POST /v1/embed HTTP handler (Sub-phase 1.2 - Stub)
//!
//! This module implements the HTTP handler for the embedding endpoint.
//! Full implementation will be completed in Phase 4.
//!
//! TODO (Phase 4.1): Implement request validation
//! TODO (Phase 4.2): Integrate with EmbeddingModelManager
//! TODO (Phase 4.3): Implement response formatting
//! TODO (Phase 4.4): Add error handling

use crate::api::embed::{EmbedRequest, EmbedResponse, EmbeddingResult};
use axum::{http::StatusCode, response::IntoResponse, Json};

/// POST /v1/embed handler
///
/// Generates embeddings for input texts using ONNX Runtime.
///
/// # Request Body
/// ```json
/// {
///   "texts": ["text1", "text2", ...],
///   "model": "all-MiniLM-L6-v2",  // optional
///   "chainId": 84532               // optional
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
/// # TODO (Phase 4)
/// - Validate request (1-96 texts, valid model, valid chain_id)
/// - Load model via EmbeddingModelManager
/// - Generate embeddings via OnnxEmbeddingModel
/// - Calculate token counts
/// - Map chain_id to chain_name and native_token
/// - Return formatted response
pub async fn embed_handler(
    Json(request): Json<EmbedRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Stub implementation - will be completed in Phase 4
    // For now, return a stub response with zero embeddings

    let embeddings: Vec<EmbeddingResult> = request
        .texts
        .iter()
        .map(|text| EmbeddingResult {
            embedding: vec![0.0; 384], // Stub: zeros
            text: text.clone(),
            token_count: text.split_whitespace().count(), // Rough token estimate
        })
        .collect();

    let total_tokens: usize = embeddings.iter().map(|e| e.token_count).sum();

    // Map chain_id to chain info (stub - will use real mapping in Phase 4)
    let (chain_name, native_token) = match request.chain_id {
        84532 => ("Base Sepolia", "ETH"),
        5611 => ("opBNB Testnet", "BNB"),
        _ => ("Unknown", "UNKNOWN"),
    };

    let response = EmbedResponse {
        embeddings,
        model: request.model,
        provider: "host".to_string(),
        total_tokens,
        cost: 0.0,
        chain_id: request.chain_id,
        chain_name: chain_name.to_string(),
        native_token: native_token.to_string(),
    };

    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_handler_stub() {
        let request = EmbedRequest {
            texts: vec!["test1".to_string(), "test2".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = embed_handler(Json(request)).await;
        assert!(result.is_ok());

        let Json(response) = result.unwrap().into_response().0;
        assert_eq!(response.embeddings.len(), 2);
        assert_eq!(response.embeddings[0].embedding.len(), 384);
        assert_eq!(response.model, "all-MiniLM-L6-v2");
        assert_eq!(response.provider, "host");
        assert_eq!(response.cost, 0.0);
        assert_eq!(response.chain_id, 84532);
        assert_eq!(response.chain_name, "Base Sepolia");
        assert_eq!(response.native_token, "ETH");
    }

    #[tokio::test]
    async fn test_handler_opbnb() {
        let request = EmbedRequest {
            texts: vec!["test".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 5611,
        };

        let result = embed_handler(Json(request)).await;
        assert!(result.is_ok());

        let Json(response) = result.unwrap().into_response().0;
        assert_eq!(response.chain_name, "opBNB Testnet");
        assert_eq!(response.native_token, "BNB");
    }
}
