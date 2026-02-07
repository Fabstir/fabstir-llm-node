// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Response structure tests for EmbedResponse (Sub-phase 2.2)
//!
//! These TDD tests verify that the EmbedResponse and EmbeddingResult structures
//! work correctly with chain context, aggregation, and JSON serialization.
//! Written FIRST before implementation.
//!
//! Test-Driven Development (TDD) Approach:
//! 1. Write these tests FIRST (they will fail initially)
//! 2. Implement helper methods in src/api/embed/response.rs
//! 3. Run tests to verify response structure works correctly

#[cfg(test)]
mod response_structure_tests {
    use fabstir_llm_node::api::{EmbedResponse, EmbeddingResult};

    /// Test 1: Response structure has all required fields
    ///
    /// Verifies that EmbedResponse contains all necessary fields with correct types.
    #[test]
    fn test_response_structure() {
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

        // Verify all fields are accessible
        assert_eq!(response.embeddings.len(), 1);
        assert_eq!(response.model, "all-MiniLM-L6-v2");
        assert_eq!(response.provider, "host");
        assert_eq!(response.total_tokens, 1);
        assert_eq!(response.cost, 0.0);
        assert_eq!(response.chain_id, 84532);
        assert_eq!(response.chain_name, "Base Sepolia");
        assert_eq!(response.native_token, "ETH");
    }

    /// Test 2: EmbeddingResult structure has all required fields
    ///
    /// Verifies that EmbeddingResult contains embedding vector, text, and token count.
    #[test]
    fn test_embedding_result_structure() {
        let result = EmbeddingResult {
            embedding: vec![0.1, 0.2, 0.3, 0.4],
            text: "test text".to_string(),
            token_count: 2,
        };

        assert_eq!(result.embedding.len(), 4);
        assert_eq!(result.embedding[0], 0.1);
        assert_eq!(result.text, "test text");
        assert_eq!(result.token_count, 2);
    }

    /// Test 3: Chain context is correctly populated
    ///
    /// Verifies that add_chain_context() method populates chain_name and native_token
    /// correctly based on chain_id.
    #[test]
    fn test_chain_context_included() {
        // Create response with Base Sepolia chain_id
        let response = EmbedResponse {
            embeddings: vec![EmbeddingResult {
                embedding: vec![0.1; 384],
                text: "test".to_string(),
                token_count: 1,
            }],
            model: "all-MiniLM-L6-v2".to_string(),
            provider: "host".to_string(),
            total_tokens: 1,
            cost: 0.0,
            chain_id: 84532,
            chain_name: String::new(),   // Empty initially
            native_token: String::new(), // Empty initially
        };

        // Add chain context
        let response = response.add_chain_context(84532);

        // Verify chain context was populated
        assert_eq!(response.chain_id, 84532);
        assert_eq!(response.chain_name, "Base Sepolia");
        assert_eq!(response.native_token, "ETH");

        // Test opBNB Testnet
        let response2 = EmbedResponse {
            embeddings: vec![EmbeddingResult {
                embedding: vec![0.1; 384],
                text: "test".to_string(),
                token_count: 1,
            }],
            model: "all-MiniLM-L6-v2".to_string(),
            provider: "host".to_string(),
            total_tokens: 1,
            cost: 0.0,
            chain_id: 5611,
            chain_name: String::new(),
            native_token: String::new(),
        };

        let response2 = response2.add_chain_context(5611);
        assert_eq!(response2.chain_name, "opBNB Testnet");
        assert_eq!(response2.native_token, "BNB");
    }

    /// Test 4: Token count aggregation works correctly
    ///
    /// Verifies that total_tokens equals the sum of all individual embedding token_counts.
    #[test]
    fn test_token_count_aggregation() {
        let embeddings = vec![
            EmbeddingResult {
                embedding: vec![0.1; 384],
                text: "first".to_string(),
                token_count: 5,
            },
            EmbeddingResult {
                embedding: vec![0.2; 384],
                text: "second".to_string(),
                token_count: 10,
            },
            EmbeddingResult {
                embedding: vec![0.3; 384],
                text: "third".to_string(),
                token_count: 7,
            },
        ];

        // Create response from embeddings (builder pattern)
        let response: EmbedResponse = embeddings.into();

        // Verify total_tokens is the sum: 5 + 10 + 7 = 22
        assert_eq!(response.total_tokens, 22);
        assert_eq!(response.embeddings.len(), 3);
    }

    /// Test 5: Cost field is always zero
    ///
    /// Verifies that host-side embeddings always have zero cost.
    #[test]
    fn test_cost_always_zero() {
        let response = EmbedResponse {
            embeddings: vec![EmbeddingResult {
                embedding: vec![0.1; 384],
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

        assert_eq!(
            response.cost, 0.0,
            "Host-side embeddings must have zero cost"
        );
    }

    /// Test 6: Provider field is always "host"
    ///
    /// Verifies that host-side embeddings always identify provider as "host".
    #[test]
    fn test_provider_always_host() {
        let response = EmbedResponse {
            embeddings: vec![EmbeddingResult {
                embedding: vec![0.1; 384],
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

        assert_eq!(
            response.provider, "host",
            "Provider must be 'host' for host-side embeddings"
        );
    }

    /// Test 7: JSON serialization uses camelCase
    ///
    /// Verifies that JSON output uses camelCase for all field names.
    #[test]
    fn test_json_serialization_camelcase() {
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

        // Verify camelCase field names
        assert!(
            json.contains("\"tokenCount\":1"),
            "Should use camelCase 'tokenCount', got: {}",
            json
        );
        assert!(
            json.contains("\"totalTokens\":1"),
            "Should use camelCase 'totalTokens', got: {}",
            json
        );
        assert!(
            json.contains("\"chainId\":84532"),
            "Should use camelCase 'chainId', got: {}",
            json
        );
        assert!(
            json.contains("\"chainName\":\"Base Sepolia\""),
            "Should use camelCase 'chainName', got: {}",
            json
        );
        assert!(
            json.contains("\"nativeToken\":\"ETH\""),
            "Should use camelCase 'nativeToken', got: {}",
            json
        );

        // Verify other fields
        assert!(json.contains("\"model\":\"all-MiniLM-L6-v2\""));
        assert!(json.contains("\"provider\":\"host\""));
        assert!(json.contains("\"cost\":0.0"));
    }

    /// Test 8: Embedding vector length validation (384 dimensions)
    ///
    /// Verifies that embeddings are validated to be exactly 384 dimensions.
    #[test]
    fn test_embedding_vector_length_384() {
        // Valid response with 384-dimensional embeddings
        let valid_response = EmbedResponse {
            embeddings: vec![
                EmbeddingResult {
                    embedding: vec![0.1; 384],
                    text: "test1".to_string(),
                    token_count: 1,
                },
                EmbeddingResult {
                    embedding: vec![0.2; 384],
                    text: "test2".to_string(),
                    token_count: 1,
                },
            ],
            model: "all-MiniLM-L6-v2".to_string(),
            provider: "host".to_string(),
            total_tokens: 2,
            cost: 0.0,
            chain_id: 84532,
            chain_name: "Base Sepolia".to_string(),
            native_token: "ETH".to_string(),
        };

        let result = valid_response.validate_embedding_dimensions();
        assert!(
            result.is_ok(),
            "384-dimensional embeddings should pass validation"
        );

        // Invalid response with wrong dimensions
        let invalid_response = EmbedResponse {
            embeddings: vec![EmbeddingResult {
                embedding: vec![0.1; 512], // Wrong dimension
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

        let result = invalid_response.validate_embedding_dimensions();
        assert!(
            result.is_err(),
            "Non-384-dimensional embeddings should fail validation"
        );

        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("384") || err_msg.contains("dimension"),
            "Error message should mention 384 dimensions, got: {}",
            err_msg
        );
    }

    /// Test 9: Helper methods work correctly
    ///
    /// Verifies that total_dimensions() and embedding_count() helper methods work.
    #[test]
    fn test_helper_methods() {
        let response = EmbedResponse {
            embeddings: vec![
                EmbeddingResult {
                    embedding: vec![0.1; 384],
                    text: "test1".to_string(),
                    token_count: 5,
                },
                EmbeddingResult {
                    embedding: vec![0.2; 384],
                    text: "test2".to_string(),
                    token_count: 10,
                },
                EmbeddingResult {
                    embedding: vec![0.3; 384],
                    text: "test3".to_string(),
                    token_count: 7,
                },
            ],
            model: "all-MiniLM-L6-v2".to_string(),
            provider: "host".to_string(),
            total_tokens: 22,
            cost: 0.0,
            chain_id: 84532,
            chain_name: "Base Sepolia".to_string(),
            native_token: "ETH".to_string(),
        };

        // Test embedding_count()
        assert_eq!(response.embedding_count(), 3);

        // Test total_dimensions()
        assert_eq!(response.total_dimensions(), 384 * 3); // 1152 total floats
    }

    /// Test 10: Builder pattern from Vec<EmbeddingResult>
    ///
    /// Verifies that From<Vec<EmbeddingResult>> creates a valid response.
    #[test]
    fn test_builder_from_embedding_results() {
        let embeddings = vec![
            EmbeddingResult {
                embedding: vec![0.1; 384],
                text: "first".to_string(),
                token_count: 5,
            },
            EmbeddingResult {
                embedding: vec![0.2; 384],
                text: "second".to_string(),
                token_count: 10,
            },
        ];

        let response: EmbedResponse = embeddings.into();

        // Verify defaults
        assert_eq!(response.embeddings.len(), 2);
        assert_eq!(response.provider, "host");
        assert_eq!(response.cost, 0.0);
        assert_eq!(response.total_tokens, 15); // 5 + 10
        assert_eq!(response.chain_id, 84532); // Default to Base Sepolia
    }
}
