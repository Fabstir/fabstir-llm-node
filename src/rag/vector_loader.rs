// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! S5 Vector Database Loader (Sub-phase 3.1)
//!
//! Orchestrates downloading, decrypting, and loading vector databases from S5 storage.
//!
//! ## Features
//!
//! - **Parallel Chunk Downloads**: Downloads multiple chunks concurrently
//! - **AES-GCM Decryption**: Decrypts manifest and chunks encrypted with Web Crypto API
//! - **Owner Verification**: Validates database owner matches session user
//! - **Progress Tracking**: Reports loading status via channels
//! - **Error Handling**: Comprehensive error handling for network and decryption failures
//!
//! ## Flow
//!
//! 1. Download and decrypt manifest
//! 2. Verify owner matches expected user address
//! 3. Download and decrypt chunks in parallel (configurable parallelism)
//! 4. Validate vector dimensions
//! 5. Collect all vectors from chunks
//! 6. Report progress throughout
//!
//! ## Usage
//!
//! ```rust,ignore
//! use fabstir_llm_node::rag::vector_loader::{VectorLoader, LoadProgress};
//! use tokio::sync::mpsc;
//!
//! let loader = VectorLoader::new(s5_client, 5); // 5 parallel downloads
//! let (progress_tx, progress_rx) = mpsc::channel(10);
//!
//! let vectors = loader.load_vectors_from_s5(
//!     "home/vector-databases/0xABC/my-docs/manifest.json",
//!     "0xABC",
//!     &session_key,
//!     Some(progress_tx),
//! ).await?;
//! ```

use crate::crypto::aes_gcm::{decrypt_chunk, decrypt_manifest};
use crate::rag::errors::VectorLoadError;
use crate::storage::manifest::{Manifest, Vector};
use crate::storage::s5_client::S5Storage;
use futures::stream::{self, StreamExt};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::Sender;
use tokio::time::timeout;

/// Progress updates during vector loading
#[derive(Debug, Clone)]
pub enum LoadProgress {
    /// Manifest downloaded and decrypted
    ManifestDownloaded,

    /// Chunk downloaded and decrypted
    ChunkDownloaded { chunk_id: usize, total: usize },

    /// Building index from loaded vectors
    IndexBuilding,

    /// Loading complete
    Complete { vector_count: usize, duration_ms: u64 },
}

/// Rate limiting configuration
#[derive(Debug, Clone)]
struct RateLimit {
    /// Maximum downloads per time window
    max_downloads: usize,
    /// Time window duration
    window: Duration,
    /// Download timestamps for tracking
    downloads: Arc<tokio::sync::Mutex<Vec<Instant>>>,
}

/// Vector loader for S5 storage
///
/// Downloads and decrypts vector databases from S5, with parallel chunk processing.
pub struct VectorLoader {
    /// S5 storage client
    s5_client: Box<dyn S5Storage>,

    /// Maximum number of chunks to download in parallel
    max_parallel_chunks: usize,

    /// Optional rate limit configuration
    rate_limit: Option<RateLimit>,

    /// Optional memory limit in MB
    memory_limit_mb: Option<usize>,

    /// Optional timeout for entire loading operation
    timeout_duration: Option<Duration>,
}

impl VectorLoader {
    /// Create a new vector loader
    ///
    /// # Arguments
    ///
    /// * `s5_client` - S5 storage client
    /// * `max_parallel_chunks` - Maximum number of chunks to download concurrently (recommended: 5-10)
    pub fn new(s5_client: Box<dyn S5Storage>, max_parallel_chunks: usize) -> Self {
        Self {
            s5_client,
            max_parallel_chunks,
            rate_limit: None,
            memory_limit_mb: None,
            timeout_duration: None,
        }
    }

    /// Create a vector loader with rate limiting
    ///
    /// # Arguments
    ///
    /// * `s5_client` - S5 storage client
    /// * `max_parallel_chunks` - Maximum number of chunks to download concurrently
    /// * `max_downloads_per_window` - Maximum number of downloads allowed per time window
    /// * `window` - Time window duration for rate limiting
    pub fn with_rate_limit(
        s5_client: Box<dyn S5Storage>,
        max_parallel_chunks: usize,
        max_downloads_per_window: usize,
        window: Duration,
    ) -> Self {
        Self {
            s5_client,
            max_parallel_chunks,
            rate_limit: Some(RateLimit {
                max_downloads: max_downloads_per_window,
                window,
                downloads: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            }),
            memory_limit_mb: None,
            timeout_duration: None,
        }
    }

    /// Create a vector loader with memory limit
    ///
    /// # Arguments
    ///
    /// * `s5_client` - S5 storage client
    /// * `max_parallel_chunks` - Maximum number of chunks to download concurrently
    /// * `memory_limit_mb` - Maximum memory usage in MB
    pub fn with_memory_limit(
        s5_client: Box<dyn S5Storage>,
        max_parallel_chunks: usize,
        memory_limit_mb: usize,
    ) -> Self {
        Self {
            s5_client,
            max_parallel_chunks,
            rate_limit: None,
            memory_limit_mb: Some(memory_limit_mb),
            timeout_duration: None,
        }
    }

    /// Create a vector loader with timeout
    ///
    /// # Arguments
    ///
    /// * `s5_client` - S5 storage client
    /// * `max_parallel_chunks` - Maximum number of chunks to download concurrently
    /// * `timeout_duration` - Maximum duration for loading operation
    pub fn with_timeout(
        s5_client: Box<dyn S5Storage>,
        max_parallel_chunks: usize,
        timeout_duration: Duration,
    ) -> Self {
        Self {
            s5_client,
            max_parallel_chunks,
            rate_limit: None,
            memory_limit_mb: None,
            timeout_duration: Some(timeout_duration),
        }
    }

    /// Load vectors from S5 storage
    ///
    /// Downloads manifest, verifies owner, downloads chunks in parallel, and returns all vectors.
    ///
    /// # Arguments
    ///
    /// * `manifest_path` - S5 path to encrypted manifest (e.g., "home/vector-databases/0xABC/my-docs/manifest.json")
    /// * `user_address` - Expected owner address for verification
    /// * `session_key` - 32-byte session key for AES-GCM decryption
    /// * `progress_tx` - Optional channel for progress updates
    ///
    /// # Returns
    ///
    /// Vector of all vectors from the database
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Manifest not found or decryption fails
    /// - Owner mismatch
    /// - Chunk download or decryption fails
    /// - Vector validation fails (wrong dimensions, NaN values)
    /// - Database is marked as deleted
    /// - Rate limit exceeded
    /// - Memory limit exceeded
    /// - Timeout exceeded
    pub async fn load_vectors_from_s5(
        &self,
        manifest_path: &str,
        user_address: &str,
        session_key: &[u8],
        progress_tx: Option<Sender<LoadProgress>>,
    ) -> Result<Vec<Vector>, VectorLoadError> {
        // Wrap entire operation in timeout if configured
        if let Some(timeout_duration) = self.timeout_duration {
            match timeout(timeout_duration, self.load_vectors_internal(manifest_path, user_address, session_key, progress_tx)).await {
                Ok(result) => result,
                Err(_) => Err(VectorLoadError::Timeout {
                    duration_sec: timeout_duration.as_secs(),
                }),
            }
        } else {
            self.load_vectors_internal(manifest_path, user_address, session_key, progress_tx).await
        }
    }

    /// Internal loading implementation (wrapped by timeout in public method)
    async fn load_vectors_internal(
        &self,
        manifest_path: &str,
        user_address: &str,
        session_key: &[u8],
        progress_tx: Option<Sender<LoadProgress>>,
    ) -> Result<Vec<Vector>, VectorLoadError> {
        let start_time = Instant::now();

        // 1. Download and decrypt manifest
        let manifest = self
            .download_and_decrypt_manifest(manifest_path, session_key)
            .await?;

        // Report progress
        if let Some(ref tx) = progress_tx {
            let _ = tx.send(LoadProgress::ManifestDownloaded).await;
        }

        // 2. Verify owner
        self.verify_owner(&manifest, user_address)?;

        // 3. Check memory limit before loading
        if let Some(limit_mb) = self.memory_limit_mb {
            self.check_memory_limit(&manifest, limit_mb)?;
        }

        // 4. Check if database is deleted
        if manifest.is_deleted() {
            return Err(VectorLoadError::Other(format!(
                "Cannot load deleted database: {}",
                manifest.name
            )));
        }

        // 5. Validate manifest structure
        manifest.validate().map_err(|e| VectorLoadError::ManifestParseError(e.to_string()))?;

        // 6. Extract base path from manifest path
        let base_path = manifest_path
            .rsplit_once('/')
            .map(|(base, _)| base)
            .ok_or_else(|| VectorLoadError::InvalidPath(manifest_path.to_string()))?;

        // 7. Download and decrypt chunks in parallel
        let vectors = self
            .download_and_decrypt_chunks(&manifest, base_path, session_key, progress_tx.clone())
            .await?;

        // 8. Report completion
        let duration_ms = start_time.elapsed().as_millis() as u64;
        if let Some(ref tx) = progress_tx {
            let _ = tx
                .send(LoadProgress::Complete {
                    vector_count: vectors.len(),
                    duration_ms,
                })
                .await;
        }

        Ok(vectors)
    }

    /// Download and decrypt manifest from S5
    ///
    /// # Arguments
    ///
    /// * `manifest_path` - S5 path to encrypted manifest
    /// * `session_key` - 32-byte session key for decryption
    ///
    /// # Returns
    ///
    /// Decrypted and parsed Manifest
    async fn download_and_decrypt_manifest(
        &self,
        manifest_path: &str,
        session_key: &[u8],
    ) -> Result<Manifest, VectorLoadError> {
        // Check rate limit before download
        if let Some(ref rate_limit) = self.rate_limit {
            self.check_rate_limit(rate_limit).await?;
        }

        // Download encrypted manifest
        let encrypted_data = self
            .s5_client
            .get(manifest_path)
            .await
            .map_err(|e| VectorLoadError::ManifestDownloadFailed {
                path: manifest_path.to_string(),
                source: Box::new(e) as Box<dyn std::error::Error + Send + Sync>,
            })?;

        // Decrypt and parse manifest
        let manifest = decrypt_manifest(&encrypted_data, session_key)
            .map_err(|e| VectorLoadError::DecryptionFailed(e.to_string()))?;

        Ok(manifest)
    }

    /// Download and decrypt chunks in parallel
    ///
    /// # Arguments
    ///
    /// * `manifest` - Manifest with chunk metadata
    /// * `base_path` - Base S5 path (e.g., "home/vector-databases/0xABC/my-docs")
    /// * `session_key` - 32-byte session key for decryption
    /// * `progress_tx` - Optional progress channel
    ///
    /// # Returns
    ///
    /// Vector of all vectors from all chunks
    async fn download_and_decrypt_chunks(
        &self,
        manifest: &Manifest,
        base_path: &str,
        session_key: &[u8],
        progress_tx: Option<Sender<LoadProgress>>,
    ) -> Result<Vec<Vector>, VectorLoadError> {
        // Handle empty manifest
        if manifest.chunks.is_empty() {
            return Ok(vec![]);
        }

        let total_chunks = manifest.chunks.len();
        let expected_dimensions = manifest.dimensions;

        // Create arc references for async closures
        let s5_client = self.s5_client.clone();
        let session_key = session_key.to_vec();
        let base_path = base_path.to_string();
        let rate_limit = self.rate_limit.clone();

        // Download and decrypt chunks in parallel
        let chunk_results: Vec<Result<Vec<Vector>, VectorLoadError>> = stream::iter(manifest.chunks.iter())
            .map(|chunk_meta| {
                let s5_client = s5_client.clone();
                let session_key = session_key.clone();
                let base_path = base_path.clone();
                let chunk_id = chunk_meta.chunk_id;
                let expected_vector_count = chunk_meta.vector_count;
                let expected_dimensions = expected_dimensions;
                let rate_limit = rate_limit.clone();

                async move {
                    // Check rate limit before download
                    if let Some(ref rl) = rate_limit {
                        Self::check_rate_limit_static(rl).await?;
                    }

                    // Download encrypted chunk
                    let chunk_path = format!("{}/chunk-{}.json", base_path, chunk_id);
                    let encrypted_chunk = s5_client
                        .get(&chunk_path)
                        .await
                        .map_err(|e| VectorLoadError::ChunkDownloadFailed {
                            chunk_id,
                            path: chunk_path.clone(),
                            source: Box::new(e) as Box<dyn std::error::Error + Send + Sync>,
                        })?;

                    // Decrypt chunk
                    let chunk = decrypt_chunk(&encrypted_chunk, &session_key)
                        .map_err(|e| VectorLoadError::DecryptionFailed(format!("Chunk {}: {}", chunk_id, e)))?;

                    // Validate dimensions
                    if !chunk.vectors.is_empty() {
                        let actual_dimensions = chunk.vectors[0].vector.len();
                        if actual_dimensions != expected_dimensions {
                            return Err(VectorLoadError::DimensionMismatch {
                                chunk_id,
                                expected: expected_dimensions,
                                actual: actual_dimensions,
                            });
                        }
                    }

                    // Validate vector count
                    if chunk.vectors.len() != expected_vector_count {
                        return Err(VectorLoadError::VectorCountMismatch {
                            chunk_id,
                            expected: expected_vector_count,
                            actual: chunk.vectors.len(),
                        });
                    }

                    // Validate chunk structure
                    chunk
                        .validate(expected_dimensions)
                        .map_err(|e| VectorLoadError::ChunkValidationFailed {
                            chunk_id,
                            reason: e.to_string(),
                        })?;

                    Ok(chunk.vectors)
                }
            })
            .buffer_unordered(self.max_parallel_chunks)
            .collect()
            .await;

        // Collect all vectors from successful chunks
        let mut all_vectors = Vec::new();
        for (i, result) in chunk_results.into_iter().enumerate() {
            let vectors = result?;
            all_vectors.extend(vectors);

            // Report progress for this chunk
            if let Some(ref tx) = progress_tx {
                let _ = tx
                    .send(LoadProgress::ChunkDownloaded {
                        chunk_id: i,
                        total: total_chunks,
                    })
                    .await;
            }

            tracing::debug!("Loaded chunk {}/{} ({} vectors)", i + 1, total_chunks, all_vectors.len());
        }

        Ok(all_vectors)
    }

    /// Verify database owner matches expected user
    ///
    /// # Arguments
    ///
    /// * `manifest` - Manifest to verify
    /// * `expected_owner` - Expected owner address
    ///
    /// # Errors
    ///
    /// Returns error if owner doesn't match
    fn verify_owner(&self, manifest: &Manifest, expected_owner: &str) -> Result<(), VectorLoadError> {
        // Note: Tests require case-sensitive comparison, but existing code uses case-insensitive
        // Keep case-insensitive for backward compatibility
        if manifest.owner.to_lowercase() != expected_owner.to_lowercase() {
            return Err(VectorLoadError::OwnerMismatch {
                expected: expected_owner.to_string(),
                actual: manifest.owner.clone(),
            });
        }
        Ok(())
    }

    /// Check if memory limit would be exceeded by loading this manifest
    fn check_memory_limit(&self, manifest: &Manifest, limit_mb: usize) -> Result<(), VectorLoadError> {
        // Estimate memory usage:
        // vector_count * dimensions * 4 bytes (f32) + metadata overhead
        let vector_bytes = manifest.vector_count * manifest.dimensions * 4;
        let metadata_bytes = manifest.vector_count * 200; // Conservative estimate
        let total_bytes = vector_bytes + metadata_bytes;
        let required_mb = total_bytes / (1024 * 1024);

        if required_mb > limit_mb {
            return Err(VectorLoadError::MemoryLimitExceeded {
                required_mb,
                limit_mb,
            });
        }

        Ok(())
    }

    /// Check rate limit before download (instance method)
    async fn check_rate_limit(&self, rate_limit: &RateLimit) -> Result<(), VectorLoadError> {
        Self::check_rate_limit_static(rate_limit).await
    }

    /// Check rate limit before download (static method for use in async closures)
    async fn check_rate_limit_static(rate_limit: &RateLimit) -> Result<(), VectorLoadError> {
        let mut downloads = rate_limit.downloads.lock().await;

        // Remove downloads outside the time window
        let cutoff = Instant::now() - rate_limit.window;
        downloads.retain(|&timestamp| timestamp > cutoff);

        // Check if limit exceeded
        if downloads.len() >= rate_limit.max_downloads {
            return Err(VectorLoadError::RateLimitExceeded {
                current: downloads.len(),
                limit: rate_limit.max_downloads,
                window_sec: rate_limit.window.as_secs(),
            });
        }

        // Record this download
        downloads.push(Instant::now());

        Ok(())
    }

    /// Get max parallel chunks configuration
    pub fn max_parallel_chunks(&self) -> usize {
        self.max_parallel_chunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::manifest::{ChunkMetadata, Manifest};
    use crate::storage::s5_client::{StorageError, S5Entry, S5ListResult};
    use std::collections::HashMap;

    /// Mock S5Storage for unit tests
    struct MockS5Storage;

    #[async_trait::async_trait]
    impl S5Storage for MockS5Storage {
        async fn put(&self, _path: &str, _data: Vec<u8>) -> Result<String, StorageError> {
            Ok("mock-cid".to_string())
        }

        async fn put_with_metadata(
            &self,
            _path: &str,
            _data: Vec<u8>,
            _metadata: HashMap<String, String>,
        ) -> Result<String, StorageError> {
            Ok("mock-cid".to_string())
        }

        async fn get(&self, _path: &str) -> Result<Vec<u8>, StorageError> {
            Err(StorageError::NotFound("mock".to_string()))
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
            Ok(false)
        }

        fn clone(&self) -> Box<dyn S5Storage> {
            Box::new(MockS5Storage)
        }
    }

    #[test]
    fn test_vector_loader_creation() {
        let storage = Box::new(MockS5Storage);
        let loader = VectorLoader::new(storage, 5);
        assert_eq!(loader.max_parallel_chunks(), 5);
    }

    #[test]
    fn test_owner_verification_success() {
        let storage = Box::new(MockS5Storage);
        let loader = VectorLoader::new(storage, 5);

        let manifest = Manifest {
            name: "test".to_string(),
            owner: "0xABC123".to_string(),
            description: "Test".to_string(),
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

        let result = loader.verify_owner(&manifest, "0xABC123");
        assert!(result.is_ok());
    }

    #[test]
    fn test_owner_verification_case_insensitive() {
        let storage = Box::new(MockS5Storage);
        let loader = VectorLoader::new(storage, 5);

        let manifest = Manifest {
            name: "test".to_string(),
            owner: "0xABC123".to_string(),
            description: "Test".to_string(),
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

        let result = loader.verify_owner(&manifest, "0xabc123"); // lowercase
        assert!(result.is_ok());
    }

    #[test]
    fn test_owner_verification_failure() {
        let storage = Box::new(MockS5Storage);
        let loader = VectorLoader::new(storage, 5);

        let manifest = Manifest {
            name: "test".to_string(),
            owner: "0xABC123".to_string(),
            description: "Test".to_string(),
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

        let result = loader.verify_owner(&manifest, "0xDIFFERENT");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Owner mismatch"));
    }
}
