use fabstir_llm_node::vector::{
    EmbeddingGenerator, EmbeddingConfig, EmbeddingModel,
    Embedding, EmbeddingError, TokenizerConfig,
    BatchEmbeddingRequest, EmbeddingCache
};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;
    
    fn create_test_config() -> EmbeddingConfig {
        EmbeddingConfig {
            model: EmbeddingModel::MiniLM,
            dimension: 384,
            max_tokens: 512,
            normalize: true,
            tokenizer_config: TokenizerConfig {
                vocab_size: 30522,
                lowercase: true,
                remove_punctuation: false,
            },
            cache_embeddings: true,
            cache_ttl_seconds: 3600,
            quantize: None,
            quantization_bits: None,
        }
    }

    #[tokio::test]
    async fn test_generate_embedding() {
        let config = create_test_config();
        let generator = EmbeddingGenerator::new(config).await.unwrap();
        
        let text = "What is machine learning?";
        let embedding = generator.generate_embedding(text).await.unwrap();
        
        assert_eq!(embedding.dimension(), 384);
        assert!(embedding.magnitude() > 0.0);
        
        // Check normalization
        let magnitude = embedding.magnitude();
        assert!((magnitude - 1.0).abs() < 0.01, "Expected normalized vector");
    }

    #[tokio::test]
    async fn test_embedding_deterministic() {
        let config = create_test_config();
        let generator = EmbeddingGenerator::new(config).await.unwrap();
        
        let text = "Deterministic embedding test";
        
        // Generate multiple times
        let embedding1 = generator.generate_embedding(text).await.unwrap();
        let embedding2 = generator.generate_embedding(text).await.unwrap();
        let embedding3 = generator.generate_embedding(text).await.unwrap();
        
        // Should be identical
        assert_eq!(embedding1.data(), embedding2.data());
        assert_eq!(embedding2.data(), embedding3.data());
    }

    #[tokio::test]
    async fn test_batch_embeddings() {
        let config = create_test_config();
        let generator = EmbeddingGenerator::new(config).await.unwrap();
        
        let texts = vec![
            "First text about AI",
            "Second text about machine learning",
            "Third text about neural networks",
            "Fourth text about deep learning",
            "Fifth text about computer vision",
        ];
        
        let embeddings = generator.generate_batch(texts.iter().map(|s| s.to_string()).collect()).await.unwrap();
        
        assert_eq!(embeddings.len(), 5);
        
        // All should have same dimension
        for embedding in &embeddings {
            assert_eq!(embedding.dimension(), 384);
        }
        
        // Different texts should produce different embeddings
        let similarity_0_1 = embeddings[0].cosine_similarity(&embeddings[1]);
        let similarity_0_4 = embeddings[0].cosine_similarity(&embeddings[4]);
        assert!(similarity_0_1 < 0.99); // Not identical
        assert!(similarity_0_4 < 0.99); // Not identical
    }

    #[tokio::test]
    async fn test_similarity_calculations() {
        let config = create_test_config();
        let generator = EmbeddingGenerator::new(config).await.unwrap();
        
        // Similar texts
        let text1 = "Machine learning is a subset of artificial intelligence";
        let text2 = "ML is a branch of AI";
        let text3 = "The weather is nice today";
        
        let emb1 = generator.generate_embedding(text1).await.unwrap();
        let emb2 = generator.generate_embedding(text2).await.unwrap();
        let emb3 = generator.generate_embedding(text3).await.unwrap();
        
        let similarity_1_2 = emb1.cosine_similarity(&emb2);
        let similarity_1_3 = emb1.cosine_similarity(&emb3);
        
        // Similar texts should have higher similarity
        assert!(similarity_1_2 > 0.7, "Expected high similarity for related texts");
        assert!(similarity_1_3 < 0.5, "Expected low similarity for unrelated texts");
        assert!(similarity_1_2 > similarity_1_3);
    }

    #[tokio::test]
    async fn test_embedding_cache() {
        let config = create_test_config();
        let generator = EmbeddingGenerator::new(config).await.unwrap();
        
        let text = "This text will be cached";
        
        // First generation (cache miss)
        let start = std::time::Instant::now();
        let embedding1 = generator.generate_embedding(text).await.unwrap();
        let first_duration = start.elapsed();
        
        // Second generation (cache hit)
        let start = std::time::Instant::now();
        let embedding2 = generator.generate_embedding(text).await.unwrap();
        let second_duration = start.elapsed();
        
        // Cache hit should be much faster
        assert!(second_duration < first_duration / 10);
        
        // Should return identical embeddings
        assert_eq!(embedding1.data(), embedding2.data());
        
        // Check cache stats
        let stats = generator.get_cache_stats().await;
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hit_rate, 0.5);
    }

    #[tokio::test]
    async fn test_long_text_truncation() {
        let config = create_test_config();
        let generator = EmbeddingGenerator::new(config).await.unwrap();
        
        // Create text longer than max_tokens
        let long_text = "This is a very long text. ".repeat(100);
        
        let result = generator.generate_embedding(&long_text).await;
        
        // Should still succeed (with truncation)
        assert!(result.is_ok());
        let embedding = result.unwrap();
        assert_eq!(embedding.dimension(), 384);
        
        // Get truncation info
        let info = generator.get_last_truncation_info().await;
        assert!(info.was_truncated);
        assert!(info.original_tokens > 512);
        assert_eq!(info.truncated_tokens, 512);
    }

    #[tokio::test]
    async fn test_special_characters_handling() {
        let config = create_test_config();
        let generator = EmbeddingGenerator::new(config).await.unwrap();
        
        let texts = vec![
            "Normal text",
            "Text with émojis 🚀🤖",
            "Text\nwith\nnewlines",
            "Text with <html>tags</html>",
            "文本与中文字符",
            "Τext with Ελληνικά",
        ];
        
        for text in texts {
            let result = generator.generate_embedding(text).await;
            assert!(result.is_ok(), "Failed on text: {}", text);
            
            let embedding = result.unwrap();
            assert_eq!(embedding.dimension(), 384);
            assert!(embedding.magnitude() > 0.0);
        }
    }

    #[tokio::test]
    async fn test_embedding_distance_metrics() {
        let config = create_test_config();
        let generator = EmbeddingGenerator::new(config).await.unwrap();
        
        let text1 = "First document";
        let text2 = "Second document";
        
        let emb1 = generator.generate_embedding(text1).await.unwrap();
        let emb2 = generator.generate_embedding(text2).await.unwrap();
        
        // Test different distance metrics
        let cosine_sim = emb1.cosine_similarity(&emb2);
        let euclidean_dist = emb1.euclidean_distance(&emb2);
        let manhattan_dist = emb1.manhattan_distance(&emb2);
        let _dot_product = emb1.dot_product(&emb2);
        
        // Basic sanity checks
        assert!(cosine_sim >= -1.0 && cosine_sim <= 1.0);
        assert!(euclidean_dist >= 0.0);
        assert!(manhattan_dist >= 0.0);
        
        // For normalized vectors, cosine similarity ≈ 1 - (euclidean_distance²/2)
        let estimated_cosine = 1.0 - (euclidean_dist * euclidean_dist) / 2.0;
        assert!((cosine_sim - estimated_cosine).abs() < 0.1);
    }

    #[tokio::test]
    async fn test_embedding_quantization() {
        let mut config = create_test_config();
        config.quantize = Some(true);
        config.quantization_bits = Some(8);
        
        let generator = EmbeddingGenerator::new(config).await.unwrap();
        
        let text = "Test quantization";
        let embedding = generator.generate_embedding(text).await.unwrap();
        
        // Check that values are quantized
        let data = embedding.data();
        for value in data {
            // In 8-bit quantization, values should be multiples of 1/127
            let quantized = (*value * 127.0).round() / 127.0;
            assert!((value - quantized).abs() < 0.001);
        }
    }

    #[tokio::test]
    async fn test_multilingual_embeddings() {
        let config = create_test_config();
        let generator = EmbeddingGenerator::new(config).await.unwrap();
        
        // Same meaning in different languages
        let texts = vec![
            ("Hello world", "en"),
            ("Hola mundo", "es"),
            ("Bonjour le monde", "fr"),
            ("Hallo Welt", "de"),
            ("こんにちは世界", "ja"),
        ];
        
        let mut embeddings = Vec::new();
        for (text, lang) in texts {
            let emb = generator.generate_embedding_with_lang(text, lang).await.unwrap();
            embeddings.push(emb);
        }
        
        // Embeddings of same meaning should be somewhat similar
        for i in 1..embeddings.len() {
            let similarity = embeddings[0].cosine_similarity(&embeddings[i]);
            assert!(similarity > 0.5, "Expected some similarity across languages");
        }
    }

    #[tokio::test]
    async fn test_embedding_models_comparison() {
        let models = vec![
            EmbeddingModel::MiniLM,
            EmbeddingModel::AllMiniLM,
            EmbeddingModel::E5Small,
        ];
        
        let text = "Compare different embedding models";
        
        for model in models {
            let mut config = create_test_config();
            config.model = model.clone();
            config.dimension = model.default_dimension();
            
            let generator = EmbeddingGenerator::new(config).await.unwrap();
            let embedding = generator.generate_embedding(text).await.unwrap();
            
            assert_eq!(embedding.dimension(), model.default_dimension());
            assert!(embedding.magnitude() > 0.0);
            
            // Get model info
            let info = generator.get_model_info().await;
            assert_eq!(info.model_name, format!("{:?}", model));
            assert_eq!(info.embedding_dimension, model.default_dimension());
        }
    }
}