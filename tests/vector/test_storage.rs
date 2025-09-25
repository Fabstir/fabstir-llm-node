use chrono::{Duration, Utc};
use fabstir_llm_node::vector::{
    IndexType, MigrationConfig, MigrationStatus, MigrationStatusType, S5Backend, S5Client,
    S5Storage, S5StorageConfig, StorageBackend, StorageMetadata, StorageStats, VectorEntry,
    VectorId, VectorStorage, VectorStorageConfig, VectorStorageError as StorageError,
};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    async fn create_test_storage() -> Result<VectorStorage, StorageError> {
        // Create S5 storage backend
        let s5_config = match std::env::var("TEST_S5_BACKEND").as_deref() {
            Ok("real") => S5StorageConfig {
                backend: S5Backend::Real {
                    portal_url: std::env::var("S5_PORTAL_URL")
                        .unwrap_or_else(|_| "http://localhost:5522".to_string()),
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

        // Create vector storage config
        let config = VectorStorageConfig {
            backend: StorageBackend::S5(s5_client),
            base_path: "home/vectors".to_string(),
            chunk_size_bytes: 1024 * 1024, // 1MB chunks
            compression_enabled: true,
            index_type: IndexType::Hybrid,
            recent_threshold_hours: 24,
            migration_config: MigrationConfig {
                enabled: true,
                batch_size: 100,
                check_interval_seconds: 3600,
            },
        };

        Ok(VectorStorage::new(config).await?)
    }

    fn create_test_vector_entry(id: &str) -> VectorEntry {
        VectorEntry {
            id: id.to_string(),
            vector: vec![0.1; 384], // 384-dimensional vector
            metadata: HashMap::from([
                ("created_at".to_string(), Utc::now().to_string()),
                ("model".to_string(), "test-model".to_string()),
            ]),
        }
    }

    #[tokio::test]
    async fn test_store_and_retrieve_vector() {
        let storage = create_test_storage().await.unwrap();

        let entry = create_test_vector_entry("test_vec_001");

        // Store vector
        let result = storage.store_vector(&entry).await.unwrap();
        assert_eq!(result.vector_id, "test_vec_001");
        assert!(!result.storage_path.is_empty());
        assert!(result.index_type == IndexType::Recent || result.index_type == IndexType::Mock);

        // Retrieve vector
        let retrieved = storage.get_vector("test_vec_001").await.unwrap();
        assert_eq!(retrieved.id, entry.id);
        assert_eq!(retrieved.vector, entry.vector);
        assert_eq!(retrieved.metadata.get("model"), entry.metadata.get("model"));
    }

    #[tokio::test]
    async fn test_batch_vector_storage() {
        let storage = create_test_storage().await.unwrap();

        // Create batch of vectors
        let vectors: Vec<VectorEntry> = (0..50)
            .map(|i| {
                let mut entry = create_test_vector_entry(&format!("batch_vec_{:03}", i));
                entry
                    .metadata
                    .insert("batch_index".to_string(), i.to_string());
                entry
            })
            .collect();

        // Store batch
        let results = storage.store_batch(vectors.clone()).await.unwrap();
        assert_eq!(results.successful, 50);
        assert_eq!(results.failed, 0);

        // Retrieve random vectors from batch
        for i in [0, 10, 25, 49] {
            let id = format!("batch_vec_{:03}", i);
            let retrieved = storage.get_vector(&id).await.unwrap();
            assert_eq!(retrieved.metadata.get("batch_index"), Some(&i.to_string()));
        }
    }

    #[tokio::test]
    async fn test_vector_chunking() {
        let storage = create_test_storage().await.unwrap();

        // Create large vector that will be chunked
        let mut entry = create_test_vector_entry("large_vec");
        entry.vector = vec![0.5; 500_000]; // ~2MB of float data

        let result = storage.store_vector(&entry).await.unwrap();

        // Check chunking info
        let chunk_info = storage.get_chunk_info("large_vec").await.unwrap();
        assert!(chunk_info.total_chunks > 1);
        assert_eq!(chunk_info.chunk_size_bytes, 1024 * 1024);

        // Retrieve and verify
        let retrieved = storage.get_vector("large_vec").await.unwrap();
        assert_eq!(retrieved.vector.len(), entry.vector.len());
        assert_eq!(retrieved.vector[0], 0.5);
        assert_eq!(retrieved.vector[retrieved.vector.len() - 1], 0.5);
    }

    #[tokio::test]
    async fn test_index_migration() {
        let storage = create_test_storage().await.unwrap();

        // Create old vector (should go to historical index)
        let mut old_entry = create_test_vector_entry("old_vec");
        old_entry.metadata.insert(
            "created_at".to_string(),
            (Utc::now() - Duration::days(7)).to_string(),
        );

        // Create recent vector
        let recent_entry = create_test_vector_entry("recent_vec");

        // Store both
        let old_result = storage.store_vector(&old_entry).await.unwrap();
        let recent_result = storage.store_vector(&recent_entry).await.unwrap();

        // Check index assignment
        assert_eq!(old_result.index_type, IndexType::Historical);
        assert_eq!(recent_result.index_type, IndexType::Recent);

        // Trigger migration check
        let migration_status = storage.check_and_migrate().await.unwrap();

        assert!(migration_status.vectors_checked > 0);
        // If any were migrated, verify the count
        if migration_status.vectors_migrated > 0 {
            assert_eq!(migration_status.status, MigrationStatusType::Completed);
        }
    }

    #[tokio::test]
    async fn test_storage_paths() {
        let storage = create_test_storage().await.unwrap();

        // Store vectors with custom paths
        let categories = vec!["videos", "documents", "images"];

        for category in categories {
            for i in 0..3 {
                let id = format!("{}_{}", category, i);
                let mut entry = create_test_vector_entry(&id);
                entry
                    .metadata
                    .insert("category".to_string(), category.to_string());

                let custom_path = format!("home/vectors/{}/{}", category, id);
                storage
                    .store_vector_with_path(&entry, &custom_path)
                    .await
                    .unwrap();
            }
        }

        // List vectors by category
        for category in ["videos", "documents", "images"] {
            let path = format!("home/vectors/{}", category);
            let entries = storage.list_vectors_at_path(&path).await.unwrap();
            assert_eq!(entries.len(), 3);

            for entry in entries {
                assert!(entry.id.starts_with(category));
            }
        }
    }

    #[tokio::test]
    async fn test_metadata_indexing() {
        let storage = create_test_storage().await.unwrap();

        // Store vectors with various metadata
        for i in 0..20 {
            let mut entry = create_test_vector_entry(&format!("meta_vec_{}", i));
            entry
                .metadata
                .insert("score".to_string(), (i * 10).to_string());
            entry.metadata.insert(
                "category".to_string(),
                if i % 2 == 0 { "even" } else { "odd" }.to_string(),
            );
            entry.metadata.insert(
                "tags".to_string(),
                serde_json::to_string(&vec![format!("tag_{}", i % 3), format!("tag_{}", i % 5)])
                    .unwrap(),
            );

            storage.store_vector(&entry).await.unwrap();
        }

        // Query by metadata
        let filter = HashMap::from([("category".to_string(), "even".to_string())]);

        let results = storage.query_by_metadata(filter).await.unwrap();
        assert_eq!(results.len(), 10); // Half are even

        for entry in results {
            assert_eq!(entry.metadata.get("category"), Some(&"even".to_string()));
        }
    }

    #[tokio::test]
    async fn test_storage_statistics() {
        let storage = create_test_storage().await.unwrap();

        // Get initial stats
        let initial_stats = storage.get_stats().await.unwrap();

        // Store various vectors
        for i in 0..10 {
            let entry = create_test_vector_entry(&format!("stats_vec_{}", i));
            storage.store_vector(&entry).await.unwrap();
        }

        // Get updated stats
        let updated_stats = storage.get_stats().await.unwrap();

        assert_eq!(
            updated_stats.total_vectors,
            initial_stats.total_vectors + 10
        );
        assert!(updated_stats.total_size_bytes > initial_stats.total_size_bytes);
        assert!(updated_stats.recent_index_size >= 10);
        assert!(updated_stats.compression_ratio > 0.0);
    }

    #[tokio::test]
    async fn test_vector_deletion() {
        let storage = create_test_storage().await.unwrap();

        // Store vectors
        let ids = vec!["del_vec_1", "del_vec_2", "del_vec_3"];
        for id in &ids {
            let entry = create_test_vector_entry(id);
            storage.store_vector(&entry).await.unwrap();
        }

        // Delete one vector
        storage.delete_vector("del_vec_2").await.unwrap();

        // Verify deletion
        assert!(storage.vector_exists("del_vec_1").await.unwrap());
        assert!(!storage.vector_exists("del_vec_2").await.unwrap());
        assert!(storage.vector_exists("del_vec_3").await.unwrap());

        // Batch delete
        storage
            .delete_batch(&["del_vec_1", "del_vec_3"])
            .await
            .unwrap();

        // All should be gone
        for id in ids {
            assert!(!storage.vector_exists(id).await.unwrap());
        }
    }

    #[tokio::test]
    async fn test_storage_backup_restore() {
        let storage = create_test_storage().await.unwrap();

        // Store test data
        let vectors: Vec<VectorEntry> = (0..5)
            .map(|i| create_test_vector_entry(&format!("backup_vec_{}", i)))
            .collect();

        for vector in &vectors {
            storage.store_vector(vector).await.unwrap();
        }

        // Create backup
        let backup_path = "home/vectors/backups/test_backup";
        let backup_result = storage.create_backup(backup_path).await.unwrap();

        assert!(backup_result.success);
        assert_eq!(backup_result.vectors_backed_up, 5);
        assert!(!backup_result.backup_id.is_empty());

        // Clear storage
        for i in 0..5 {
            storage
                .delete_vector(&format!("backup_vec_{}", i))
                .await
                .unwrap();
        }

        // Restore from backup
        let restore_result = storage
            .restore_from_backup(&backup_result.backup_id)
            .await
            .unwrap();

        assert!(restore_result.success);
        assert_eq!(restore_result.vectors_restored, 5);

        // Verify restoration
        for i in 0..5 {
            assert!(storage
                .vector_exists(&format!("backup_vec_{}", i))
                .await
                .unwrap());
        }
    }

    #[tokio::test]
    async fn test_concurrent_storage_operations() {
        let storage = create_test_storage().await.unwrap();

        // Spawn multiple concurrent operations
        let mut handles = vec![];

        // Writers
        for i in 0..5 {
            let storage_clone = storage.clone();
            let handle = tokio::spawn(async move {
                for j in 0..10 {
                    let entry = create_test_vector_entry(&format!("concurrent_{}_{}", i, j));
                    storage_clone.store_vector(&entry).await.unwrap();
                }
            });
            handles.push(handle);
        }

        // Readers
        for i in 0..5 {
            let storage_clone = storage.clone();
            let handle = tokio::spawn(async move {
                // Try to read vectors that might not exist yet
                for j in 0..10 {
                    let id = format!("concurrent_{}_{}", i, j);
                    let _ = storage_clone.vector_exists(&id).await;
                }
            });
            handles.push(handle);
        }

        // Wait for all operations
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all vectors were stored
        let stats = storage.get_stats().await.unwrap();
        assert!(stats.total_vectors >= 50);
    }

    #[tokio::test]
    async fn test_compression_effectiveness() {
        let storage = create_test_storage().await.unwrap();

        // Create vector with repetitive pattern (compresses well)
        let mut entry = create_test_vector_entry("compress_test");
        entry.vector = vec![0.123456789; 10_000]; // Highly compressible

        let result = storage.store_vector(&entry).await.unwrap();

        // Get compression info
        let info = storage
            .get_compression_info(&result.vector_id)
            .await
            .unwrap();

        assert!(info.compressed_size < info.original_size);
        assert!(info.compression_ratio > 5.0); // Should compress very well
        assert_eq!(info.compression_type, "zstd");

        // Verify decompression
        let retrieved = storage.get_vector("compress_test").await.unwrap();
        assert_eq!(retrieved.vector.len(), 10_000);
        assert!((retrieved.vector[0] - 0.123456789).abs() < 1e-9);
    }
}
