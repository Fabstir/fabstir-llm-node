// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::{anyhow, Result};
use lru::LruCache;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub max_entries: usize,
    pub max_memory_bytes: usize,
    pub ttl: Duration,
    pub eviction_policy: EvictionPolicy,
    pub enable_semantic_search: bool,
    pub similarity_threshold: f32,
    pub persistence_path: Option<std::path::PathBuf>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 1000,
            max_memory_bytes: 1024 * 1024 * 1024, // 1GB
            ttl: Duration::from_secs(3600),
            eviction_policy: EvictionPolicy::Lru,
            enable_semantic_search: false,
            similarity_threshold: 0.85,
            persistence_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EvictionPolicy {
    Lru,
    Memory,
    Ttl,
}

#[derive(Debug, Clone)]
pub struct CacheKey {
    pub model_id: String,
    pub prompt: String,
    pub temperature: f32,
    pub max_tokens: usize,
}

impl CacheKey {
    pub fn new(model_id: String, prompt: String, temperature: f32, max_tokens: usize) -> Self {
        Self {
            model_id,
            prompt,
            temperature,
            max_tokens,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub response: String,
    pub tokens_generated: usize,
    pub generation_time: Duration,
    pub timestamp: SystemTime,
    pub access_count: usize,
    pub size_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub hits: usize,
    pub misses: usize,
    pub evictions: usize,
    pub total_entries: usize,
    pub memory_usage: usize,
    pub avg_response_time: Duration,
    pub avg_latency: Duration,
}

impl CacheStats {
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

pub struct InferenceCache {
    config: CacheConfig,
    lru_cache: Arc<RwLock<LruCache<String, CacheEntry>>>,
    memory_usage: Arc<RwLock<usize>>,
    stats: Arc<RwLock<CacheStats>>,
    semantic_cache: Option<Arc<SemanticCache>>,
}

impl InferenceCache {
    pub async fn new(config: CacheConfig) -> Result<Self> {
        let lru_cache = LruCache::new(
            NonZeroUsize::new(config.max_entries)
                .ok_or_else(|| anyhow!("Invalid max_entries: must be > 0"))?,
        );

        let semantic_cache = if config.enable_semantic_search {
            Some(Arc::new(SemanticCache::new(config.similarity_threshold)?))
        } else {
            None
        };

        Ok(Self {
            config,
            lru_cache: Arc::new(RwLock::new(lru_cache)),
            memory_usage: Arc::new(RwLock::new(0)),
            stats: Arc::new(RwLock::new(CacheStats {
                hits: 0,
                misses: 0,
                evictions: 0,
                total_entries: 0,
                memory_usage: 0,
                avg_response_time: Duration::default(),
                avg_latency: Duration::default(),
            })),
            semantic_cache,
        })
    }

    pub fn size(&self) -> usize {
        futures::executor::block_on(async { self.lru_cache.read().await.len() })
    }

    pub fn memory_usage(&self) -> usize {
        futures::executor::block_on(async { *self.memory_usage.read().await })
    }

    pub async fn get(&self, key: &CacheKey) -> Option<CacheEntry> {
        let key_str = self.hash_key(key);

        let mut cache = self.lru_cache.write().await;
        if let Some(entry) = cache.get_mut(&key_str) {
            // Check TTL
            if entry.timestamp.elapsed().unwrap_or(Duration::MAX) < self.config.ttl {
                entry.access_count += 1;

                // Update stats
                self.stats.write().await.hits += 1;

                return Some(entry.clone());
            } else {
                // Entry expired
                cache.pop(&key_str);
                self.stats.write().await.evictions += 1;
            }
        }

        // Update stats
        self.stats.write().await.misses += 1;

        // Try semantic search if enabled
        if self.config.enable_semantic_search {
            if let Some(semantic) = &self.semantic_cache {
                return semantic
                    .find_similar(&key.prompt, self.config.similarity_threshold)
                    .await;
            }
        }

        None
    }

    pub async fn put(&mut self, key: CacheKey, mut entry: CacheEntry) -> Result<()> {
        let key_str = self.hash_key(&key);

        // Calculate entry size
        entry.size_bytes = entry.response.len() + std::mem::size_of::<CacheEntry>();

        // Check memory limit
        let mut memory = self.memory_usage.write().await;

        if self.config.eviction_policy == EvictionPolicy::Memory {
            while *memory + entry.size_bytes > self.config.max_memory_bytes && self.size() > 0 {
                // Evict least recently used
                if let Some((_, evicted)) = self.lru_cache.write().await.pop_lru() {
                    *memory = memory.saturating_sub(evicted.size_bytes);
                    self.stats.write().await.evictions += 1;
                }
            }
        }

        // Insert entry
        let mut cache = self.lru_cache.write().await;

        if let Some((_, old_entry)) = cache.push(key_str.clone(), entry.clone()) {
            *memory = memory.saturating_sub(old_entry.size_bytes);
        }

        *memory += entry.size_bytes;

        // Update stats
        let mut stats = self.stats.write().await;
        stats.total_entries = cache.len();
        stats.memory_usage = *memory;

        // Add to semantic cache if enabled
        if self.config.enable_semantic_search {
            if let Some(semantic) = &self.semantic_cache {
                semantic.add(key.prompt.clone(), entry).await?;
            }
        }

        Ok(())
    }

    pub async fn clear(&mut self) {
        self.lru_cache.write().await.clear();
        *self.memory_usage.write().await = 0;

        let mut stats = self.stats.write().await;
        stats.total_entries = 0;
        stats.memory_usage = 0;

        if let Some(semantic) = &self.semantic_cache {
            semantic.clear().await;
        }
    }

    pub async fn get_stats(&self) -> CacheStats {
        self.stats.read().await.clone()
    }

    pub async fn invalidate(&mut self, pattern: &str) -> usize {
        let mut cache = self.lru_cache.write().await;
        let mut memory = self.memory_usage.write().await;
        let mut invalidated = 0;

        // Collect keys to remove
        let keys_to_remove: Vec<String> = cache
            .iter()
            .filter_map(|(k, _)| {
                if k.contains(pattern) {
                    Some(k.clone())
                } else {
                    None
                }
            })
            .collect();

        // Remove matching entries
        for key in keys_to_remove {
            if let Some(entry) = cache.pop(&key) {
                *memory = memory.saturating_sub(entry.size_bytes);
                invalidated += 1;
            }
        }

        // Update stats
        let mut stats = self.stats.write().await;
        stats.total_entries = cache.len();
        stats.memory_usage = *memory;
        stats.evictions += invalidated;

        invalidated
    }

    pub async fn warm_up(&mut self, entries: Vec<(CacheKey, CacheEntry)>) -> Result<()> {
        for (key, entry) in entries {
            self.put(key, entry).await?;
        }
        Ok(())
    }

    pub async fn persist(&self, _path: &std::path::Path) -> Result<()> {
        // In real implementation, would serialize cache to disk
        Ok(())
    }

    pub async fn load(&mut self, _path: &std::path::Path) -> Result<()> {
        // In real implementation, would deserialize cache from disk
        Ok(())
    }

    fn hash_key(&self, key: &CacheKey) -> String {
        let mut hasher = Sha256::new();
        hasher.update(&key.model_id);
        hasher.update(&key.prompt);
        hasher.update(((key.temperature * 1000.0) as u32).to_le_bytes());
        hasher.update(key.max_tokens.to_le_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub async fn initialize_embeddings(&mut self) -> Result<()> {
        // Mock initialization
        Ok(())
    }

    pub async fn get_semantic(&self, key: &CacheKey) -> Option<CacheEntry> {
        // For now, use regular get
        self.get(key).await
    }

    pub fn reset_stats(&mut self) {
        futures::executor::block_on(async {
            *self.stats.write().await = CacheStats {
                hits: 0,
                misses: 0,
                evictions: 0,
                total_entries: self.lru_cache.read().await.len(),
                memory_usage: *self.memory_usage.read().await,
                avg_response_time: Duration::default(),
                avg_latency: Duration::default(),
            };
        });
    }

    pub async fn invalidate_model(&mut self, model_id: &str) -> usize {
        self.invalidate(model_id).await
    }
}

// Placeholder for semantic cache
pub struct SemanticCache {
    threshold: f32,
    entries: Arc<RwLock<HashMap<String, CacheEntry>>>,
}

impl SemanticCache {
    pub fn new(threshold: f32) -> Result<Self> {
        Ok(Self {
            threshold,
            entries: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn add(&self, prompt: String, entry: CacheEntry) -> Result<()> {
        self.entries.write().await.insert(prompt, entry);
        Ok(())
    }

    pub async fn find_similar(&self, _prompt: &str, _threshold: f32) -> Option<CacheEntry> {
        // In real implementation, would use embeddings and vector similarity
        None
    }

    pub async fn clear(&self) {
        self.entries.write().await.clear();
    }
}
