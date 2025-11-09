// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Request validation tests for EmbedRequest (Sub-phase 2.1)
//!
//! These TDD tests verify that the EmbedRequest validation logic works correctly.
//! Written FIRST before implementation.
//!
//! Test-Driven Development (TDD) Approach:
//! 1. Write these tests FIRST (they will fail initially)
//! 2. Implement validation logic in src/api/embed/request.rs
//! 3. Run tests to verify validation works correctly

#[cfg(test)]
mod request_validation_tests {
    use fabstir_llm_node::api::EmbedRequest;

    /// Test 1: Valid request with single text
    ///
    /// Verifies that a simple, valid request with one text passes validation.
    #[test]
    fn test_valid_request_single_text() {
        let request = EmbedRequest {
            texts: vec!["Hello world".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = request.validate();
        assert!(result.is_ok(), "Valid single text request should pass validation");
    }

    /// Test 2: Valid request with batch of texts
    ///
    /// Verifies that a batch request with multiple texts passes validation.
    #[test]
    fn test_valid_request_batch() {
        let texts: Vec<String> = (0..50)
            .map(|i| format!("Test text number {}", i))
            .collect();

        let request = EmbedRequest {
            texts,
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = request.validate();
        assert!(result.is_ok(), "Valid batch request should pass validation");
    }

    /// Test 3: Default model is applied correctly
    ///
    /// Verifies that when model is not specified, default is "all-MiniLM-L6-v2".
    #[test]
    fn test_default_model_applied() {
        let json = r#"{"texts": ["test"]}"#;
        let request: EmbedRequest = serde_json::from_str(json).unwrap();

        assert_eq!(
            request.model, "all-MiniLM-L6-v2",
            "Default model should be all-MiniLM-L6-v2"
        );

        let result = request.validate();
        assert!(result.is_ok(), "Request with default model should be valid");
    }

    /// Test 4: Default chain_id is applied correctly
    ///
    /// Verifies that when chain_id is not specified, default is 84532 (Base Sepolia).
    #[test]
    fn test_default_chain_id_applied() {
        let json = r#"{"texts": ["test"]}"#;
        let request: EmbedRequest = serde_json::from_str(json).unwrap();

        assert_eq!(
            request.chain_id, 84532,
            "Default chain_id should be 84532 (Base Sepolia)"
        );

        let result = request.validate();
        assert!(result.is_ok(), "Request with default chain_id should be valid");
    }

    /// Test 5: Empty texts array is rejected
    ///
    /// Verifies that a request with zero texts fails validation with clear error.
    #[test]
    fn test_empty_texts_rejected() {
        let request = EmbedRequest {
            texts: vec![],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = request.validate();
        assert!(result.is_err(), "Empty texts array should be rejected");

        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("texts") && err_msg.contains("at least 1"),
            "Error message should mention 'texts' and 'at least 1', got: {}",
            err_msg
        );
    }

    /// Test 6: Too many texts rejected (>96)
    ///
    /// Verifies that requests with more than 96 texts fail validation.
    #[test]
    fn test_too_many_texts_rejected() {
        let texts: Vec<String> = (0..100)
            .map(|i| format!("Text {}", i))
            .collect();

        let request = EmbedRequest {
            texts,
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = request.validate();
        assert!(result.is_err(), "More than 96 texts should be rejected");

        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("texts") && (err_msg.contains("96") || err_msg.contains("maximum")),
            "Error message should mention 'texts' and '96' or 'maximum', got: {}",
            err_msg
        );
    }

    /// Test 7: Text too long rejected (>8192 chars)
    ///
    /// Verifies that individual texts longer than 8192 characters are rejected.
    #[test]
    fn test_text_too_long_rejected() {
        let long_text = "a".repeat(8193); // 8193 characters

        let request = EmbedRequest {
            texts: vec![long_text],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = request.validate();
        assert!(result.is_err(), "Text longer than 8192 chars should be rejected");

        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("8192") || err_msg.contains("too long"),
            "Error message should mention '8192' or 'too long', got: {}",
            err_msg
        );
    }

    /// Test 8: Invalid chain_id rejected
    ///
    /// Verifies that chain_id values other than 84532 or 5611 are rejected.
    #[test]
    fn test_invalid_chain_id_rejected() {
        let request = EmbedRequest {
            texts: vec!["test".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 99999, // Invalid chain ID
        };

        let result = request.validate();
        assert!(result.is_err(), "Invalid chain_id should be rejected");

        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("chain") && (err_msg.contains("84532") || err_msg.contains("5611")),
            "Error message should mention valid chain IDs (84532, 5611), got: {}",
            err_msg
        );
    }

    /// Test 9: Whitespace-only text rejected
    ///
    /// Verifies that texts containing only whitespace are rejected.
    #[test]
    fn test_whitespace_only_text_rejected() {
        let request = EmbedRequest {
            texts: vec!["   \n\t  ".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = request.validate();
        assert!(result.is_err(), "Whitespace-only text should be rejected");

        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("empty") || err_msg.contains("whitespace"),
            "Error message should mention 'empty' or 'whitespace', got: {}",
            err_msg
        );
    }

    /// Test 10: JSON serialization works correctly
    ///
    /// Verifies that EmbedRequest can be serialized to JSON with camelCase fields.
    #[test]
    fn test_json_serialization() {
        let request = EmbedRequest {
            texts: vec!["test".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"texts\""), "JSON should contain 'texts' field");
        assert!(json.contains("\"model\""), "JSON should contain 'model' field");
        assert!(json.contains("\"chainId\""), "JSON should contain 'chainId' field (camelCase)");
    }

    /// Test 11: JSON deserialization works correctly
    ///
    /// Verifies that EmbedRequest can be deserialized from JSON with camelCase fields.
    #[test]
    fn test_json_deserialization() {
        let json = r#"{
            "texts": ["test1", "test2"],
            "model": "all-MiniLM-L6-v2",
            "chainId": 5611
        }"#;

        let request: EmbedRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.texts.len(), 2);
        assert_eq!(request.texts[0], "test1");
        assert_eq!(request.model, "all-MiniLM-L6-v2");
        assert_eq!(request.chain_id, 5611);
    }

    /// Test 12: Validation error messages are clear and actionable
    ///
    /// Verifies that all validation errors provide clear, actionable messages.
    #[test]
    fn test_validation_error_messages_clear() {
        // Test empty texts error message
        let request1 = EmbedRequest {
            texts: vec![],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };
        let err1 = request1.validate().unwrap_err().to_string();
        assert!(
            err1.len() > 20,
            "Error message should be descriptive (>20 chars), got: {}",
            err1
        );

        // Test too many texts error message
        let request2 = EmbedRequest {
            texts: (0..100).map(|i| format!("Text {}", i)).collect(),
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };
        let err2 = request2.validate().unwrap_err().to_string();
        assert!(
            err2.len() > 20,
            "Error message should be descriptive (>20 chars), got: {}",
            err2
        );

        // Test invalid chain_id error message
        let request3 = EmbedRequest {
            texts: vec!["test".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 99999,
        };
        let err3 = request3.validate().unwrap_err().to_string();
        assert!(
            err3.len() > 20,
            "Error message should be descriptive (>20 chars), got: {}",
            err3
        );
    }

    /// Test 13: Maximum valid batch size (96 texts)
    ///
    /// Verifies that exactly 96 texts (the maximum) passes validation.
    #[test]
    fn test_maximum_batch_size_valid() {
        let texts: Vec<String> = (0..96)
            .map(|i| format!("Text number {}", i))
            .collect();

        let request = EmbedRequest {
            texts,
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = request.validate();
        assert!(result.is_ok(), "Maximum batch size (96 texts) should be valid");
    }

    /// Test 14: Maximum text length (8192 chars)
    ///
    /// Verifies that text with exactly 8192 characters passes validation.
    #[test]
    fn test_maximum_text_length_valid() {
        let max_length_text = "a".repeat(8192); // Exactly 8192 characters

        let request = EmbedRequest {
            texts: vec![max_length_text],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 84532,
        };

        let result = request.validate();
        assert!(result.is_ok(), "Text with exactly 8192 chars should be valid");
    }

    /// Test 15: Valid opBNB Testnet chain_id (5611)
    ///
    /// Verifies that opBNB Testnet chain_id is accepted.
    #[test]
    fn test_opbnb_chain_id_valid() {
        let request = EmbedRequest {
            texts: vec!["test".to_string()],
            model: "all-MiniLM-L6-v2".to_string(),
            chain_id: 5611, // opBNB Testnet
        };

        let result = request.validate();
        assert!(result.is_ok(), "opBNB Testnet chain_id (5611) should be valid");
    }
}
