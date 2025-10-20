// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EmbeddingModel {
    MiniLM,
    AllMiniLM,
    E5Small,
}

impl EmbeddingModel {
    pub fn default_dimension(&self) -> usize {
        match self {
            EmbeddingModel::MiniLM => 384,
            EmbeddingModel::AllMiniLM => 384,
            EmbeddingModel::E5Small => 512,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenizerConfig {
    pub vocab_size: usize,
    pub lowercase: bool,
    pub remove_punctuation: bool,
}

impl Default for TokenizerConfig {
    fn default() -> Self {
        Self {
            vocab_size: 30522,
            lowercase: true,
            remove_punctuation: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EmbeddingConfig {
    pub model: EmbeddingModel,
    pub dimension: usize,
    pub max_tokens: usize,
    pub normalize: bool,
    pub tokenizer_config: TokenizerConfig,
    pub cache_embeddings: bool,
    pub cache_ttl_seconds: u64,
    pub quantize: Option<bool>,
    pub quantization_bits: Option<u8>,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model: EmbeddingModel::MiniLM,
            dimension: 384,
            max_tokens: 512,
            normalize: true,
            tokenizer_config: TokenizerConfig::default(),
            cache_embeddings: true,
            cache_ttl_seconds: 3600,
            quantize: None,
            quantization_bits: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Embedding {
    data: Vec<f32>,
    dimension: usize,
}

impl Embedding {
    pub fn new(data: Vec<f32>) -> Self {
        let dimension = data.len();
        Self { data, dimension }
    }

    pub fn data(&self) -> &[f32] {
        &self.data
    }

    pub fn dimension(&self) -> usize {
        self.dimension
    }

    pub fn magnitude(&self) -> f32 {
        self.data.iter().map(|x| x * x).sum::<f32>().sqrt()
    }

    pub fn cosine_similarity(&self, other: &Embedding) -> f32 {
        if self.dimension != other.dimension {
            return 0.0;
        }

        let dot_product: f32 = self
            .data
            .iter()
            .zip(other.data.iter())
            .map(|(a, b)| a * b)
            .sum();

        let magnitude_self = self.magnitude();
        let magnitude_other = other.magnitude();

        if magnitude_self == 0.0 || magnitude_other == 0.0 {
            0.0
        } else {
            dot_product / (magnitude_self * magnitude_other)
        }
    }

    pub fn euclidean_distance(&self, other: &Embedding) -> f32 {
        if self.dimension != other.dimension {
            return f32::INFINITY;
        }

        self.data
            .iter()
            .zip(other.data.iter())
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>()
            .sqrt()
    }

    pub fn manhattan_distance(&self, other: &Embedding) -> f32 {
        if self.dimension != other.dimension {
            return f32::INFINITY;
        }

        self.data
            .iter()
            .zip(other.data.iter())
            .map(|(a, b)| (a - b).abs())
            .sum()
    }

    pub fn dot_product(&self, other: &Embedding) -> f32 {
        if self.dimension != other.dimension {
            return 0.0;
        }

        self.data
            .iter()
            .zip(other.data.iter())
            .map(|(a, b)| a * b)
            .sum()
    }

    fn normalize(&mut self) {
        let magnitude = self.magnitude();
        if magnitude > 0.0 {
            for value in &mut self.data {
                *value /= magnitude;
            }
        }
    }

    fn quantize(&mut self, bits: u8) {
        let max_value = (1 << (bits - 1)) - 1;
        for value in &mut self.data {
            let quantized = (*value * max_value as f32).round() / max_value as f32;
            *value = quantized;
        }
    }
}

#[derive(Debug, Clone)]
pub struct BatchEmbeddingRequest {
    pub texts: Vec<String>,
    pub batch_size: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct EmbeddingCache {
    cache: Arc<RwLock<HashMap<String, (Embedding, u64)>>>,
    ttl_seconds: u64,
}

impl EmbeddingCache {
    fn new(ttl_seconds: u64) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            ttl_seconds,
        }
    }

    async fn get(&self, key: &str) -> Option<Embedding> {
        let cache = self.cache.read().await;
        if let Some((embedding, timestamp)) = cache.get(key) {
            let now = chrono::Utc::now().timestamp() as u64;
            if now - timestamp < self.ttl_seconds {
                return Some(embedding.clone());
            }
        }
        None
    }

    async fn put(&self, key: String, embedding: Embedding) {
        let mut cache = self.cache.write().await;
        let timestamp = chrono::Utc::now().timestamp() as u64;
        cache.insert(key, (embedding, timestamp));
    }

    async fn stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        let now = chrono::Utc::now().timestamp() as u64;
        let valid_entries = cache
            .values()
            .filter(|(_, timestamp)| now - timestamp < self.ttl_seconds)
            .count();

        CacheStats {
            total_entries: cache.len(),
            valid_entries,
            hits: 0, // These would be tracked separately in a real implementation
            misses: 0,
            hit_rate: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: usize,
    pub valid_entries: usize,
    pub hits: u64,
    pub misses: u64,
    pub hit_rate: f32,
}

#[derive(Debug, Clone)]
pub struct TruncationInfo {
    pub was_truncated: bool,
    pub original_tokens: usize,
    pub truncated_tokens: usize,
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub model_name: String,
    pub embedding_dimension: usize,
    pub max_tokens: usize,
    pub supports_languages: Vec<String>,
}

#[derive(Error, Debug)]
pub enum EmbeddingError {
    #[error("Model loading failed: {0}")]
    ModelLoad(String),
    #[error("Tokenization failed: {0}")]
    Tokenization(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Cache error: {0}")]
    Cache(String),
    #[error("Configuration error: {0}")]
    Config(String),
}

pub struct EmbeddingGenerator {
    config: EmbeddingConfig,
    cache: Option<EmbeddingCache>,
    cache_stats: Arc<RwLock<CacheStats>>,
    last_truncation: Arc<RwLock<TruncationInfo>>,
}

impl EmbeddingGenerator {
    pub async fn new(config: EmbeddingConfig) -> Result<Self, EmbeddingError> {
        let cache = if config.cache_embeddings {
            Some(EmbeddingCache::new(config.cache_ttl_seconds))
        } else {
            None
        };

        Ok(Self {
            config,
            cache,
            cache_stats: Arc::new(RwLock::new(CacheStats {
                total_entries: 0,
                valid_entries: 0,
                hits: 0,
                misses: 0,
                hit_rate: 0.0,
            })),
            last_truncation: Arc::new(RwLock::new(TruncationInfo {
                was_truncated: false,
                original_tokens: 0,
                truncated_tokens: 0,
            })),
        })
    }

    pub async fn generate_embedding(&self, text: &str) -> Result<Embedding, EmbeddingError> {
        let cache_key = if self.config.cache_embeddings {
            Some(self.generate_cache_key(text))
        } else {
            None
        };

        // Check cache first
        if let Some(cache) = &self.cache {
            if let Some(ref key) = cache_key {
                if let Some(cached_embedding) = cache.get(key).await {
                    self.increment_cache_hits().await;
                    return Ok(cached_embedding);
                }
            }
        }

        self.increment_cache_misses().await;

        // Generate embedding
        let embedding = self.generate_embedding_impl(text).await?;

        // Cache the result
        if let (Some(cache), Some(key)) = (&self.cache, cache_key) {
            cache.put(key, embedding.clone()).await;
        }

        Ok(embedding)
    }

    pub async fn generate_batch(
        &self,
        texts: Vec<String>,
    ) -> Result<Vec<Embedding>, EmbeddingError> {
        let mut embeddings = Vec::with_capacity(texts.len());

        for text in texts {
            let embedding = self.generate_embedding(&text).await?;
            embeddings.push(embedding);
        }

        Ok(embeddings)
    }

    pub async fn generate_embedding_with_lang(
        &self,
        text: &str,
        _lang: &str,
    ) -> Result<Embedding, EmbeddingError> {
        // For simplicity, ignore language parameter in mock implementation
        self.generate_embedding(text).await
    }

    async fn generate_embedding_impl(&self, text: &str) -> Result<Embedding, EmbeddingError> {
        // Tokenize text
        let tokens = self.tokenize(text)?;
        let truncated_tokens = if tokens.len() > self.config.max_tokens {
            let truncated = tokens[..self.config.max_tokens].to_vec();

            // Update truncation info
            let mut truncation = self.last_truncation.write().await;
            *truncation = TruncationInfo {
                was_truncated: true,
                original_tokens: tokens.len(),
                truncated_tokens: truncated.len(),
            };

            truncated
        } else {
            // Update truncation info
            let mut truncation = self.last_truncation.write().await;
            *truncation = TruncationInfo {
                was_truncated: false,
                original_tokens: tokens.len(),
                truncated_tokens: tokens.len(),
            };

            tokens
        };

        // Generate embedding using mock implementation
        let embedding_vector = self.mock_generate_embedding(&truncated_tokens)?;
        let mut embedding = Embedding::new(embedding_vector);

        // Apply quantization if configured
        if let (Some(true), Some(bits)) = (self.config.quantize, self.config.quantization_bits) {
            embedding.quantize(bits);
        }

        // Normalize if configured
        if self.config.normalize {
            embedding.normalize();
        }

        Ok(embedding)
    }

    fn tokenize(&self, text: &str) -> Result<Vec<String>, EmbeddingError> {
        let mut processed_text = text.to_string();

        if self.config.tokenizer_config.lowercase {
            processed_text = processed_text.to_lowercase();
        }

        if self.config.tokenizer_config.remove_punctuation {
            processed_text = processed_text
                .chars()
                .filter(|c| c.is_alphanumeric() || c.is_whitespace())
                .collect();
        }

        // Simple whitespace tokenization
        let tokens: Vec<String> = processed_text
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        Ok(tokens)
    }

    fn mock_generate_embedding(&self, tokens: &[String]) -> Result<Vec<f32>, EmbeddingError> {
        // Create a deterministic embedding based on token content
        let combined_text = tokens.join(" ");
        let mut hasher = Sha256::new();
        hasher.update(combined_text.as_bytes());
        let hash = hasher.finalize();

        let mut embedding = Vec::with_capacity(self.config.dimension);

        for i in 0..self.config.dimension {
            let byte_index = i % hash.len();
            let byte_value = hash[byte_index];

            // Convert byte to float in range [-1, 1]
            let float_value = (byte_value as f32 / 255.0) * 2.0 - 1.0;
            embedding.push(float_value);
        }

        Ok(embedding)
    }

    fn generate_cache_key(&self, text: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        hasher.update(format!("{:?}", self.config.model).as_bytes());
        hasher.update(&self.config.dimension.to_le_bytes());
        hasher.update(&self.config.max_tokens.to_le_bytes());
        format!("{:x}", hasher.finalize())
    }

    async fn increment_cache_hits(&self) {
        let mut stats = self.cache_stats.write().await;
        stats.hits += 1;
        stats.hit_rate = stats.hits as f32 / (stats.hits + stats.misses) as f32;
    }

    async fn increment_cache_misses(&self) {
        let mut stats = self.cache_stats.write().await;
        stats.misses += 1;
        stats.hit_rate = stats.hits as f32 / (stats.hits + stats.misses) as f32;
    }

    pub async fn get_cache_stats(&self) -> CacheStats {
        self.cache_stats.read().await.clone()
    }

    pub async fn get_last_truncation_info(&self) -> TruncationInfo {
        self.last_truncation.read().await.clone()
    }

    pub async fn get_model_info(&self) -> ModelInfo {
        ModelInfo {
            model_name: format!("{:?}", self.config.model),
            embedding_dimension: self.config.dimension,
            max_tokens: self.config.max_tokens,
            supports_languages: vec![
                "en".to_string(),
                "es".to_string(),
                "fr".to_string(),
                "de".to_string(),
                "ja".to_string(),
            ],
        }
    }
}
