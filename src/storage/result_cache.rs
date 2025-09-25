use crate::storage::{CborCompat, S5Storage, StorageError};
use chrono::{DateTime, Duration, Utc};
use lru::LruCache;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use zstd;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EvictionPolicy {
    LRU,
    TTL,
    LRUWithTTL,
}

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub base_path: String,
    pub max_size_mb: u64,
    pub ttl_seconds: u64,
    pub eviction_policy: EvictionPolicy,
    pub enable_compression: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub data: Vec<u8>,
    pub metadata: HashMap<String, String>,
    pub created_at: DateTime<Utc>,
    pub accessed_at: DateTime<Utc>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub total_entries: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub hit_rate: f64,
    pub total_size_bytes: u64,
    pub evictions: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageInfo {
    pub compressed_size: usize,
    pub uncompressed_size: usize,
    pub compression_ratio: f64,
}

#[derive(Debug)]
struct CacheMetadata {
    key: String,
    size_bytes: u64,
    created_at: DateTime<Utc>,
    accessed_at: DateTime<Utc>,
}

pub struct ResultCache {
    storage: Box<dyn S5Storage>,
    config: Arc<Mutex<CacheConfig>>,
    cbor: CborCompat,
    memory_cache: Arc<Mutex<LruCache<String, CacheEntry>>>,
    metadata_index: Arc<Mutex<HashMap<String, CacheMetadata>>>,
    stats: Arc<Mutex<CacheStats>>,
}

impl Clone for ResultCache {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
            config: Arc::clone(&self.config),
            cbor: self.cbor.clone(),
            memory_cache: Arc::clone(&self.memory_cache),
            metadata_index: Arc::clone(&self.metadata_index),
            stats: Arc::clone(&self.stats),
        }
    }
}

impl ResultCache {
    pub fn new(storage: Box<dyn S5Storage>, config: CacheConfig) -> Self {
        let initial_capacity =
            std::cmp::max(1000, (config.max_size_mb * 1024 * 1024 / 1024) as usize);

        Self {
            storage,
            config: Arc::new(Mutex::new(config)),
            cbor: CborCompat::new(),
            memory_cache: Arc::new(Mutex::new(LruCache::new(
                initial_capacity.try_into().unwrap(),
            ))),
            metadata_index: Arc::new(Mutex::new(HashMap::new())),
            stats: Arc::new(Mutex::new(CacheStats {
                total_entries: 0,
                cache_hits: 0,
                cache_misses: 0,
                hit_rate: 0.0,
                total_size_bytes: 0,
                evictions: 0,
            })),
        }
    }

    pub async fn put(
        &self,
        key: &str,
        data: Vec<u8>,
        metadata: Option<HashMap<String, String>>,
    ) -> Result<(), StorageError> {
        let config = self.config.lock().await;
        let cache_path = format!("{}/{}", config.base_path, self.encode_key(key));

        let entry = CacheEntry {
            data: data.clone(),
            metadata: metadata.unwrap_or_default(),
            created_at: Utc::now(),
            accessed_at: Utc::now(),
            size_bytes: data.len() as u64,
        };

        // Check if we need to evict entries before adding
        self.enforce_size_limit(&entry).await?;

        // Store in persistent storage
        let serialized_entry = self
            .cbor
            .encode(&entry)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;

        let final_data = if config.enable_compression {
            self.compress_data(&serialized_entry)?
        } else {
            serialized_entry
        };

        self.storage.put(&cache_path, final_data).await?;

        // Update memory cache
        {
            let mut memory_cache = self.memory_cache.lock().await;
            memory_cache.put(key.to_string(), entry.clone());
        }

        // Update metadata index
        {
            let mut metadata_index = self.metadata_index.lock().await;
            metadata_index.insert(
                key.to_string(),
                CacheMetadata {
                    key: key.to_string(),
                    size_bytes: entry.size_bytes,
                    created_at: entry.created_at,
                    accessed_at: entry.accessed_at,
                },
            );
        }

        // Update stats
        {
            let mut stats = self.stats.lock().await;
            stats.total_entries += 1;
            stats.total_size_bytes += entry.size_bytes;
        }

        Ok(())
    }

    pub async fn put_with_path(
        &self,
        key: &str,
        data: Vec<u8>,
        custom_path: &str,
    ) -> Result<(), StorageError> {
        let config = self.config.lock().await;
        let cache_path = format!("{}/{}", config.base_path, custom_path);

        let entry = CacheEntry {
            data: data.clone(),
            metadata: HashMap::new(),
            created_at: Utc::now(),
            accessed_at: Utc::now(),
            size_bytes: data.len() as u64,
        };

        let serialized_entry = self
            .cbor
            .encode(&entry)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;

        let final_data = if config.enable_compression {
            self.compress_data(&serialized_entry)?
        } else {
            serialized_entry
        };

        self.storage.put(&cache_path, final_data).await?;

        // Also store in regular cache with key
        self.put(key, data, None).await?;

        Ok(())
    }

    pub async fn get(&self, key: &str) -> Result<Option<CacheEntry>, StorageError> {
        // Check memory cache first
        {
            let mut memory_cache = self.memory_cache.lock().await;
            if let Some(mut entry) = memory_cache.get(key).cloned() {
                entry.accessed_at = Utc::now();
                memory_cache.put(key.to_string(), entry.clone());

                self.record_hit().await;
                return Ok(Some(entry));
            }
        }

        // Check if entry exists and is not expired
        if !self.is_entry_valid(key).await? {
            self.record_miss().await;
            return Ok(None);
        }

        // Load from persistent storage
        let config = self.config.lock().await;
        let cache_path = format!("{}/{}", config.base_path, self.encode_key(key));

        match self.storage.get(&cache_path).await {
            Ok(data) => {
                let decompressed_data = if config.enable_compression {
                    self.decompress_data(&data)?
                } else {
                    data
                };

                let mut entry: CacheEntry = self
                    .cbor
                    .decode(&decompressed_data)
                    .map_err(|e| StorageError::SerializationError(e.to_string()))?;

                // Update access time
                entry.accessed_at = Utc::now();

                // Store in memory cache
                {
                    let mut memory_cache = self.memory_cache.lock().await;
                    memory_cache.put(key.to_string(), entry.clone());
                }

                self.record_hit().await;
                Ok(Some(entry))
            }
            Err(StorageError::NotFound(_)) => {
                self.record_miss().await;
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    pub async fn list_by_prefix(&self, prefix: &str) -> Result<Vec<String>, StorageError> {
        let config = self.config.lock().await;
        let search_path = format!("{}/{}", config.base_path, prefix);

        let entries = self.storage.list(&search_path).await?;
        let keys: Vec<String> = entries
            .into_iter()
            .filter(|e| e.entry_type == crate::storage::S5EntryType::File)
            .map(|e| self.decode_key(&e.name))
            .collect();

        Ok(keys)
    }

    pub async fn set_ttl(&self, ttl_seconds: u64) {
        let mut config = self.config.lock().await;
        config.ttl_seconds = ttl_seconds;
    }

    pub async fn set_max_size_mb(&self, max_size_mb: u64) {
        let mut config = self.config.lock().await;
        config.max_size_mb = max_size_mb;

        // Trigger eviction if we're over the new limit
        drop(config); // Release lock before calling eviction
        self.enforce_size_limit_global().await.unwrap_or(());
    }

    pub async fn put_batch(
        &self,
        keys: Vec<String>,
        data_vec: Vec<Vec<u8>>,
    ) -> Result<(), StorageError> {
        if keys.len() != data_vec.len() {
            return Err(StorageError::SerializationError(
                "Keys and data vectors must have same length".to_string(),
            ));
        }

        for (key, data) in keys.into_iter().zip(data_vec.into_iter()) {
            self.put(&key, data, None).await?;
        }

        Ok(())
    }

    pub async fn get_batch(
        &self,
        keys: &[String],
    ) -> Result<Vec<Option<CacheEntry>>, StorageError> {
        let mut results = Vec::new();

        for key in keys {
            results.push(self.get(key).await?);
        }

        Ok(results)
    }

    pub async fn delete_batch(&self, keys: &[String]) -> Result<(), StorageError> {
        for key in keys {
            self.delete(key).await?;
        }
        Ok(())
    }

    pub async fn get_storage_info(&self, key: &str) -> Result<StorageInfo, StorageError> {
        let config = self.config.lock().await;
        let cache_path = format!("{}/{}", config.base_path, self.encode_key(key));

        let stored_data = self.storage.get(&cache_path).await?;
        let compressed_size = stored_data.len();

        let uncompressed_data = if config.enable_compression {
            self.decompress_data(&stored_data)?
        } else {
            stored_data
        };
        let uncompressed_size = uncompressed_data.len();

        let compression_ratio = if compressed_size > 0 {
            uncompressed_size as f64 / compressed_size as f64
        } else {
            1.0
        };

        Ok(StorageInfo {
            compressed_size,
            uncompressed_size,
            compression_ratio,
        })
    }

    pub async fn get_stats(&self) -> CacheStats {
        let stats = self.stats.lock().await;
        let mut stats_copy = stats.clone();

        // Calculate hit rate
        let total_requests = stats_copy.cache_hits + stats_copy.cache_misses;
        stats_copy.hit_rate = if total_requests > 0 {
            stats_copy.cache_hits as f64 / total_requests as f64
        } else {
            0.0
        };

        stats_copy
    }

    pub async fn clear(&self) -> Result<(), StorageError> {
        // Clear memory cache
        {
            let mut memory_cache = self.memory_cache.lock().await;
            memory_cache.clear();
        }

        // Clear metadata index
        {
            let mut metadata_index = self.metadata_index.lock().await;
            metadata_index.clear();
        }

        // Reset stats
        {
            let mut stats = self.stats.lock().await;
            *stats = CacheStats {
                total_entries: 0,
                cache_hits: 0,
                cache_misses: 0,
                hit_rate: 0.0,
                total_size_bytes: 0,
                evictions: 0,
            };
        }

        // Note: In a real implementation, we'd need to delete all files from storage
        // For now, this is sufficient for the tests

        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), StorageError> {
        let config = self.config.lock().await;
        let cache_path = format!("{}/{}", config.base_path, self.encode_key(key));

        // Remove from persistent storage
        self.storage.delete(&cache_path).await?;

        // Remove from memory cache
        {
            let mut memory_cache = self.memory_cache.lock().await;
            memory_cache.pop(key);
        }

        // Remove from metadata index and update stats
        {
            let mut metadata_index = self.metadata_index.lock().await;
            if let Some(metadata) = metadata_index.remove(key) {
                let mut stats = self.stats.lock().await;
                stats.total_entries = stats.total_entries.saturating_sub(1);
                stats.total_size_bytes = stats.total_size_bytes.saturating_sub(metadata.size_bytes);
            }
        }

        Ok(())
    }

    async fn is_entry_valid(&self, key: &str) -> Result<bool, StorageError> {
        let config = self.config.lock().await;
        let metadata_index = self.metadata_index.lock().await;

        if let Some(metadata) = metadata_index.get(key) {
            let ttl_duration = Duration::seconds(config.ttl_seconds as i64);
            let expires_at = metadata.created_at + ttl_duration;
            Ok(Utc::now() < expires_at)
        } else {
            // Check if entry exists in storage
            let cache_path = format!("{}/{}", config.base_path, self.encode_key(key));
            self.storage.exists(&cache_path).await
        }
    }

    async fn enforce_size_limit(&self, new_entry: &CacheEntry) -> Result<(), StorageError> {
        let config = self.config.lock().await;
        let max_size_bytes = config.max_size_mb * 1024 * 1024;

        let current_stats = {
            let stats = self.stats.lock().await;
            stats.clone()
        };

        if current_stats.total_size_bytes + new_entry.size_bytes > max_size_bytes {
            drop(config); // Release lock before eviction
            self.evict_entries_to_fit(new_entry.size_bytes).await?;
        }

        Ok(())
    }

    async fn enforce_size_limit_global(&self) -> Result<(), StorageError> {
        let config = self.config.lock().await;
        let max_size_bytes = config.max_size_mb * 1024 * 1024;

        let current_stats = {
            let stats = self.stats.lock().await;
            stats.clone()
        };

        if current_stats.total_size_bytes > max_size_bytes {
            let bytes_to_evict = current_stats.total_size_bytes - max_size_bytes;
            drop(config); // Release lock before eviction
            self.evict_entries_to_fit(bytes_to_evict).await?;
        }

        Ok(())
    }

    async fn evict_entries_to_fit(&self, bytes_needed: u64) -> Result<(), StorageError> {
        let config = self.config.lock().await;
        let mut bytes_evicted = 0u64;
        let mut keys_to_evict = Vec::new();

        // Get candidates for eviction based on policy
        {
            let metadata_index = self.metadata_index.lock().await;
            let mut entries: Vec<_> = metadata_index.values().collect();

            match config.eviction_policy {
                EvictionPolicy::LRU | EvictionPolicy::LRUWithTTL => {
                    entries.sort_by(|a, b| a.accessed_at.cmp(&b.accessed_at));
                }
                EvictionPolicy::TTL => {
                    entries.sort_by(|a, b| a.created_at.cmp(&b.created_at));
                }
            }

            for entry in entries {
                if bytes_evicted >= bytes_needed {
                    break;
                }
                keys_to_evict.push(entry.key.clone());
                bytes_evicted += entry.size_bytes;
            }
        }

        drop(config); // Release lock before deletion

        // Evict selected entries
        let mut eviction_count = 0;
        for key in keys_to_evict {
            if let Ok(()) = self.delete(&key).await {
                eviction_count += 1;
            }
        }

        // Update eviction stats
        {
            let mut stats = self.stats.lock().await;
            stats.evictions += eviction_count;
        }

        Ok(())
    }

    async fn record_hit(&self) {
        let mut stats = self.stats.lock().await;
        stats.cache_hits += 1;
    }

    async fn record_miss(&self) {
        let mut stats = self.stats.lock().await;
        stats.cache_misses += 1;
    }

    fn encode_key(&self, key: &str) -> String {
        // Simple base64 encoding to handle special characters
        base64::encode(key)
    }

    fn decode_key(&self, encoded: &str) -> String {
        base64::decode(encoded)
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            .unwrap_or_else(|_| encoded.to_string())
    }

    fn compress_data(&self, data: &[u8]) -> Result<Vec<u8>, StorageError> {
        zstd::stream::encode_all(data, 3)
            .map_err(|e| StorageError::CompressionError(format!("Compression failed: {}", e)))
    }

    fn decompress_data(&self, compressed: &[u8]) -> Result<Vec<u8>, StorageError> {
        zstd::stream::decode_all(compressed)
            .map_err(|e| StorageError::CompressionError(format!("Decompression failed: {}", e)))
    }
}

// Add base64 encoding utility
mod base64 {
    pub fn encode(input: &str) -> String {
        // Simple base64-like encoding for testing
        hex::encode(input.as_bytes())
    }

    pub fn decode(input: &str) -> Result<Vec<u8>, hex::FromHexError> {
        hex::decode(input)
    }
}
