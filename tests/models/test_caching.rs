use anyhow::Result;
use fabstir_llm_node::models::{
    ModelCache, CacheConfig, CacheEntry, CacheStatus, CacheError,
    EvictionPolicy, CacheMetrics, PersistenceConfig, CacheEvent,
    ModelHandle, CachePriority, WarmupStrategy
};
use std::path::PathBuf;
use std::sync::Arc;
use tokio;

async fn create_test_cache() -> Result<ModelCache> {
    let config = CacheConfig {
        max_memory_gb: 16,
        max_models: 10,
        eviction_policy: EvictionPolicy::LRU,
        enable_persistence: true,
        persistence_path: PathBuf::from("test_data/cache"),
        compression_enabled: true,
        preload_popular: true,
        min_free_memory_gb: 2,
    };
    
    ModelCache::new(config).await
}

fn create_test_model_id(index: usize) -> String {
    format!("model_{}", index)
}

#[tokio::test]
async fn test_basic_cache_operations() {
    let cache = create_test_cache().await.unwrap();
    
    let model_id = "llama-7b";
    let model_path = PathBuf::from("test_data/models/llama-7b.gguf");
    
    // Load model into cache
    let handle = cache.load_model(model_id, &model_path).await.unwrap();
    
    assert!(handle.is_loaded());
    assert_eq!(handle.model_id(), model_id);
    assert!(handle.memory_usage_bytes() > 0);
    
    // Get from cache
    let cached_handle = cache.get_model(model_id).await.unwrap();
    assert_eq!(cached_handle.model_id(), handle.model_id());
    
    // Check if in cache
    assert!(cache.contains(model_id).await);
    
    // Remove from cache
    cache.evict_model(model_id).await.unwrap();
    assert!(!cache.contains(model_id).await);
}

#[tokio::test]
async fn test_lru_eviction() {
    let mut config = CacheConfig::default();
    config.max_models = 3;
    config.eviction_policy = EvictionPolicy::LRU;
    
    let cache = ModelCache::new(config).await.unwrap();
    
    // Load models to fill cache
    for i in 0..4 {
        let model_id = create_test_model_id(i);
        let path = PathBuf::from(format!("test_data/models/model_{}.gguf", i));
        cache.load_model(&model_id, &path).await.unwrap();
    }
    
    // First model should be evicted
    assert!(!cache.contains(&create_test_model_id(0)).await);
    assert!(cache.contains(&create_test_model_id(1)).await);
    assert!(cache.contains(&create_test_model_id(2)).await);
    assert!(cache.contains(&create_test_model_id(3)).await);
    
    // Access model 1 to make it recently used
    cache.get_model(&create_test_model_id(1)).await.unwrap();
    
    // Load another model
    let model_id = create_test_model_id(4);
    let path = PathBuf::from("test_data/models/model_4.gguf");
    cache.load_model(&model_id, &path).await.unwrap();
    
    // Model 2 should be evicted (least recently used)
    assert!(!cache.contains(&create_test_model_id(2)).await);
    assert!(cache.contains(&create_test_model_id(1)).await); // Still in cache
}

#[tokio::test]
async fn test_memory_limit_enforcement() {
    let mut config = CacheConfig::default();
    config.max_memory_gb = 1; // Very low limit
    config.max_models = 100; // High model limit
    
    let cache = ModelCache::new(config).await.unwrap();
    
    let mut loaded_count = 0;
    
    // Try to load many models
    for i in 0..10 {
        let model_id = create_test_model_id(i);
        let path = PathBuf::from(format!("test_data/models/model_{}.gguf", i));
        
        match cache.load_model(&model_id, &path).await {
            Ok(_) => loaded_count += 1,
            Err(e) => {
                // Should eventually hit memory limit
                match e.downcast_ref::<CacheError>() {
                    Some(CacheError::InsufficientMemory { .. }) => break,
                    _ => panic!("Unexpected error: {:?}", e),
                }
            }
        }
    }
    
    assert!(loaded_count > 0 && loaded_count < 10);
    
    let metrics = cache.get_metrics().await;
    assert!(metrics.memory_usage_gb <= 1.0);
}

#[tokio::test]
async fn test_cache_persistence() {
    let cache_path = PathBuf::from("test_data/cache_persist");
    
    // Create and populate cache
    {
        let mut config = CacheConfig::default();
        config.enable_persistence = true;
        config.persistence_path = cache_path.clone();
        
        let cache = ModelCache::new(config).await.unwrap();
        
        // Load some models
        for i in 0..3 {
            let model_id = create_test_model_id(i);
            let path = PathBuf::from(format!("test_data/models/model_{}.gguf", i));
            cache.load_model(&model_id, &path).await.unwrap();
        }
        
        // Persist cache
        cache.persist().await.unwrap();
    }
    
    // Create new cache and restore
    {
        let mut config = CacheConfig::default();
        config.enable_persistence = true;
        config.persistence_path = cache_path;
        
        let cache = ModelCache::new(config).await.unwrap();
        
        // Restore from persistence
        cache.restore().await.unwrap();
        
        // Check models are restored
        for i in 0..3 {
            assert!(cache.contains(&create_test_model_id(i)).await);
        }
    }
}

#[tokio::test]
async fn test_cache_warmup() {
    let cache = create_test_cache().await.unwrap();
    
    let warmup_models = vec![
        ("llama-7b", PathBuf::from("test_data/models/llama-7b.gguf")),
        ("gpt-j-6b", PathBuf::from("test_data/models/gpt-j-6b.gguf")),
        ("bert-base", PathBuf::from("test_data/models/bert-base.onnx")),
    ];
    
    let strategy = WarmupStrategy::Parallel { max_concurrent: 2 };
    
    let warmup_result = cache
        .warmup_cache(warmup_models, strategy)
        .await
        .unwrap();
    
    assert_eq!(warmup_result.models_loaded, 3);
    assert_eq!(warmup_result.models_failed, 0);
    assert!(warmup_result.total_time_ms > 0);
    assert!(warmup_result.total_memory_gb > 0.0);
    
    // All models should be in cache
    assert!(cache.contains("llama-7b").await);
    assert!(cache.contains("gpt-j-6b").await);
    assert!(cache.contains("bert-base").await);
}

#[tokio::test]
async fn test_priority_based_eviction() {
    let mut config = CacheConfig::default();
    config.max_models = 3;
    config.eviction_policy = EvictionPolicy::Priority;
    
    let cache = ModelCache::new(config).await.unwrap();
    
    // Load models with different priorities
    let models = vec![
        ("model_low", CachePriority::Low),
        ("model_normal", CachePriority::Normal),
        ("model_high", CachePriority::High),
        ("model_critical", CachePriority::Critical),
    ];
    
    for (model_id, priority) in models {
        let path = PathBuf::from(format!("test_data/models/{}.gguf", model_id));
        cache
            .load_model_with_priority(model_id, &path, priority)
            .await
            .unwrap();
    }
    
    // Low priority should be evicted first
    assert!(!cache.contains("model_low").await);
    assert!(cache.contains("model_normal").await);
    assert!(cache.contains("model_high").await);
    assert!(cache.contains("model_critical").await);
}

#[tokio::test]
async fn test_cache_metrics() {
    let cache = create_test_cache().await.unwrap();
    
    // Perform various operations
    for i in 0..5 {
        let model_id = create_test_model_id(i);
        let path = PathBuf::from(format!("test_data/models/model_{}.gguf", i));
        cache.load_model(&model_id, &path).await.unwrap();
    }
    
    // Some cache hits
    for i in 0..3 {
        cache.get_model(&create_test_model_id(i)).await.unwrap();
    }
    
    // Some cache misses
    for i in 5..7 {
        let _ = cache.get_model(&create_test_model_id(i)).await;
    }
    
    let metrics = cache.get_metrics().await;
    
    assert_eq!(metrics.total_models, 5);
    assert!(metrics.memory_usage_gb > 0.0);
    assert_eq!(metrics.cache_hits, 3);
    assert_eq!(metrics.cache_misses, 2);
    assert!(metrics.hit_rate > 0.0 && metrics.hit_rate < 1.0);
    assert_eq!(metrics.evictions, 0); // No evictions yet
    assert!(metrics.avg_load_time_ms > 0.0);
}

#[tokio::test]
async fn test_concurrent_access() {
    let cache = Arc::new(create_test_cache().await.unwrap());
    
    // Load initial model
    let model_id = "shared_model";
    let path = PathBuf::from("test_data/models/shared_model.gguf");
    cache.load_model(model_id, &path).await.unwrap();
    
    // Spawn multiple tasks accessing the same model
    let mut handles = vec![];
    
    for i in 0..10 {
        let cache_clone = Arc::clone(&cache);
        let handle = tokio::spawn(async move {
            let model_handle = cache_clone.get_model("shared_model").await.unwrap();
            // Simulate some work
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            assert!(model_handle.is_loaded());
            i
        });
        handles.push(handle);
    }
    
    // Wait for all tasks
    let results: Vec<_> = futures::future::join_all(handles).await;
    
    assert_eq!(results.len(), 10);
    for result in results {
        assert!(result.is_ok());
    }
}

#[tokio::test]
async fn test_model_update_in_cache() {
    let cache = create_test_cache().await.unwrap();
    
    let model_id = "updatable_model";
    let old_path = PathBuf::from("test_data/models/model_v1.gguf");
    let new_path = PathBuf::from("test_data/models/model_v2.gguf");
    
    // Load initial version
    let handle_v1 = cache.load_model(model_id, &old_path).await.unwrap();
    let checksum_v1 = handle_v1.checksum();
    
    // Update to new version
    let handle_v2 = cache.update_model(model_id, &new_path).await.unwrap();
    let checksum_v2 = handle_v2.checksum();
    
    assert_ne!(checksum_v1, checksum_v2);
    assert_eq!(handle_v2.model_id(), model_id);
    assert!(handle_v2.version() > handle_v1.version());
}

#[tokio::test]
async fn test_cache_compression() {
    let mut config = CacheConfig::default();
    config.compression_enabled = true;
    
    let cache = ModelCache::new(config).await.unwrap();
    
    let model_id = "compressible_model";
    let path = PathBuf::from("test_data/models/compressible.gguf");
    
    // Load with compression
    let handle = cache.load_model(model_id, &path).await.unwrap();
    
    let metrics = cache.get_model_metrics(model_id).await.unwrap();
    
    assert!(metrics.compressed);
    assert!(metrics.compressed_size_bytes < metrics.original_size_bytes);
    assert!(metrics.compression_ratio > 1.0);
    assert!(handle.is_loaded()); // Should still be usable
}

#[tokio::test]
async fn test_cache_event_stream() {
    let cache = create_test_cache().await.unwrap();
    
    // Subscribe to cache events
    let mut event_stream = cache.subscribe_events().await;
    
    // Trigger some events
    let model_id = "event_test_model";
    let path = PathBuf::from("test_data/models/event_test.gguf");
    
    // Load model
    cache.load_model(model_id, &path).await.unwrap();
    
    // Access model
    cache.get_model(model_id).await.unwrap();
    
    // Evict model
    cache.evict_model(model_id).await.unwrap();
    
    // Collect events
    let mut events = Vec::new();
    while let Ok(Some(event)) = tokio::time::timeout(
        tokio::time::Duration::from_millis(100),
        event_stream.recv()
    ).await {
        events.push(event);
    }
    
    // Verify events
    assert!(events.iter().any(|e| matches!(e, CacheEvent::ModelLoaded { .. })));
    assert!(events.iter().any(|e| matches!(e, CacheEvent::ModelAccessed { .. })));
    assert!(events.iter().any(|e| matches!(e, CacheEvent::ModelEvicted { .. })));
}

#[tokio::test]
async fn test_memory_pressure_handling() {
    let mut config = CacheConfig::default();
    config.max_memory_gb = 8;
    config.min_free_memory_gb = 2;
    
    let cache = ModelCache::new(config).await.unwrap();
    
    // Simulate memory pressure
    cache.simulate_memory_pressure(6.5).await.unwrap(); // 6.5GB used
    
    // Try to load a large model
    let model_id = "large_model";
    let path = PathBuf::from("test_data/models/large_model.gguf");
    
    let result = cache.load_model(model_id, &path).await;
    
    // Should either succeed after evicting others, or fail gracefully
    match result {
        Ok(handle) => {
            assert!(handle.is_loaded());
            let metrics = cache.get_metrics().await;
            assert!(metrics.memory_usage_gb + 2.0 <= 8.0); // Respects min free memory
        }
        Err(e) => {
            match e.downcast_ref::<CacheError>() {
                Some(CacheError::InsufficientMemory { .. }) => {
                    // Expected if model is too large
                }
                _ => panic!("Unexpected error: {:?}", e),
            }
        }
    }
}