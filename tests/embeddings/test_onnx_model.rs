// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! ONNX Model tests for embedding generation (Sub-phase 3.1)
//!
//! These TDD tests verify that the OnnxEmbeddingModel correctly loads
//! the all-MiniLM-L6-v2 ONNX model and generates 384-dimensional embeddings.
//! Written FIRST before implementation.
//!
//! Test-Driven Development (TDD) Approach:
//! 1. Write these tests FIRST (they will fail initially)
//! 2. Implement ONNX model loading and inference in src/embeddings/onnx_model.rs
//! 3. Run tests to verify embeddings work correctly

use fabstir_llm_node::embeddings::OnnxEmbeddingModel;

// Model file paths (downloaded by scripts/download_embedding_model.sh)
const MODEL_PATH: &str = "/workspace/models/all-MiniLM-L6-v2-onnx/model.onnx";
const TOKENIZER_PATH: &str = "/workspace/models/all-MiniLM-L6-v2-onnx/tokenizer.json";

#[cfg(test)]
mod onnx_model_tests {
    use super::*;

    /// Test 1: Model loads successfully from disk
    ///
    /// Verifies that the ONNX model can be loaded from the downloaded files.
    #[tokio::test]
    async fn test_model_loads_successfully() {
        let result = OnnxEmbeddingModel::new("all-MiniLM-L6-v2", MODEL_PATH, TOKENIZER_PATH).await;

        assert!(
            result.is_ok(),
            "Failed to load ONNX model: {:?}",
            result.err()
        );

        let model = result.unwrap();
        assert_eq!(model.model_name(), "all-MiniLM-L6-v2");
        assert_eq!(model.dimension(), 384);
    }

    /// Test 2: Model validates 384 dimensions at load time
    ///
    /// Verifies that the model outputs exactly 384 dimensions during validation.
    #[tokio::test]
    async fn test_model_validates_384_dimensions() {
        let model = OnnxEmbeddingModel::new("all-MiniLM-L6-v2", MODEL_PATH, TOKENIZER_PATH)
            .await
            .expect("Failed to load model");

        // Model should have validated dimensions during construction
        assert_eq!(
            model.dimension(),
            384,
            "Model dimension should be 384 (validated at load time)"
        );
    }

    /// Test 3: embed() returns 384-dimensional vector
    ///
    /// Verifies that embedding a single text returns exactly 384 floats.
    #[tokio::test]
    async fn test_embed_single_returns_384_dims() {
        let model = OnnxEmbeddingModel::new("all-MiniLM-L6-v2", MODEL_PATH, TOKENIZER_PATH)
            .await
            .expect("Failed to load model");

        let text = "Hello world";
        let result = model.embed(text).await;

        assert!(result.is_ok(), "Embedding failed: {:?}", result.err());

        let embedding = result.unwrap();
        assert_eq!(
            embedding.len(),
            384,
            "Embedding should have exactly 384 dimensions"
        );

        // Verify all values are valid f32 (not NaN or Inf)
        for (i, &val) in embedding.iter().enumerate() {
            assert!(
                val.is_finite(),
                "Embedding[{}] is not finite: {}",
                i,
                val
            );
        }
    }

    /// Test 4: embed_batch() returns correct count
    ///
    /// Verifies that embedding a batch of texts returns correct number of embeddings.
    #[tokio::test]
    async fn test_embed_batch_returns_correct_count() {
        let model = OnnxEmbeddingModel::new("all-MiniLM-L6-v2", MODEL_PATH, TOKENIZER_PATH)
            .await
            .expect("Failed to load model");

        let texts = vec![
            "First text".to_string(),
            "Second text".to_string(),
            "Third text".to_string(),
            "Fourth text".to_string(),
            "Fifth text".to_string(),
        ];

        let result = model.embed_batch(&texts).await;
        assert!(result.is_ok(), "Batch embedding failed: {:?}", result.err());

        let embeddings = result.unwrap();
        assert_eq!(
            embeddings.len(),
            5,
            "Should return 5 embeddings for 5 texts"
        );

        // Verify each embedding is 384 dimensions
        for (i, embedding) in embeddings.iter().enumerate() {
            assert_eq!(
                embedding.len(),
                384,
                "Embedding {} should have 384 dimensions",
                i
            );
        }
    }

    /// Test 5: Embeddings are deterministic
    ///
    /// Verifies that embedding the same text twice produces identical results.
    #[tokio::test]
    async fn test_embeddings_are_deterministic() {
        let model = OnnxEmbeddingModel::new("all-MiniLM-L6-v2", MODEL_PATH, TOKENIZER_PATH)
            .await
            .expect("Failed to load model");

        let text = "test input for determinism";

        let embedding1 = model.embed(text).await.expect("First embedding failed");
        let embedding2 = model.embed(text).await.expect("Second embedding failed");

        assert_eq!(embedding1.len(), embedding2.len());

        // Compare embeddings with small tolerance for floating point precision
        let tolerance = 1e-5;
        for (i, (&val1, &val2)) in embedding1.iter().zip(embedding2.iter()).enumerate() {
            let diff = (val1 - val2).abs();
            assert!(
                diff < tolerance,
                "Embedding[{}] differs: {} vs {} (diff: {})",
                i,
                val1,
                val2,
                diff
            );
        }
    }

    /// Test 6: Different texts produce different embeddings
    ///
    /// Verifies that semantically different texts have different embeddings,
    /// and similar texts have more similar embeddings.
    #[tokio::test]
    async fn test_different_texts_different_embeddings() {
        let model = OnnxEmbeddingModel::new("all-MiniLM-L6-v2", MODEL_PATH, TOKENIZER_PATH)
            .await
            .expect("Failed to load model");

        let cat = model.embed("cat").await.expect("Failed to embed 'cat'");
        let dog = model.embed("dog").await.expect("Failed to embed 'dog'");
        let kitty = model.embed("kitty").await.expect("Failed to embed 'kitty'");

        // Helper: Calculate cosine similarity
        let cosine_similarity = |a: &[f32], b: &[f32]| -> f32 {
            let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
            let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
            let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
            dot / (norm_a * norm_b)
        };

        let sim_cat_dog = cosine_similarity(&cat, &dog);
        let sim_cat_kitty = cosine_similarity(&cat, &kitty);

        // Cat and kitty should be more similar than cat and dog
        assert!(
            sim_cat_kitty > sim_cat_dog,
            "Cat-Kitty similarity ({}) should be higher than Cat-Dog similarity ({})",
            sim_cat_kitty,
            sim_cat_dog
        );

        // All similarities should be reasonable (between 0 and 1)
        assert!(
            sim_cat_dog > 0.0 && sim_cat_dog < 1.0,
            "Cat-Dog similarity should be between 0 and 1, got {}",
            sim_cat_dog
        );
        assert!(
            sim_cat_kitty > 0.0 && sim_cat_kitty < 1.0,
            "Cat-Kitty similarity should be between 0 and 1, got {}",
            sim_cat_kitty
        );
    }

    /// Test 7: Token counting works correctly
    ///
    /// Verifies that count_tokens() returns accurate token counts.
    #[tokio::test]
    async fn test_token_counting() {
        let model = OnnxEmbeddingModel::new("all-MiniLM-L6-v2", MODEL_PATH, TOKENIZER_PATH)
            .await
            .expect("Failed to load model");

        // Short text
        let count1 = model
            .count_tokens("hello")
            .await
            .expect("Failed to count tokens");
        assert!(
            count1 >= 2 && count1 <= 4,
            "Expected 2-4 tokens for 'hello', got {}",
            count1
        );

        // Longer text
        let text2 = "the quick brown fox jumps over the lazy dog";
        let count2 = model
            .count_tokens(text2)
            .await
            .expect("Failed to count tokens");
        assert!(
            count2 >= 10 && count2 <= 15,
            "Expected 10-15 tokens for long text, got {}",
            count2
        );

        // Empty text
        let count3 = model
            .count_tokens("")
            .await
            .expect("Failed to count tokens for empty string");
        assert!(
            count3 >= 0 && count3 <= 2,
            "Expected 0-2 tokens for empty string (special tokens), got {}",
            count3
        );
    }

    /// Test 8: Empty text handling
    ///
    /// Verifies that embedding an empty string doesn't panic and returns valid vector.
    #[tokio::test]
    async fn test_empty_text_handling() {
        let model = OnnxEmbeddingModel::new("all-MiniLM-L6-v2", MODEL_PATH, TOKENIZER_PATH)
            .await
            .expect("Failed to load model");

        let result = model.embed("").await;
        assert!(
            result.is_ok(),
            "Empty text embedding should not fail: {:?}",
            result.err()
        );

        let embedding = result.unwrap();
        assert_eq!(
            embedding.len(),
            384,
            "Empty text should still produce 384-dim vector"
        );

        // Verify all values are finite
        for &val in &embedding {
            assert!(val.is_finite(), "Empty text embedding contains non-finite value");
        }
    }

    /// Test 9: Long text truncation
    ///
    /// Verifies that very long texts are truncated gracefully without panicking.
    #[tokio::test]
    async fn test_long_text_truncation() {
        let model = OnnxEmbeddingModel::new("all-MiniLM-L6-v2", MODEL_PATH, TOKENIZER_PATH)
            .await
            .expect("Failed to load model");

        // Create a very long text (5000+ characters)
        let long_text = "word ".repeat(1000); // 5000 characters

        let result = model.embed(&long_text).await;
        assert!(
            result.is_ok(),
            "Long text embedding should not fail: {:?}",
            result.err()
        );

        let embedding = result.unwrap();
        assert_eq!(
            embedding.len(),
            384,
            "Long text should still produce 384-dim vector"
        );

        // Verify all values are finite
        for &val in &embedding {
            assert!(
                val.is_finite(),
                "Long text embedding contains non-finite value"
            );
        }
    }

    /// Test 10: Invalid model path returns clear error
    ///
    /// Verifies that attempting to load from invalid path gives clear error message.
    #[tokio::test]
    async fn test_invalid_model_path_error() {
        let invalid_path = "/nonexistent/path/to/model.onnx";
        let result = OnnxEmbeddingModel::new("test-model", invalid_path, TOKENIZER_PATH).await;

        assert!(
            result.is_err(),
            "Loading from invalid path should return error"
        );

        let error = result.unwrap_err();
        let error_msg = error.to_string();

        // Error message should mention the path or "not found"
        assert!(
            error_msg.contains("not found")
                || error_msg.contains("No such file")
                || error_msg.contains(invalid_path),
            "Error message should be clear about missing file, got: {}",
            error_msg
        );
    }
}
