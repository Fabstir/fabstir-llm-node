// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Tests for S5 Vector Loader (Sub-phase 3.1)
// Comprehensive test suite for load_vectors_from_s5 orchestration

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use anyhow::Result;
use fabstir_llm_node::storage::manifest::{ChunkMetadata, Manifest, Vector, VectorChunk};
use fabstir_llm_node::storage::s5_client::{S5Storage, StorageError};
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

#[cfg(test)]
mod vector_loader_tests {
    use super::*;

    /// Mock S5 storage for testing
    /// Stores encrypted manifest and chunks in memory
    struct MockS5Storage {
        files: Arc<tokio::sync::Mutex<HashMap<String, Vec<u8>>>>,
    }

    impl MockS5Storage {
        fn new() -> Self {
            Self {
                files: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            }
        }

        async fn add_file(&self, path: &str, data: Vec<u8>) {
            let mut files = self.files.lock().await;
            files.insert(path.to_string(), data);
        }
    }

    #[async_trait::async_trait]
    impl S5Storage for MockS5Storage {
        async fn put(&self, path: &str, data: Vec<u8>) -> Result<String, StorageError> {
            self.add_file(path, data).await;
            Ok(format!("s5://mock-cid-{}", path))
        }

        async fn put_with_metadata(
            &self,
            path: &str,
            data: Vec<u8>,
            _metadata: HashMap<String, String>,
        ) -> Result<String, StorageError> {
            self.put(path, data).await
        }

        async fn get(&self, path: &str) -> Result<Vec<u8>, StorageError> {
            let files = self.files.lock().await;
            files
                .get(path)
                .cloned()
                .ok_or_else(|| StorageError::NotFound(format!("File not found: {}", path)))
        }

        async fn get_metadata(&self, _path: &str) -> Result<HashMap<String, String>, StorageError> {
            Ok(HashMap::new())
        }

        async fn get_by_cid(&self, cid: &str) -> Result<Vec<u8>, StorageError> {
            // Extract path from mock CID
            if let Some(path) = cid.strip_prefix("s5://mock-cid-") {
                self.get(path).await
            } else {
                Err(StorageError::NotFound(format!("CID not found: {}", cid)))
            }
        }

        async fn list(
            &self,
            _path: &str,
        ) -> Result<Vec<fabstir_llm_node::storage::s5_client::S5Entry>, StorageError> {
            Ok(vec![])
        }

        async fn list_with_options(
            &self,
            _path: &str,
            _limit: Option<usize>,
            _cursor: Option<String>,
        ) -> Result<fabstir_llm_node::storage::s5_client::S5ListResult, StorageError> {
            Ok(fabstir_llm_node::storage::s5_client::S5ListResult {
                entries: vec![],
                cursor: None,
                has_more: false,
            })
        }

        async fn delete(&self, path: &str) -> Result<(), StorageError> {
            let mut files = self.files.lock().await;
            files.remove(path);
            Ok(())
        }

        async fn exists(&self, path: &str) -> Result<bool, StorageError> {
            let files = self.files.lock().await;
            Ok(files.contains_key(path))
        }

        fn clone(&self) -> Box<dyn S5Storage> {
            Box::new(Self {
                files: Arc::clone(&self.files),
            })
        }
    }

    /// Helper: Encrypt data with Web Crypto API format
    fn encrypt_web_crypto_format(plaintext: &str, key: &[u8]) -> Vec<u8> {
        let nonce_bytes = [1u8; 12]; // Fixed nonce for deterministic tests
        let nonce = Nonce::from_slice(&nonce_bytes);

        let cipher = Aes256Gcm::new_from_slice(key).unwrap();
        let ciphertext = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: b"",
                },
            )
            .unwrap();

        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);
        result
    }

    /// Helper: Create test manifest
    fn create_test_manifest(owner: &str, chunk_count: usize) -> Manifest {
        let chunks: Vec<ChunkMetadata> = (0..chunk_count)
            .map(|i| ChunkMetadata {
                chunk_id: i,
                cid: format!("s5://mock-cid-home/vector-databases/{}/test-db/chunk-{}.json", owner, i),
                vector_count: 10,
                size_bytes: 5000,
                updated_at: 1700000000000,
            })
            .collect();

        Manifest {
            name: "test-db".to_string(),
            owner: owner.to_string(),
            description: "Test database".to_string(),
            dimensions: 384,
            vector_count: chunk_count * 10,
            storage_size_bytes: chunk_count as u64 * 5000,
            created: 1700000000000,
            last_accessed: 1700000000000,
            updated: 1700000000000,
            chunks,
            chunk_count,
            folder_paths: vec![],
            deleted: false,
        }
    }

    /// Helper: Create test vector chunk
    fn create_test_chunk(chunk_id: usize, vector_count: usize, dimensions: usize) -> VectorChunk {
        let vectors: Vec<Vector> = (0..vector_count)
            .map(|i| Vector {
                id: format!("vec-{}-{}", chunk_id, i),
                vector: vec![0.1; dimensions],
                metadata: serde_json::json!({
                    "source": "test.pdf",
                    "page": i,
                }),
            })
            .collect();

        VectorChunk { chunk_id, vectors }
    }

    /// Helper: Set up mock storage with encrypted data
    async fn setup_mock_storage(
        owner: &str,
        db_name: &str,
        chunk_count: usize,
        session_key: &[u8],
    ) -> MockS5Storage {
        let storage = MockS5Storage::new();

        // Create and encrypt manifest
        let manifest = create_test_manifest(owner, chunk_count);
        let manifest_json = serde_json::to_string(&manifest).unwrap();
        let encrypted_manifest = encrypt_web_crypto_format(&manifest_json, session_key);

        let manifest_path = format!("home/vector-databases/{}/{}/manifest.json", owner, db_name);
        storage.add_file(&manifest_path, encrypted_manifest).await;

        // Create and encrypt chunks
        for i in 0..chunk_count {
            let chunk = create_test_chunk(i, 10, 384);
            let chunk_json = serde_json::to_string(&chunk).unwrap();
            let encrypted_chunk = encrypt_web_crypto_format(&chunk_json, session_key);

            let chunk_path = format!("home/vector-databases/{}/{}/chunk-{}.json", owner, db_name, i);
            storage.add_file(&chunk_path, encrypted_chunk).await;
        }

        storage
    }

    /// Test 1: Successful end-to-end vector loading
    #[tokio::test]
    async fn test_load_vectors_success() {
        use fabstir_llm_node::rag::vector_loader::VectorLoader;

        let owner = "0xABC123";
        let db_name = "test-db";
        let session_key = [0u8; 32];

        // Set up mock storage with 3 chunks
        let storage = setup_mock_storage(owner, db_name, 3, &session_key).await;

        // Create vector loader
        let loader = VectorLoader::new(Box::new(storage), 5);

        // Create progress channel
        let (progress_tx, mut progress_rx) = mpsc::channel(10);

        // Load vectors
        let manifest_path = format!("home/vector-databases/{}/{}/manifest.json", owner, db_name);
        let result = loader
            .load_vectors_from_s5(&manifest_path, owner, &session_key, Some(progress_tx))
            .await;

        assert!(result.is_ok(), "Loading should succeed: {:?}", result.err());
        let vectors = result.unwrap();

        // Verify vector count (3 chunks × 10 vectors)
        assert_eq!(vectors.len(), 30);

        // Verify dimensions
        assert_eq!(vectors[0].vector.len(), 384);

        // Verify progress messages
        let mut progress_messages = vec![];
        while let Ok(msg) = progress_rx.try_recv() {
            progress_messages.push(msg);
        }

        assert!(!progress_messages.is_empty(), "Should receive progress messages");
    }

    /// Test 2: Manifest download and decryption
    #[tokio::test]
    async fn test_manifest_download_and_decrypt() {
        use fabstir_llm_node::rag::vector_loader::VectorLoader;

        let owner = "0xDEF456";
        let db_name = "manifest-test";
        let session_key = [1u8; 32];

        let storage = setup_mock_storage(owner, db_name, 2, &session_key).await;
        let loader = VectorLoader::new(Box::new(storage), 5);

        let manifest_path = format!("home/vector-databases/{}/{}/manifest.json", owner, db_name);
        let result = loader
            .download_and_decrypt_manifest(&manifest_path, &session_key)
            .await;

        assert!(result.is_ok(), "Manifest download should succeed: {:?}", result.err());
        let manifest = result.unwrap();

        assert_eq!(manifest.name, "test-db");
        assert_eq!(manifest.owner, owner);
        assert_eq!(manifest.dimensions, 384);
        assert_eq!(manifest.chunk_count, 2);
    }

    /// Test 3: Owner verification success
    #[tokio::test]
    async fn test_owner_verification_success() {
        use fabstir_llm_node::rag::vector_loader::VectorLoader;

        let owner = "0xABC123";
        let manifest = create_test_manifest(owner, 1);

        let storage = MockS5Storage::new();
        let loader = VectorLoader::new(Box::new(storage), 5);

        let result = loader.verify_owner(&manifest, owner);
        assert!(result.is_ok(), "Owner verification should succeed");
    }

    /// Test 4: Owner verification failure (mismatch)
    #[tokio::test]
    async fn test_owner_verification_mismatch() {
        use fabstir_llm_node::rag::vector_loader::VectorLoader;

        let manifest = create_test_manifest("0xABC123", 1);

        let storage = MockS5Storage::new();
        let loader = VectorLoader::new(Box::new(storage), 5);

        let result = loader.verify_owner(&manifest, "0xDIFFERENT");
        assert!(result.is_err(), "Owner verification should fail");
        assert!(
            result.unwrap_err().to_string().contains("Owner mismatch"),
            "Error should mention owner mismatch"
        );
    }

    /// Test 5: Parallel chunk downloads
    #[tokio::test]
    async fn test_parallel_chunk_downloads() {
        use fabstir_llm_node::rag::vector_loader::VectorLoader;

        let owner = "0xPARALLEL";
        let db_name = "parallel-test";
        let session_key = [2u8; 32];

        // Create storage with many chunks to test parallelism
        let storage = setup_mock_storage(owner, db_name, 10, &session_key).await;
        let loader = VectorLoader::new(Box::new(storage), 5);

        let manifest_path = format!("home/vector-databases/{}/{}/manifest.json", owner, db_name);

        // Download manifest first
        let manifest = loader
            .download_and_decrypt_manifest(&manifest_path, &session_key)
            .await
            .unwrap();

        // Download chunks in parallel
        let base_path = format!("home/vector-databases/{}/{}", owner, db_name);
        let result = loader
            .download_and_decrypt_chunks(&manifest, &base_path, &session_key, None)
            .await;

        assert!(result.is_ok(), "Parallel downloads should succeed: {:?}", result.err());
        let vectors = result.unwrap();

        // Verify all vectors loaded (10 chunks × 10 vectors)
        assert_eq!(vectors.len(), 100);
    }

    /// Test 6: Partial download failure
    #[tokio::test]
    async fn test_partial_download_failure() {
        use fabstir_llm_node::rag::vector_loader::VectorLoader;

        let owner = "0xFAIL";
        let db_name = "fail-test";
        let session_key = [3u8; 32];

        // Set up storage with only manifest, no chunks
        let storage = MockS5Storage::new();
        let manifest = create_test_manifest(owner, 3);
        let manifest_json = serde_json::to_string(&manifest).unwrap();
        let encrypted_manifest = encrypt_web_crypto_format(&manifest_json, &session_key);

        let manifest_path = format!("home/vector-databases/{}/{}/manifest.json", owner, db_name);
        storage.add_file(&manifest_path, encrypted_manifest).await;

        let loader = VectorLoader::new(Box::new(storage), 5);

        // Try to load (should fail because chunks don't exist)
        let result = loader
            .load_vectors_from_s5(&manifest_path, owner, &session_key, None)
            .await;

        assert!(result.is_err(), "Should fail when chunks are missing");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not found") || err_msg.contains("NotFound"),
            "Error should indicate missing file"
        );
    }

    /// Test 7: Decryption failure (wrong key)
    #[tokio::test]
    async fn test_decryption_failure_wrong_key() {
        use fabstir_llm_node::rag::vector_loader::VectorLoader;

        let owner = "0xDECRYPT";
        let db_name = "decrypt-fail";
        let correct_key = [4u8; 32];
        let wrong_key = [5u8; 32];

        // Set up storage with correct key
        let storage = setup_mock_storage(owner, db_name, 1, &correct_key).await;
        let loader = VectorLoader::new(Box::new(storage), 5);

        let manifest_path = format!("home/vector-databases/{}/{}/manifest.json", owner, db_name);

        // Try to load with wrong key
        let result = loader
            .load_vectors_from_s5(&manifest_path, owner, &wrong_key, None)
            .await;

        assert!(result.is_err(), "Should fail with wrong decryption key");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("authentication") || err_msg.contains("decryption"),
            "Error should indicate decryption failure"
        );
    }

    /// Test 8: Manifest not found
    #[tokio::test]
    async fn test_manifest_not_found() {
        use fabstir_llm_node::rag::vector_loader::VectorLoader;

        let storage = MockS5Storage::new();
        let loader = VectorLoader::new(Box::new(storage), 5);

        let session_key = [0u8; 32];
        let result = loader
            .load_vectors_from_s5("nonexistent/path/manifest.json", "0xABC", &session_key, None)
            .await;

        assert!(result.is_err(), "Should fail when manifest doesn't exist");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not found") || err_msg.contains("NotFound"),
            "Error should indicate file not found"
        );
    }

    /// Test 9: Invalid manifest structure
    #[tokio::test]
    async fn test_invalid_manifest_structure() {
        use fabstir_llm_node::rag::vector_loader::VectorLoader;

        let storage = MockS5Storage::new();
        let session_key = [0u8; 32];

        // Create invalid JSON
        let invalid_json = "{ invalid json here }";
        let encrypted_invalid = encrypt_web_crypto_format(invalid_json, &session_key);

        let manifest_path = "home/vector-databases/0xINVALID/test/manifest.json";
        storage.add_file(manifest_path, encrypted_invalid).await;

        let loader = VectorLoader::new(Box::new(storage), 5);

        let result = loader
            .download_and_decrypt_manifest(manifest_path, &session_key)
            .await;

        assert!(result.is_err(), "Should fail with invalid manifest JSON");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("parse") || err_msg.contains("JSON"),
            "Error should indicate JSON parse failure"
        );
    }

    /// Test 10: Progress reporting
    #[tokio::test]
    async fn test_progress_reporting() {
        use fabstir_llm_node::rag::vector_loader::{LoadProgress, VectorLoader};

        let owner = "0xPROGRESS";
        let db_name = "progress-test";
        let session_key = [6u8; 32];

        let storage = setup_mock_storage(owner, db_name, 5, &session_key).await;
        let loader = VectorLoader::new(Box::new(storage), 5);

        let (progress_tx, mut progress_rx) = mpsc::channel(20);

        let manifest_path = format!("home/vector-databases/{}/{}/manifest.json", owner, db_name);
        let _result = loader
            .load_vectors_from_s5(&manifest_path, owner, &session_key, Some(progress_tx))
            .await;

        // Collect all progress messages
        let mut progress_messages = vec![];
        while let Ok(msg) = progress_rx.try_recv() {
            progress_messages.push(msg);
        }

        // Verify we received progress updates
        assert!(!progress_messages.is_empty(), "Should receive progress messages");

        // Verify we have manifest downloaded message
        let has_manifest_downloaded = progress_messages.iter().any(|msg| {
            matches!(msg, LoadProgress::ManifestDownloaded)
        });
        assert!(has_manifest_downloaded, "Should report manifest downloaded");

        // Verify we have chunk downloaded messages
        let chunk_messages: Vec<_> = progress_messages
            .iter()
            .filter_map(|msg| match msg {
                LoadProgress::ChunkDownloaded { chunk_id, total } => Some((chunk_id, total)),
                _ => None,
            })
            .collect();
        assert!(!chunk_messages.is_empty(), "Should report chunk downloads");

        // Verify we have completion message
        let has_complete = progress_messages.iter().any(|msg| {
            matches!(msg, LoadProgress::Complete { .. })
        });
        assert!(has_complete, "Should report completion");
    }

    /// Test 11: Empty manifest (no chunks)
    #[tokio::test]
    async fn test_empty_manifest() {
        use fabstir_llm_node::rag::vector_loader::VectorLoader;

        let owner = "0xEMPTY";
        let storage = MockS5Storage::new();
        let session_key = [7u8; 32];

        // Create manifest with zero chunks
        let manifest = Manifest {
            name: "empty-db".to_string(),
            owner: owner.to_string(),
            description: "Empty database".to_string(),
            dimensions: 384,
            vector_count: 0,
            storage_size_bytes: 0,
            created: 1700000000000,
            last_accessed: 1700000000000,
            updated: 1700000000000,
            chunks: vec![],
            chunk_count: 0,
            folder_paths: vec![],
            deleted: false,
        };

        let manifest_json = serde_json::to_string(&manifest).unwrap();
        let encrypted_manifest = encrypt_web_crypto_format(&manifest_json, &session_key);

        let manifest_path = "home/vector-databases/0xEMPTY/empty-db/manifest.json";
        storage.add_file(manifest_path, encrypted_manifest).await;

        let loader = VectorLoader::new(Box::new(storage), 5);

        let result = loader
            .load_vectors_from_s5(manifest_path, owner, &session_key, None)
            .await;

        assert!(result.is_ok(), "Empty manifest should be valid");
        let vectors = result.unwrap();
        assert_eq!(vectors.len(), 0, "Should return empty vector list");
    }

    /// Test 12: Deleted database flag
    #[tokio::test]
    async fn test_deleted_database() {
        use fabstir_llm_node::rag::vector_loader::VectorLoader;

        let owner = "0xDELETED";
        let storage = MockS5Storage::new();
        let session_key = [8u8; 32];

        // Create manifest with deleted=true
        let manifest = Manifest {
            name: "deleted-db".to_string(),
            owner: owner.to_string(),
            description: "Deleted database".to_string(),
            dimensions: 384,
            vector_count: 10,
            storage_size_bytes: 5000,
            created: 1700000000000,
            last_accessed: 1700000000000,
            updated: 1700000000000,
            chunks: vec![],
            chunk_count: 0,
            folder_paths: vec![],
            deleted: true, // Soft delete flag
        };

        let manifest_json = serde_json::to_string(&manifest).unwrap();
        let encrypted_manifest = encrypt_web_crypto_format(&manifest_json, &session_key);

        let manifest_path = "home/vector-databases/0xDELETED/deleted-db/manifest.json";
        storage.add_file(manifest_path, encrypted_manifest).await;

        let loader = VectorLoader::new(Box::new(storage), 5);

        let result = loader
            .load_vectors_from_s5(manifest_path, owner, &session_key, None)
            .await;

        assert!(result.is_err(), "Should reject deleted database");
        assert!(
            result.unwrap_err().to_string().contains("deleted"),
            "Error should mention database is deleted"
        );
    }

    /// Test 13: Invalid vector dimensions in chunk
    #[tokio::test]
    async fn test_invalid_vector_dimensions() {
        use fabstir_llm_node::rag::vector_loader::VectorLoader;

        let owner = "0xDIMENSIONS";
        let db_name = "dimension-fail";
        let storage = MockS5Storage::new();
        let session_key = [9u8; 32];

        // Create manifest expecting 384 dimensions
        let manifest = create_test_manifest(owner, 1);
        let manifest_json = serde_json::to_string(&manifest).unwrap();
        let encrypted_manifest = encrypt_web_crypto_format(&manifest_json, &session_key);

        let manifest_path = format!("home/vector-databases/{}/{}/manifest.json", owner, db_name);
        storage.add_file(&manifest_path, encrypted_manifest).await;

        // Create chunk with WRONG dimensions (256 instead of 384)
        let bad_chunk = create_test_chunk(0, 10, 256); // Wrong dimensions!
        let chunk_json = serde_json::to_string(&bad_chunk).unwrap();
        let encrypted_chunk = encrypt_web_crypto_format(&chunk_json, &session_key);

        let chunk_path = format!("home/vector-databases/{}/{}/chunk-0.json", owner, db_name);
        storage.add_file(&chunk_path, encrypted_chunk).await;

        let loader = VectorLoader::new(Box::new(storage), 5);

        let result = loader
            .load_vectors_from_s5(&manifest_path, owner, &session_key, None)
            .await;

        assert!(result.is_err(), "Should reject mismatched dimensions");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("dimension") || err_msg.contains("384"),
            "Error should mention dimension mismatch"
        );
    }

    /// Test 14: Max parallel chunks configuration
    #[tokio::test]
    async fn test_max_parallel_chunks() {
        use fabstir_llm_node::rag::vector_loader::VectorLoader;

        // Test different max_parallel_chunks values
        let storage1 = MockS5Storage::new();
        let loader1 = VectorLoader::new(Box::new(storage1), 1);
        assert_eq!(loader1.max_parallel_chunks(), 1);

        let storage5 = MockS5Storage::new();
        let loader5 = VectorLoader::new(Box::new(storage5), 5);
        assert_eq!(loader5.max_parallel_chunks(), 5);

        let storage10 = MockS5Storage::new();
        let loader10 = VectorLoader::new(Box::new(storage10), 10);
        assert_eq!(loader10.max_parallel_chunks(), 10);
    }

    /// Test 15: Large vector database (stress test)
    #[tokio::test]
    async fn test_large_database() {
        use fabstir_llm_node::rag::vector_loader::VectorLoader;

        let owner = "0xLARGE";
        let db_name = "large-db";
        let session_key = [10u8; 32];

        // Create storage with 50 chunks (500 vectors total)
        let storage = setup_mock_storage(owner, db_name, 50, &session_key).await;
        let loader = VectorLoader::new(Box::new(storage), 10); // 10 parallel downloads

        let manifest_path = format!("home/vector-databases/{}/{}/manifest.json", owner, db_name);
        let result = loader
            .load_vectors_from_s5(&manifest_path, owner, &session_key, None)
            .await;

        assert!(result.is_ok(), "Large database loading should succeed: {:?}", result.err());
        let vectors = result.unwrap();

        // Verify all vectors loaded (50 chunks × 10 vectors)
        assert_eq!(vectors.len(), 500);
    }
}
