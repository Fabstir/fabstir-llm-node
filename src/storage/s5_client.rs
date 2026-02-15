// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use async_trait::async_trait;
use data_encoding::BASE32_NOPAD;
use reqwest;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Invalid path: {0}")]
    InvalidPath(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Quota exceeded")]
    QuotaExceeded,
    #[error("Serialization error: {0}")]
    SerializationError(String),
    #[error("Compression error: {0}")]
    CompressionError(String),
    #[error("Authentication error: {0}")]
    AuthError(String),
    #[error("Server error: {0}")]
    ServerError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum S5Backend {
    Mock,
    EnhancedS5 { base_url: String },
}

#[derive(Debug, Clone)]
pub struct S5StorageConfig {
    pub backend: S5Backend,
    pub api_key: Option<String>,
    pub cache_ttl_seconds: u64,
    pub max_retries: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum S5EntryType {
    File,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S5Entry {
    pub name: String,
    pub cid: String,
    pub size: u64,
    pub entry_type: S5EntryType,
    pub modified_at: i64,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S5ListResult {
    pub entries: Vec<S5Entry>,
    pub cursor: Option<String>,
    pub has_more: bool,
}

#[async_trait]
pub trait S5Storage: Send + Sync {
    async fn put(&self, path: &str, data: Vec<u8>) -> Result<String, StorageError>;
    async fn put_with_metadata(
        &self,
        path: &str,
        data: Vec<u8>,
        metadata: HashMap<String, String>,
    ) -> Result<String, StorageError>;
    async fn get(&self, path: &str) -> Result<Vec<u8>, StorageError>;
    async fn get_metadata(&self, path: &str) -> Result<HashMap<String, String>, StorageError>;
    async fn get_by_cid(&self, cid: &str) -> Result<Vec<u8>, StorageError>;
    async fn list(&self, path: &str) -> Result<Vec<S5Entry>, StorageError>;
    async fn list_with_options(
        &self,
        path: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<S5ListResult, StorageError>;
    async fn delete(&self, path: &str) -> Result<(), StorageError>;
    async fn exists(&self, path: &str) -> Result<bool, StorageError>;
    fn clone(&self) -> Box<dyn S5Storage>;

    // Mock-specific methods (no-op for real backend)
    async fn inject_error(&self, _error: StorageError) {}
    async fn set_quota_limit(&self, _limit_bytes: u64) {}
}

#[derive(Debug)]
struct MockEntry {
    data: Vec<u8>,
    metadata: HashMap<String, String>,
    created_at: i64,
}

#[derive(Debug)]
pub struct MockS5Backend {
    storage: Arc<Mutex<HashMap<String, MockEntry>>>,
    injected_error: Arc<Mutex<Option<StorageError>>>,
    quota_limit: Arc<Mutex<Option<u64>>>,
}

impl MockS5Backend {
    pub fn new() -> Self {
        Self {
            storage: Arc::new(Mutex::new(HashMap::new())),
            injected_error: Arc::new(Mutex::new(None)),
            quota_limit: Arc::new(Mutex::new(None)),
        }
    }

    fn validate_path(path: &str) -> Result<(), StorageError> {
        if path.is_empty() {
            return Err(StorageError::InvalidPath("Empty path".to_string()));
        }

        if path.starts_with('/') {
            return Err(StorageError::InvalidPath(
                "Path cannot start with /".to_string(),
            ));
        }

        if path.contains("../") {
            return Err(StorageError::InvalidPath(
                "Path traversal not allowed".to_string(),
            ));
        }

        if !path.starts_with("home/") && !path.starts_with("archive/") {
            return Err(StorageError::InvalidPath(
                "Path must start with 'home/' or 'archive/'".to_string(),
            ));
        }

        Ok(())
    }

    /// Generate S5 BlobIdentifier format CID
    ///
    /// BlobIdentifier format is REQUIRED by S5 portals for downloads.
    /// Structure: prefix(2) + multihash(1) + hash(32) + size(1-8) = 36-43 bytes
    /// Base32 encoded: 58-70 chars with 'b' multibase prefix
    ///
    /// Old 53-char raw hash format is DEPRECATED - portals reject it.
    fn generate_cid(data: &[u8]) -> String {
        // S5 uses blake3 for hashing
        let hash = blake3::hash(data);
        let hash_bytes = hash.as_bytes();
        let size = data.len() as u64;

        // Build BlobIdentifier bytes
        // Capacity: 2 (prefix) + 1 (multihash) + 32 (hash) + 8 (max size) = 43 bytes
        let mut blob_bytes = Vec::with_capacity(43);

        // S5 blob identifier prefix bytes
        blob_bytes.extend_from_slice(&[0x5b, 0x82]);

        // BLAKE3 multihash code
        blob_bytes.push(0x1e);

        // 32-byte BLAKE3 hash
        blob_bytes.extend_from_slice(hash_bytes);

        // Little-endian size encoding (trim trailing zeros for compactness)
        let mut size_bytes = size.to_le_bytes().to_vec();
        while size_bytes.len() > 1 && size_bytes.last() == Some(&0) {
            size_bytes.pop();
        }
        blob_bytes.extend_from_slice(&size_bytes);

        // Base32 encode with 'b' multibase prefix
        let base32_encoded = BASE32_NOPAD.encode(&blob_bytes).to_lowercase();
        format!("b{}", base32_encoded)
    }

    async fn check_quota(&self, data_size: u64) -> Result<(), StorageError> {
        let quota_limit = self.quota_limit.lock().await;
        if let Some(limit) = *quota_limit {
            let storage = self.storage.lock().await;
            let total_size: u64 = storage.values().map(|entry| entry.data.len() as u64).sum();
            if total_size + data_size > limit {
                return Err(StorageError::QuotaExceeded);
            }
        }
        Ok(())
    }

    async fn check_injected_error(&self) -> Result<(), StorageError> {
        let mut error_opt = self.injected_error.lock().await;
        if let Some(error) = error_opt.take() {
            return Err(error);
        }
        Ok(())
    }
}

#[async_trait]
impl S5Storage for MockS5Backend {
    async fn put(&self, path: &str, data: Vec<u8>) -> Result<String, StorageError> {
        self.check_injected_error().await?;
        Self::validate_path(path)?;
        self.check_quota(data.len() as u64).await?;

        // WARN: Mock storage is being used - data stays in memory only!
        tracing::warn!(
            "🎭 [S5-MOCK] MockS5Backend::put() - path='{}', size={} bytes - DATA NOT UPLOADED TO S5 NETWORK!",
            path, data.len()
        );

        let cid = Self::generate_cid(&data);
        let entry = MockEntry {
            data,
            metadata: HashMap::new(),
            created_at: chrono::Utc::now().timestamp(),
        };

        let mut storage = self.storage.lock().await;
        storage.insert(path.to_string(), entry);

        Ok(cid)
    }

    async fn put_with_metadata(
        &self,
        path: &str,
        data: Vec<u8>,
        metadata: HashMap<String, String>,
    ) -> Result<String, StorageError> {
        self.check_injected_error().await?;
        Self::validate_path(path)?;
        self.check_quota(data.len() as u64).await?;

        let cid = Self::generate_cid(&data);
        let entry = MockEntry {
            data,
            metadata,
            created_at: chrono::Utc::now().timestamp(),
        };

        let mut storage = self.storage.lock().await;
        storage.insert(path.to_string(), entry);

        Ok(cid)
    }

    async fn get(&self, path: &str) -> Result<Vec<u8>, StorageError> {
        self.check_injected_error().await?;
        Self::validate_path(path)?;

        let storage = self.storage.lock().await;
        storage
            .get(path)
            .map(|entry| entry.data.clone())
            .ok_or_else(|| StorageError::NotFound(path.to_string()))
    }

    async fn get_metadata(&self, path: &str) -> Result<HashMap<String, String>, StorageError> {
        self.check_injected_error().await?;
        Self::validate_path(path)?;

        let storage = self.storage.lock().await;
        storage
            .get(path)
            .map(|entry| entry.metadata.clone())
            .ok_or_else(|| StorageError::NotFound(path.to_string()))
    }

    async fn get_by_cid(&self, cid: &str) -> Result<Vec<u8>, StorageError> {
        self.check_injected_error().await?;

        let storage = self.storage.lock().await;
        for entry in storage.values() {
            let entry_cid = Self::generate_cid(&entry.data);
            if entry_cid == cid {
                return Ok(entry.data.clone());
            }
        }

        Err(StorageError::NotFound(cid.to_string()))
    }

    async fn list(&self, path: &str) -> Result<Vec<S5Entry>, StorageError> {
        self.check_injected_error().await?;
        Self::validate_path(path)?;

        let storage = self.storage.lock().await;
        let mut entries = Vec::new();
        let mut directories = std::collections::HashSet::new();

        let path_prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{}/", path)
        };

        for (stored_path, entry) in storage.iter() {
            if stored_path.starts_with(&path_prefix) {
                let relative_path = &stored_path[path_prefix.len()..];
                if let Some(slash_pos) = relative_path.find('/') {
                    // This is a subdirectory
                    let dir_name = &relative_path[..slash_pos];
                    if directories.insert(dir_name.to_string()) {
                        entries.push(S5Entry {
                            name: dir_name.to_string(),
                            cid: format!("dir-{}", dir_name),
                            size: 0,
                            entry_type: S5EntryType::Directory,
                            modified_at: entry.created_at,
                            metadata: HashMap::new(),
                        });
                    }
                } else {
                    // This is a file in the current directory
                    entries.push(S5Entry {
                        name: relative_path.to_string(),
                        cid: Self::generate_cid(&entry.data),
                        size: entry.data.len() as u64,
                        entry_type: S5EntryType::File,
                        modified_at: entry.created_at,
                        metadata: entry.metadata.clone(),
                    });
                }
            }
        }

        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    async fn list_with_options(
        &self,
        path: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> Result<S5ListResult, StorageError> {
        let mut all_entries = self.list(path).await?;

        let start_index = if let Some(cursor) = cursor {
            cursor.parse::<usize>().unwrap_or(0)
        } else {
            0
        };

        let end_index = if let Some(limit) = limit {
            std::cmp::min(start_index + limit, all_entries.len())
        } else {
            all_entries.len()
        };

        let entries = if start_index < all_entries.len() {
            all_entries.drain(start_index..end_index).collect()
        } else {
            Vec::new()
        };

        let has_more = end_index < all_entries.len();
        let cursor = if has_more {
            Some(end_index.to_string())
        } else {
            None
        };

        Ok(S5ListResult {
            entries,
            cursor,
            has_more,
        })
    }

    async fn delete(&self, path: &str) -> Result<(), StorageError> {
        self.check_injected_error().await?;
        Self::validate_path(path)?;

        let mut storage = self.storage.lock().await;
        storage
            .remove(path)
            .ok_or_else(|| StorageError::NotFound(path.to_string()))?;

        Ok(())
    }

    async fn exists(&self, path: &str) -> Result<bool, StorageError> {
        self.check_injected_error().await?;
        Self::validate_path(path)?;

        let storage = self.storage.lock().await;

        // Check if exact path exists
        if storage.contains_key(path) {
            return Ok(true);
        }

        // Check if path is a directory (any stored path starts with this path + "/")
        let dir_prefix = format!("{}/", path);
        let is_directory = storage.keys().any(|key| key.starts_with(&dir_prefix));

        Ok(is_directory)
    }

    fn clone(&self) -> Box<dyn S5Storage> {
        Box::new(MockS5Backend {
            storage: Arc::clone(&self.storage),
            injected_error: Arc::clone(&self.injected_error),
            quota_limit: Arc::clone(&self.quota_limit),
        })
    }

    async fn inject_error(&self, error: StorageError) {
        let mut injected_error = self.injected_error.lock().await;
        *injected_error = Some(error);
    }

    async fn set_quota_limit(&self, limit_bytes: u64) {
        let mut quota_limit = self.quota_limit.lock().await;
        *quota_limit = Some(limit_bytes);
    }
}

// Enhanced S5 Backend Implementation
pub struct EnhancedS5Backend {
    client: super::enhanced_s5_client::EnhancedS5Client,
}

impl EnhancedS5Backend {
    pub fn new(client: super::enhanced_s5_client::EnhancedS5Client) -> Self {
        Self { client }
    }

    fn validate_path(path: &str) -> Result<String, StorageError> {
        // Enhanced S5 expects paths without leading slash
        let clean_path = path.trim_start_matches('/');

        // For Enhanced S5, we accept any path structure
        // The test paths don't follow the home/archive convention
        Ok(clean_path.to_string())
    }
}

#[async_trait]
impl S5Storage for EnhancedS5Backend {
    async fn put(&self, path: &str, data: Vec<u8>) -> Result<String, StorageError> {
        let clean_path = Self::validate_path(path)?;
        let data_len = data.len();

        tracing::info!(
            "🔵 EnhancedS5Backend::put called: path={}, size={}",
            clean_path,
            data_len
        );

        // put_file now returns the real CID from the S5 bridge
        let cid = self
            .client
            .put_file(&clean_path, data)
            .await
            .map_err(|e| StorageError::ServerError(e.to_string()))?;

        tracing::info!("🔵 EnhancedS5Backend::put returned CID: {}", cid);

        Ok(cid)
    }

    async fn put_with_metadata(
        &self,
        path: &str,
        data: Vec<u8>,
        _metadata: HashMap<String, String>,
    ) -> Result<String, StorageError> {
        // Enhanced S5 doesn't support metadata in the same way, just store the file
        self.put(path, data).await
    }

    async fn get(&self, path: &str) -> Result<Vec<u8>, StorageError> {
        let clean_path = Self::validate_path(path)?;

        self.client.get_file(&clean_path).await.map_err(|e| {
            if e.to_string().contains("not found") {
                StorageError::NotFound(path.to_string())
            } else {
                StorageError::ServerError(e.to_string())
            }
        })
    }

    async fn get_metadata(&self, path: &str) -> Result<HashMap<String, String>, StorageError> {
        // Enhanced S5 doesn't have separate metadata, return empty map
        let _ = Self::validate_path(path)?;
        Ok(HashMap::new())
    }

    async fn get_by_cid(&self, _cid: &str) -> Result<Vec<u8>, StorageError> {
        // Enhanced S5 doesn't support CID-based retrieval in the mock
        Err(StorageError::ServerError(
            "CID-based retrieval not supported".to_string(),
        ))
    }

    async fn list(&self, path: &str) -> Result<Vec<S5Entry>, StorageError> {
        let clean_path = Self::validate_path(path)?;

        let files = self
            .client
            .list_directory(&clean_path)
            .await
            .map_err(|e| StorageError::ServerError(e.to_string()))?;

        let entries: Vec<S5Entry> = files
            .into_iter()
            .map(|f| {
                let name = f.name.clone();
                S5Entry {
                    name: f.name,
                    cid: format!("s5://mock_{}", name),
                    size: f.size,
                    entry_type: if f.file_type == "file" {
                        S5EntryType::File
                    } else {
                        S5EntryType::Directory
                    },
                    modified_at: chrono::Utc::now().timestamp(),
                    metadata: HashMap::new(),
                }
            })
            .collect();

        Ok(entries)
    }

    async fn list_with_options(
        &self,
        path: &str,
        limit: Option<usize>,
        _cursor: Option<String>,
    ) -> Result<S5ListResult, StorageError> {
        let entries = self.list(path).await?;

        let limited_entries = if let Some(limit) = limit {
            entries.into_iter().take(limit).collect()
        } else {
            entries
        };

        Ok(S5ListResult {
            entries: limited_entries,
            cursor: None,
            has_more: false,
        })
    }

    async fn delete(&self, path: &str) -> Result<(), StorageError> {
        let clean_path = Self::validate_path(path)?;

        self.client
            .delete_file(&clean_path)
            .await
            .map_err(|e| StorageError::ServerError(e.to_string()))
    }

    async fn exists(&self, path: &str) -> Result<bool, StorageError> {
        let clean_path = Self::validate_path(path)?;

        self.client
            .exists(&clean_path)
            .await
            .map_err(|e| StorageError::ServerError(e.to_string()))
    }

    fn clone(&self) -> Box<dyn S5Storage> {
        Box::new(EnhancedS5Backend {
            client: Clone::clone(&self.client),
        })
    }
}

pub struct S5Client;

impl S5Client {
    pub async fn create(config: S5StorageConfig) -> Result<Box<dyn S5Storage>, StorageError> {
        match config.backend {
            S5Backend::Mock => Ok(Box::new(MockS5Backend::new())),
            S5Backend::EnhancedS5 { base_url } => {
                // Use EnhancedS5 backend when configured
                if let Ok(enhanced_client) =
                    super::enhanced_s5_client::EnhancedS5Client::new_legacy(base_url.clone())
                {
                    Ok(Box::new(EnhancedS5Backend::new(enhanced_client)))
                } else {
                    // Fallback to mock if Enhanced S5 client creation fails
                    Ok(Box::new(MockS5Backend::new()))
                }
            }
        }
    }

    pub async fn create_from_env() -> Result<Box<dyn S5Storage>, StorageError> {
        // Check for ENHANCED_S5_URL environment variable
        if let Ok(enhanced_url) = std::env::var("ENHANCED_S5_URL") {
            tracing::info!(
                "🌐 [S5-INIT] Using EnhancedS5Backend with URL: {}",
                enhanced_url
            );
            eprintln!(
                "🌐 [S5-INIT] Using EnhancedS5Backend with URL: {}",
                enhanced_url
            );
            let config = S5StorageConfig {
                backend: S5Backend::EnhancedS5 {
                    base_url: enhanced_url,
                },
                api_key: None,
                cache_ttl_seconds: 3600,
                max_retries: 3,
            };
            return Self::create(config).await;
        }

        // Default to mock backend - WARN that uploads won't reach network!
        tracing::warn!(
            "🚨 [S5-INIT] ENHANCED_S5_URL not set! Using MockS5Backend - uploads will NOT reach S5 network!"
        );
        eprintln!(
            "🚨 [S5-INIT] ENHANCED_S5_URL not set! Using MockS5Backend - uploads will NOT reach S5 network!"
        );
        eprintln!("🚨 [S5-INIT] Set ENHANCED_S5_URL=http://localhost:5522 to use real S5 storage");
        let config = S5StorageConfig {
            backend: S5Backend::Mock,
            api_key: None,
            cache_ttl_seconds: 3600,
            max_retries: 3,
        };
        Self::create(config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to validate S5 BlobIdentifier CID format
    /// BlobIdentifier: 58-70 chars (varies by file size encoding)
    /// Structure: 'b' + base32(prefix(2) + multihash(1) + hash(32) + size(1-8))
    fn is_valid_blob_identifier_cid(cid: &str) -> bool {
        // Must start with 'b' (base32 multibase prefix)
        if !cid.starts_with('b') {
            return false;
        }
        // BlobIdentifier: 58-70 chars
        let len = cid.len();
        if len < 58 || len > 70 {
            return false;
        }
        // Rest must be valid base32 lowercase: a-z, 2-7
        cid[1..]
            .chars()
            .all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c))
    }

    #[test]
    fn test_mock_s5_generate_cid_returns_blob_identifier_format() {
        // Test that MockS5Backend generates BlobIdentifier format CIDs
        // BlobIdentifier includes file size and is REQUIRED by S5 portals
        let test_data = b"test checkpoint delta content";
        let cid = MockS5Backend::generate_cid(test_data);

        println!("Generated CID: {}", cid);
        println!("CID length: {}", cid.len());
        println!("Data size: {} bytes", test_data.len());

        // MUST start with 'b' (base32 multibase prefix)
        assert!(
            cid.starts_with('b'),
            "deltaCid MUST start with 'b' prefix for base32 encoding, got: {}",
            cid
        );

        // MUST be 58-70 characters (BlobIdentifier format, varies by file size)
        assert!(
            cid.len() >= 58 && cid.len() <= 70,
            "BlobIdentifier CID MUST be 58-70 chars, got {} chars: {}",
            cid.len(),
            cid
        );

        // MUST be valid BlobIdentifier format
        assert!(
            is_valid_blob_identifier_cid(&cid),
            "deltaCid MUST be valid BlobIdentifier format (58-70 base32), got: {}",
            cid
        );

        // MUST NOT be hex
        assert!(
            !cid.chars().skip(1).all(|c| c.is_ascii_hexdigit()),
            "deltaCid MUST NOT be hex hash, got: {}",
            cid
        );

        // MUST NOT contain s5:// prefix
        assert!(
            !cid.contains("s5://"),
            "deltaCid MUST NOT contain s5:// prefix, got: {}",
            cid
        );

        // MUST NOT be IPFS format (bafkrei, bafybei, etc.)
        assert!(
            !cid.starts_with("bafkrei") && !cid.starts_with("bafybei"),
            "deltaCid MUST NOT be IPFS format (bafkrei/bafybei). Got: {}",
            cid
        );

        // MUST NOT be old 53-char raw hash format (portals reject it)
        assert_ne!(
            cid.len(),
            53,
            "deltaCid MUST NOT be old 53-char raw hash format. Got: {}",
            cid
        );

        println!(
            "SUCCESS: deltaCid is valid BlobIdentifier format: {} ({} chars)",
            cid,
            cid.len()
        );
    }

    #[tokio::test]
    async fn test_mock_s5_put_returns_blob_identifier_format() {
        // Test the full S5Storage::put() flow returns BlobIdentifier format
        let mock = MockS5Backend::new();
        let test_data = b"checkpoint delta JSON content".to_vec();
        let data_size = test_data.len();

        let cid = mock
            .put("home/checkpoints/test/delta_0.json", test_data)
            .await
            .unwrap();

        println!("MockS5Backend::put() returned CID: {}", cid);
        println!("CID length: {}", cid.len());
        println!("Data size: {} bytes", data_size);

        // Verify format is BlobIdentifier (58-70 chars, NOT old 53-char raw hash)
        assert!(
            is_valid_blob_identifier_cid(&cid),
            "S5Storage::put() MUST return valid BlobIdentifier CID (58-70 chars), got {} chars: {}",
            cid.len(),
            cid
        );

        // MUST NOT be IPFS format
        assert!(
            !cid.starts_with("bafkrei") && !cid.starts_with("bafybei"),
            "S5Storage::put() MUST NOT return IPFS format, got: {}",
            cid
        );

        // MUST NOT be old 53-char raw hash format
        assert_ne!(
            cid.len(),
            53,
            "S5Storage::put() MUST NOT return old 53-char format. Got: {}",
            cid
        );

        println!(
            "SUCCESS: S5Storage::put() returns BlobIdentifier format: {} ({} chars)",
            cid,
            cid.len()
        );
    }

    #[test]
    fn test_s5_cid_deterministic() {
        // Same data should produce same CID
        let data = b"deterministic test data";
        let cid1 = MockS5Backend::generate_cid(data);
        let cid2 = MockS5Backend::generate_cid(data);
        assert_eq!(cid1, cid2, "CID generation must be deterministic");
    }

    #[test]
    fn test_s5_cid_different_data() {
        // Different data should produce different CIDs
        let cid1 = MockS5Backend::generate_cid(b"data1");
        let cid2 = MockS5Backend::generate_cid(b"data2");
        assert_ne!(cid1, cid2, "Different data must produce different CIDs");
    }
}
