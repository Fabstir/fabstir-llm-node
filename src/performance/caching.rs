// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use lru::LruCache;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub max_entries: usize,
    pub max_memory_mb: usize,
    pub ttl_seconds: u64,
    pub eviction_policy: EvictionPolicy,
    pub enable_semantic_cache: bool,
    pub similarity_threshold: f32,
    pub warm_cache_on_startup: bool,
    pub persistence_path: Option<PathBuf>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 1000,
            max_memory_mb: 512,
            ttl_seconds: 3600,
            eviction_policy: EvictionPolicy::LRU,
            enable_semantic_cache: true,
            similarity_threshold: 0.95,
            warm_cache_on_startup: false,
            persistence_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EvictionPolicy {
    LRU,
    LFU,
    FIFO,
    TTL,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CacheStatus {
    Hit,
    Miss,
    Expired,
    Evicted,
}

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct CacheKey {
    pub model_id: String,
    pub prompt: String,
    pub parameters_hash: String,
}

impl CacheKey {
    pub fn new(model_id: String, prompt: String, params: &HashMap<String, String>) -> Self {
        let mut hasher = Sha256::new();
        for (k, v) in params {
            hasher.update(k.as_bytes());
            hasher.update(v.as_bytes());
        }
        let parameters_hash = format!("{:x}", hasher.finalize());

        Self {
            model_id,
            prompt,
            parameters_hash,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub response: String,
    pub tokens_saved: usize,
    pub latency_saved_ms: u64,
    pub created_at: Instant,
    pub access_count: u64,
    pub last_accessed: Instant,
    pub embedding: Option<Vec<f32>>,
    pub similarity_score: f32,
    pub is_semantic_match: bool,
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_hits: u64,
    pub total_misses: u64,
    pub hit_rate: f64,
    pub memory_usage_mb: f64,
    pub entries_count: usize,
    pub tokens_saved_total: u64,
    pub latency_saved_total_ms: u64,
    pub evictions_count: u64,
    // Additional fields expected by tests
    pub total_entries: usize,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub total_tokens_saved: u64,
    pub batch_efficiency: f64,
}

#[derive(Debug, Clone)]
pub struct SemanticCacheEntry {
    pub key: CacheKey,
    pub embedding: Vec<f32>,
    pub entry: CacheEntry,
}

pub type SimilarityThreshold = f32;
pub type SemanticCache = InferenceCache;

#[derive(Error, Debug)]
pub enum CacheError {
    #[error("Cache miss for key: {key:?}")]
    CacheMiss { key: CacheKey },
    #[error("Cache is full")]
    CacheFull,
    #[error("Invalid embedding dimension")]
    InvalidEmbedding,
    #[error("Persistence error: {0}")]
    PersistenceError(String),
}

// Mock embedding generator
pub struct EmbeddingGenerator {
    dimension: usize,
    model_name: String,
}

impl EmbeddingGenerator {
    // Constructor that accepts a model name (sync version for tests)
    pub fn new(model_name: &str) -> Self {
        // Mock: Different models have different dimensions
        let dimension = match model_name {
            "sentence-transformers/all-MiniLM-L6-v2" => 384,
            _ => 512,
        };
        Self {
            dimension,
            model_name: model_name.to_string(),
        }
    }

    pub fn with_dimension(dimension: usize) -> Self {
        Self {
            dimension,
            model_name: "default".to_string(),
        }
    }

    pub async fn generate(&self, text: &str) -> Result<Vec<f32>> {
        // Mock embedding generation based on text hash
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        let hash = hasher.finalize();

        let mut embedding = Vec::with_capacity(self.dimension);
        for i in 0..self.dimension {
            let byte_idx = i % hash.len();
            let value = (hash[byte_idx] as f32) / 255.0;
            embedding.push(value);
        }

        // Normalize
        let magnitude: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if magnitude > 0.0 {
            for val in &mut embedding {
                *val /= magnitude;
            }
        }

        Ok(embedding)
    }

    // Alias for generate that tests expect
    pub async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>> {
        self.generate(text).await
    }

    pub fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        Self::calculate_cosine_similarity(a, b)
    }

    pub fn calculate_cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot_product: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        dot_product // Assuming normalized vectors
    }
}

struct CacheState {
    lru_cache: LruCache<CacheKey, CacheEntry>,
    semantic_entries: Vec<SemanticCacheEntry>,
    memory_usage_bytes: usize,
    stats: CacheStats,
    embedding_generator: EmbeddingGenerator,
}

#[derive(Clone)]
pub struct InferenceCache {
    config: CacheConfig,
    state: Arc<RwLock<CacheState>>,
}

impl InferenceCache {
    pub async fn new(config: CacheConfig) -> Result<Self> {
        let lru_cache = LruCache::new(NonZeroUsize::new(config.max_entries).unwrap());
        let embedding_generator = EmbeddingGenerator::with_dimension(384); // Mock dimension

        let state = CacheState {
            lru_cache,
            semantic_entries: Vec::new(),
            memory_usage_bytes: 0,
            stats: CacheStats {
                total_hits: 0,
                total_misses: 0,
                hit_rate: 0.0,
                memory_usage_mb: 0.0,
                entries_count: 0,
                tokens_saved_total: 0,
                latency_saved_total_ms: 0,
                evictions_count: 0,
                total_entries: 0,
                cache_hits: 0,
                cache_misses: 0,
                total_tokens_saved: 0,
                batch_efficiency: 1.0,
            },
            embedding_generator,
        };

        let cache = Self {
            config,
            state: Arc::new(RwLock::new(state)),
        };

        // Load from persistence if configured
        if let Some(ref path) = cache.config.persistence_path {
            if path.exists() {
                cache.load_from_disk().await.ok();
            }
        }

        Ok(cache)
    }

    pub async fn get(&self, key: &CacheKey) -> Result<CacheEntry> {
        let mut state = self.state.write().await;

        // Check exact match first
        if let Some(entry) = state.lru_cache.get_mut(key) {
            let entry_clone = entry.clone();
            entry.access_count += 1;
            entry.last_accessed = Instant::now();

            // Update stats after we're done with the entry
            state.stats.total_hits += 1;
            state.stats.cache_hits += 1;
            state.stats.hit_rate = state.stats.total_hits as f64
                / (state.stats.total_hits + state.stats.total_misses) as f64;
            return Ok(entry_clone);
        }

        // Try semantic search if enabled
        if self.config.enable_semantic_cache {
            let prompt_embedding = state.embedding_generator.generate(&key.prompt).await?;

            let mut best_match: Option<(f32, &SemanticCacheEntry)> = None;
            for semantic_entry in &state.semantic_entries {
                if semantic_entry.key.model_id == key.model_id {
                    let similarity = EmbeddingGenerator::calculate_cosine_similarity(
                        &prompt_embedding,
                        &semantic_entry.embedding,
                    );

                    if similarity >= self.config.similarity_threshold {
                        match best_match {
                            None => best_match = Some((similarity, semantic_entry)),
                            Some((best_sim, _)) if similarity > best_sim => {
                                best_match = Some((similarity, semantic_entry));
                            }
                            _ => {}
                        }
                    }
                }
            }

            if let Some((similarity, semantic_entry)) = best_match {
                let mut entry = semantic_entry.entry.clone();
                entry.access_count += 1;
                entry.last_accessed = Instant::now();
                entry.similarity_score = similarity;
                entry.is_semantic_match = true;
                let total_hits = state.stats.total_hits + 1;
                let total_misses = state.stats.total_misses;
                state.stats.total_hits = total_hits;
                state.stats.cache_hits = total_hits;
                state.stats.hit_rate = total_hits as f64 / (total_hits + total_misses) as f64;
                return Ok(entry);
            }
        }

        let total_hits = state.stats.total_hits;
        let total_misses = state.stats.total_misses + 1;
        state.stats.total_misses = total_misses;
        state.stats.cache_misses = total_misses;
        state.stats.hit_rate = total_hits as f64 / (total_hits + total_misses) as f64;
        Err(CacheError::CacheMiss { key: key.clone() }.into())
    }

    // Semantic search method that tests expect
    pub async fn get_semantic(&self, key: &CacheKey) -> Result<CacheEntry> {
        // First try exact match
        if let Ok(entry) = self.get(key).await {
            return Ok(entry);
        }

        if !self.config.enable_semantic_cache {
            return Err(CacheError::CacheMiss { key: key.clone() }.into());
        }

        // Generate embedding for the prompt (do this before acquiring write lock)
        let prompt_embedding = {
            let state = self.state.read().await;
            state
                .embedding_generator
                .generate_embedding(&key.prompt)
                .await?
        };

        // Now acquire write lock for semantic search
        let mut state = self.state.write().await;

        // Find best semantic match
        let mut best_match: Option<(f32, &SemanticCacheEntry)> = None;
        for semantic_entry in &state.semantic_entries {
            let similarity = self.cosine_similarity(&prompt_embedding, &semantic_entry.embedding);
            if similarity >= self.config.similarity_threshold {
                match best_match {
                    None => best_match = Some((similarity, semantic_entry)),
                    Some((best_sim, _)) if similarity > best_sim => {
                        best_match = Some((similarity, semantic_entry));
                    }
                    _ => {}
                }
            }
        }

        if let Some((similarity, semantic_entry)) = best_match {
            let mut entry = semantic_entry.entry.clone();
            entry.access_count += 1;
            entry.last_accessed = Instant::now();
            entry.similarity_score = similarity;
            entry.is_semantic_match = true;
            let total_hits = state.stats.total_hits + 1;
            let total_misses = state.stats.total_misses;
            state.stats.total_hits = total_hits;
            state.stats.cache_hits = total_hits;
            state.stats.hit_rate = total_hits as f64 / (total_hits + total_misses) as f64;
            return Ok(entry);
        }

        Err(CacheError::CacheMiss { key: key.clone() }.into())
    }

    fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        EmbeddingGenerator::calculate_cosine_similarity(a, b)
    }

    pub async fn put(&self, key: &CacheKey, response: &str, tokens: usize) -> Result<()> {
        let mut state = self.state.write().await;

        let entry_size = response.len() + std::mem::size_of::<CacheEntry>();
        let max_memory_bytes = self.config.max_memory_mb * 1024 * 1024;

        // Check memory limit
        while state.memory_usage_bytes + entry_size > max_memory_bytes
            && !state.lru_cache.is_empty()
        {
            // Evict oldest entry
            if let Some((evicted_key, evicted_entry)) = state.lru_cache.pop_lru() {
                state.memory_usage_bytes -=
                    evicted_entry.response.len() + std::mem::size_of::<CacheEntry>();
                state.stats.evictions_count += 1;

                // Remove from semantic cache if present
                state.semantic_entries.retain(|se| se.key != evicted_key);
            }
        }

        // Generate embedding for semantic cache
        let embedding = if self.config.enable_semantic_cache {
            Some(state.embedding_generator.generate(&key.prompt).await?)
        } else {
            None
        };

        let entry = CacheEntry {
            response: response.to_string(),
            tokens_saved: tokens,
            latency_saved_ms: tokens as u64 * 10, // Mock: 10ms per token
            created_at: Instant::now(),
            access_count: 1,
            last_accessed: Instant::now(),
            embedding: embedding.clone(),
            similarity_score: 1.0,    // Exact match when storing
            is_semantic_match: false, // Direct storage
        };

        // Add to LRU cache
        state.lru_cache.put(key.clone(), entry.clone());
        state.memory_usage_bytes += entry_size;
        state.stats.entries_count = state.lru_cache.len();
        state.stats.total_entries = state.lru_cache.len();
        state.stats.memory_usage_mb = state.memory_usage_bytes as f64 / (1024.0 * 1024.0);
        state.stats.tokens_saved_total += tokens as u64;
        state.stats.total_tokens_saved += tokens as u64;
        state.stats.latency_saved_total_ms += entry.latency_saved_ms;

        // Add to semantic cache if enabled
        if let Some(embedding) = embedding {
            state.semantic_entries.push(SemanticCacheEntry {
                key: key.clone(),
                embedding,
                entry,
            });
        }

        Ok(())
    }

    pub async fn invalidate(&self, key: &CacheKey) -> Result<()> {
        let mut state = self.state.write().await;

        if let Some(entry) = state.lru_cache.pop(key) {
            state.memory_usage_bytes -= entry.response.len() + std::mem::size_of::<CacheEntry>();
            state.stats.entries_count = state.lru_cache.len();
            state.stats.total_entries = state.lru_cache.len();
            state.stats.memory_usage_mb = state.memory_usage_bytes as f64 / (1024.0 * 1024.0);

            // Remove from semantic cache
            state.semantic_entries.retain(|se| se.key != *key);
        }

        Ok(())
    }

    pub async fn clear(&self) -> Result<()> {
        let mut state = self.state.write().await;
        state.lru_cache.clear();
        state.semantic_entries.clear();
        state.memory_usage_bytes = 0;
        state.stats.entries_count = 0;
        state.stats.total_entries = 0;
        state.stats.memory_usage_mb = 0.0;
        Ok(())
    }

    pub async fn evict_under_memory_pressure(&self, target_memory_mb: u64) -> Result<u64> {
        let mut state = self.state.write().await;
        let target_bytes = target_memory_mb * 1024 * 1024;
        let mut evicted_count = 0;

        // Evict entries until we're under the target memory
        while state.memory_usage_bytes > target_bytes as usize && !state.lru_cache.is_empty() {
            // Get the LRU entry
            let oldest_key = state
                .lru_cache
                .iter()
                .min_by_key(|(_, entry)| entry.last_accessed)
                .map(|(k, _)| k.clone());

            if let Some(key) = oldest_key {
                if let Some(entry) = state.lru_cache.pop(&key) {
                    // Calculate approximate memory freed
                    let entry_size = std::mem::size_of_val(&entry)
                        + entry.response.len()
                        + entry.embedding.as_ref().map(|e| e.len() * 4).unwrap_or(0);

                    state.memory_usage_bytes = state.memory_usage_bytes.saturating_sub(entry_size);
                    evicted_count += 1;

                    // Remove from semantic entries too
                    state.semantic_entries.retain(|se| se.key != key);
                }
            } else {
                break;
            }
        }

        // Update stats
        state.stats.cache_misses += evicted_count;
        state.stats.total_entries = state.lru_cache.len();

        Ok(evicted_count)
    }

    pub async fn get_stats(&self) -> CacheStats {
        let state = self.state.read().await;
        state.stats.clone()
    }

    pub async fn persist_to_disk(&self) -> Result<()> {
        if let Some(ref path) = self.config.persistence_path {
            // Mock persistence
            tokio::fs::create_dir_all(path.parent().unwrap()).await?;
            tokio::fs::write(path, b"cache_data").await?;
            Ok(())
        } else {
            Err(CacheError::PersistenceError("No persistence path configured".to_string()).into())
        }
    }

    // Alias for persist_to_disk that tests expect
    pub async fn persist(&self) -> Result<()> {
        self.persist_to_disk().await
    }

    async fn load_from_disk(&self) -> Result<()> {
        if let Some(ref path) = self.config.persistence_path {
            if path.exists() {
                // Mock loading
                let _data = tokio::fs::read(path).await?;
                // In real implementation, deserialize and populate cache
            }
        }
        Ok(())
    }

    pub async fn check_ttl(&self) -> Result<()> {
        let mut state = self.state.write().await;
        let ttl = Duration::from_secs(self.config.ttl_seconds);
        let now = Instant::now();

        let mut expired_keys = Vec::new();

        // Collect expired keys
        for (key, entry) in state.lru_cache.iter() {
            if now.duration_since(entry.created_at) > ttl {
                expired_keys.push(key.clone());
            }
        }

        // Remove expired entries
        for key in expired_keys {
            if let Some(entry) = state.lru_cache.pop(&key) {
                state.memory_usage_bytes -=
                    entry.response.len() + std::mem::size_of::<CacheEntry>();
                state.semantic_entries.retain(|se| se.key != key);
            }
        }

        state.stats.entries_count = state.lru_cache.len();
        state.stats.total_entries = state.lru_cache.len();
        state.stats.memory_usage_mb = state.memory_usage_bytes as f64 / (1024.0 * 1024.0);

        Ok(())
    }
}

// Semantic cache wrapper is now defined as type alias above
// pub type SemanticCache = InferenceCache;

// Cache warming functionality
pub struct CacheWarming {
    cache: Arc<InferenceCache>,
}

impl CacheWarming {
    pub fn new(cache: Arc<InferenceCache>) -> Self {
        Self { cache }
    }
    pub async fn warm_from_prompts(
        &self,
        model_id: &str,
        prompt_response_pairs: Vec<(String, String)>,
    ) -> Result<()> {
        for (prompt, response) in prompt_response_pairs {
            let key = CacheKey {
                model_id: model_id.to_string(),
                prompt: prompt.clone(),
                parameters_hash: "default".to_string(),
            };

            let tokens = response.len() / 4; // Approximate token count

            self.cache.put(&key, &response, tokens).await?;
        }
        Ok(())
    }

    pub async fn warm_cache(
        cache: &InferenceCache,
        prompts: Vec<(&str, &str, &str)>,
    ) -> Result<()> {
        for (model_id, prompt, response) in prompts {
            let key = CacheKey {
                model_id: model_id.to_string(),
                prompt: prompt.to_string(),
                parameters_hash: "default".to_string(),
            };

            cache.put(&key, response, 100).await?;
        }

        Ok(())
    }

    pub async fn warm_from_popular(cache: &InferenceCache) -> Result<()> {
        let popular_prompts = vec![
            (
                "llama-7b",
                "What is the capital of France?",
                "The capital of France is Paris.",
            ),
            (
                "llama-7b",
                "Explain quantum computing",
                "Quantum computing uses quantum bits...",
            ),
            (
                "llama-7b",
                "Write a Python hello world",
                "print('Hello, World!')",
            ),
        ];

        Self::warm_cache(cache, popular_prompts).await
    }
}
