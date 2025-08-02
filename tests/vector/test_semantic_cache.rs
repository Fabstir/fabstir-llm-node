use fabstir_llm_node::vector::{
    SemanticCache, SemanticCacheConfig, CacheEntry, CacheHit,
    SimilarityThreshold, CacheStats, CacheError, CacheEvictionPolicy,
    VectorDBClient, VectorDBConfig, VectorBackend, EmbeddingGenerator, EmbeddingConfig, EmbeddingModel
};
use std::time::Duration;
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;
    
    async fn create_test_cache() -> Result<SemanticCache, CacheError> {
        // Create embedding generator
        let embedding_config = EmbeddingConfig {
            model: EmbeddingModel::MiniLM,
            dimension: 384,
            max_tokens: 512,
            normalize: true,
            cache_embeddings: true,
            ..Default::default()
        };
        let embedding_generator = EmbeddingGenerator::new(embedding_config).await?;
        
        // Create vector DB client
        let vector_config = match std::env::var("TEST_VECTOR_BACKEND").as_deref() {
            Ok("real") => VectorDBConfig {
                backend: VectorBackend::Real { 
                    api_url: std::env::var("VECTOR_DB_URL")
                        .unwrap_or_else(|_| "http://localhost:7530".to_string())
                },
                api_key: std::env::var("VECTOR_DB_API_KEY").ok(),
                timeout_ms: 5000,
                max_retries: 3,
            },
            _ => VectorDBConfig {
                backend: VectorBackend::Mock,
                api_key: None,
                timeout_ms: 5000,
                max_retries: 3,
            }
        };
        let vector_client = VectorDBClient::new(vector_config).await?;
        
        // Create cache config
        let cache_config = SemanticCacheConfig {
            similarity_threshold: 0.85,
            ttl_seconds: 3600,
            max_cache_size: 10_000,
            eviction_policy: CacheEvictionPolicy::LRU,
            namespace: "test_cache".to_string(),
            enable_compression: true,
        };
        
        Ok(SemanticCache::new(
            cache_config,
            embedding_generator,
            vector_client
        ).await?)
    }

    #[tokio::test]
    async fn test_cache_miss_and_store() {
        let cache = create_test_cache().await.unwrap();
        
        let prompt = "What is the capital of France?";
        let response = "The capital of France is Paris.";
        
        // First query - should be a miss
        let result = cache.lookup(prompt).await.unwrap();
        assert!(result.is_none());
        
        // Store the response
        let entry_id = cache.store(prompt, response, None).await.unwrap();
        assert!(!entry_id.is_empty());
        
        // Get cache stats
        let stats = cache.get_stats().await;
        assert_eq!(stats.total_lookups, 1);
        assert_eq!(stats.cache_misses, 1);
        assert_eq!(stats.cache_hits, 0);
        assert_eq!(stats.entries_stored, 1);
    }

    #[tokio::test]
    async fn test_cache_hit_exact_match() {
        let cache = create_test_cache().await.unwrap();
        
        let prompt = "What is machine learning?";
        let response = "Machine learning is a branch of AI that enables systems to learn from data.";
        
        // Store entry
        cache.store(prompt, response, None).await.unwrap();
        
        // Lookup exact same prompt
        let result = cache.lookup(prompt).await.unwrap();
        assert!(result.is_some());
        
        let hit = result.unwrap();
        assert_eq!(hit.response, response);
        assert!(hit.similarity >= 0.99); // Exact match should have very high similarity
        assert_eq!(hit.prompt, prompt);
    }

    #[tokio::test]
    async fn test_cache_hit_similar_query() {
        let cache = create_test_cache().await.unwrap();
        
        let original_prompt = "What is artificial intelligence?";
        let response = "AI is the simulation of human intelligence by machines.";
        
        // Store original
        cache.store(original_prompt, response, None).await.unwrap();
        
        // Test similar queries
        let similar_queries = vec![
            "What's AI?",
            "Tell me about artificial intelligence",
            "Explain what AI is",
            "Define artificial intelligence",
        ];
        
        for query in similar_queries {
            let result = cache.lookup(query).await.unwrap();
            assert!(result.is_some(), "Failed to find cache hit for: {}", query);
            
            let hit = result.unwrap();
            assert_eq!(hit.response, response);
            assert!(hit.similarity >= 0.85); // Above threshold
            assert_eq!(hit.original_prompt, original_prompt);
        }
    }

    #[tokio::test]
    async fn test_similarity_threshold() {
        let cache = create_test_cache().await.unwrap();
        
        // Store an entry
        let prompt = "How do neural networks work?";
        let response = "Neural networks are computing systems inspired by biological neural networks.";
        cache.store(prompt, response, None).await.unwrap();
        
        // Query with decreasing similarity
        let test_cases = vec![
            ("How do neural networks function?", true),  // Very similar
            ("What are neural networks?", true),         // Similar enough
            ("How does deep learning work?", true),      // Related concept
            ("What is the weather today?", false),       // Completely different
        ];
        
        for (query, should_hit) in test_cases {
            let result = cache.lookup(query).await.unwrap();
            assert_eq!(
                result.is_some(),
                should_hit,
                "Query '{}' hit expectation mismatch",
                query
            );
        }
    }

    #[tokio::test]
    async fn test_cache_with_metadata() {
        let cache = create_test_cache().await.unwrap();
        
        let prompt = "Explain quantum computing";
        let response = "Quantum computing uses quantum mechanical phenomena.";
        let metadata = HashMap::from([
            ("model".to_string(), "gpt-4".to_string()),
            ("temperature".to_string(), "0.7".to_string()),
            ("timestamp".to_string(), chrono::Utc::now().to_string()),
        ]);
        
        // Store with metadata
        cache.store(prompt, response, Some(metadata.clone())).await.unwrap();
        
        // Retrieve and verify metadata
        let result = cache.lookup(prompt).await.unwrap().unwrap();
        assert_eq!(result.metadata.get("model"), metadata.get("model"));
        assert_eq!(result.metadata.get("temperature"), metadata.get("temperature"));
    }

    #[tokio::test]
    async fn test_cache_ttl_expiration() {
        // Create cache with short TTL
        let mut cache_config = SemanticCacheConfig::default();
        cache_config.ttl_seconds = 1; // 1 second TTL
        
        let embedding_generator = EmbeddingGenerator::new(EmbeddingConfig::default()).await.unwrap();
        let vector_client = VectorDBClient::new(VectorDBConfig::default()).await.unwrap();
        
        let cache = SemanticCache::new(
            cache_config,
            embedding_generator,
            vector_client
        ).await.unwrap();
        
        let prompt = "Test TTL";
        let response = "Response";
        
        // Store entry
        cache.store(prompt, response, None).await.unwrap();
        
        // Should hit immediately
        assert!(cache.lookup(prompt).await.unwrap().is_some());
        
        // Wait for expiration
        tokio::time::sleep(Duration::from_secs(2)).await;
        
        // Should miss after expiration
        assert!(cache.lookup(prompt).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_batch_cache_operations() {
        let cache = create_test_cache().await.unwrap();
        
        // Prepare batch data
        let entries = vec![
            ("What is Rust?", "Rust is a systems programming language."),
            ("What is Python?", "Python is a high-level programming language."),
            ("What is JavaScript?", "JavaScript is a scripting language for web."),
            ("What is Go?", "Go is a statically typed compiled language."),
            ("What is Java?", "Java is an object-oriented programming language."),
        ];
        
        // Batch store
        let mut prompts = Vec::new();
        let mut responses = Vec::new();
        for (prompt, response) in &entries {
            prompts.push(prompt.to_string());
            responses.push(response.to_string());
        }
        
        let results = cache.batch_store(prompts.clone(), responses).await.unwrap();
        assert_eq!(results.len(), 5);
        assert!(results.iter().all(|r| r.is_ok()));
        
        // Batch lookup
        let lookups = cache.batch_lookup(&prompts).await.unwrap();
        assert_eq!(lookups.len(), 5);
        assert!(lookups.iter().all(|r| r.is_some()));
    }

    #[tokio::test]
    async fn test_cache_eviction_lru() {
        // Create cache with small size
        let mut cache_config = SemanticCacheConfig::default();
        cache_config.max_cache_size = 3;
        cache_config.eviction_policy = CacheEvictionPolicy::LRU;
        
        let embedding_generator = EmbeddingGenerator::new(EmbeddingConfig::default()).await.unwrap();
        let vector_client = VectorDBClient::new(VectorDBConfig::default()).await.unwrap();
        
        let cache = SemanticCache::new(
            cache_config,
            embedding_generator,
            vector_client
        ).await.unwrap();
        
        // Store 4 entries (one more than capacity)
        let entries = vec![
            ("Query 1", "Response 1"),
            ("Query 2", "Response 2"),
            ("Query 3", "Response 3"),
            ("Query 4", "Response 4"),
        ];
        
        for (prompt, response) in &entries[..3] {
            cache.store(prompt, response, None).await.unwrap();
        }
        
        // Access Query 1 to make it recently used
        cache.lookup("Query 1").await.unwrap();
        
        // Add Query 4, should evict Query 2 (least recently used)
        cache.store(entries[3].0, entries[3].1, None).await.unwrap();
        
        // Check what's in cache
        assert!(cache.lookup("Query 1").await.unwrap().is_some()); // Recently accessed
        assert!(cache.lookup("Query 2").await.unwrap().is_none());  // Evicted
        assert!(cache.lookup("Query 3").await.unwrap().is_some());
        assert!(cache.lookup("Query 4").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_namespace_isolation() {
        // Create two caches with different namespaces
        let mut config1 = SemanticCacheConfig::default();
        config1.namespace = "namespace1".to_string();
        
        let mut config2 = SemanticCacheConfig::default();
        config2.namespace = "namespace2".to_string();
        
        let embedding_config = EmbeddingConfig::default();
        let embedding_generator1 = EmbeddingGenerator::new(embedding_config.clone()).await.unwrap();
        let embedding_generator2 = EmbeddingGenerator::new(embedding_config).await.unwrap();
        
        let vector_config = VectorDBConfig {
            backend: VectorBackend::Mock,
            api_key: None,
            timeout_ms: 5000,
            max_retries: 3,
        };
        let vector_client1 = VectorDBClient::new(vector_config.clone()).await.unwrap();
        let vector_client2 = VectorDBClient::new(vector_config).await.unwrap();
        
        let cache1 = SemanticCache::new(
            config1,
            embedding_generator1,
            vector_client1
        ).await.unwrap();
        
        let cache2 = SemanticCache::new(
            config2,
            embedding_generator2,
            vector_client2
        ).await.unwrap();
        
        // Store same prompt in both namespaces with different responses
        let prompt = "What is AI?";
        cache1.store(prompt, "Response from namespace 1", None).await.unwrap();
        cache2.store(prompt, "Response from namespace 2", None).await.unwrap();
        
        // Each should return its own response
        let result1 = cache1.lookup(prompt).await.unwrap().unwrap();
        let result2 = cache2.lookup(prompt).await.unwrap().unwrap();
        
        assert_eq!(result1.response, "Response from namespace 1");
        assert_eq!(result2.response, "Response from namespace 2");
    }

    #[tokio::test]
    async fn test_cache_performance_metrics() {
        let cache = create_test_cache().await.unwrap();
        
        // Perform various operations
        let test_prompts = vec![
            "What is machine learning?",
            "Explain deep learning",
            "What are neural networks?",
            "How does AI work?",
            "What is computer vision?",
        ];
        
        // Store entries
        for (i, prompt) in test_prompts.iter().enumerate() {
            cache.store(prompt, &format!("Response {}", i), None).await.unwrap();
        }
        
        // Perform lookups (some hits, some misses)
        for prompt in &test_prompts {
            cache.lookup(prompt).await.unwrap();
        }
        
        // Some misses
        cache.lookup("Random query 1").await.unwrap();
        cache.lookup("Random query 2").await.unwrap();
        
        // Get performance metrics
        let metrics = cache.get_performance_metrics().await;
        
        assert_eq!(metrics.total_stores, 5);
        assert_eq!(metrics.total_lookups, 7);
        assert_eq!(metrics.cache_hits, 5);
        assert_eq!(metrics.cache_misses, 2);
        assert!((metrics.hit_rate - 0.714).abs() < 0.01); // ~71.4% hit rate
        assert!(metrics.avg_lookup_time_ms > 0.0);
        assert!(metrics.avg_store_time_ms > 0.0);
    }

    #[tokio::test]
    async fn test_compression() {
        let cache = create_test_cache().await.unwrap();
        
        // Store large response
        let prompt = "Generate a long story";
        let long_response = "Once upon a time... ".repeat(1000); // ~20KB
        
        let entry_id = cache.store(prompt, &long_response, None).await.unwrap();
        
        // Get storage info
        let info = cache.get_storage_info(&entry_id).await.unwrap();
        
        assert!(info.compressed_size < info.original_size);
        assert!(info.compression_ratio > 1.0);
        
        // Verify decompression works
        let result = cache.lookup(prompt).await.unwrap().unwrap();
        assert_eq!(result.response, long_response);
    }
}