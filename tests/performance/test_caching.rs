// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use fabstir_llm_node::performance::{
    InferenceCache, CacheConfig, CacheKey, CacheEntry, CacheStatus,
    CacheStats, EvictionPolicy, CacheError, SemanticCache,
    EmbeddingGenerator, SimilarityThreshold, CacheWarming
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio;

async fn create_test_cache() -> Result<InferenceCache> {
    let config = CacheConfig {
        max_entries: 1000,
        max_memory_mb: 512,
        ttl_seconds: 3600,
        eviction_policy: EvictionPolicy::LRU,
        enable_semantic_cache: true,
        similarity_threshold: 0.95,
        warm_cache_on_startup: false,
        persistence_path: Some(PathBuf::from("test_data/inference_cache")),
    };
    
    InferenceCache::new(config).await
}

#[tokio::test]
async fn test_basic_cache_operations() {
    let cache = create_test_cache().await.unwrap();
    
    let key = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "What is the capital of France?".to_string(),
        parameters_hash: "abc123".to_string(),
    };
    
    let response = "The capital of France is Paris.".to_string();
    
    // Store in cache
    cache.put(&key, &response, 150).await.unwrap();
    
    // Retrieve from cache
    let cached = cache.get(&key).await.unwrap();
    assert_eq!(cached.response, response);
    assert_eq!(cached.tokens_saved, 150);
    assert!(cached.latency_saved_ms > 0);
}

#[tokio::test]
async fn test_cache_miss() {
    let cache = create_test_cache().await.unwrap();
    
    let key = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "Unknown prompt".to_string(),
        parameters_hash: "xyz789".to_string(),
    };
    
    let result = cache.get(&key).await;
    assert!(result.is_err());
    
    match result.unwrap_err().downcast::<CacheError>() {
        Ok(CacheError::CacheMiss { key: _ }) => {}
        _ => panic!("Expected CacheMiss error"),
    }
}

#[tokio::test]
async fn test_semantic_caching() {
    let cache = create_test_cache().await.unwrap();
    
    // Store original
    let key1 = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "What's the capital city of France?".to_string(),
        parameters_hash: "abc123".to_string(),
    };
    
    cache.put(&key1, "Paris is the capital of France.", 100).await.unwrap();
    
    // Query with similar prompt
    let key2 = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "What is the capital of France?".to_string(), // Slightly different
        parameters_hash: "abc123".to_string(),
    };
    
    // Should find via semantic similarity
    let result = cache.get_semantic(&key2).await.unwrap();
    assert_eq!(result.response, "Paris is the capital of France.");
    assert!(result.similarity_score >= 0.95);
    assert!(result.is_semantic_match);
}

#[tokio::test]
async fn test_cache_eviction_lru() {
    let mut config = CacheConfig::default();
    config.max_entries = 3;
    config.eviction_policy = EvictionPolicy::LRU;
    
    let cache = InferenceCache::new(config).await.unwrap();
    
    // Fill cache
    for i in 0..4 {
        let key = CacheKey {
            model_id: "llama-7b".to_string(),
            prompt: format!("Prompt {}", i),
            parameters_hash: format!("hash{}", i),
        };
        cache.put(&key, &format!("Response {}", i), 50).await.unwrap();
    }
    
    // First entry should be evicted
    let key0 = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "Prompt 0".to_string(),
        parameters_hash: "hash0".to_string(),
    };
    
    assert!(cache.get(&key0).await.is_err());
    
    // Others should still be there
    for i in 1..4 {
        let key = CacheKey {
            model_id: "llama-7b".to_string(),
            prompt: format!("Prompt {}", i),
            parameters_hash: format!("hash{}", i),
        };
        assert!(cache.get(&key).await.is_ok());
    }
}

#[tokio::test]
async fn test_cache_ttl_expiration() {
    let mut config = CacheConfig::default();
    config.ttl_seconds = 1; // 1 second TTL
    
    let cache = InferenceCache::new(config).await.unwrap();
    
    let key = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "Temporary prompt".to_string(),
        parameters_hash: "temp123".to_string(),
    };
    
    cache.put(&key, "Temporary response", 50).await.unwrap();
    
    // Should be there immediately
    assert!(cache.get(&key).await.is_ok());
    
    // Wait for expiration
    tokio::time::sleep(Duration::from_secs(2)).await;
    
    // Should be expired
    assert!(cache.get(&key).await.is_err());
}

#[tokio::test]
async fn test_cache_statistics() {
    let cache = create_test_cache().await.unwrap();
    
    // Perform various operations
    let key1 = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "Test prompt 1".to_string(),
        parameters_hash: "hash1".to_string(),
    };
    
    let key2 = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "Test prompt 2".to_string(),
        parameters_hash: "hash2".to_string(),
    };
    
    cache.put(&key1, "Response 1", 100).await.unwrap();
    cache.get(&key1).await.unwrap(); // Hit
    cache.get(&key2).await.ok(); // Miss
    cache.put(&key2, "Response 2", 150).await.unwrap();
    cache.get(&key2).await.unwrap(); // Hit
    
    let stats = cache.get_stats().await;
    
    assert_eq!(stats.total_entries, 2);
    assert_eq!(stats.cache_hits, 2);
    assert_eq!(stats.cache_misses, 1);
    assert_eq!(stats.hit_rate, 2.0 / 3.0);
    assert_eq!(stats.total_tokens_saved, 250);
    assert!(stats.memory_usage_mb > 0.0);
}

#[tokio::test]
async fn test_embedding_generation() {
    let generator = EmbeddingGenerator::new("sentence-transformers/all-MiniLM-L6-v2");
    
    let text1 = "The weather is nice today";
    let text2 = "Today the weather is pleasant";
    let text3 = "I like pizza";
    
    let embedding1 = generator.generate_embedding(text1).await.unwrap();
    let embedding2 = generator.generate_embedding(text2).await.unwrap();
    let embedding3 = generator.generate_embedding(text3).await.unwrap();
    
    // Similar texts should have high similarity
    let sim12 = generator.cosine_similarity(&embedding1, &embedding2);
    let sim13 = generator.cosine_similarity(&embedding1, &embedding3);
    
    assert!(sim12 > 0.8); // Similar meaning
    assert!(sim13 < 0.5); // Different topics
}

#[tokio::test]
async fn test_cache_persistence() {
    let cache_path = PathBuf::from("test_data/test_persistence_cache");
    
    // Create and populate cache
    {
        let mut config = CacheConfig::default();
        config.persistence_path = Some(cache_path.clone());
        
        let cache = InferenceCache::new(config).await.unwrap();
        
        let key = CacheKey {
            model_id: "llama-7b".to_string(),
            prompt: "Persistent prompt".to_string(),
            parameters_hash: "persist123".to_string(),
        };
        
        cache.put(&key, "Persistent response", 75).await.unwrap();
        cache.persist().await.unwrap();
    }
    
    // Load cache from disk
    {
        let mut config = CacheConfig::default();
        config.persistence_path = Some(cache_path);
        
        let cache = InferenceCache::new(config).await.unwrap();
        
        let key = CacheKey {
            model_id: "llama-7b".to_string(),
            prompt: "Persistent prompt".to_string(),
            parameters_hash: "persist123".to_string(),
        };
        
        let entry = cache.get(&key).await.unwrap();
        assert_eq!(entry.response, "Persistent response");
    }
}

#[tokio::test]
async fn test_cache_warming() {
    let cache = create_test_cache().await.unwrap();
    
    // Define common prompts to warm
    let warming_prompts = vec![
        ("What is AI?", "AI is artificial intelligence..."),
        ("How does machine learning work?", "Machine learning is..."),
        ("Explain neural networks", "Neural networks are..."),
    ];
    
    // Warm the cache
    let warmer = CacheWarming::new(Arc::new(cache.clone()));
    warmer.warm_from_prompts(
        "llama-7b",
        warming_prompts.iter().map(|(p, r)| (p.to_string(), r.to_string())).collect(),
    ).await.unwrap();
    
    // All warmed prompts should be in cache
    for (prompt, expected_response) in warming_prompts {
        let key = CacheKey {
            model_id: "llama-7b".to_string(),
            prompt: prompt.to_string(),
            parameters_hash: "default".to_string(),
        };
        
        let entry = cache.get(&key).await.unwrap();
        assert_eq!(entry.response, expected_response);
    }
}

#[tokio::test]
async fn test_memory_pressure_eviction() {
    let mut config = CacheConfig::default();
    config.max_memory_mb = 1; // Very small limit
    
    let cache = InferenceCache::new(config).await.unwrap();
    
    // Add entries until memory pressure triggers eviction
    let mut added = 0;
    for i in 0..100 {
        let key = CacheKey {
            model_id: "llama-7b".to_string(),
            prompt: format!("Large prompt with lots of text {}", "x".repeat(1000)),
            parameters_hash: format!("hash{}", i),
        };
        
        let result = cache.put(
            &key,
            &format!("Large response {}", "y".repeat(1000)),
            100,
        ).await;
        
        if result.is_ok() {
            added += 1;
        }
    }
    
    // Should have evicted some entries due to memory limit
    let stats = cache.get_stats().await;
    assert!(stats.total_entries < added);
    assert!(stats.memory_usage_mb <= 1.0);
}

#[tokio::test]
async fn test_model_specific_caching() {
    let cache = create_test_cache().await.unwrap();
    
    let prompt = "Same prompt for different models";
    
    // Cache for different models
    let models = vec!["llama-7b", "mistral-7b", "gpt-j"];
    
    for model in &models {
        let key = CacheKey {
            model_id: model.to_string(),
            prompt: prompt.to_string(),
            parameters_hash: "same".to_string(),
        };
        
        cache.put(
            &key,
            &format!("Response from {}", model),
            100,
        ).await.unwrap();
    }
    
    // Each model should have its own cached response
    for model in &models {
        let key = CacheKey {
            model_id: model.to_string(),
            prompt: prompt.to_string(),
            parameters_hash: "same".to_string(),
        };
        
        let entry = cache.get(&key).await.unwrap();
        assert_eq!(entry.response, format!("Response from {}", model));
    }
}

#[tokio::test]
async fn test_concurrent_cache_access() {
    let cache = Arc::new(create_test_cache().await.unwrap());
    
    let mut handles = vec![];
    
    // Spawn multiple tasks accessing cache
    for i in 0..10 {
        let cache_clone = cache.clone();
        let handle = tokio::spawn(async move {
            let key = CacheKey {
                model_id: "llama-7b".to_string(),
                prompt: format!("Concurrent prompt {}", i % 3), // Some overlap
                parameters_hash: "concurrent".to_string(),
            };
            
            // Try to get first
            if cache_clone.get(&key).await.is_err() {
                // If miss, put
                cache_clone.put(
                    &key,
                    &format!("Concurrent response {}", i),
                    50,
                ).await.ok();
            }
        });
        handles.push(handle);
    }
    
    // Wait for all to complete
    for handle in handles {
        handle.await.unwrap();
    }
    
    // Check cache state
    let stats = cache.get_stats().await;
    assert!(stats.total_entries > 0);
    assert!(stats.total_entries <= 3); // At most 3 unique prompts
}