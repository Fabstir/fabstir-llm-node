// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Module structure tests for embedding API (Sub-phase 1.2)
//!
//! These TDD tests verify that the module structure is correctly set up
//! and all types are properly exported. Written FIRST before implementation.
//!
//! Test-Driven Development (TDD) Approach:
//! 1. Write these tests FIRST (they will fail initially)
//! 2. Create module files to make tests pass
//! 3. Run tests to verify module structure is correct

#[cfg(test)]
mod module_structure_tests {
    /// Test 1: Verify embed module exists and is accessible
    ///
    /// This test ensures the `api::embed` module can be imported,
    /// confirming the module structure is set up correctly.
    #[test]
    fn test_embed_module_exists() {
        // Verify module compiles and is accessible
        use fabstir_llm_node::api::embed;

        // If we can reference the module, it exists
        let _ = std::any::type_name::<embed::EmbedRequest>();
    }

    /// Test 2: Verify request and response types are exported from api module
    ///
    /// This test ensures that EmbedRequest, EmbedResponse, and EmbeddingResult
    /// are re-exported from the top-level `api` module for convenient access.
    #[test]
    fn test_request_response_types_exported() {
        use fabstir_llm_node::api::{EmbedRequest, EmbedResponse, EmbeddingResult};

        // Verify types are re-exported from api module
        let _ = std::any::type_name::<EmbedRequest>();
        let _ = std::any::type_name::<EmbedResponse>();
        let _ = std::any::type_name::<EmbeddingResult>();
    }

    /// Test 3: Verify handler function is accessible
    ///
    /// This test ensures the `embed_handler` function is exported
    /// and accessible for route registration in the HTTP server.
    #[test]
    fn test_handler_accessible() {
        use fabstir_llm_node::api::embed_handler;

        // Verify handler function is exported
        let _ = std::any::type_name_of_val(&embed_handler);
    }

    /// Test 4: Verify request deserialization works
    ///
    /// This test ensures EmbedRequest can be deserialized from JSON,
    /// which is required for Axum's `Json<EmbedRequest>` extractor.
    #[test]
    fn test_request_deserialization() {
        use fabstir_llm_node::api::EmbedRequest;

        let json = r#"{
            "texts": ["test1", "test2"],
            "model": "all-MiniLM-L6-v2",
            "chain_id": 84532
        }"#;

        let req: EmbedRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.texts.len(), 2);
        assert_eq!(req.texts[0], "test1");
        assert_eq!(req.texts[1], "test2");
        assert_eq!(req.model, "all-MiniLM-L6-v2");
        assert_eq!(req.chain_id, 84532);
    }

    /// Test 5: Verify request uses correct defaults
    ///
    /// This test ensures that when model and chain_id are omitted,
    /// EmbedRequest uses the correct defaults:
    /// - model: "all-MiniLM-L6-v2"
    /// - chain_id: 84532 (Base Sepolia)
    #[test]
    fn test_request_defaults() {
        use fabstir_llm_node::api::EmbedRequest;

        let json = r#"{"texts": ["test"]}"#;
        let req: EmbedRequest = serde_json::from_str(json).unwrap();

        // Should use defaults
        assert_eq!(req.model, "all-MiniLM-L6-v2", "Model should default to all-MiniLM-L6-v2");
        assert_eq!(req.chain_id, 84532, "Chain ID should default to 84532 (Base Sepolia)");
        assert_eq!(req.texts.len(), 1);
        assert_eq!(req.texts[0], "test");
    }

    /// Test 6: Verify response serialization uses camelCase
    ///
    /// This test ensures that EmbedResponse serializes field names to camelCase
    /// (tokenCount, totalTokens) to match the API specification and SDK expectations.
    #[test]
    fn test_response_serialization() {
        use fabstir_llm_node::api::{EmbedResponse, EmbeddingResult};

        let response = EmbedResponse {
            embeddings: vec![EmbeddingResult {
                embedding: vec![0.1, 0.2, 0.3],
                text: "test".to_string(),
                token_count: 1,
            }],
            model: "all-MiniLM-L6-v2".to_string(),
            provider: "host".to_string(),
            total_tokens: 1,
            cost: 0.0,
            chain_id: 84532,
            chain_name: "Base Sepolia".to_string(),
            native_token: "ETH".to_string(),
        };

        let json = serde_json::to_string(&response).unwrap();

        // Check camelCase fields are present
        assert!(
            json.contains("tokenCount"),
            "Response should serialize token_count as tokenCount (camelCase)"
        );
        assert!(
            json.contains("totalTokens"),
            "Response should serialize total_tokens as totalTokens (camelCase)"
        );

        // Verify other fields
        assert!(json.contains(r#""model":"all-MiniLM-L6-v2""#));
        assert!(json.contains(r#""provider":"host""#));
        assert!(json.contains(r#""cost":0.0"#));
    }

    /// Test 7: Verify embedding module types are accessible
    ///
    /// This test ensures that OnnxEmbeddingModel and EmbeddingModelManager
    /// are exported from the `embeddings` module and accessible.
    #[test]
    fn test_embedding_module_types_accessible() {
        use fabstir_llm_node::embeddings::{EmbeddingModelManager, OnnxEmbeddingModel};

        // Verify embedding module types are accessible
        let _ = std::any::type_name::<OnnxEmbeddingModel>();
        let _ = std::any::type_name::<EmbeddingModelManager>();
    }

    /// Test 8: Verify EmbeddingResult structure
    ///
    /// This test ensures that EmbeddingResult has the correct fields
    /// and can be constructed properly.
    #[test]
    fn test_embedding_result_structure() {
        use fabstir_llm_node::api::EmbeddingResult;

        let result = EmbeddingResult {
            embedding: vec![0.1, 0.2, 0.3, 0.4],
            text: "test text".to_string(),
            token_count: 2,
        };

        // Verify fields are accessible
        assert_eq!(result.embedding.len(), 4);
        assert_eq!(result.embedding[0], 0.1);
        assert_eq!(result.text, "test text");
        assert_eq!(result.token_count, 2);

        // Verify serialization
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("tokenCount")); // camelCase
        assert!(json.contains(r#""text":"test text""#));
    }
}
