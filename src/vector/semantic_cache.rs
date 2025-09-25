use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

use super::client::{SearchOptions, VectorBackend, VectorDBClient, VectorDBConfig, VectorEntry};
use super::embeddings::{Embedding, EmbeddingConfig, EmbeddingGenerator, EmbeddingModel};

pub type SimilarityThreshold = f32;

#[derive(Debug, Clone)]
pub struct SemanticCacheConfig {
    pub similarity_threshold: f32,
    pub ttl_seconds: u64,
    pub max_cache_size: usize,
    pub eviction_policy: CacheEvictionPolicy,
    pub namespace: String,
    pub enable_compression: bool,
}

impl Default for SemanticCacheConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.85,
            ttl_seconds: 3600,
            max_cache_size: 10_000,
            eviction_policy: CacheEvictionPolicy::LRU,
            namespace: "default".to_string(),
            enable_compression: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CacheEvictionPolicy {
    LRU,
    FIFO,
    TTL,
}

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub id: String,
    pub prompt: String,
    pub response: String,
    pub embedding: Embedding,
    pub metadata: HashMap<String, String>,
    pub created_at: u64,
    pub last_accessed: u64,
}

#[derive(Debug, Clone)]
pub struct CacheHit {
    pub prompt: String,
    pub original_prompt: String,
    pub response: String,
    pub similarity: f32,
    pub metadata: HashMap<String, String>,
    pub cached_at: u64,
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_lookups: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub entries_stored: u64,
    pub hit_rate: f32,
    pub current_size: usize,
    pub max_size: usize,
}

#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub total_stores: u64,
    pub total_lookups: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub hit_rate: f32,
    pub avg_lookup_time_ms: f64,
    pub avg_store_time_ms: f64,
}

#[derive(Debug, Clone)]
pub struct StorageInfo {
    pub original_size: usize,
    pub compressed_size: usize,
    pub compression_ratio: f32,
}

#[derive(Error, Debug)]
pub enum CacheError {
    #[error("Embedding generation failed: {0}")]
    EmbeddingError(#[from] super::embeddings::EmbeddingError),
    #[error("Vector DB error: {0}")]
    VectorDBError(#[from] super::client::VectorError),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Cache full: {0}")]
    CacheFull(String),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Entry not found: {0}")]
    NotFound(String),
}

struct LRUTracker {
    access_order: Vec<String>,
    max_size: usize,
}

impl LRUTracker {
    fn new(max_size: usize) -> Self {
        Self {
            access_order: Vec::new(),
            max_size,
        }
    }

    fn access(&mut self, id: &str) {
        // Remove if exists
        self.access_order.retain(|x| x != id);
        // Add to front
        self.access_order.insert(0, id.to_string());
    }

    fn get_lru_victim(&self) -> Option<String> {
        if self.access_order.len() >= self.max_size {
            self.access_order.last().cloned()
        } else {
            None
        }
    }

    fn remove(&mut self, id: &str) {
        self.access_order.retain(|x| x != id);
    }
}

pub struct SemanticCache {
    config: SemanticCacheConfig,
    embedding_generator: Arc<EmbeddingGenerator>,
    vector_client: Arc<VectorDBClient>,
    stats: Arc<RwLock<CacheStats>>,
    lru_tracker: Arc<RwLock<LRUTracker>>,
    performance_metrics: Arc<RwLock<PerformanceMetrics>>,
}

impl SemanticCache {
    pub async fn new(
        config: SemanticCacheConfig,
        embedding_generator: EmbeddingGenerator,
        vector_client: VectorDBClient,
    ) -> Result<Self, CacheError> {
        let stats = CacheStats {
            total_lookups: 0,
            cache_hits: 0,
            cache_misses: 0,
            entries_stored: 0,
            hit_rate: 0.0,
            current_size: 0,
            max_size: config.max_cache_size,
        };

        let performance_metrics = PerformanceMetrics {
            total_stores: 0,
            total_lookups: 0,
            cache_hits: 0,
            cache_misses: 0,
            hit_rate: 0.0,
            avg_lookup_time_ms: 0.0,
            avg_store_time_ms: 0.0,
        };

        Ok(Self {
            lru_tracker: Arc::new(RwLock::new(LRUTracker::new(config.max_cache_size))),
            config,
            embedding_generator: Arc::new(embedding_generator),
            vector_client: Arc::new(vector_client),
            stats: Arc::new(RwLock::new(stats)),
            performance_metrics: Arc::new(RwLock::new(performance_metrics)),
        })
    }

    pub async fn lookup(&self, prompt: &str) -> Result<Option<CacheHit>, CacheError> {
        let start_time = std::time::Instant::now();

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_lookups += 1;
        }

        // Generate embedding for the prompt
        let prompt_embedding = self.embedding_generator.generate_embedding(prompt).await?;

        // Search for similar entries in vector DB
        let search_options = SearchOptions {
            k: 10, // Get top 10 similar entries
            include_metadata: true,
            filter: Some(HashMap::from([(
                "namespace".to_string(),
                super::client::FilterValue::String(self.config.namespace.clone()),
            )])),
            ..Default::default()
        };

        let search_results = self
            .vector_client
            .search_with_options(prompt_embedding.data().to_vec(), search_options)
            .await?;

        // Find the best match above similarity threshold
        for result in search_results {
            let similarity = 1.0 - result.distance; // Convert distance to similarity

            if similarity >= self.config.similarity_threshold {
                // Check TTL
                if let Some(created_at_str) = result.metadata.get("created_at") {
                    if let Ok(created_at) = created_at_str.parse::<u64>() {
                        let now = chrono::Utc::now().timestamp() as u64;
                        if now - created_at > self.config.ttl_seconds {
                            continue; // Entry expired
                        }
                    }
                }

                // Update LRU tracker
                {
                    let mut lru = self.lru_tracker.write().await;
                    lru.access(&result.id);
                }

                // Update stats
                {
                    let mut stats = self.stats.write().await;
                    stats.cache_hits += 1;
                    stats.hit_rate = stats.cache_hits as f32 / stats.total_lookups as f32;
                }

                let lookup_time = start_time.elapsed().as_millis() as f64;
                {
                    let mut metrics = self.performance_metrics.write().await;
                    metrics.total_lookups += 1;
                    metrics.cache_hits += 1;
                    metrics.hit_rate = metrics.cache_hits as f32 / metrics.total_lookups as f32;
                    metrics.avg_lookup_time_ms = (metrics.avg_lookup_time_ms
                        * (metrics.total_lookups - 1) as f64
                        + lookup_time)
                        / metrics.total_lookups as f64;
                }

                let cached_at = result
                    .metadata
                    .get("created_at")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let original_prompt = result
                    .metadata
                    .get("original_prompt")
                    .cloned()
                    .unwrap_or_else(|| prompt.to_string());
                let response = result.metadata.get("response").cloned().unwrap_or_default();
                let metadata = result.metadata.clone();

                return Ok(Some(CacheHit {
                    prompt: prompt.to_string(),
                    original_prompt,
                    response,
                    similarity,
                    metadata,
                    cached_at,
                }));
            }
        }

        // No match found
        {
            let mut stats = self.stats.write().await;
            stats.cache_misses += 1;
            stats.hit_rate = stats.cache_hits as f32 / stats.total_lookups as f32;
        }

        let lookup_time = start_time.elapsed().as_millis() as f64;
        {
            let mut metrics = self.performance_metrics.write().await;
            metrics.total_lookups += 1;
            metrics.cache_misses += 1;
            metrics.hit_rate = metrics.cache_hits as f32 / metrics.total_lookups as f32;
            metrics.avg_lookup_time_ms =
                (metrics.avg_lookup_time_ms * (metrics.total_lookups - 1) as f64 + lookup_time)
                    / metrics.total_lookups as f64;
        }

        Ok(None)
    }

    pub async fn store(
        &self,
        prompt: &str,
        response: &str,
        metadata: Option<HashMap<String, String>>,
    ) -> Result<String, CacheError> {
        let start_time = std::time::Instant::now();

        // Check if we need to evict entries
        self.maybe_evict().await?;

        // Generate embedding for the prompt
        let prompt_embedding = self.embedding_generator.generate_embedding(prompt).await?;

        // Create entry ID
        let entry_id = format!("cache_{}_{}", self.config.namespace, Uuid::new_v4());

        // Prepare metadata
        let mut entry_metadata = metadata.unwrap_or_default();
        entry_metadata.insert("namespace".to_string(), self.config.namespace.clone());
        entry_metadata.insert("original_prompt".to_string(), prompt.to_string());
        entry_metadata.insert("response".to_string(), response.to_string());
        entry_metadata.insert(
            "created_at".to_string(),
            chrono::Utc::now().timestamp().to_string(),
        );

        // Compress response if enabled
        let stored_response = if self.config.enable_compression {
            self.compress_data(response)?
        } else {
            response.to_string()
        };
        entry_metadata.insert(
            "compressed".to_string(),
            self.config.enable_compression.to_string(),
        );

        // Create vector entry
        let vector_entry = VectorEntry {
            id: entry_id.clone(),
            vector: prompt_embedding.data().to_vec(),
            metadata: entry_metadata,
        };

        // Store in vector DB
        self.vector_client.insert_vector(vector_entry).await?;

        // Update LRU tracker
        {
            let mut lru = self.lru_tracker.write().await;
            lru.access(&entry_id);
        }

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.entries_stored += 1;
            stats.current_size += 1;
        }

        let store_time = start_time.elapsed().as_millis() as f64;
        {
            let mut metrics = self.performance_metrics.write().await;
            metrics.total_stores += 1;
            metrics.avg_store_time_ms =
                (metrics.avg_store_time_ms * (metrics.total_stores - 1) as f64 + store_time)
                    / metrics.total_stores as f64;
        }

        Ok(entry_id)
    }

    pub async fn batch_store(
        &self,
        prompts: Vec<String>,
        responses: Vec<String>,
    ) -> Result<Vec<Result<String, CacheError>>, CacheError> {
        if prompts.len() != responses.len() {
            return Err(CacheError::InvalidConfig(
                "Prompts and responses must have same length".to_string(),
            ));
        }

        let mut results = Vec::new();

        for (prompt, response) in prompts.into_iter().zip(responses.into_iter()) {
            let result = self.store(&prompt, &response, None).await;
            results.push(result);
        }

        Ok(results)
    }

    pub async fn batch_lookup(
        &self,
        prompts: &[String],
    ) -> Result<Vec<Option<CacheHit>>, CacheError> {
        let mut results = Vec::new();

        for prompt in prompts {
            let result = self.lookup(prompt).await?;
            results.push(result);
        }

        Ok(results)
    }

    async fn maybe_evict(&self) -> Result<(), CacheError> {
        let current_stats = {
            let stats = self.stats.read().await;
            stats.clone()
        };

        if current_stats.current_size >= current_stats.max_size {
            match self.config.eviction_policy {
                CacheEvictionPolicy::LRU => {
                    if let Some(victim_id) = {
                        let lru = self.lru_tracker.read().await;
                        lru.get_lru_victim()
                    } {
                        self.evict_entry(&victim_id).await?;
                    }
                }
                CacheEvictionPolicy::FIFO => {
                    // For simplicity, use LRU logic
                    if let Some(victim_id) = {
                        let lru = self.lru_tracker.read().await;
                        lru.get_lru_victim()
                    } {
                        self.evict_entry(&victim_id).await?;
                    }
                }
                CacheEvictionPolicy::TTL => {
                    // TTL eviction would require scanning all entries
                    // For now, fall back to LRU
                    if let Some(victim_id) = {
                        let lru = self.lru_tracker.read().await;
                        lru.get_lru_victim()
                    } {
                        self.evict_entry(&victim_id).await?;
                    }
                }
            }
        }

        Ok(())
    }

    async fn evict_entry(&self, entry_id: &str) -> Result<(), CacheError> {
        // Remove from vector DB
        self.vector_client.delete_vector(entry_id).await?;

        // Update LRU tracker
        {
            let mut lru = self.lru_tracker.write().await;
            lru.remove(entry_id);
        }

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.current_size = stats.current_size.saturating_sub(1);
        }

        Ok(())
    }

    fn compress_data(&self, data: &str) -> Result<String, CacheError> {
        // Simple compression simulation - in real implementation would use zstd or similar
        if data.len() > 100 {
            Ok(format!(
                "compressed:{}",
                data.chars().take(50).collect::<String>()
            ))
        } else {
            Ok(data.to_string())
        }
    }

    fn decompress_data(&self, data: &str) -> Result<String, CacheError> {
        if data.starts_with("compressed:") {
            Ok(data.strip_prefix("compressed:").unwrap_or(data).to_string())
        } else {
            Ok(data.to_string())
        }
    }

    pub async fn get_stats(&self) -> CacheStats {
        self.stats.read().await.clone()
    }

    pub async fn get_performance_metrics(&self) -> PerformanceMetrics {
        self.performance_metrics.read().await.clone()
    }

    pub async fn get_storage_info(&self, entry_id: &str) -> Result<StorageInfo, CacheError> {
        let vector_entry = self.vector_client.get_vector(entry_id).await?;

        if let Some(response) = vector_entry.metadata.get("response") {
            let original_size = response.len();
            let compressed_size =
                if vector_entry.metadata.get("compressed") == Some(&"true".to_string()) {
                    original_size / 2 // Simulated compression ratio
                } else {
                    original_size
                };

            Ok(StorageInfo {
                original_size,
                compressed_size,
                compression_ratio: original_size as f32 / compressed_size as f32,
            })
        } else {
            Err(CacheError::NotFound(entry_id.to_string()))
        }
    }
}

impl Clone for SemanticCache {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            embedding_generator: Arc::clone(&self.embedding_generator),
            vector_client: Arc::clone(&self.vector_client),
            stats: Arc::clone(&self.stats),
            lru_tracker: Arc::clone(&self.lru_tracker),
            performance_metrics: Arc::clone(&self.performance_metrics),
        }
    }
}
