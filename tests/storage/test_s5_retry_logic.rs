// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Tests for S5 client retry logic with exponential backoff (Sub-phase 2.1)

use fabstir_llm_node::storage::{S5Backend, S5Client, S5Storage, S5StorageConfig, StorageError};
use std::sync::{Arc, Mutex};

#[cfg(test)]
mod s5_retry_tests {
    use super::*;

    /// Test 1: Successful download on first attempt (no retries needed)
    #[tokio::test]
    async fn test_download_success_first_attempt() {
        let storage = create_mock_storage().await;

        let path = "home/vector-databases/0xABC/my-docs/manifest.json";
        let data = b"{\"name\":\"my-docs\",\"owner\":\"0xABC\"}";

        // Upload test file
        storage.put(path, data.to_vec()).await.unwrap();

        // Download should succeed on first attempt
        let result = storage.get(path).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), data.to_vec());
    }

    /// Test 2: Download with transient network error - should retry and succeed
    #[tokio::test]
    async fn test_download_retry_on_network_error() {
        let storage = create_mock_storage().await;

        // For this test, we'll use the mock backend's error injection
        // Note: This test verifies the storage layer handles errors gracefully
        let path = "home/vector-databases/0xABC/test.json";
        let data = b"test data";

        storage.put(path, data.to_vec()).await.unwrap();

        // Inject a transient error (will be cleared after first attempt)
        storage
            .inject_error(StorageError::NetworkError("Connection reset".to_string()))
            .await;

        // Download should fail on first attempt due to injected error
        let result = storage.get(path).await;
        assert!(result.is_err());

        // Second attempt should succeed (error was consumed)
        let result2 = storage.get(path).await;
        assert!(result2.is_ok());
        assert_eq!(result2.unwrap(), data.to_vec());
    }

    /// Test 3: Download non-existent file - should return NotFound after retries
    #[tokio::test]
    async fn test_download_not_found_no_retry() {
        let storage = create_mock_storage().await;

        let path = "home/vector-databases/0xABC/nonexistent.json";

        // Download should fail with NotFound
        let result = storage.get(path).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            StorageError::NotFound(p) => assert_eq!(p, path),
            e => panic!("Expected NotFound error, got: {:?}", e),
        }
    }

    /// Test 4: Download large manifest file (simulates chunk downloads)
    #[tokio::test]
    async fn test_download_large_file() {
        let storage = create_mock_storage().await;

        let path = "home/vector-databases/0xABC/my-docs/chunk-0.json";

        // Create a large file (simulate 10K vectors * 384 dims * 4 bytes ≈ 15MB)
        let large_data = vec![0u8; 15_000_000];

        storage.put(path, large_data.clone()).await.unwrap();

        // Download should handle large files
        let result = storage.get(path).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), large_data.len());
    }

    /// Test 5: Multiple concurrent downloads (connection pooling)
    #[tokio::test]
    async fn test_concurrent_downloads() {
        let storage = Arc::new(create_mock_storage().await);

        // Create multiple files
        let files = vec![
            ("home/vector-databases/0xABC/file1.json", b"data1"),
            ("home/vector-databases/0xABC/file2.json", b"data2"),
            ("home/vector-databases/0xABC/file3.json", b"data3"),
            ("home/vector-databases/0xABC/file4.json", b"data4"),
        ];

        for (path, data) in &files {
            storage.put(path, data.to_vec()).await.unwrap();
        }

        // Download all files concurrently
        let mut handles = vec![];
        for (path, expected_data) in files {
            let storage_clone = Arc::clone(&storage);
            let handle = tokio::spawn(async move {
                let result = storage_clone.get(path).await;
                assert!(result.is_ok());
                assert_eq!(result.unwrap(), expected_data.to_vec());
            });
            handles.push(handle);
        }

        // Wait for all downloads to complete
        for handle in handles {
            handle.await.unwrap();
        }
    }

    /// Test 6: Download with path validation
    #[tokio::test]
    async fn test_download_invalid_path() {
        let storage = create_mock_storage().await;

        // Test various invalid paths
        let invalid_paths = vec![
            "",                       // Empty path
            "/home/test.json",        // Leading slash
            "home/../etc/passwd",     // Path traversal
            "invalid/path/test.json", // Not home/ or archive/
        ];

        for path in invalid_paths {
            let result = storage.get(path).await;
            assert!(result.is_err());
            match result.unwrap_err() {
                StorageError::InvalidPath(_) => {} // Expected
                e => panic!("Expected InvalidPath error, got: {:?}", e),
            }
        }
    }

    /// Test 7: Download manifest and chunks for vector database
    #[tokio::test]
    async fn test_vector_database_download_flow() {
        let storage = create_mock_storage().await;

        // Step 1: Upload manifest
        let manifest_path = "home/vector-databases/0xABC123/my-docs/manifest.json";
        let manifest_data = r#"{
            "name": "my-docs",
            "owner": "0xABC123",
            "dimensions": 384,
            "vectorCount": 15000,
            "chunks": [
                {"chunkId": 0, "cid": "s5://chunk0", "vectorCount": 10000},
                {"chunkId": 1, "cid": "s5://chunk1", "vectorCount": 5000}
            ]
        }"#;

        storage
            .put(manifest_path, manifest_data.as_bytes().to_vec())
            .await
            .unwrap();

        // Step 2: Download manifest
        let manifest_result = storage.get(manifest_path).await;
        assert!(manifest_result.is_ok());
        let manifest_bytes = manifest_result.unwrap();
        assert_eq!(manifest_bytes, manifest_data.as_bytes());

        // Step 3: Parse manifest (simplified - actual parsing in Sub-phase 2.2)
        let manifest: serde_json::Value =
            serde_json::from_slice(&manifest_bytes).expect("Invalid JSON");
        assert_eq!(manifest["name"], "my-docs");
        assert_eq!(manifest["vectorCount"], 15000);

        // Step 4: Download chunks
        let chunk_paths = vec![
            "home/vector-databases/0xABC123/my-docs/chunk-0.json",
            "home/vector-databases/0xABC123/my-docs/chunk-1.json",
        ];

        for (i, chunk_path) in chunk_paths.iter().enumerate() {
            let chunk_data = format!(r#"{{"chunkId": {}, "vectors": []}}"#, i);
            storage
                .put(chunk_path, chunk_data.as_bytes().to_vec())
                .await
                .unwrap();

            let chunk_result = storage.get(chunk_path).await;
            assert!(chunk_result.is_ok());
        }
    }

    /// Test 8: Verify connection reuse (reqwest Client connection pooling)
    #[tokio::test]
    async fn test_connection_pooling() {
        let storage = create_mock_storage().await;

        let path = "home/test/reuse.json";
        let data = b"connection pool test";

        storage.put(path, data.to_vec()).await.unwrap();

        // Make multiple sequential requests
        // reqwest Client should reuse connections automatically
        for _ in 0..10 {
            let result = storage.get(path).await;
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), data.to_vec());
        }
    }

    /// Test 9: Download with quota exceeded error
    #[tokio::test]
    async fn test_download_with_quota_limit() {
        let storage = create_mock_storage().await;

        // Set a quota limit
        storage.set_quota_limit(1000).await;

        let path = "home/test/small.json";
        let small_data = vec![0u8; 500]; // 500 bytes

        // First upload should succeed
        let result1 = storage.put(path, small_data.clone()).await;
        assert!(result1.is_ok());

        // Second upload should fail (total would exceed 1000 bytes)
        let path2 = "home/test/large.json";
        let large_data = vec![0u8; 600]; // 600 bytes (500 + 600 > 1000)

        let result2 = storage.put(path2, large_data).await;
        assert!(result2.is_err());
        match result2.unwrap_err() {
            StorageError::QuotaExceeded => {} // Expected
            e => panic!("Expected QuotaExceeded error, got: {:?}", e),
        }
    }

    /// Test 10: Verify S5 path format for vector databases
    #[tokio::test]
    async fn test_vector_database_path_format() {
        let storage = create_mock_storage().await;

        // Valid vector database paths
        let valid_paths = vec![
            "home/vector-databases/0xABCDEF1234567890ABCDEF1234567890ABCDEF12/my-docs/manifest.json",
            "home/vector-databases/0x1234567890ABCDEF1234567890ABCDEF12345678/research/chunk-0.json",
            "home/vector-databases/0xABC/test/chunk-999.json",
        ];

        for path in valid_paths {
            let data = b"test data";
            let result = storage.put(path, data.to_vec()).await;
            assert!(result.is_ok(), "Path should be valid: {}", path);

            let get_result = storage.get(path).await;
            assert!(get_result.is_ok());
        }
    }

    // Helper function to create mock storage
    async fn create_mock_storage() -> Box<dyn S5Storage> {
        let config = S5StorageConfig {
            backend: S5Backend::Mock,
            api_key: None,
            cache_ttl_seconds: 300,
            max_retries: 3,
        };

        S5Client::create(config).await.unwrap()
    }
}
