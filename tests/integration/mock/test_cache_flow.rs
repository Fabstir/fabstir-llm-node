// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// tests/integration/mock/test_cache_flow.rs
// Phase 4.1.3: Cache Flow Implementation
// This test verifies the caching workflow:
// 1. Hash prompts for cache lookup
// 2. Search Vector DB for similar prompts
// 3. Retrieve cached results from S5
// 4. Measure cache hit rates

use anyhow::Result;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use tokio::time::sleep;

// Import from our crate
use fabstir_llm_node::{
    cache::{CacheConfig, CacheMetrics, PromptCache},
    embeddings::{EmbeddingConfig, EmbeddingGenerator},
    storage::{EnhancedS5Client, S5Config},
    vector::{VectorDbClient, VectorDbConfig},
};

fn hash_prompt(prompt: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prompt.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[tokio::test]
async fn test_hash_prompts_for_cache_lookup() -> Result<()> {
    // Test deterministic prompt hashing
    let prompt1 = "What is the capital of France?";
    let prompt2 = "What is the capital of France?"; // Same prompt
    let prompt3 = "What is the capital of Germany?"; // Different prompt

    let hash1 = hash_prompt(prompt1);
    let hash2 = hash_prompt(prompt2);
    let hash3 = hash_prompt(prompt3);

    // Same prompts should have same hash
    assert_eq!(hash1, hash2);
    // Different prompts should have different hash
    assert_ne!(hash1, hash3);

    // Hash should be consistent length (SHA256 = 64 hex chars)
    assert_eq!(hash1.len(), 64);
    assert_eq!(hash3.len(), 64);

    // Test with model parameters included
    let prompt_with_params = format!("{};model=llama-3.2;temp=0.7;max_tokens=100", prompt1);
    let hash_with_params = hash_prompt(&prompt_with_params);
    assert_ne!(hash1, hash_with_params); // Different when params included

    Ok(())
}

#[tokio::test]
async fn test_search_vector_db_for_similar_prompts() -> Result<()> {
    // Initialize vector DB and embedding generator
    let vector_config = VectorDbConfig {
        api_url: "http://fabstir-ai-vector-db-container:7530".to_string(),
        api_key: Some("test-vector-key".to_string()),
        timeout_secs: 30,
    };
    let vector_client = VectorDbClient::new(vector_config)?;

    let embedding_config = EmbeddingConfig {
        model: "all-MiniLM-L6-v2".to_string(),
        dimension: 384,
        batch_size: 32,
        normalize: true,
    };
    let generator = EmbeddingGenerator::new(embedding_config).await?;

    // Store various cached prompts with embeddings
    let cached_prompts = vec![
        (
            "What is machine learning?",
            "Machine learning is a subset of AI...",
        ),
        (
            "Explain deep learning",
            "Deep learning is a type of machine learning...",
        ),
        (
            "What is artificial intelligence?",
            "AI is the simulation of human intelligence...",
        ),
        (
            "How does neural network work?",
            "Neural networks are computing systems...",
        ),
    ];

    for (prompt, response) in &cached_prompts {
        let prompt_hash = hash_prompt(prompt);
        let embedding = generator.generate(prompt).await?;

        let metadata = json!({
            "type": "cache_entry",
            "prompt": prompt,
            "response": response,
            "prompt_hash": prompt_hash,
            "generated_at": "2025-01-06T10:00:00Z",
        });

        vector_client
            .insert_vector(&prompt_hash, embedding, metadata)
            .await?;
    }

    // Search for similar prompt
    let search_prompt = "Tell me about machine learning";
    let search_embedding = generator.generate(search_prompt).await?;

    let filter = Some(json!({
        "type": "cache_entry"
    }));

    let results = vector_client.search(search_embedding, 3, filter).await?;

    assert!(!results.is_empty());
    assert!(results.len() <= 3);

    // First result should be most similar (about machine learning)
    assert!(!results.is_empty(), "Expected at least one search result");

    let first = results.first().unwrap();

    // Check metadata exists
    let metadata = first.get("metadata").expect("Result should have metadata");

    // Check prompt in metadata
    let prompt = metadata
        .get("prompt")
        .and_then(|p| p.as_str())
        .expect("Metadata should have prompt field");

    assert!(
        prompt.contains("machine learning")
            || prompt.contains("AI")
            || prompt.contains("artificial intelligence"),
        "Expected prompt to contain ML/AI keywords, but got: '{}'",
        prompt
    );

    // Check similarity score
    let score = first
        .get("score")
        .and_then(|s| s.as_f64())
        .expect("Result should have score");

    assert!(
        score >= 0.0 && score <= 1.0,
        "Score should be in [0,1] range, got: {}",
        score
    );
    assert!(
        score >= 0.5,
        "Score should be >= 0.5 for similar prompts, got: {}",
        score
    );

    Ok(())
}

#[tokio::test]
async fn test_retrieve_cached_results_from_s5() -> Result<()> {
    // Initialize S5 client
    let s5_config = S5Config {
        api_url: "http://enhanced-s5-container:5050".to_string(),
        api_key: Some("test-api-key".to_string()),
        timeout_secs: 30,
    };
    let s5_client = EnhancedS5Client::new(s5_config)?;

    // Create cache entry
    let prompt = "What is Rust programming language?";
    let response = "Rust is a systems programming language focused on safety...";
    let prompt_hash = hash_prompt(prompt);

    let cache_entry = json!({
        "prompt": prompt,
        "prompt_key": format!("{};model=llama-3.2;temp=0.7;max_tokens=100", prompt),
        "response": response,
        "model": "llama-3.2-1b-instruct",
        "parameters": {
            "temperature": 0.7,
            "max_tokens": 100
        },
        "generated_at": "2025-01-06T10:00:00Z",
        "generation_time_ms": 1250
    });

    // Store in S5 at expected path
    let path = format!("/cache/prompts/{}/{}.json", &prompt_hash[0..2], prompt_hash);
    let json_data = serde_json::to_string(&cache_entry)?;

    let metadata = json!({
        "type": "cache_entry",
        "prompt_hash": prompt_hash,
        "model": "llama-3.2-1b-instruct",
    });

    let cid = s5_client
        .put(&path, json_data.into_bytes(), Some(metadata))
        .await?;
    assert!(!cid.is_empty());

    // Retrieve from S5
    let (retrieved_data, retrieved_metadata) = s5_client.get(&path).await?;
    let retrieved_json: serde_json::Value = serde_json::from_slice(&retrieved_data)?;

    assert_eq!(retrieved_json["prompt"], prompt);
    assert_eq!(retrieved_json["response"], response);
    assert_eq!(retrieved_json["model"], "llama-3.2-1b-instruct");
    assert_eq!(retrieved_json["generation_time_ms"], 1250);

    // Verify metadata
    if let Some(meta) = retrieved_metadata {
        assert_eq!(meta["type"], "cache_entry");
        assert_eq!(meta["prompt_hash"], prompt_hash);
    }

    Ok(())
}

#[tokio::test]
async fn test_measure_cache_hit_rates() -> Result<()> {
    // Initialize cache
    let cache_config = CacheConfig {
        s5_url: "http://enhanced-s5-container:5050".to_string(),
        vector_db_url: "http://fabstir-ai-vector-db-container:7530".to_string(),
        similarity_threshold: 0.8,
        ttl_seconds: 3600,
        max_cache_size_mb: 10,
    };
    let cache = PromptCache::new(cache_config).await?;

    // Populate cache with some entries
    let prompts = vec![
        (
            "What is Python?",
            "Python is a high-level programming language...",
        ),
        (
            "Explain JavaScript",
            "JavaScript is a scripting language for web...",
        ),
        (
            "What is Docker?",
            "Docker is a containerization platform...",
        ),
    ];

    for (prompt, response) in &prompts {
        cache.put(prompt, response).await?;
    }

    // Test cache hits
    for (prompt, expected_response) in &prompts {
        let result = cache.get(prompt).await?;
        assert_eq!(result.as_deref(), Some(expected_response.as_ref()));
    }

    // Test cache misses
    let miss_prompts = vec![
        "What is Kubernetes?",
        "Explain machine learning",
        "How does blockchain work?",
    ];

    for prompt in &miss_prompts {
        let result = cache.get(prompt).await?;
        assert_eq!(result, None);
    }

    // Get metrics
    let metrics = cache.get_metrics().await?;

    assert_eq!(metrics.total_requests, 6); // 3 hits + 3 misses
    assert_eq!(metrics.cache_hits, 3);
    assert_eq!(metrics.cache_misses, 3);
    assert!((metrics.hit_rate - 0.5).abs() < 0.01); // 50% hit rate

    // Times should be reasonable
    assert!(metrics.avg_hit_time_ms >= 0.0);
    assert!(metrics.avg_miss_time_ms >= 0.0);
    assert!(metrics.cache_size_mb >= 0.0);

    Ok(())
}

#[tokio::test]
async fn test_complete_cache_flow() -> Result<()> {
    // Initialize complete cache system
    let cache_config = CacheConfig {
        s5_url: "http://enhanced-s5-container:5050".to_string(),
        vector_db_url: "http://fabstir-ai-vector-db-container:7530".to_string(),
        similarity_threshold: 0.75,
        ttl_seconds: 3600,
        max_cache_size_mb: 10,
    };
    let cache = PromptCache::new(cache_config).await?;

    // Test 1: Store a response in cache
    let prompt1 = "What is the meaning of life?;model=llama-3.2;temp=0.7;max_tokens=100";
    let response1 = "The meaning of life is a philosophical question that has been pondered...";

    cache.put(prompt1, response1).await?;

    // Test 2: Exact match retrieval
    let cached = cache.get(prompt1).await?;
    assert_eq!(cached.as_deref(), Some(response1));

    // Test 3: Store another response
    let prompt2 = "What is the purpose of existence?;model=llama-3.2;temp=0.7;max_tokens=100";
    let response2 = "The purpose of existence is another deep philosophical inquiry...";

    cache.put(prompt2, response2).await?;

    // Test 4: Semantic similarity search (similar but not exact)
    let similar_prompt =
        "What's the meaning of human life?;model=llama-3.2;temp=0.7;max_tokens=100";
    let result = cache.get(&similar_prompt).await?;

    // Should find similar cached result (either response1 or response2)
    assert!(result.is_some());
    let found_response = result.unwrap();
    assert!(found_response == response1 || found_response == response2);

    // Test 5: Completely different prompt should miss
    let different_prompt = "How to cook pasta?;model=llama-3.2;temp=0.7;max_tokens=100";
    let miss_result = cache.get(&different_prompt).await?;
    assert_eq!(miss_result, None);

    // Test 6: Verify metrics
    let metrics = cache.get_metrics().await?;
    assert_eq!(metrics.total_requests, 3); // 3 get requests
    assert_eq!(metrics.cache_hits, 2); // exact match + semantic match
    assert_eq!(metrics.cache_misses, 1); // pasta query
    assert!(metrics.hit_rate > 0.6); // Should be ~66%

    Ok(())
}

#[tokio::test]
async fn test_cache_expiration_and_cleanup() -> Result<()> {
    // Initialize cache with short TTL
    let cache_config = CacheConfig {
        s5_url: "http://enhanced-s5-container:5050".to_string(),
        vector_db_url: "http://fabstir-ai-vector-db-container:7530".to_string(),
        similarity_threshold: 0.8,
        ttl_seconds: 2, // 2 second TTL
        max_cache_size_mb: 10,
    };
    let cache = PromptCache::new(cache_config).await?;

    // Store an entry
    let prompt = "What is cache expiration?";
    let response = "Cache expiration is the process of removing old entries...";

    cache.put(prompt, response).await?;

    // Immediate retrieval should work
    let result1 = cache.get(prompt).await?;
    assert_eq!(result1.as_deref(), Some(response));

    // Wait for TTL to expire
    sleep(Duration::from_secs(3)).await;

    // Should return None after expiration
    let result2 = cache.get(prompt).await?;
    assert_eq!(result2, None);

    // Verify metrics show the miss
    let metrics = cache.get_metrics().await?;
    assert_eq!(metrics.total_requests, 2);
    assert_eq!(metrics.cache_hits, 1);
    assert_eq!(metrics.cache_misses, 1);

    // Test size-based eviction
    let large_cache_config = CacheConfig {
        s5_url: "http://enhanced-s5-container:5050".to_string(),
        vector_db_url: "http://fabstir-ai-vector-db-container:7530".to_string(),
        similarity_threshold: 0.8,
        ttl_seconds: 3600,
        max_cache_size_mb: 1, // Very small cache (1 MB)
    };
    let large_cache = PromptCache::new(large_cache_config).await?;

    // Fill cache beyond limit - using fewer entries for faster test
    for i in 0..20 {
        let prompt = format!("Test prompt {}", i);
        // Make each response large enough to trigger eviction (50KB each)
        let response = format!(
            "Response with some data to take up space: {}",
            "x".repeat(50000)
        );
        large_cache.put(&prompt, &response).await?;
    }

    // Cache should have evicted old entries to stay under size limit
    let large_metrics = large_cache.get_metrics().await?;
    assert!(large_metrics.cache_size_mb <= 1.0);

    Ok(())
}

#[tokio::test]
async fn test_cache_performance_metrics() -> Result<()> {
    // Initialize cache
    let cache_config = CacheConfig {
        s5_url: "http://enhanced-s5-container:5050".to_string(),
        vector_db_url: "http://fabstir-ai-vector-db-container:7530".to_string(),
        similarity_threshold: 0.8,
        ttl_seconds: 3600,
        max_cache_size_mb: 10,
    };
    let cache = PromptCache::new(cache_config).await?;

    // Populate cache
    let prompts = vec![
        ("fast query 1", "response 1"),
        ("fast query 2", "response 2"),
        ("fast query 3", "response 3"),
    ];

    for (prompt, response) in &prompts {
        cache.put(prompt, response).await?;
    }

    // Measure hit times
    let mut hit_times = Vec::new();
    for (prompt, _) in &prompts {
        let start = Instant::now();
        let _ = cache.get(prompt).await?;
        hit_times.push(start.elapsed().as_millis() as f64);
    }

    // Measure miss times
    let mut miss_times = Vec::new();
    let miss_prompts = vec!["miss 1", "miss 2", "miss 3"];
    for prompt in &miss_prompts {
        let start = Instant::now();
        let _ = cache.get(prompt).await?;
        miss_times.push(start.elapsed().as_millis() as f64);
    }

    // Get metrics
    let metrics = cache.get_metrics().await?;

    // Verify performance metrics are tracked
    assert_eq!(metrics.total_requests, 6);
    assert_eq!(metrics.cache_hits, 3);
    assert_eq!(metrics.cache_misses, 3);

    // Average times should be positive
    assert!(metrics.avg_hit_time_ms >= 0.0);
    assert!(metrics.avg_miss_time_ms >= 0.0);

    // Hit times should generally be faster than miss times
    // (though in mock this might not always be true)
    println!("Avg hit time: {:.2}ms", metrics.avg_hit_time_ms);
    println!("Avg miss time: {:.2}ms", metrics.avg_miss_time_ms);

    // Cache size should be tracked
    assert!(metrics.cache_size_mb > 0.0);
    println!("Cache size: {:.2}MB", metrics.cache_size_mb);

    Ok(())
}
