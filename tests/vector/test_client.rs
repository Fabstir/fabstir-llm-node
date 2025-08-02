use fabstir_llm_node::vector::{
    VectorDBClient, VectorDBConfig, VectorBackend, VectorId,
    VectorEntry, SearchOptions, SearchResult, VectorError,
    FilterOperator, FilterValue, VectorStats
};
use std::collections::HashMap;
use futures::StreamExt;

#[cfg(test)]
mod tests {
    use super::*;
    
    // Helper to create client based on environment
    async fn create_test_client() -> Result<VectorDBClient, VectorError> {
        let config = match std::env::var("TEST_VECTOR_BACKEND").as_deref() {
            Ok("real") => {
                let api_url = std::env::var("VECTOR_DB_URL")
                    .unwrap_or_else(|_| "http://localhost:7530".to_string());
                let api_key = std::env::var("VECTOR_DB_API_KEY").ok();
                
                VectorDBConfig {
                    backend: VectorBackend::Real { api_url: api_url.clone() },
                    api_key,
                    timeout_ms: 5000,
                    max_retries: 3,
                }
            }
            _ => VectorDBConfig {
                backend: VectorBackend::Mock,
                api_key: None,
                timeout_ms: 5000,
                max_retries: 3,
            }
        };
        
        VectorDBClient::new(config).await
    }
    
    fn create_test_vector(id: &str, dim: usize) -> VectorEntry {
        let vector: Vec<f32> = (0..dim)
            .map(|i| (i as f32 * 0.1).sin())
            .collect();
        
        VectorEntry {
            id: id.to_string(),
            vector,
            metadata: HashMap::from([
                ("model".to_string(), "llama-3.2-1b".to_string()),
                ("timestamp".to_string(), chrono::Utc::now().to_string()),
            ]),
        }
    }

    #[tokio::test]
    async fn test_client_health_check() {
        let client = create_test_client().await.unwrap();
        
        let health = client.health().await.unwrap();
        
        assert_eq!(health.status, "ok");
        assert!(health.version.len() > 0);
        assert!(health.total_vectors >= 0);
        assert!(health.indices.contains_key("recent") || health.indices.contains_key("mock"));
    }

    #[tokio::test]
    async fn test_insert_single_vector() {
        let client = create_test_client().await.unwrap();
        
        let vector = create_test_vector("test_vec_001", 384);
        
        let result = client.insert_vector(vector.clone()).await.unwrap();
        
        assert_eq!(result.id, "test_vec_001");
        assert!(result.index == "recent" || result.index == "mock");
        assert!(result.timestamp > 0);
    }

    #[tokio::test]
    async fn test_batch_insert() {
        let client = create_test_client().await.unwrap();
        
        let vectors: Vec<VectorEntry> = (0..10)
            .map(|i| create_test_vector(&format!("batch_vec_{:03}", i), 384))
            .collect();
        
        let result = client.batch_insert(vectors.clone()).await.unwrap();
        
        assert_eq!(result.successful, 10);
        assert_eq!(result.failed, 0);
        assert!(result.errors.is_empty());
    }

    #[tokio::test]
    async fn test_get_vector_by_id() {
        let client = create_test_client().await.unwrap();
        
        let vector = create_test_vector("get_test_vec", 384);
        client.insert_vector(vector.clone()).await.unwrap();
        
        let retrieved = client.get_vector("get_test_vec").await.unwrap();
        
        assert_eq!(retrieved.id, "get_test_vec");
        assert_eq!(retrieved.vector.len(), 384);
        assert_eq!(retrieved.metadata.get("model"), vector.metadata.get("model"));
    }

    #[tokio::test]
    async fn test_delete_vector() {
        let client = create_test_client().await.unwrap();
        
        let vector = create_test_vector("delete_test_vec", 384);
        client.insert_vector(vector).await.unwrap();
        
        // Verify it exists
        let exists = client.vector_exists("delete_test_vec").await.unwrap();
        assert!(exists);
        
        // Delete it
        client.delete_vector("delete_test_vec").await.unwrap();
        
        // Verify it's gone
        let exists = client.vector_exists("delete_test_vec").await.unwrap();
        assert!(!exists);
    }

    #[tokio::test]
    async fn test_basic_search() {
        let client = create_test_client().await.unwrap();
        
        // Insert test vectors
        for i in 0..5 {
            let mut vector = create_test_vector(&format!("search_vec_{}", i), 384);
            vector.metadata.insert("index".to_string(), i.to_string());
            client.insert_vector(vector).await.unwrap();
        }
        
        // Create query vector similar to first one
        let query_vector: Vec<f32> = (0..384)
            .map(|i| (i as f32 * 0.1).sin() + 0.01) // Slightly different
            .collect();
        
        let results = client.search(query_vector, 3).await.unwrap();
        
        assert!(results.len() <= 3);
        assert!(results.len() > 0);
        
        // Results should be ordered by distance
        for i in 1..results.len() {
            assert!(results[i].distance >= results[i-1].distance);
        }
    }

    #[tokio::test]
    async fn test_search_with_options() {
        let client = create_test_client().await.unwrap();
        
        // Insert vectors with different metadata
        for i in 0..10 {
            let mut vector = create_test_vector(&format!("option_vec_{}", i), 384);
            vector.metadata.insert("category".to_string(), if i < 5 { "A" } else { "B" }.to_string());
            vector.metadata.insert("score".to_string(), (i * 10).to_string());
            client.insert_vector(vector).await.unwrap();
        }
        
        let query_vector = vec![0.1; 384];
        
        let options = SearchOptions {
            k: 5,
            search_recent: true,
            search_historical: false,
            hnsw_ef: Some(100),
            ivf_n_probe: Some(32),
            timeout_ms: Some(2000),
            include_metadata: true,
            score_threshold: Some(0.5),
            filter: Some(HashMap::from([
                ("category".to_string(), FilterValue::String("A".to_string())),
            ])),
        };
        
        let results = client.search_with_options(query_vector, options).await.unwrap();
        
        // All results should be from category A
        for result in &results {
            assert_eq!(
                result.metadata.get("category"),
                Some(&"A".to_string())
            );
        }
    }

    #[tokio::test]
    async fn test_complex_metadata_filter() {
        let client = create_test_client().await.unwrap();
        
        // Insert vectors with complex metadata
        let tags = vec!["ai", "ml", "deep-learning", "tutorial", "beginner"];
        
        for i in 0..20 {
            let mut vector = create_test_vector(&format!("filter_vec_{}", i), 384);
            
            // Random tags
            let vec_tags: Vec<String> = tags.iter()
                .take((i % 3) + 1)
                .map(|s| s.to_string())
                .collect();
            
            vector.metadata.insert("tags".to_string(), serde_json::to_string(&vec_tags).unwrap());
            vector.metadata.insert("duration_seconds".to_string(), ((i + 1) * 100).to_string());
            vector.metadata.insert("views".to_string(), ((i + 1) * 1000).to_string());
            
            client.insert_vector(vector).await.unwrap();
        }
        
        // Complex filter query
        let filter = HashMap::from([
            ("tags".to_string(), FilterValue::Array(vec!["ai".to_string(), "ml".to_string()])),
            ("duration_seconds".to_string(), FilterValue::Range {
                min: Some(300.0),
                max: Some(1500.0),
            }),
        ]);
        
        let options = SearchOptions {
            k: 10,
            filter: Some(filter),
            include_metadata: true,
            ..Default::default()
        };
        
        let query_vector = vec![0.2; 384];
        let results = client.search_with_options(query_vector, options).await.unwrap();
        
        // Verify all results match filter criteria
        for result in results {
            let duration: i32 = result.metadata.get("duration_seconds")
                .and_then(|s| s.parse().ok())
                .unwrap();
            assert!(duration >= 300 && duration <= 1500);
        }
    }

    #[tokio::test]
    async fn test_vector_statistics() {
        let client = create_test_client().await.unwrap();
        
        // Get initial stats
        let initial_stats = client.get_stats().await.unwrap();
        let initial_count = initial_stats.total_vectors;
        
        // Insert some vectors
        for i in 0..5 {
            let vector = create_test_vector(&format!("stats_vec_{}", i), 384);
            client.insert_vector(vector).await.unwrap();
        }
        
        // Get updated stats
        let updated_stats = client.get_stats().await.unwrap();
        
        assert_eq!(updated_stats.total_vectors, initial_count + 5);
        assert!(updated_stats.recent_vectors >= 5);
        assert!(updated_stats.indices_count > 0);
        assert!(updated_stats.total_size_bytes > 0);
    }

    #[tokio::test]
    async fn test_concurrent_operations() {
        let client = create_test_client().await.unwrap();
        
        // Spawn multiple concurrent operations
        let mut handles = vec![];
        
        for i in 0..10 {
            let client_clone = client.clone();
            let handle = tokio::spawn(async move {
                let vector = create_test_vector(&format!("concurrent_vec_{}", i), 384);
                client_clone.insert_vector(vector).await.unwrap();
                
                // Also perform a search
                let query = vec![0.1; 384];
                let results = client_clone.search(query, 5).await.unwrap();
                assert!(results.len() <= 5);
            });
            handles.push(handle);
        }
        
        // Wait for all to complete
        for handle in handles {
            handle.await.unwrap();
        }
        
        // Verify all vectors were inserted
        for i in 0..10 {
            let exists = client.vector_exists(&format!("concurrent_vec_{}", i)).await.unwrap();
            assert!(exists);
        }
    }

    #[tokio::test]
    async fn test_streaming_updates() {
        let client = create_test_client().await.unwrap();
        
        // Subscribe to updates
        let mut update_stream = client.subscribe_updates().await.unwrap();
        
        // Insert a vector in another task
        let client_clone = client.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let vector = create_test_vector("stream_test_vec", 384);
            client_clone.insert_vector(vector).await.unwrap();
        });
        
        // Wait for update event
        let update = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            update_stream.next()
        ).await;
        
        assert!(update.is_ok());
        let event = update.unwrap().unwrap().unwrap();
        assert_eq!(event.event_type, "vector_added");
        assert_eq!(event.vector_id, "stream_test_vec");
    }

    // Mock-specific tests
    #[cfg(not(feature = "integration"))]
    mod mock_tests {
        use super::*;
        
        #[tokio::test]
        async fn test_mock_search_performance() {
            let config = VectorDBConfig {
                backend: VectorBackend::Mock,
                api_key: None,
                timeout_ms: 5000,
                max_retries: 3,
            };
            
            let client = VectorDBClient::new(config).await.unwrap();
            
            // Insert many vectors
            for i in 0..1000 {
                let vector = create_test_vector(&format!("perf_vec_{}", i), 384);
                client.insert_vector(vector).await.unwrap();
            }
            
            // Measure search time
            let query = vec![0.5; 384];
            let start = std::time::Instant::now();
            let results = client.search(query, 10).await.unwrap();
            let duration = start.elapsed();
            
            assert_eq!(results.len(), 10);
            assert!(duration.as_millis() < 50); // Should be fast in mock mode
        }
    }
}