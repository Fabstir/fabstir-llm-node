use chrono::{Duration, Utc};
use fabstir_llm_node::storage::{
    CacheConfig, CacheEntry, CacheStats, EvictionPolicy, ResultCache, S5Backend, S5Client,
    S5Storage, S5StorageConfig, StorageError,
};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_test_cache() -> Result<ResultCache, StorageError> {
        let s5_config = match std::env::var("TEST_S5_BACKEND").as_deref() {
            Ok("real") => S5StorageConfig {
                backend: S5Backend::Real {
                    portal_url: std::env::var("S5_PORTAL_URL")
                        .unwrap_or_else(|_| "https://s5.vup.cx".to_string()),
                },
                api_key: std::env::var("S5_API_KEY").ok(),
                cache_ttl_seconds: 300,
                max_retries: 3,
            },
            _ => S5StorageConfig {
                backend: S5Backend::Mock,
                api_key: None,
                cache_ttl_seconds: 300,
                max_retries: 3,
            },
        };

        let s5_client = S5Client::create(s5_config).await?;
        let config = CacheConfig {
            base_path: "home/cache/results".to_string(),
            max_size_mb: 1000,
            ttl_seconds: 3600,
            eviction_policy: EvictionPolicy::LRU,
            enable_compression: true,
        };

        Ok(ResultCache::new(s5_client, config))
    }

    fn create_cache_key(job_id: &str, prompt: &str, params: &HashMap<String, String>) -> String {
        format!("{}-{}-{:?}", job_id, prompt, params)
    }

    #[tokio::test]
    async fn test_cache_put_and_get() {
        let cache = create_test_cache().await.unwrap();

        let job_id = "job-123";
        let prompt = "What is Rust?";
        let params = HashMap::from([
            ("model".to_string(), "llama-3.2-1b".to_string()),
            ("temperature".to_string(), "0.7".to_string()),
        ]);

        let result = "Rust is a systems programming language focused on safety and performance.";
        let key = create_cache_key(job_id, prompt, &params);

        // Cache result
        cache
            .put(&key, result.as_bytes().to_vec(), Some(params.clone()))
            .await
            .unwrap();

        // Retrieve from cache
        let cached = cache.get(&key).await.unwrap();
        assert!(cached.is_some());

        let entry = cached.unwrap();
        assert_eq!(entry.data, result.as_bytes());
        assert_eq!(
            entry.metadata.get("model"),
            Some(&"llama-3.2-1b".to_string())
        );
    }

    #[tokio::test]
    async fn test_cache_expiration() {
        let cache = create_test_cache().await.unwrap();

        // Set short TTL for testing
        cache.set_ttl(1).await; // 1 second

        let key = "test-expiry";
        let data = b"temporary data".to_vec();

        cache.put(key, data.clone(), None).await.unwrap();

        // Should exist immediately
        assert!(cache.get(key).await.unwrap().is_some());

        // Wait for expiration
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Should be expired
        assert!(cache.get(key).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_cache_path_structure() {
        let cache = create_test_cache().await.unwrap();

        // Cache results for different jobs
        let jobs = vec![
            ("job-1", "prompt1", "result1"),
            ("job-2", "prompt2", "result2"),
            ("job-1", "prompt3", "result3"), // Same job, different prompt
        ];

        for (job_id, prompt, result) in jobs {
            let key = format!("{}-{}", job_id, prompt);
            cache
                .put_with_path(
                    &key,
                    result.as_bytes().to_vec(),
                    &format!("jobs/{}/{}", job_id, prompt),
                )
                .await
                .unwrap();
        }

        // List cached results by job
        let job1_results = cache.list_by_prefix("jobs/job-1").await.unwrap();
        assert_eq!(job1_results.len(), 2);

        let job2_results = cache.list_by_prefix("jobs/job-2").await.unwrap();
        assert_eq!(job2_results.len(), 1);
    }

    #[tokio::test]
    async fn test_cache_eviction_lru() {
        let cache = create_test_cache().await.unwrap();

        // Set small cache size to trigger eviction
        cache.set_max_size_mb(1).await; // 1MB limit

        // Add entries that will exceed limit
        for i in 0..10 {
            let key = format!("entry-{}", i);
            let data = vec![0u8; 200_000]; // 200KB each
            cache.put(&key, data, None).await.unwrap();

            // Small delay to ensure different access times
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }

        // Oldest entries should be evicted
        assert!(cache.get("entry-0").await.unwrap().is_none());
        assert!(cache.get("entry-1").await.unwrap().is_none());

        // Newest entries should still exist
        assert!(cache.get("entry-8").await.unwrap().is_some());
        assert!(cache.get("entry-9").await.unwrap().is_some());

        // Access entry-8 to make it recently used
        cache.get("entry-8").await.unwrap();

        // Add another large entry
        cache
            .put("entry-10", vec![0u8; 200_000], None)
            .await
            .unwrap();

        // Entry-8 should still exist (recently used)
        assert!(cache.get("entry-8").await.unwrap().is_some());
        // Entry-9 might be evicted
    }

    #[tokio::test]
    async fn test_cache_statistics() {
        let cache = create_test_cache().await.unwrap();

        // Perform various operations
        cache.put("hit1", b"data1".to_vec(), None).await.unwrap();
        cache.put("hit2", b"data2".to_vec(), None).await.unwrap();
        cache.put("miss1", b"data3".to_vec(), None).await.unwrap();

        // Hits
        assert!(cache.get("hit1").await.unwrap().is_some());
        assert!(cache.get("hit2").await.unwrap().is_some());
        assert!(cache.get("hit1").await.unwrap().is_some()); // Second hit

        // Misses
        assert!(cache.get("nonexistent").await.unwrap().is_none());
        assert!(cache.get("also-missing").await.unwrap().is_none());

        let stats = cache.get_stats().await;
        assert_eq!(stats.total_entries, 3);
        assert_eq!(stats.cache_hits, 3);
        assert_eq!(stats.cache_misses, 2);
        assert_eq!(stats.hit_rate, 0.6); // 3 hits / 5 total requests
    }

    #[tokio::test]
    async fn test_batch_cache_operations() {
        let cache = create_test_cache().await.unwrap();

        // Batch put
        let entries = vec![
            ("batch1", b"data1".to_vec()),
            ("batch2", b"data2".to_vec()),
            ("batch3", b"data3".to_vec()),
        ];

        let keys: Vec<String> = entries.iter().map(|(k, _)| k.to_string()).collect();
        let data: Vec<Vec<u8>> = entries.iter().map(|(_, d)| d.clone()).collect();

        cache.put_batch(keys.clone(), data).await.unwrap();

        // Batch get
        let results = cache.get_batch(&keys).await.unwrap();
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.is_some()));

        // Batch delete
        cache.delete_batch(&keys[0..2]).await.unwrap();

        assert!(cache.get("batch1").await.unwrap().is_none());
        assert!(cache.get("batch2").await.unwrap().is_none());
        assert!(cache.get("batch3").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_cache_compression() {
        let cache = create_test_cache().await.unwrap();

        // Create highly compressible data
        let uncompressed_size = 100_000;
        let data: Vec<u8> = vec![b'A'; uncompressed_size]; // Repeated character

        let key = "compressible";
        cache.put(key, data.clone(), None).await.unwrap();

        // Get storage info
        let info = cache.get_storage_info(key).await.unwrap();

        assert!(info.compressed_size < uncompressed_size);
        assert!(info.compression_ratio > 10.0); // Should compress very well

        // Verify decompression
        let retrieved = cache.get(key).await.unwrap().unwrap();
        assert_eq!(retrieved.data.len(), uncompressed_size);
        assert_eq!(retrieved.data, data);
    }

    #[tokio::test]
    async fn test_cache_metadata() {
        let cache = create_test_cache().await.unwrap();

        let key = "metadata-test";
        let data = b"test data".to_vec();
        let metadata = HashMap::from([
            ("model".to_string(), "llama-3.2-1b".to_string()),
            ("temperature".to_string(), "0.7".to_string()),
            ("timestamp".to_string(), Utc::now().to_string()),
            ("tokens_generated".to_string(), "150".to_string()),
        ]);

        cache
            .put(key, data.clone(), Some(metadata.clone()))
            .await
            .unwrap();

        let entry = cache.get(key).await.unwrap().unwrap();
        assert_eq!(entry.metadata.get("model"), metadata.get("model"));
        assert_eq!(
            entry.metadata.get("temperature"),
            metadata.get("temperature")
        );
        assert_eq!(
            entry.metadata.get("tokens_generated"),
            metadata.get("tokens_generated")
        );
    }

    #[tokio::test]
    async fn test_cache_clear() {
        let cache = create_test_cache().await.unwrap();

        // Add multiple entries
        for i in 0..5 {
            cache
                .put(&format!("entry-{}", i), vec![i as u8; 100], None)
                .await
                .unwrap();
        }

        let stats = cache.get_stats().await;
        assert_eq!(stats.total_entries, 5);

        // Clear cache
        cache.clear().await.unwrap();

        let stats = cache.get_stats().await;
        assert_eq!(stats.total_entries, 0);

        // Verify all entries are gone
        for i in 0..5 {
            assert!(cache.get(&format!("entry-{}", i)).await.unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn test_concurrent_cache_access() {
        let cache = create_test_cache().await.unwrap();

        // Spawn multiple tasks accessing cache
        let mut handles = vec![];

        for i in 0..10 {
            let cache_clone = cache.clone();
            let handle = tokio::spawn(async move {
                let key = format!("concurrent-{}", i % 3); // Some key collision
                let data = format!("data-{}", i).into_bytes();

                // Put and get multiple times
                for _ in 0..5 {
                    cache_clone.put(&key, data.clone(), None).await.unwrap();
                    let _ = cache_clone.get(&key).await.unwrap();
                }
            });
            handles.push(handle);
        }

        // Wait for all tasks
        for handle in handles {
            handle.await.unwrap();
        }

        // Cache should be in consistent state
        let stats = cache.get_stats().await;
        assert!(stats.total_entries <= 3); // At most 3 unique keys
    }
}
