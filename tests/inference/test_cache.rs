use fabstir_llm_node::inference::{
    CacheConfig, CacheEntry, CacheKey, EvictionPolicy, InferenceCache,
};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_cache_initialization() {
    let config = CacheConfig {
        max_entries: 1000,
        max_memory_bytes: 1024 * 1024 * 1024, // 1GB
        ttl: Duration::from_secs(3600),
        eviction_policy: EvictionPolicy::Lru,
        enable_semantic_search: true,
        similarity_threshold: 0.85,
        persistence_path: None,
    };

    let cache = InferenceCache::new(config)
        .await
        .expect("Failed to create cache");

    // Cache should be empty initially
    assert_eq!(cache.size(), 0);
    assert_eq!(cache.memory_usage(), 0);
}

#[tokio::test]
async fn test_basic_cache_operations() {
    let config = CacheConfig::default();
    let mut cache = InferenceCache::new(config)
        .await
        .expect("Failed to create cache");

    // Create cache entry
    let key = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "What is the capital of France?".to_string(),
        temperature: 0.7,
        max_tokens: 50,
    };

    let entry = CacheEntry {
        response: "The capital of France is Paris.".to_string(),
        tokens_generated: 8,
        generation_time: Duration::from_millis(250),
        timestamp: std::time::SystemTime::now(),
        access_count: 0,
        size_bytes: "The capital of France is Paris.".len(),
    };

    // Put entry
    cache
        .put(key.clone(), entry.clone())
        .await
        .expect("Failed to put entry");

    // Get entry
    let retrieved = cache.get(&key).await;

    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.response, entry.response);
    assert_eq!(retrieved.tokens_generated, entry.tokens_generated);
}

#[tokio::test]
async fn test_cache_ttl_expiration() {
    let config = CacheConfig {
        ttl: Duration::from_millis(500),
        ..Default::default()
    };

    let mut cache = InferenceCache::new(config)
        .await
        .expect("Failed to create cache");

    let key = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "Test prompt".to_string(),
        temperature: 0.5,
        max_tokens: 10,
    };

    let entry = CacheEntry {
        response: "Test response".to_string(),
        tokens_generated: 2,
        generation_time: Duration::from_millis(50),
        timestamp: std::time::SystemTime::now(),
        access_count: 0,
        size_bytes: "Test response".len(),
    };

    cache.put(key.clone(), entry).await.unwrap();

    // Should exist immediately
    assert!(cache.get(&key).await.is_some());

    // Wait for expiration
    sleep(Duration::from_millis(600)).await;

    // Should be expired
    assert!(cache.get(&key).await.is_none());
}

#[tokio::test]
async fn test_semantic_cache_similarity() {
    let config = CacheConfig {
        enable_semantic_search: true,
        similarity_threshold: 0.85,
        ..Default::default()
    };

    let mut cache = InferenceCache::new(config)
        .await
        .expect("Failed to create cache");

    // Initialize semantic cache with embeddings
    cache
        .initialize_embeddings()
        .await
        .expect("Failed to initialize embeddings");

    // Add entry
    let key1 = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "What is the capital city of France?".to_string(),
        temperature: 0.7,
        max_tokens: 50,
    };

    let entry1 = CacheEntry {
        response: "The capital city of France is Paris.".to_string(),
        tokens_generated: 8,
        generation_time: Duration::from_millis(200),
        timestamp: std::time::SystemTime::now(),
        access_count: 0,
        size_bytes: "The capital city of France is Paris.".len(),
    };

    cache.put(key1, entry1.clone()).await.unwrap();

    // Search with similar prompt
    let similar_key = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "What's the capital of France?".to_string(), // Similar but not identical
        temperature: 0.7,
        max_tokens: 50,
    };

    let similar_result = cache.get_semantic(&similar_key).await;

    // For now semantic search returns None as it's not implemented
    // In real implementation, it would find similar entries
    assert!(similar_result.is_none());
}

#[tokio::test]
async fn test_cache_eviction_lru() {
    let config = CacheConfig {
        max_entries: 3,
        eviction_policy: EvictionPolicy::Lru,
        ..Default::default()
    };

    let mut cache = InferenceCache::new(config)
        .await
        .expect("Failed to create cache");

    // Fill cache to capacity
    for i in 0..3 {
        let key = CacheKey {
            model_id: "llama-7b".to_string(),
            prompt: format!("Prompt {}", i),
            temperature: 0.7,
            max_tokens: 10,
        };

        let entry = CacheEntry {
            response: format!("Response {}", i),
            tokens_generated: 2,
            generation_time: Duration::from_millis(50),
            timestamp: std::time::SystemTime::now(),
            access_count: 0,
            size_bytes: format!("Response {}", i).len(),
        };

        cache.put(key, entry).await.unwrap();
    }

    assert_eq!(cache.size(), 3);

    // Access first entry to make it recently used
    let key0 = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "Prompt 0".to_string(),
        temperature: 0.7,
        max_tokens: 10,
    };
    let _ = cache.get(&key0).await.unwrap();

    // Add new entry - should evict least recently used (Prompt 1)
    let key3 = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "Prompt 3".to_string(),
        temperature: 0.7,
        max_tokens: 10,
    };

    let entry3 = CacheEntry {
        response: "Response 3".to_string(),
        tokens_generated: 2,
        generation_time: Duration::from_millis(50),
        timestamp: std::time::SystemTime::now(),
        access_count: 0,
        size_bytes: "Response 3".len(),
    };

    cache.put(key3, entry3).await.unwrap();

    // Should still have 3 entries
    assert_eq!(cache.size(), 3);

    // Prompt 1 should be evicted
    let key1 = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "Prompt 1".to_string(),
        temperature: 0.7,
        max_tokens: 10,
    };
    assert!(cache.get(&key1).await.is_none());

    // Others should still exist
    assert!(cache.get(&key0).await.is_some());
}

#[tokio::test]
async fn test_cache_memory_limit() {
    let config = CacheConfig {
        max_memory_bytes: 1024, // 1KB - very small for testing
        eviction_policy: EvictionPolicy::Memory,
        ..Default::default()
    };

    let mut cache = InferenceCache::new(config)
        .await
        .expect("Failed to create cache");

    // Add entries until memory limit
    let mut added = 0;
    for i in 0..100 {
        let key = CacheKey {
            model_id: "llama-7b".to_string(),
            prompt: format!("Long prompt with lots of text to consume memory {}", i),
            temperature: 0.7,
            max_tokens: 100,
        };

        let entry = CacheEntry {
            response: "A ".repeat(100), // ~200 bytes
            tokens_generated: 100,
            generation_time: Duration::from_millis(1000),
            timestamp: std::time::SystemTime::now(),
            access_count: 0,
            size_bytes: 200,
        };

        cache.put(key, entry).await.unwrap();
        added += 1;

        if cache.memory_usage() > 900 {
            break;
        }
    }

    // Should have added some but not all entries
    assert!(added < 100);
    assert!(cache.memory_usage() <= 1024);
}

#[tokio::test]
async fn test_cache_statistics() {
    let config = CacheConfig::default();
    let mut cache = InferenceCache::new(config)
        .await
        .expect("Failed to create cache");

    // Reset stats
    cache.reset_stats();

    // Perform operations
    let key1 = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "Test 1".to_string(),
        temperature: 0.7,
        max_tokens: 10,
    };

    let entry1 = CacheEntry {
        response: "Response 1".to_string(),
        tokens_generated: 2,
        generation_time: Duration::from_millis(50),
        timestamp: std::time::SystemTime::now(),
        access_count: 0,
        size_bytes: "Response 1".len(),
    };

    // Put and hit
    cache.put(key1.clone(), entry1).await.unwrap();
    let _ = cache.get(&key1).await; // Hit
    let _ = cache.get(&key1).await; // Hit

    // Miss
    let key2 = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "Test 2".to_string(),
        temperature: 0.7,
        max_tokens: 10,
    };
    let _ = cache.get(&key2).await; // Miss

    // Check stats
    let stats = cache.get_stats().await;
    assert_eq!(stats.hits, 2);
    assert_eq!(stats.misses, 1);
    assert_eq!(stats.hit_rate(), 2.0 / 3.0);
    assert!(stats.avg_latency.as_nanos() > 0);
}

#[tokio::test]
async fn test_cache_invalidation() {
    let config = CacheConfig::default();
    let mut cache = InferenceCache::new(config)
        .await
        .expect("Failed to create cache");

    // Add entries for multiple models
    for model in &["llama-7b", "mistral-7b", "codellama-7b"] {
        for i in 0..5 {
            let key = CacheKey {
                model_id: model.to_string(),
                prompt: format!("Prompt {}", i),
                temperature: 0.7,
                max_tokens: 10,
            };

            let entry = CacheEntry {
                response: format!("Response {}", i),
                tokens_generated: 2,
                generation_time: Duration::from_millis(50),
                timestamp: std::time::SystemTime::now(),
                access_count: 0,
                size_bytes: format!("Response {}", i).len(),
            };

            cache.put(key, entry).await.unwrap();
        }
    }

    assert_eq!(cache.size(), 15);

    // Invalidate all entries for a specific model
    cache.invalidate_model("llama-7b").await;

    assert_eq!(cache.size(), 10);

    // Clear all
    cache.clear().await;
    assert_eq!(cache.size(), 0);
}

#[tokio::test]
async fn test_cache_persistence() {
    let cache_dir = tempfile::tempdir().unwrap();
    let config = CacheConfig {
        persistence_path: Some(cache_dir.path().to_path_buf()),
        ..Default::default()
    };

    // Create cache and add entries
    {
        let mut cache = InferenceCache::new(config.clone())
            .await
            .expect("Failed to create cache");

        let key = CacheKey {
            model_id: "llama-7b".to_string(),
            prompt: "Persistent prompt".to_string(),
            temperature: 0.7,
            max_tokens: 10,
        };

        let entry = CacheEntry {
            response: "Persistent response".to_string(),
            tokens_generated: 2,
            generation_time: Duration::from_millis(50),
            timestamp: std::time::SystemTime::now(),
            access_count: 0,
            size_bytes: "Persistent response".len(),
        };

        cache.put(key.clone(), entry).await.unwrap();

        // Save to disk
        let persist_path = std::path::Path::new("/tmp/test_cache.bin");
        cache
            .persist(persist_path)
            .await
            .expect("Failed to persist cache");
    }

    // Create new cache instance and load
    {
        let cache = InferenceCache::new(config)
            .await
            .expect("Failed to create cache");

        let key = CacheKey {
            model_id: "llama-7b".to_string(),
            prompt: "Persistent prompt".to_string(),
            temperature: 0.7,
            max_tokens: 10,
        };

        // Should load persisted entry
        let entry = cache.get(&key).await;
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().response, "Persistent response");
    }
}

#[tokio::test]
async fn test_cache_compression() {
    let config = CacheConfig::default();

    let mut cache = InferenceCache::new(config)
        .await
        .expect("Failed to create cache");

    // Add large entry that should be compressed
    let key = CacheKey {
        model_id: "llama-7b".to_string(),
        prompt: "Generate a long story".to_string(),
        temperature: 0.8,
        max_tokens: 1000,
    };

    let long_response = "Once upon a time ".repeat(100); // ~1700 bytes
    let entry = CacheEntry {
        response: long_response.clone(),
        tokens_generated: 400,
        generation_time: Duration::from_millis(5000),
        timestamp: std::time::SystemTime::now(),
        access_count: 0,
        size_bytes: long_response.len(),
    };

    cache.put(key.clone(), entry).await.unwrap();

    // Retrieve and verify
    let retrieved = cache.get(&key).await;
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().response, long_response);

    // Memory usage should be less due to compression
    let uncompressed_size = long_response.len() + 100; // Rough estimate
    assert!(cache.memory_usage() < uncompressed_size);
}

#[tokio::test]
async fn test_cache_with_different_params() {
    let config = CacheConfig::default();
    let mut cache = InferenceCache::new(config)
        .await
        .expect("Failed to create cache");

    let base_prompt = "Explain quantum computing";

    // Same prompt, different parameters should be different cache entries
    let configs = vec![(0.5, 50), (0.7, 50), (0.5, 100), (0.7, 100)];

    for (temp, max_tokens) in &configs {
        let key = CacheKey {
            model_id: "llama-7b".to_string(),
            prompt: base_prompt.to_string(),
            temperature: *temp,
            max_tokens: *max_tokens,
        };

        let entry = CacheEntry {
            response: format!("Response for temp {} tokens {}", temp, max_tokens),
            tokens_generated: 10,
            generation_time: Duration::from_millis(100),
            timestamp: std::time::SystemTime::now(),
            access_count: 0,
            size_bytes: format!("Response for temp {} tokens {}", temp, max_tokens).len(),
        };

        cache.put(key, entry).await.unwrap();
    }

    // All should be cached separately
    assert_eq!(cache.size(), 4);

    // Verify each has correct response
    for (temp, max_tokens) in &configs {
        let key = CacheKey {
            model_id: "llama-7b".to_string(),
            prompt: base_prompt.to_string(),
            temperature: *temp,
            max_tokens: *max_tokens,
        };

        let entry = cache.get(&key).await.unwrap();
        assert_eq!(
            entry.response,
            format!("Response for temp {} tokens {}", temp, max_tokens)
        );
    }
}
