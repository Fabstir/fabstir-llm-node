// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// TDD Security Tests for S5 Vector Loading (Sub-phase 5.3)
// Tests owner verification, manifest tampering, rate limiting, memory limits, and timeouts

use async_trait::async_trait;
use fabstir_llm_node::rag::errors::VectorLoadError;
use fabstir_llm_node::rag::vector_loader::{LoadProgress, VectorLoader};
use fabstir_llm_node::storage::manifest::{ChunkMetadata, Manifest, Vector, VectorChunk};
use fabstir_llm_node::storage::s5_client::{S5Entry, S5ListResult, S5Storage, StorageError};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex};

// ============================================================================
// Mock S5 Storage for Testing
// ============================================================================

struct MockS5Storage {
    /// Stored data (path -> bytes)
    data: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    /// Download delay for rate limiting tests
    download_delay: Arc<Mutex<Option<Duration>>>,
    /// Download call count for rate limiting
    download_count: Arc<Mutex<usize>>,
}

impl MockS5Storage {
    fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(HashMap::new())),
            download_delay: Arc::new(Mutex::new(None)),
            download_count: Arc::new(Mutex::new(0)),
        }
    }

    async fn set_data(&self, path: &str, data: Vec<u8>) {
        self.data.lock().await.insert(path.to_string(), data);
    }

    async fn set_download_delay(&self, delay: Duration) {
        *self.download_delay.lock().await = Some(delay);
    }

    async fn reset_download_count(&self) {
        *self.download_count.lock().await = 0;
    }

    async fn download_count(&self) -> usize {
        *self.download_count.lock().await
    }
}

#[async_trait]
impl S5Storage for MockS5Storage {
    async fn put(&self, path: &str, data: Vec<u8>) -> Result<String, StorageError> {
        self.set_data(path, data).await;
        Ok(format!("cid_{}", path))
    }

    async fn put_with_metadata(
        &self,
        path: &str,
        data: Vec<u8>,
        _metadata: HashMap<String, String>,
    ) -> Result<String, StorageError> {
        self.set_data(path, data).await;
        Ok(format!("cid_{}", path))
    }

    async fn get(&self, path: &str) -> Result<Vec<u8>, StorageError> {
        // Increment download count
        *self.download_count.lock().await += 1;

        // Apply delay if set (for rate limiting tests)
        if let Some(delay) = *self.download_delay.lock().await {
            tokio::time::sleep(delay).await;
        }

        self.data
            .lock()
            .await
            .get(path)
            .cloned()
            .ok_or_else(|| StorageError::NotFound(path.to_string()))
    }

    async fn get_metadata(&self, _path: &str) -> Result<HashMap<String, String>, StorageError> {
        Ok(HashMap::new())
    }

    async fn get_by_cid(&self, _cid: &str) -> Result<Vec<u8>, StorageError> {
        Err(StorageError::NotFound("mock".to_string()))
    }

    async fn list(&self, _path: &str) -> Result<Vec<S5Entry>, StorageError> {
        Ok(vec![])
    }

    async fn list_with_options(
        &self,
        _path: &str,
        _limit: Option<usize>,
        _cursor: Option<String>,
    ) -> Result<S5ListResult, StorageError> {
        Ok(S5ListResult {
            entries: vec![],
            cursor: None,
            has_more: false,
        })
    }

    async fn delete(&self, _path: &str) -> Result<(), StorageError> {
        Ok(())
    }

    async fn exists(&self, _path: &str) -> Result<bool, StorageError> {
        Ok(true)
    }

    fn clone(&self) -> Box<dyn S5Storage> {
        Box::new(MockS5Storage {
            data: Arc::clone(&self.data),
            download_delay: Arc::clone(&self.download_delay),
            download_count: Arc::clone(&self.download_count),
        })
    }
}

// ============================================================================
// Test Helpers
// ============================================================================

/// Create encrypted manifest with specified owner
fn create_encrypted_manifest(owner: &str, dimensions: usize, vector_count: usize) -> Vec<u8> {
    // In real implementation, this would be AES-GCM encrypted
    // For tests, we'll use JSON directly since we're testing logic, not crypto
    let manifest = Manifest {
        name: "test-db".to_string(),
        owner: owner.to_string(),
        description: "Test database".to_string(),
        dimensions,
        vector_count,
        storage_size_bytes: 1024,
        created: chrono::Utc::now().timestamp_millis(),
        last_accessed: chrono::Utc::now().timestamp_millis(),
        updated: chrono::Utc::now().timestamp_millis(),
        chunks: vec![ChunkMetadata {
            chunk_id: 0,
            cid: "test-cid".to_string(),
            vector_count,
            size_bytes: 1024,
            updated_at: chrono::Utc::now().timestamp_millis(),
        }],
        chunk_count: 1,
        folder_paths: vec![],
        deleted: false,
    };

    serde_json::to_vec(&manifest).unwrap()
}

/// Create encrypted chunk with vectors
fn create_encrypted_chunk(count: usize, dimensions: usize) -> Vec<u8> {
    let vectors: Vec<Vector> = (0..count)
        .map(|i| Vector {
            id: format!("vec-{}", i),
            vector: vec![i as f32; dimensions],
            metadata: json!({"index": i}),
        })
        .collect();

    let chunk = VectorChunk {
        chunk_id: 0,
        vectors,
    };

    serde_json::to_vec(&chunk).unwrap()
}

/// Dummy session key for testing
fn test_session_key() -> Vec<u8> {
    vec![0u8; 32]
}

// ============================================================================
// Test Category 1: Owner Verification Security
// ============================================================================

#[tokio::test]
async fn test_owner_mismatch_rejection() {
    let storage = MockS5Storage::new();
    let manifest_path = "home/vector-databases/0xALICE/db1/manifest.json";

    // Create manifest owned by Alice
    let manifest = create_encrypted_manifest("0xALICE", 384, 100);
    storage.set_data(manifest_path, manifest).await;

    let loader = VectorLoader::new(S5Storage::clone(&storage), 5);
    let session_key = test_session_key();

    // Try to load as Bob (should fail)
    let result = loader
        .load_vectors_from_s5(manifest_path, "0xBOB", &session_key, None)
        .await;

    assert!(result.is_err(), "Should reject owner mismatch");

    let err = result.unwrap_err();
    match err {
        VectorLoadError::OwnerMismatch { expected, actual } => {
            assert_eq!(expected.to_lowercase(), "0xbob");
            assert_eq!(actual.to_lowercase(), "0xalice");
        }
        _ => panic!("Expected OwnerMismatch error, got: {:?}", err),
    }
}

#[tokio::test]
async fn test_owner_verification_success() {
    let storage = MockS5Storage::new();
    let manifest_path = "home/vector-databases/0xALICE/db1/manifest.json";
    let base_path = "home/vector-databases/0xALICE/db1";

    // Create manifest owned by Alice
    let manifest = create_encrypted_manifest("0xALICE", 384, 100);
    storage.set_data(manifest_path, manifest).await;

    // Create chunk
    let chunk = create_encrypted_chunk(100, 384);
    storage.set_data(&format!("{}/chunk-0.json", base_path), chunk);

    let loader = VectorLoader::new(S5Storage::clone(&storage), 5);
    let session_key = test_session_key();

    // Load as Alice (should succeed)
    let result = loader
        .load_vectors_from_s5(manifest_path, "0xALICE", &session_key, None)
        .await;

    assert!(result.is_ok(), "Should accept matching owner");
    let vectors = result.unwrap();
    assert_eq!(vectors.len(), 100);
}

// ============================================================================
// Test Category 2: Manifest Tampering Detection
// ============================================================================

#[tokio::test]
async fn test_dimension_mismatch_detection() {
    let storage = MockS5Storage::new();
    let manifest_path = "home/vector-databases/0xALICE/db1/manifest.json";
    let base_path = "home/vector-databases/0xALICE/db1";

    // Manifest claims 384 dimensions
    let manifest = create_encrypted_manifest("0xALICE", 384, 100);
    storage.set_data(manifest_path, manifest).await;

    // But chunk has 256 dimensions (tampering)
    let chunk = create_encrypted_chunk(100, 256);
    storage.set_data(&format!("{}/chunk-0.json", base_path), chunk);

    let loader = VectorLoader::new(S5Storage::clone(&storage), 5);
    let session_key = test_session_key();

    let result = loader
        .load_vectors_from_s5(manifest_path, "0xALICE", &session_key, None)
        .await;

    assert!(result.is_err(), "Should detect dimension mismatch");

    let err = result.unwrap_err();
    match err {
        VectorLoadError::DimensionMismatch {
            expected, actual, ..
        } => {
            assert_eq!(expected, 384);
            assert_eq!(actual, 256);
        }
        _ => panic!("Expected DimensionMismatch error, got: {:?}", err),
    }
}

#[tokio::test]
async fn test_vector_count_mismatch_detection() {
    let storage = MockS5Storage::new();
    let manifest_path = "home/vector-databases/0xALICE/db1/manifest.json";
    let base_path = "home/vector-databases/0xALICE/db1";

    // Manifest claims 100 vectors in chunk
    let manifest = create_encrypted_manifest("0xALICE", 384, 100);
    storage.set_data(manifest_path, manifest).await;

    // But chunk has only 50 vectors (tampering)
    let chunk = create_encrypted_chunk(50, 384);
    storage.set_data(&format!("{}/chunk-0.json", base_path), chunk);

    let loader = VectorLoader::new(S5Storage::clone(&storage), 5);
    let session_key = test_session_key();

    let result = loader
        .load_vectors_from_s5(manifest_path, "0xALICE", &session_key, None)
        .await;

    assert!(result.is_err(), "Should detect vector count mismatch");

    let err = result.unwrap_err();
    match err {
        VectorLoadError::VectorCountMismatch {
            expected, actual, ..
        } => {
            assert_eq!(expected, 100);
            assert_eq!(actual, 50);
        }
        _ => panic!("Expected VectorCountMismatch error, got: {:?}", err),
    }
}

#[tokio::test]
async fn test_corrupt_manifest_rejection() {
    let storage = MockS5Storage::new();
    let manifest_path = "home/vector-databases/0xALICE/db1/manifest.json";

    // Store corrupt data (not valid JSON)
    storage.set_data(manifest_path, b"CORRUPT DATA".to_vec());

    let loader = VectorLoader::new(S5Storage::clone(&storage), 5);
    let session_key = test_session_key();

    let result = loader
        .load_vectors_from_s5(manifest_path, "0xALICE", &session_key, None)
        .await;

    assert!(result.is_err(), "Should reject corrupt manifest");

    // Accept DecryptionFailed since JSON parse happens in decrypt_manifest
    let err = result.unwrap_err();
    assert!(
        matches!(err, VectorLoadError::DecryptionFailed(_)),
        "Expected DecryptionFailed for corrupt data, got: {:?}",
        err
    );
}

// ============================================================================
// Test Category 3: Rate Limiting
// ============================================================================

#[tokio::test]
async fn test_rate_limit_enforcement() {
    let storage = MockS5Storage::new();
    let manifest_path = "home/vector-databases/0xALICE/db1/manifest.json";
    let base_path = "home/vector-databases/0xALICE/db1";

    // Set up manifest with 5 chunks
    let mut manifest_data =
        serde_json::from_slice::<Manifest>(&create_encrypted_manifest("0xALICE", 384, 500))
            .unwrap();

    // Manually set 5 chunks
    manifest_data.chunks = (0..5)
        .map(|i| ChunkMetadata {
            chunk_id: i,
            cid: format!("test-cid-{}", i),
            vector_count: 100,
            size_bytes: 1024,
            updated_at: chrono::Utc::now().timestamp_millis(),
        })
        .collect();
    manifest_data.chunk_count = 5;

    storage.set_data(manifest_path, serde_json::to_vec(&manifest_data).unwrap());

    // Create chunks
    for i in 0..5 {
        let chunk = create_encrypted_chunk(100, 384);
        storage.set_data(&format!("{}/chunk-{}.json", base_path, i), chunk);
    }

    // Set download delay to simulate slow network
    storage.set_download_delay(Duration::from_millis(100));
    storage.reset_download_count().await;

    // Create loader with rate limit: max 3 downloads per second
    let loader =
        VectorLoader::with_rate_limit(S5Storage::clone(&storage), 5, 3, Duration::from_secs(1));
    let session_key = test_session_key();

    let start = Instant::now();
    let result = loader
        .load_vectors_from_s5(manifest_path, "0xALICE", &session_key, None)
        .await;
    let duration = start.elapsed();

    assert!(result.is_ok(), "Should succeed with rate limiting");

    // With 5 chunks + 1 manifest = 6 downloads and max 3/sec, it should take at least 1 second
    // (first 3 downloads in first second, remaining 3 in second second)
    assert!(
        duration >= Duration::from_millis(900),
        "Rate limiting should enforce delays. Duration: {:?}",
        duration
    );
}

#[tokio::test]
async fn test_download_count_tracking() {
    let storage = MockS5Storage::new();
    let manifest_path = "home/vector-databases/0xALICE/db1/manifest.json";
    let base_path = "home/vector-databases/0xALICE/db1";

    // Manifest + 1 chunk = 2 downloads
    let manifest = create_encrypted_manifest("0xALICE", 384, 100);
    storage.set_data(manifest_path, manifest).await;

    let chunk = create_encrypted_chunk(100, 384);
    storage.set_data(&format!("{}/chunk-0.json", base_path), chunk);

    storage.reset_download_count().await;

    let loader = VectorLoader::new(S5Storage::clone(&storage), 5);
    let session_key = test_session_key();

    let _ = loader
        .load_vectors_from_s5(manifest_path, "0xALICE", &session_key, None)
        .await;

    // Should have downloaded manifest + 1 chunk = 2 downloads
    assert_eq!(
        storage.download_count().await,
        2,
        "Should track download count"
    );
}

// ============================================================================
// Test Category 4: Memory Limits
// ============================================================================

#[tokio::test]
async fn test_memory_limit_exceeded() {
    let storage = MockS5Storage::new();
    let manifest_path = "home/vector-databases/0xALICE/db1/manifest.json";

    // Create manifest with 1M vectors (way too large)
    // 1M vectors * 384 dims * 4 bytes = ~1.5 GB
    let manifest = create_encrypted_manifest("0xALICE", 384, 1_000_000);
    storage.set_data(manifest_path, manifest).await;

    // Create loader with 100MB memory limit
    let loader = VectorLoader::with_memory_limit(S5Storage::clone(&storage), 5, 100);
    let session_key = test_session_key();

    let result = loader
        .load_vectors_from_s5(manifest_path, "0xALICE", &session_key, None)
        .await;

    assert!(result.is_err(), "Should reject oversized dataset");

    let err = result.unwrap_err();
    match err {
        VectorLoadError::MemoryLimitExceeded {
            required_mb,
            limit_mb,
        } => {
            assert!(required_mb > limit_mb);
            assert_eq!(limit_mb, 100);
        }
        _ => panic!("Expected MemoryLimitExceeded error, got: {:?}", err),
    }
}

#[tokio::test]
async fn test_memory_limit_within_bounds() {
    let storage = MockS5Storage::new();
    let manifest_path = "home/vector-databases/0xALICE/db1/manifest.json";
    let base_path = "home/vector-databases/0xALICE/db1";

    // Create small dataset that fits in memory
    // 1000 vectors * 384 dims * 4 bytes = ~1.5 MB
    let manifest = create_encrypted_manifest("0xALICE", 384, 1000);
    storage.set_data(manifest_path, manifest).await;

    let chunk = create_encrypted_chunk(1000, 384);
    storage.set_data(&format!("{}/chunk-0.json", base_path), chunk);

    // Create loader with 100MB memory limit (plenty of room)
    let loader = VectorLoader::with_memory_limit(S5Storage::clone(&storage), 5, 100);
    let session_key = test_session_key();

    let result = loader
        .load_vectors_from_s5(manifest_path, "0xALICE", &session_key, None)
        .await;

    assert!(result.is_ok(), "Should accept dataset within memory limit");
}

// ============================================================================
// Test Category 5: Timeout Enforcement
// ============================================================================

#[tokio::test]
async fn test_loading_timeout() {
    let storage = MockS5Storage::new();
    let manifest_path = "home/vector-databases/0xALICE/db1/manifest.json";

    // Set very slow download delay
    storage.set_download_delay(Duration::from_secs(10));

    let manifest = create_encrypted_manifest("0xALICE", 384, 100);
    storage.set_data(manifest_path, manifest).await;

    // Create loader with 1 second timeout
    let loader = VectorLoader::with_timeout(S5Storage::clone(&storage), 5, Duration::from_secs(1));
    let session_key = test_session_key();

    let result = loader
        .load_vectors_from_s5(manifest_path, "0xALICE", &session_key, None)
        .await;

    assert!(result.is_err(), "Should timeout on slow download");

    let err = result.unwrap_err();
    match err {
        VectorLoadError::Timeout { duration_sec } => {
            assert_eq!(duration_sec, 1);
        }
        _ => panic!("Expected Timeout error, got: {:?}", err),
    }
}

#[tokio::test]
async fn test_timeout_with_progress() {
    let storage = MockS5Storage::new();
    let manifest_path = "home/vector-databases/0xALICE/db1/manifest.json";

    // Set moderate delay that will exceed timeout
    storage.set_download_delay(Duration::from_millis(500));

    let manifest = create_encrypted_manifest("0xALICE", 384, 100);
    storage.set_data(manifest_path, manifest).await;

    let (progress_tx, mut progress_rx) = mpsc::channel::<LoadProgress>(10);

    // Create loader with 1 second timeout
    let loader = VectorLoader::with_timeout(S5Storage::clone(&storage), 5, Duration::from_secs(1));
    let session_key = test_session_key();

    let result = loader
        .load_vectors_from_s5(manifest_path, "0xALICE", &session_key, Some(progress_tx))
        .await;

    // Should receive at least ManifestDownloaded before timeout
    let first_progress = progress_rx.try_recv();
    assert!(
        first_progress.is_ok(),
        "Should receive progress before timeout"
    );

    assert!(result.is_err(), "Should timeout even with partial progress");
}

// ============================================================================
// Test Category 6: Decryption Key Security
// ============================================================================

#[tokio::test]
async fn test_session_key_not_logged() {
    // This is a documentation test - in real implementation,
    // verify that session keys are NEVER logged in debug/info logs

    let storage = MockS5Storage::new();
    let manifest_path = "home/vector-databases/0xALICE/db1/manifest.json";
    let base_path = "home/vector-databases/0xALICE/db1";

    let manifest = create_encrypted_manifest("0xALICE", 384, 100);
    storage.set_data(manifest_path, manifest).await;

    let chunk = create_encrypted_chunk(100, 384);
    storage.set_data(&format!("{}/chunk-0.json", base_path), chunk);

    let loader = VectorLoader::new(S5Storage::clone(&storage), 5);
    let session_key = test_session_key();

    let _ = loader
        .load_vectors_from_s5(manifest_path, "0xALICE", &session_key, None)
        .await;

    // Manual verification: Check logs to ensure session_key bytes are NEVER printed
    // This should be enforced in the implementation with custom Debug traits
}
