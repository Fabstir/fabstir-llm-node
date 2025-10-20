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
use fabstir_llm_node::{
    cache::{PromptCache, CacheConfig, CacheMetrics},
    storage::{EnhancedS5Client, S5Config},
    vector::{VectorDbClient, VectorDbConfig},
    embeddings::{EmbeddingGenerator, EmbeddingConfig},
};
use serde_json::json;
use std::time::{Duration, Instant};
use sha2::{Sha256, Digest};

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
    // Initialize clients
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
    
    // Store some cached prompts with their embeddings
    let cached_prompts = vec![
        ("What is machine learning?", "Machine learning is a subset of AI..."),
        ("Explain neural networks", "Neural networks are computing systems..."),
        ("How does deep learning work?", "Deep learning uses multiple layers..."),
        ("What is artificial intelligence?", "AI refers to computer systems..."),
    ];
    
    for (prompt, response) in &cached_prompts {
        let embedding = generator.generate(prompt).await?;
        let prompt_hash = hash_prompt(prompt);
        
        let metadata = json!({
            "prompt": prompt,
            "prompt_hash": prompt_hash,
            "response_preview": &response[..20.min(response.len())],
            "model": "llama-3.2-1b-instruct",
            "timestamp": "2025-01-06T12:00:00Z",
            "token_count": 50
        });
        
        vector_client.insert_vector(
            &format!("prompt-{}", &prompt_hash[..8]),
            embedding,
            metadata
        ).await?;
    }
    
    // Search for similar prompt
    let new_prompt = "Tell me about machine learning algorithms";
    let new_embedding = generator.generate(new_prompt).await?;
    
    let similar_prompts = vector_client.search(
        new_embedding,
        3,
        None
    ).await?;
    
    // Should find the ML-related cached prompt
    assert!(!similar_prompts.is_empty());
    let top_similar = &similar_prompts[0];
    assert!(top_similar["metadata"]["prompt"].as_str().unwrap().contains("machine learning"));
    
    // Check similarity score threshold
    let similarity_score = top_similar["score"].as_f64().unwrap();
    assert!(similarity_score > 0.6); // Reasonably similar
    
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
    
    // Store a cached response
    let prompt = "Explain quantum computing";
    let prompt_hash = hash_prompt(prompt);
    let response = json!({
        "prompt": prompt,
        "response": "Quantum computing uses quantum mechanical phenomena like superposition and entanglement to perform computations. Unlike classical bits that are either 0 or 1, quantum bits (qubits) can exist in superposition of both states simultaneously.",
        "model": "llama-3.2-3b-instruct",
        "parameters": {
            "temperature": 0.7,
            "max_tokens": 150,
            "top_p": 0.9
        },
        "generated_at": "2025-01-06T12:30:00Z",
        "token_count": 87,
        "generation_time_ms": 1250
    });
    
    // Store in S5 cache directory
    let cache_path = format!("/cache/prompts/{}/{}.json", &prompt_hash[..2], prompt_hash);
    let cid = s5_client.put(
        &cache_path,
        serde_json::to_vec(&response)?,
        Some(json!({
            "type": "prompt_cache",
            "hash": prompt_hash,
            "model": "llama-3.2-3b-instruct"
        }))
    ).await?;
    
    // Retrieve cached result
    let (cached_data, cached_metadata) = s5_client.get(&cache_path).await?;
    let cached_response: serde_json::Value = serde_json::from_slice(&cached_data)?;
    
    assert_eq!(cached_response["prompt"], prompt);
    assert_eq!(cached_response["token_count"], 87);
    assert_eq!(cached_metadata.unwrap()["hash"], prompt_hash);
    
    Ok(())
}

#[tokio::test]
async fn test_measure_cache_hit_rates() -> Result<()> {
    // Initialize cache with metrics
    let cache_config = CacheConfig {
        s5_url: "http://enhanced-s5-container:5050".to_string(),
        vector_db_url: "http://fabstir-ai-vector-db-container:7530".to_string(),
        similarity_threshold: 0.85, // High threshold for exact matches
        ttl_seconds: 3600,
        max_cache_size_mb: 100,
    };
    
    let cache = PromptCache::new(cache_config).await?;
    
    // Simulate cache operations
    let prompts = vec![
        "What is the speed of light?",
        "What is the speed of light?", // Duplicate - should hit
        "How fast does light travel?",  // Similar - might hit
        "What is the speed of sound?",  // Different - should miss
        "What is the speed of light?", // Another duplicate - should hit
        "Explain relativity theory",    // Different - should miss
        "What is the speed of light in vacuum?", // Very similar - might hit
    ];
    
    let mut hits = 0;
    let mut misses = 0;
    
    for (i, prompt) in prompts.iter().enumerate() {
        let result = cache.get(prompt).await?;
        
        match result {
            Some(_) => {
                hits += 1;
                println!("Prompt {}: HIT - '{}'", i, prompt);
            }
            None => {
                misses += 1;
                println!("Prompt {}: MISS - '{}'", i, prompt);
                
                // Store in cache for future hits
                let response = format!("Response for: {}", prompt);
                cache.put(prompt, &response).await?;
            }
        }
    }
    
    // Calculate metrics
    let total_requests = hits + misses;
    let hit_rate = (hits as f64) / (total_requests as f64);
    
    println!("\nCache Metrics:");
    println!("  Total Requests: {}", total_requests);
    println!("  Hits: {}", hits);
    println!("  Misses: {}", misses);
    println!("  Hit Rate: {:.2}%", hit_rate * 100.0);
    
    // Should have at least some hits from duplicates
    assert!(hits >= 2); // At least the exact duplicates
    assert!(hit_rate > 0.0);
    
    // Get cache metrics
    let metrics = cache.get_metrics().await?;
    assert_eq!(metrics.total_requests, total_requests);
    assert_eq!(metrics.cache_hits, hits);
    assert_eq!(metrics.cache_misses, misses);
    assert_eq!(metrics.hit_rate, hit_rate);
    
    Ok(())
}

#[tokio::test]
async fn test_complete_cache_flow() -> Result<()> {
    // Complete end-to-end cache flow test
    
    // Initialize all components
    let s5_config = S5Config {
        api_url: "http://enhanced-s5-container:5050".to_string(),
        api_key: Some("test-api-key".to_string()),
        timeout_secs: 30,
    };
    let s5_client = EnhancedS5Client::new(s5_config)?;
    
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
    
    // Simulate an inference request
    let prompt = "Write a Python function to calculate fibonacci numbers";
    let model = "codellama-7b";
    
    // Step 1: Hash the prompt for exact match
    let prompt_key = format!("{}:{}:temp=0.7:max=100", prompt, model);
    let prompt_hash = hash_prompt(&prompt_key);
    
    // Step 2: Check exact match in S5 (fast path)
    let cache_path = format!("/cache/prompts/{}/{}.json", &prompt_hash[..2], prompt_hash);
    let exact_match = s5_client.get(&cache_path).await;
    
    if exact_match.is_err() {
        println!("No exact match, checking semantic similarity...");
        
        // Step 3: Generate embedding for semantic search
        let prompt_embedding = generator.generate(prompt).await?;
        
        // Step 4: Search for similar prompts in Vector DB
        let similar = vector_client.search(
            prompt_embedding.clone(),
            5,
            Some(json!({"model": model}))
        ).await?;
        
        if !similar.is_empty() && similar[0]["score"].as_f64().unwrap() > 0.9 {
            println!("Found similar cached prompt with score: {}", similar[0]["score"]);
            
            // Step 5: Retrieve cached response from S5
            let cached_hash = similar[0]["metadata"]["prompt_hash"].as_str().unwrap();
            let cached_path = format!("/cache/prompts/{}/{}.json", &cached_hash[..2], cached_hash);
            let (cached_data, _) = s5_client.get(&cached_path).await?;
            let cached_response: serde_json::Value = serde_json::from_slice(&cached_data)?;
            
            println!("Cache HIT! Using cached response");
            assert!(cached_response["response"].as_str().unwrap().contains("def fibonacci"));
        } else {
            println!("Cache MISS! Generating new response...");
            
            // Step 6: Generate new response (simulated)
            let start_time = Instant::now();
            tokio::time::sleep(Duration::from_millis(100)).await; // Simulate inference
            let response = "def fibonacci(n):\n    if n <= 1:\n        return n\n    return fibonacci(n-1) + fibonacci(n-2)";
            let generation_time = start_time.elapsed().as_millis();
            
            // Step 7: Store response in S5
            let response_data = json!({
                "prompt": prompt,
                "prompt_key": prompt_key,
                "response": response,
                "model": model,
                "parameters": {
                    "temperature": 0.7,
                    "max_tokens": 100
                },
                "generated_at": "2025-01-06T13:00:00Z",
                "generation_time_ms": generation_time
            });
            
            let response_cid = s5_client.put(
                &cache_path,
                serde_json::to_vec(&response_data)?,
                Some(json!({
                    "type": "prompt_cache",
                    "hash": prompt_hash
                }))
            ).await?;
            
            // Step 8: Store embedding in Vector DB for future semantic search
            let vector_metadata = json!({
                "prompt": prompt,
                "prompt_hash": prompt_hash,
                "model": model,
                "s5_cid": response_cid,
                "s5_path": cache_path,
                "timestamp": "2025-01-06T13:00:00Z"
            });
            
            vector_client.insert_vector(
                &format!("cache-{}", &prompt_hash[..8]),
                prompt_embedding,
                vector_metadata
            ).await?;
            
            println!("Response cached for future use");
        }
    } else {
        println!("Cache HIT! Found exact match");
        let (data, _) = exact_match.unwrap();
        let response: serde_json::Value = serde_json::from_slice(&data)?;
        assert_eq!(response["prompt"], prompt);
    }
    
    // Verify the cache works on second request
    println!("\n--- Second request for same prompt ---");
    
    // This should be a cache hit
    let cache_result = s5_client.get(&cache_path).await;
    assert!(cache_result.is_ok());
    
    let (cached_data, cached_meta) = cache_result?;
    let cached: serde_json::Value = serde_json::from_slice(&cached_data)?;
    assert_eq!(cached["prompt"], prompt);
    assert_eq!(cached["model"], model);
    
    println!("✅ Cache flow complete - second request was a HIT!");
    
    Ok(())
}

#[tokio::test]
async fn test_cache_expiration_and_cleanup() -> Result<()> {
    // Test TTL and cache cleanup
    
    let cache_config = CacheConfig {
        s5_url: "http://enhanced-s5-container:5050".to_string(),
        vector_db_url: "http://fabstir-ai-vector-db-container:7530".to_string(),
        similarity_threshold: 0.85,
        ttl_seconds: 2, // Very short TTL for testing
        max_cache_size_mb: 1, // Small size to trigger cleanup
    };
    
    let cache = PromptCache::new(cache_config).await?;
    
    // Add items to cache
    let prompts = vec![
        ("prompt1", "response1"),
        ("prompt2", "response2"),
        ("prompt3", "response3"),
    ];
    
    for (prompt, response) in &prompts {
        cache.put(prompt, response).await?;
    }
    
    // Verify all cached
    for (prompt, _) in &prompts {
        let result = cache.get(prompt).await?;
        assert!(result.is_some());
    }
    
    // Wait for TTL to expire
    tokio::time::sleep(Duration::from_secs(3)).await;
    
    // Should not find expired entries
    for (prompt, _) in &prompts {
        let result = cache.get(prompt).await?;
        assert!(result.is_none(), "Cache entry should have expired");
    }
    
    // Test size-based eviction
    let large_response = "x".repeat(500_000); // 500KB response
    cache.put("large1", &large_response).await?;
    cache.put("large2", &large_response).await?;
    cache.put("large3", &large_response).await?; // Should trigger eviction
    
    // Oldest entry should be evicted
    assert!(cache.get("large1").await?.is_none());
    assert!(cache.get("large3").await?.is_some()); // Newest should remain
    
    Ok(())
}

#[tokio::test]
async fn test_cache_performance_metrics() -> Result<()> {
    // Measure cache performance improvement
    
    let cache_config = CacheConfig {
        s5_url: "http://enhanced-s5-container:5050".to_string(),
        vector_db_url: "http://fabstir-ai-vector-db-container:7530".to_string(),
        similarity_threshold: 0.85,
        ttl_seconds: 3600,
        max_cache_size_mb: 100,
    };
    
    let cache = PromptCache::new(cache_config).await?;
    
    // Measure time for cache miss (includes "inference")
    let prompt = "Explain the theory of relativity";
    let start_miss = Instant::now();
    
    let result = cache.get(prompt).await?;
    if result.is_none() {
        // Simulate inference time
        tokio::time::sleep(Duration::from_millis(500)).await;
        cache.put(prompt, "Einstein's theory states...").await?;
    }
    
    let miss_time = start_miss.elapsed();
    
    // Measure time for cache hit
    let start_hit = Instant::now();
    let cached = cache.get(prompt).await?;
    assert!(cached.is_some());
    let hit_time = start_hit.elapsed();
    
    // Cache hit should be much faster
    let speedup = miss_time.as_millis() as f64 / hit_time.as_millis().max(1) as f64;
    
    println!("Performance Metrics:");
    println!("  Cache MISS time: {:?}", miss_time);
    println!("  Cache HIT time: {:?}", hit_time);
    println!("  Speedup: {:.1}x", speedup);
    
    assert!(hit_time < miss_time);
    assert!(speedup > 10.0); // Should be at least 10x faster
    
    // Get detailed metrics
    let metrics = cache.get_metrics().await?;
    println!("\nCache Statistics:");
    println!("  Total Requests: {}", metrics.total_requests);
    println!("  Hit Rate: {:.2}%", metrics.hit_rate * 100.0);
    println!("  Avg Hit Time: {:.2}ms", metrics.avg_hit_time_ms);
    println!("  Avg Miss Time: {:.2}ms", metrics.avg_miss_time_ms);
    println!("  Cache Size: {:.2}MB", metrics.cache_size_mb);
    
    Ok(())
}