// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// src/cache/mod.rs
// Phase 4.1.3: Cache Flow Implementation

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::embeddings::{EmbeddingConfig, EmbeddingGenerator};
use crate::storage::{EnhancedS5Client, S5Config};
use crate::vector::{VectorDbClient, VectorDbConfig};

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub s5_url: String,
    pub vector_db_url: String,
    pub similarity_threshold: f32,
    pub ttl_seconds: u64,
    pub max_cache_size_mb: usize,
}

#[derive(Debug, Clone)]
pub struct CacheMetrics {
    pub total_requests: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub hit_rate: f64,
    pub avg_hit_time_ms: f64,
    pub avg_miss_time_ms: f64,
    pub cache_size_mb: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub prompt: String,
    pub prompt_key: String,
    pub response: String,
    pub model: String,
    pub parameters: JsonValue,
    pub generated_at: String,
    pub generation_time_ms: u64,
    #[serde(skip, default = "SystemTime::now")]
    pub created_at: SystemTime,
    #[serde(skip, default)]
    pub size_bytes: usize,
}

struct CacheMetricsInternal {
    total_requests: usize,
    cache_hits: usize,
    cache_misses: usize,
    hit_times_ms: Vec<f64>,
    miss_times_ms: Vec<f64>,
    cache_size_bytes: usize,
}

pub struct PromptCache {
    config: CacheConfig,
    s5_client: EnhancedS5Client,
    vector_client: VectorDbClient,
    embedding_generator: EmbeddingGenerator,
    metrics: Arc<Mutex<CacheMetricsInternal>>,
    cache_entries: Arc<Mutex<HashMap<String, CacheEntry>>>, // In-memory tracking
}

impl PromptCache {
    pub async fn new(config: CacheConfig) -> Result<Self> {
        // Initialize S5 client
        let s5_config = S5Config {
            api_url: config.s5_url.clone(),
            api_key: Some("cache-api-key".to_string()),
            timeout_secs: 30,
        };
        let s5_client = EnhancedS5Client::new(s5_config)?;

        // Initialize Vector DB client
        let vector_config = VectorDbConfig {
            api_url: config.vector_db_url.clone(),
            api_key: Some("cache-vector-key".to_string()),
            timeout_secs: 30,
        };
        let vector_client = VectorDbClient::new(vector_config)?;

        // Initialize embedding generator
        let embedding_config = EmbeddingConfig {
            model: "all-MiniLM-L6-v2".to_string(),
            dimension: 384,
            batch_size: 32,
            normalize: true,
        };
        let embedding_generator = EmbeddingGenerator::new(embedding_config).await?;

        let metrics = Arc::new(Mutex::new(CacheMetricsInternal {
            total_requests: 0,
            cache_hits: 0,
            cache_misses: 0,
            hit_times_ms: Vec::new(),
            miss_times_ms: Vec::new(),
            cache_size_bytes: 0,
        }));

        Ok(Self {
            config,
            s5_client,
            vector_client,
            embedding_generator,
            metrics,
            cache_entries: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn hash_prompt(&self, prompt: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(prompt.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub async fn get(&self, prompt: &str) -> Result<Option<String>> {
        let start = Instant::now();
        let prompt_hash = self.hash_prompt(prompt);

        // Update total requests
        {
            let mut metrics = self.metrics.lock().unwrap();
            metrics.total_requests += 1;
        }

        // Check in-memory cache first for exact match
        {
            let entries = self.cache_entries.lock().unwrap();
            if let Some(entry) = entries.get(&prompt_hash) {
                // Check TTL
                let age = SystemTime::now()
                    .duration_since(entry.created_at)
                    .unwrap_or(Duration::from_secs(0));

                if age.as_secs() <= self.config.ttl_seconds {
                    // Cache hit
                    let elapsed = start.elapsed().as_millis() as f64;
                    let mut metrics = self.metrics.lock().unwrap();
                    metrics.cache_hits += 1;
                    metrics.hit_times_ms.push(elapsed);
                    return Ok(Some(entry.response.clone()));
                }
            }
        }

        // Try to retrieve from S5
        let path = format!("/cache/prompts/{}/{}.json", &prompt_hash[0..2], prompt_hash);
        if let Ok((data, _metadata)) = self.s5_client.get(&path).await {
            if let Ok(json_str) = String::from_utf8(data) {
                if let Ok(entry) = serde_json::from_str::<CacheEntry>(&json_str) {
                    // Check TTL based on generated_at
                    if let Ok(generated_at) =
                        chrono::DateTime::parse_from_rfc3339(&entry.generated_at)
                    {
                        let age = SystemTime::now()
                            .duration_since(
                                SystemTime::UNIX_EPOCH
                                    + Duration::from_secs(generated_at.timestamp() as u64),
                            )
                            .unwrap_or(Duration::from_secs(u64::MAX));

                        if age.as_secs() <= self.config.ttl_seconds {
                            // Cache hit from S5
                            let elapsed = start.elapsed().as_millis() as f64;
                            let mut metrics = self.metrics.lock().unwrap();
                            metrics.cache_hits += 1;
                            metrics.hit_times_ms.push(elapsed);

                            // Update in-memory cache
                            let mut entries = self.cache_entries.lock().unwrap();
                            let mut cache_entry = entry.clone();
                            cache_entry.created_at = SystemTime::now() - age;
                            entries.insert(prompt_hash.clone(), cache_entry);

                            return Ok(Some(entry.response));
                        }
                    }
                }
            }
        }

        // If no exact match, try semantic search
        // Extract base prompt without parameters for semantic search
        let base_prompt = prompt.split(';').next().unwrap_or(prompt);
        let embedding = self.embedding_generator.generate(base_prompt).await?;
        let filter = Some(json!({
            "type": "cache_entry"
        }));

        let results = self.vector_client.search(embedding, 1, filter).await?;

        if !results.is_empty() {
            if let Some(first) = results.first() {
                if let Some(score) = first.get("score").and_then(|s| s.as_f64()) {
                    // Use a slightly lower threshold for better semantic matching
                    let threshold = self.config.similarity_threshold * 0.95;
                    if score as f32 >= threshold {
                        // Found similar cached prompt
                        if let Some(metadata) = first.get("metadata") {
                            if let Some(cached_response) =
                                metadata.get("response").and_then(|r| r.as_str())
                            {
                                // Check TTL
                                if let Some(generated_at_str) =
                                    metadata.get("generated_at").and_then(|g| g.as_str())
                                {
                                    if let Ok(generated_at) =
                                        chrono::DateTime::parse_from_rfc3339(generated_at_str)
                                    {
                                        let age = SystemTime::now()
                                            .duration_since(
                                                SystemTime::UNIX_EPOCH
                                                    + Duration::from_secs(
                                                        generated_at.timestamp() as u64
                                                    ),
                                            )
                                            .unwrap_or(Duration::from_secs(u64::MAX));

                                        if age.as_secs() <= self.config.ttl_seconds {
                                            // Semantic cache hit
                                            let elapsed = start.elapsed().as_millis() as f64;
                                            let mut metrics = self.metrics.lock().unwrap();
                                            metrics.cache_hits += 1;
                                            metrics.hit_times_ms.push(elapsed);
                                            return Ok(Some(cached_response.to_string()));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Cache miss
        let elapsed = start.elapsed().as_millis() as f64;
        let mut metrics = self.metrics.lock().unwrap();
        metrics.cache_misses += 1;
        metrics.miss_times_ms.push(elapsed);

        Ok(None)
    }

    pub async fn put(&self, prompt: &str, response: &str) -> Result<()> {
        let prompt_hash = self.hash_prompt(prompt);
        let now = SystemTime::now();
        let generated_at = chrono::DateTime::<chrono::Utc>::from(now)
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string();

        // Parse prompt key for model and parameters
        let parts: Vec<&str> = prompt.split(';').collect();
        let base_prompt = if !parts.is_empty() { parts[0] } else { prompt };
        let mut model = "llama-3.2-1b-instruct".to_string();
        let mut parameters = json!({});

        for part in &parts[1..] {
            if let Some((key, value)) = part.split_once('=') {
                match key {
                    "model" => model = value.to_string(),
                    "temp" => {
                        parameters["temperature"] = json!(value.parse::<f64>().unwrap_or(0.7));
                    }
                    "max_tokens" => {
                        parameters["max_tokens"] = json!(value.parse::<u64>().unwrap_or(100));
                    }
                    _ => {}
                }
            }
        }

        let entry = CacheEntry {
            prompt: base_prompt.to_string(),
            prompt_key: prompt.to_string(),
            response: response.to_string(),
            model,
            parameters,
            generated_at: generated_at.clone(),
            generation_time_ms: 1250, // Mock value
            created_at: now,
            size_bytes: response.len() + prompt.len() + 200, // Approximate
        };

        // Check cache size and evict if necessary
        {
            let mut entries = self.cache_entries.lock().unwrap();
            let mut metrics = self.metrics.lock().unwrap();

            let max_size_bytes = self.config.max_cache_size_mb * 1024 * 1024;
            let new_size = metrics.cache_size_bytes + entry.size_bytes;

            if new_size > max_size_bytes && !entries.is_empty() {
                // Evict oldest entries (LRU)
                let mut sorted_entries: Vec<_> = entries
                    .iter()
                    .map(|(k, v)| (k.clone(), v.created_at))
                    .collect();
                sorted_entries.sort_by_key(|(_k, time)| *time);

                for (key, _) in sorted_entries {
                    if let Some(removed) = entries.remove(&key) {
                        metrics.cache_size_bytes -= removed.size_bytes;

                        if metrics.cache_size_bytes + entry.size_bytes <= max_size_bytes {
                            break;
                        }
                    }
                }
            }

            // Add new entry
            metrics.cache_size_bytes += entry.size_bytes;
            entries.insert(prompt_hash.clone(), entry.clone());
        }

        // Store in S5
        let path = format!("/cache/prompts/{}/{}.json", &prompt_hash[0..2], prompt_hash);
        let json_data = serde_json::to_string(&entry)?;
        let metadata = json!({
            "type": "cache_entry",
            "prompt_hash": prompt_hash,
            "model": entry.model,
            "generated_at": generated_at,
        });

        // Store in S5 with error handling to prevent hanging
        if let Err(e) = tokio::time::timeout(
            Duration::from_secs(5),
            self.s5_client
                .put(&path, json_data.into_bytes(), Some(metadata)),
        )
        .await
        {
            // Log error but continue (don't fail the whole put operation)
            eprintln!("Warning: S5 storage timed out or failed: {:?}", e);
        }

        // Generate embedding and store in vector DB (use base prompt for embedding)
        let embedding = self.embedding_generator.generate(base_prompt).await?;
        let vector_metadata = json!({
            "type": "cache_entry",
            "prompt": entry.prompt,
            "prompt_key": entry.prompt_key,
            "response": response,
            "model": entry.model,
            "parameters": entry.parameters,
            "generated_at": generated_at,
            "prompt_hash": prompt_hash,
            "s5_path": path,
        });

        // Store in vector DB with error handling
        if let Err(e) = tokio::time::timeout(
            Duration::from_secs(5),
            self.vector_client
                .insert_vector(&prompt_hash, embedding, vector_metadata),
        )
        .await
        {
            eprintln!("Warning: Vector DB storage timed out or failed: {:?}", e);
        }

        Ok(())
    }

    pub async fn get_metrics(&self) -> Result<CacheMetrics> {
        let metrics = self.metrics.lock().unwrap();

        let hit_rate = if metrics.total_requests > 0 {
            (metrics.cache_hits as f64) / (metrics.total_requests as f64)
        } else {
            0.0
        };

        let avg_hit_time_ms = if !metrics.hit_times_ms.is_empty() {
            metrics.hit_times_ms.iter().sum::<f64>() / metrics.hit_times_ms.len() as f64
        } else {
            0.0
        };

        let avg_miss_time_ms = if !metrics.miss_times_ms.is_empty() {
            metrics.miss_times_ms.iter().sum::<f64>() / metrics.miss_times_ms.len() as f64
        } else {
            0.0
        };

        Ok(CacheMetrics {
            total_requests: metrics.total_requests,
            cache_hits: metrics.cache_hits,
            cache_misses: metrics.cache_misses,
            hit_rate,
            avg_hit_time_ms,
            avg_miss_time_ms,
            cache_size_mb: (metrics.cache_size_bytes as f64) / (1024.0 * 1024.0),
        })
    }

    pub async fn clear(&self) -> Result<()> {
        let mut entries = self.cache_entries.lock().unwrap();
        entries.clear();

        let mut metrics = self.metrics.lock().unwrap();
        metrics.cache_size_bytes = 0;

        Ok(())
    }
}
