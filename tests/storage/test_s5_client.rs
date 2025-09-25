use fabstir_llm_node::storage::{
    S5Backend, S5Client, S5ClientConfig, S5Entry, S5EntryType, S5Metadata, S5Storage,
    S5StorageConfig, StorageError,
};
use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create storage based on environment or default to mock
    async fn create_test_storage() -> Result<Box<dyn S5Storage>, StorageError> {
        let config = match std::env::var("TEST_S5_BACKEND").as_deref() {
            Ok("real") => {
                let portal_url = std::env::var("S5_PORTAL_URL")
                    .unwrap_or_else(|_| "https://s5.vup.cx".to_string());
                let api_key = std::env::var("S5_API_KEY").ok();

                S5StorageConfig {
                    backend: S5Backend::Real {
                        portal_url: portal_url.clone(),
                    },
                    api_key,
                    cache_ttl_seconds: 300,
                    max_retries: 3,
                }
            }
            _ => S5StorageConfig {
                backend: S5Backend::Mock,
                api_key: None,
                cache_ttl_seconds: 300,
                max_retries: 3,
            },
        };

        S5Client::create(config).await
    }

    #[tokio::test]
    async fn test_put_and_get_basic() {
        let storage = create_test_storage().await.unwrap();

        let data = b"Hello, S5 Storage!".to_vec();
        let path = "home/test/hello.txt";

        // Put data
        let cid = storage.put(path, data.clone()).await.unwrap();
        assert!(!cid.is_empty());
        assert!(cid.starts_with("s5://") || cid.starts_with("bafy")); // Mock or real CID

        // Get data back
        let retrieved = storage.get(path).await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn test_put_with_metadata() {
        let storage = create_test_storage().await.unwrap();

        let data = b"Content with metadata".to_vec();
        let path = "home/docs/metadata.json";

        let mut metadata = HashMap::new();
        metadata.insert("content-type".to_string(), "application/json".to_string());
        metadata.insert("author".to_string(), "test-user".to_string());

        let cid = storage
            .put_with_metadata(path, data.clone(), metadata.clone())
            .await
            .unwrap();
        assert!(!cid.is_empty());

        // Retrieve and verify metadata
        let retrieved_metadata = storage.get_metadata(path).await.unwrap();
        assert_eq!(
            retrieved_metadata.get("content-type"),
            metadata.get("content-type")
        );
        assert_eq!(retrieved_metadata.get("author"), metadata.get("author"));
    }

    #[tokio::test]
    async fn test_list_directory() {
        let storage = create_test_storage().await.unwrap();

        // Create test structure
        let files = vec![
            ("home/models/llama-3.2-1b.gguf", "model1".as_bytes()),
            ("home/models/mistral-7b.gguf", "model2".as_bytes()),
            ("home/models/configs/llama.json", "config1".as_bytes()),
            ("home/models/configs/mistral.json", "config2".as_bytes()),
        ];

        for (path, data) in files {
            storage.put(path, data.to_vec()).await.unwrap();
        }

        // List root models directory
        let entries = storage.list("home/models").await.unwrap();
        assert!(entries.len() >= 2);

        let file_names: Vec<String> = entries
            .iter()
            .filter(|e| e.entry_type == S5EntryType::File)
            .map(|e| e.name.clone())
            .collect();

        assert!(file_names.contains(&"llama-3.2-1b.gguf".to_string()));
        assert!(file_names.contains(&"mistral-7b.gguf".to_string()));

        // Check for subdirectory
        let has_configs_dir = entries
            .iter()
            .any(|e| e.name == "configs" && e.entry_type == S5EntryType::Directory);
        assert!(has_configs_dir);
    }

    #[tokio::test]
    async fn test_list_with_pagination() {
        let storage = create_test_storage().await.unwrap();

        // Create many files
        for i in 0..25 {
            let path = format!("home/pagination/file_{:03}.txt", i);
            storage
                .put(&path, format!("content {}", i).into_bytes())
                .await
                .unwrap();
        }

        // List with limit
        let page1 = storage
            .list_with_options("home/pagination", Some(10), None)
            .await
            .unwrap();
        assert_eq!(page1.entries.len(), 10);
        assert!(page1.cursor.is_some());

        // Get next page
        let page2 = storage
            .list_with_options("home/pagination", Some(10), page1.cursor)
            .await
            .unwrap();
        assert_eq!(page2.entries.len(), 10);

        // Verify different entries
        let page1_names: Vec<_> = page1.entries.iter().map(|e| &e.name).collect();
        let page2_names: Vec<_> = page2.entries.iter().map(|e| &e.name).collect();
        assert!(page1_names.iter().all(|name| !page2_names.contains(name)));
    }

    #[tokio::test]
    async fn test_delete() {
        let storage = create_test_storage().await.unwrap();

        let path = "home/temp/delete_me.txt";
        let data = b"temporary file".to_vec();

        // Create file
        storage.put(path, data).await.unwrap();

        // Verify it exists
        let exists = storage.exists(path).await.unwrap();
        assert!(exists);

        // Delete it
        storage.delete(path).await.unwrap();

        // Verify it's gone
        let exists = storage.exists(path).await.unwrap();
        assert!(!exists);

        // Getting deleted file should return None or error
        let result = storage.get(path).await;
        assert!(result.is_err() || result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_nested_directory_creation() {
        let storage = create_test_storage().await.unwrap();

        // Put file in deeply nested path
        let deep_path = "home/a/b/c/d/e/f/deep.txt";
        let data = b"deep content".to_vec();

        storage.put(deep_path, data.clone()).await.unwrap();

        // Verify intermediate directories exist
        assert!(storage.exists("home/a").await.unwrap());
        assert!(storage.exists("home/a/b").await.unwrap());
        assert!(storage.exists("home/a/b/c").await.unwrap());

        // List parent directory
        let entries = storage.list("home/a/b/c/d/e/f").await.unwrap();
        assert!(entries.iter().any(|e| e.name == "deep.txt"));
    }

    #[tokio::test]
    async fn test_large_file_handling() {
        let storage = create_test_storage().await.unwrap();

        // Create 10MB file
        let large_data: Vec<u8> = (0..10_485_760).map(|i| (i % 256) as u8).collect();
        let path = "home/large/bigfile.bin";

        let cid = storage.put(path, large_data.clone()).await.unwrap();
        assert!(!cid.is_empty());

        // Retrieve and verify
        let retrieved = storage.get(path).await.unwrap();
        assert_eq!(retrieved.len(), large_data.len());
        assert_eq!(retrieved[0..1000], large_data[0..1000]);
        assert_eq!(
            retrieved[retrieved.len() - 1000..],
            large_data[large_data.len() - 1000..]
        );
    }

    #[tokio::test]
    async fn test_special_characters_in_paths() {
        let storage = create_test_storage().await.unwrap();

        let test_paths = vec![
            "home/files/hello world.txt",
            "home/files/test-file_2024.json",
            "home/files/data (1).csv",
            "home/files/résumé.pdf",
        ];

        for path in test_paths {
            let data = format!("content for {}", path).into_bytes();

            let result = storage.put(path, data.clone()).await;
            assert!(result.is_ok(), "Failed to store {}: {:?}", path, result);

            let retrieved = storage.get(path).await;
            assert!(
                retrieved.is_ok(),
                "Failed to retrieve {}: {:?}",
                path,
                retrieved
            );
            assert_eq!(retrieved.unwrap(), data);
        }
    }

    #[tokio::test]
    async fn test_concurrent_operations() {
        let storage = create_test_storage().await.unwrap();

        // Spawn multiple concurrent operations
        let mut handles = vec![];

        for i in 0..10 {
            let storage_clone = storage.clone();
            let handle = tokio::spawn(async move {
                let path = format!("home/concurrent/file_{}.txt", i);
                let data = format!("concurrent data {}", i).into_bytes();

                storage_clone.put(&path, data.clone()).await.unwrap();
                let retrieved = storage_clone.get(&path).await.unwrap();
                assert_eq!(retrieved, data);
            });
            handles.push(handle);
        }

        // Wait for all to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all files exist
        let entries = storage.list("home/concurrent").await.unwrap();
        assert_eq!(entries.len(), 10);
    }

    #[tokio::test]
    async fn test_path_validation() {
        let storage = create_test_storage().await.unwrap();

        // Invalid paths should fail
        let invalid_paths = vec![
            "invalid/path",       // Doesn't start with home/ or archive/
            "/home/test.txt",     // Starts with /
            "home/../etc/passwd", // Path traversal
            "",                   // Empty path
            "home",               // No file name
        ];

        for path in invalid_paths {
            let result = storage.put(path, b"data".to_vec()).await;
            assert!(
                result.is_err(),
                "Path '{}' should have failed validation",
                path
            );
        }
    }

    #[tokio::test]
    async fn test_cid_retrieval() {
        let storage = create_test_storage().await.unwrap();

        let data1 = b"content 1".to_vec();
        let data2 = b"content 2".to_vec();
        let data1_dup = b"content 1".to_vec(); // Same as data1

        let cid1 = storage.put("home/cid/file1.txt", data1).await.unwrap();
        let cid2 = storage.put("home/cid/file2.txt", data2).await.unwrap();
        let cid3 = storage.put("home/cid/file3.txt", data1_dup).await.unwrap();

        // Same content should produce same CID
        assert_eq!(cid1, cid3);
        // Different content should produce different CID
        assert_ne!(cid1, cid2);

        // Get by CID
        let retrieved = storage.get_by_cid(&cid1).await.unwrap();
        assert_eq!(retrieved, b"content 1".to_vec());
    }

    // Mock-specific tests
    #[cfg(not(feature = "integration"))]
    mod mock_tests {
        use super::*;

        #[tokio::test]
        async fn test_error_injection() {
            let config = S5StorageConfig {
                backend: S5Backend::Mock,
                api_key: None,
                cache_ttl_seconds: 300,
                max_retries: 0, // No retries for testing
            };

            let storage = S5Client::create(config).await.unwrap();

            // Inject network error
            storage
                .inject_error(StorageError::NetworkError("Simulated timeout".into()))
                .await;

            let result = storage.put("home/test.txt", b"data".to_vec()).await;
            assert!(matches!(result, Err(StorageError::NetworkError(_))));
        }

        #[tokio::test]
        async fn test_quota_exceeded() {
            let config = S5StorageConfig {
                backend: S5Backend::Mock,
                api_key: None,
                cache_ttl_seconds: 300,
                max_retries: 3,
            };

            let storage = S5Client::create(config).await.unwrap();

            // Set quota limit
            storage.set_quota_limit(1_000_000).await; // 1MB

            // Try to store 2MB
            let large_data = vec![0u8; 2_000_000];
            let result = storage.put("home/large.bin", large_data).await;

            assert!(matches!(result, Err(StorageError::QuotaExceeded)));
        }
    }
}
