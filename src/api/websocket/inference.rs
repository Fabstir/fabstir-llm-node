// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info};

// Re-export from the main inference module
use crate::inference::engine::{
    EngineConfig, InferenceRequest as BaseRequest, InferenceResult as BaseResult,
    LlmEngine as BaseEngine,
};

/// Configuration for inference engine
#[derive(Debug, Clone)]
pub struct InferenceConfig {
    pub model_path: PathBuf,
    pub context_size: usize,
    pub max_tokens: usize,
    pub temperature: f32,
    pub gpu_layers: u32,
    pub use_gpu: bool,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        // Read context length from environment variable
        let context_size = std::env::var("MAX_CONTEXT_LENGTH")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(8192);

        Self {
            model_path: PathBuf::from("models/tinyllama-1b.Q4_K_M.gguf"),
            context_size,
            max_tokens: 256,
            temperature: 0.7,
            gpu_layers: 35,
            use_gpu: cfg!(feature = "cuda"),
        }
    }
}

/// Inference request
#[derive(Debug, Clone)]
pub struct InferenceRequest {
    pub prompt: String,
    pub max_tokens: usize,
    pub temperature: Option<f32>,
    pub stream: bool,
}

/// Inference response
#[derive(Debug, Clone)]
pub struct InferenceResponse {
    pub text: String,
    pub tokens_generated: usize,
    pub prompt_tokens: usize,
    pub inference_time_ms: f64,
}

/// Streaming chunk
#[derive(Debug, Clone)]
pub struct StreamChunk {
    pub text: String,
    pub tokens: usize,
    pub is_final: bool,
}

/// Real inference engine using llama-cpp-2
#[derive(Clone)]
pub struct InferenceEngine {
    config: InferenceConfig,
    engine: Arc<RwLock<BaseEngine>>,
    system_prompt: Option<String>,
}

impl InferenceEngine {
    /// Create new inference engine
    pub async fn new(config: InferenceConfig) -> Result<Self> {
        // Validate model path
        if !config.model_path.exists() {
            return Err(anyhow!("Model file not found: {:?}", config.model_path));
        }

        // Check if it's a valid GGUF file
        let extension = config
            .model_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        if extension != "gguf" {
            return Err(anyhow!("Invalid model format. Expected GGUF file"));
        }

        // Create base engine config
        // Read batch size from environment variable
        let batch_size = std::env::var("LLAMA_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(2048);

        let engine_config = EngineConfig {
            models_directory: config.model_path.parent().unwrap().to_path_buf(),
            max_loaded_models: 3,
            max_context_length: config.context_size,
            gpu_layers: config.gpu_layers as usize,
            thread_count: 8,
            batch_size,
            use_mmap: true,
            use_mlock: false,
            max_concurrent_inferences: 4,
            model_eviction_policy: "lru".to_string(),
        };

        // Create base engine
        let mut base_engine = BaseEngine::new(engine_config).await?;

        // Load the model
        use crate::inference::engine::ModelConfig;
        let model_config = ModelConfig {
            model_path: config.model_path.clone(),
            model_type: "gguf".to_string(),
            context_size: config.context_size,
            gpu_layers: config.gpu_layers as usize,
            rope_freq_base: 10000.0,
            rope_freq_scale: 1.0,
            chat_template: None, // Use model's default chat template
        };

        let model_id = base_engine.load_model(model_config).await?;

        Ok(Self {
            config,
            engine: Arc::new(RwLock::new(base_engine)),
            system_prompt: None,
        })
    }

    /// Create with system prompt
    pub async fn with_system_prompt(
        config: InferenceConfig,
        system_prompt: String,
    ) -> Result<Self> {
        let mut engine = Self::new(config).await?;
        engine.system_prompt = Some(system_prompt);
        Ok(engine)
    }

    /// Generate response
    pub async fn generate(&self, request: InferenceRequest) -> Result<InferenceResponse> {
        let start = Instant::now();

        // Build full prompt
        let full_prompt = if let Some(ref system) = self.system_prompt {
            format!("{}\n\n{}", system, request.prompt)
        } else {
            request.prompt.clone()
        };

        // Get model ID
        let model_id = self
            .config
            .model_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("default")
            .to_string();

        // Convert to base request
        let base_request = BaseRequest {
            model_id: model_id.clone(),
            prompt: full_prompt,
            max_tokens: request.max_tokens,
            temperature: request.temperature.unwrap_or(self.config.temperature),
            top_p: 0.95,
            top_k: 40,
            repeat_penalty: 1.1,
            seed: None,
            stop_sequences: vec![],
            stream: false,
        };

        // Generate with engine
        let engine = self.engine.read().await;
        let result = engine.run_inference(base_request).await?;

        let inference_time = start.elapsed();

        Ok(InferenceResponse {
            text: result.text,
            tokens_generated: result.tokens_generated,
            prompt_tokens: 0, // Not available in BaseResult
            inference_time_ms: result.generation_time.as_secs_f64() * 1000.0,
        })
    }

    /// Stream generate response
    pub async fn stream_generate(
        &self,
        request: InferenceRequest,
        tx: mpsc::Sender<StreamChunk>,
    ) -> Result<()> {
        let start = Instant::now();

        // Build full prompt
        let full_prompt = if let Some(ref system) = self.system_prompt {
            format!("{}\n\n{}", system, request.prompt)
        } else {
            request.prompt.clone()
        };

        // Get model ID
        let model_id = self
            .config
            .model_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("default")
            .to_string();

        // Convert to base request
        let base_request = BaseRequest {
            model_id: model_id.clone(),
            prompt: full_prompt,
            max_tokens: request.max_tokens,
            temperature: request.temperature.unwrap_or(self.config.temperature),
            top_p: 0.95,
            top_k: 40,
            repeat_penalty: 1.1,
            seed: None,
            stop_sequences: vec![],
            stream: false,
        };

        // For streaming, we need to use the engine's stream method
        // Since the base engine might not have direct streaming, we'll simulate it
        let engine = self.engine.read().await;

        // Generate the full response
        let result = engine.run_inference(base_request).await?;

        // Simulate streaming by sending chunks
        let words: Vec<&str> = result.text.split_whitespace().collect();
        let mut sent_tokens = 0;

        for (i, word) in words.iter().enumerate() {
            let chunk_text = format!("{} ", word);
            sent_tokens += 1;

            let stream_chunk = StreamChunk {
                text: chunk_text,
                tokens: 1,
                is_final: i == words.len() - 1,
            };

            if tx.send(stream_chunk).await.is_err() {
                break;
            }

            // Add small delay to simulate streaming
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        let inference_time = start.elapsed();
        info!("Streamed {} tokens in {:?}", sent_tokens, inference_time);

        Ok(())
    }

    /// Batch generate
    pub async fn batch_generate(
        &self,
        requests: Vec<InferenceRequest>,
    ) -> Result<Vec<InferenceResponse>> {
        let mut responses = Vec::new();

        for request in requests {
            let response = self.generate(request).await?;
            responses.push(response);
        }

        Ok(responses)
    }

    /// Check if GPU is available
    pub async fn is_gpu_available(&self) -> bool {
        #[cfg(feature = "cuda")]
        {
            // Check CUDA availability
            std::env::var("CUDA_VISIBLE_DEVICES").is_ok()
        }
        #[cfg(not(feature = "cuda"))]
        {
            false
        }
    }
}

/// Model manager for handling multiple models
pub struct ModelManager {
    models: Arc<RwLock<HashMap<PathBuf, Arc<InferenceEngine>>>>,
    config: InferenceConfig,
}

impl ModelManager {
    pub fn new(config: InferenceConfig) -> Self {
        Self {
            models: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    pub async fn get_or_load(&self, model_path: &PathBuf) -> Result<Arc<InferenceEngine>> {
        let mut models = self.models.write().await;

        if let Some(engine) = models.get(model_path) {
            return Ok(engine.clone());
        }

        // Load new model
        let mut config = self.config.clone();
        config.model_path = model_path.clone();

        let engine = Arc::new(InferenceEngine::new(config).await?);
        models.insert(model_path.clone(), engine.clone());

        Ok(engine)
    }
}

/// Model cache with LRU eviction
pub struct ModelCache {
    cache: Arc<RwLock<Vec<(PathBuf, Arc<InferenceEngine>, Instant)>>>,
    max_size: usize,
}

impl ModelCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: Arc::new(RwLock::new(Vec::new())),
            max_size,
        }
    }

    pub async fn get_or_load(&self, model_path: &PathBuf) -> Result<Arc<InferenceEngine>> {
        let mut cache = self.cache.write().await;

        // Check if in cache
        for (path, engine, last_used) in cache.iter_mut() {
            if path == model_path {
                *last_used = Instant::now();
                return Ok(engine.clone());
            }
        }

        // Load new model
        let config = InferenceConfig {
            model_path: model_path.clone(),
            ..Default::default()
        };

        let engine = Arc::new(InferenceEngine::new(config).await?);

        // Add to cache with eviction
        if cache.len() >= self.max_size {
            // Remove least recently used
            cache.sort_by_key(|(_, _, last_used)| *last_used);
            cache.remove(0);
        }

        cache.push((model_path.clone(), engine.clone(), Instant::now()));

        Ok(engine)
    }

    pub async fn contains(&self, model_path: &PathBuf) -> bool {
        let cache = self.cache.read().await;
        cache.iter().any(|(path, _, _)| path == model_path)
    }
}

/// Streaming inference helper
pub struct StreamingInference;

impl StreamingInference {
    pub async fn stream(
        engine: &InferenceEngine,
        request: InferenceRequest,
    ) -> Result<mpsc::Receiver<StreamChunk>> {
        let (tx, rx) = mpsc::channel(100);

        let engine_clone = engine.clone();
        tokio::spawn(async move {
            if let Err(e) = engine_clone.stream_generate(request, tx).await {
                error!("Streaming error: {}", e);
            }
        });

        Ok(rx)
    }
}
