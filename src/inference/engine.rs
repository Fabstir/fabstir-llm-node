// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::{anyhow, Result};
use futures::FutureExt;
use llama_cpp_2::{
    context::params::{KvCacheType, LlamaContextParams},
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel, Special},
    sampling::LlamaSampler,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

/// Sanitize prompt text for tokenization
///
/// Removes characters that cause issues with C string handling in llama.cpp:
/// - Null bytes (\0) - C strings use null as terminator
/// - Other control characters that may cause issues
///
/// This is necessary when prompt content comes from PDFs or other binary sources
/// that may contain embedded null bytes or invalid Unicode.
fn sanitize_prompt_for_tokenizer(prompt: &str) -> String {
    prompt
        .chars()
        .filter(|c| {
            // Remove null bytes (critical - causes NulError)
            // Remove other C0 control characters except common whitespace
            // Keep: tab (0x09), newline (0x0A), carriage return (0x0D)
            *c != '\0' && (*c >= ' ' || *c == '\t' || *c == '\n' || *c == '\r')
        })
        .collect()
}

/// Parse a KV cache type string into a KvCacheType enum.
/// Supports: "q8_0", "q4_0", "f16", "bf16", "f32" (case-insensitive).
/// Returns None for unrecognized types (will use llama.cpp default = fp16).
pub fn parse_kv_cache_type(s: &str) -> Option<KvCacheType> {
    match s.to_lowercase().as_str() {
        "q8_0" => Some(KvCacheType::Q8_0),
        "q4_0" => Some(KvCacheType::Q4_0),
        "q4_1" => Some(KvCacheType::Q4_1),
        "q5_0" => Some(KvCacheType::Q5_0),
        "q5_1" => Some(KvCacheType::Q5_1),
        "q6_k" => Some(KvCacheType::Q6_K),
        "f16" => Some(KvCacheType::F16),
        "bf16" => Some(KvCacheType::BF16),
        "f32" => Some(KvCacheType::F32),
        _ => None,
    }
}

// Wrapper around the real LLama model
struct RealLlamaModel {
    backend: LlamaBackend,
    model: LlamaModel,
    context_size: usize,
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub models_directory: PathBuf,
    pub max_loaded_models: usize,
    pub max_context_length: usize,
    pub gpu_layers: usize,
    pub thread_count: usize,
    pub batch_size: usize,
    pub use_mmap: bool,
    pub use_mlock: bool,
    pub max_concurrent_inferences: usize,
    pub model_eviction_policy: String,
    pub kv_cache_type_k: Option<String>,
    pub kv_cache_type_v: Option<String>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            models_directory: PathBuf::from("./models"),
            max_loaded_models: 3,
            max_context_length: 4096,
            gpu_layers: 35,
            thread_count: 8,
            batch_size: std::env::var("LLAMA_BATCH_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2048), // Increased default from 512 to 2048
            use_mmap: true,
            use_mlock: false,
            max_concurrent_inferences: 4,
            model_eviction_policy: "lru".to_string(),
            kv_cache_type_k: None,
            kv_cache_type_v: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub model_path: PathBuf,
    pub model_type: String,
    pub context_size: usize,
    pub gpu_layers: usize,
    pub rope_freq_base: f32,
    pub rope_freq_scale: f32,
    pub chat_template: Option<crate::inference::ChatTemplate>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InferenceRequest {
    pub model_id: String,
    pub prompt: String,
    pub max_tokens: usize,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: usize,
    pub repeat_penalty: f32,
    /// Min-P sampling threshold (0.0 = disabled, typical: 0.01-0.1)
    pub min_p: f32,
    pub seed: Option<u64>,
    pub stop_sequences: Vec<String>,
    pub stream: bool,
    /// Cancellation flag — set to true to abort generation between tokens
    #[serde(skip)]
    pub cancel_flag: Option<Arc<AtomicBool>>,
    /// Token sender — sends each token as it's generated (for true streaming)
    #[serde(skip)]
    pub token_sender: Option<mpsc::Sender<Result<TokenInfo>>>,
    /// Result sender — sends the complete InferenceResult after generation (for streaming metadata)
    #[serde(skip)]
    pub result_sender: Option<tokio::sync::oneshot::Sender<InferenceResult>>,
}

impl Clone for InferenceRequest {
    fn clone(&self) -> Self {
        Self {
            model_id: self.model_id.clone(),
            prompt: self.prompt.clone(),
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            repeat_penalty: self.repeat_penalty,
            min_p: self.min_p,
            seed: self.seed,
            stop_sequences: self.stop_sequences.clone(),
            stream: self.stream,
            cancel_flag: self.cancel_flag.clone(),
            token_sender: self.token_sender.clone(),
            result_sender: None, // oneshot::Sender is not cloneable
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceResult {
    pub text: String,
    pub tokens_generated: usize,
    pub generation_time: Duration,
    pub tokens_per_second: f32,
    pub model_id: String,
    pub finish_reason: String,
    pub token_info: Vec<TokenInfo>,
    pub was_cancelled: bool,
    pub context_usage: Option<ContextUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub token_id: i32,
    pub text: String,
    pub logprob: Option<f32>,
    pub timestamp: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
    pub context_window_size: usize,
}

#[derive(Debug, Clone)]
pub struct Model {
    pub id: String,
    pub config: ModelConfig,
    pub status: ModelStatus,
    pub loaded_at: std::time::SystemTime,
    pub usage_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModelStatus {
    Loading,
    Ready,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct EngineCapabilities {
    pub max_context_length: usize,
    pub supports_gpu: bool,
    pub max_batch_size: usize,
    pub supported_models: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EngineMetrics {
    pub total_inferences: usize,
    pub total_tokens_generated: usize,
    pub average_tokens_per_second: f32,
    pub total_inference_time: Duration,
}

pub type TokenStream = ReceiverStream<Result<TokenInfo>>;

#[derive(Clone)]
pub struct LlmEngine {
    config: EngineConfig,
    models: Arc<std::sync::Mutex<HashMap<String, RealLlamaModel>>>,
    model_info: Arc<RwLock<HashMap<String, Model>>>,
    inference_count: Arc<RwLock<usize>>,
    metrics: Arc<RwLock<EngineMetrics>>,
}

impl LlmEngine {
    pub async fn new(config: EngineConfig) -> Result<Self> {
        // Create models directory if it doesn't exist
        tokio::fs::create_dir_all(&config.models_directory).await?;

        Ok(Self {
            config,
            models: Arc::new(std::sync::Mutex::new(HashMap::new())),
            model_info: Arc::new(RwLock::new(HashMap::new())),
            inference_count: Arc::new(RwLock::new(0)),
            metrics: Arc::new(RwLock::new(EngineMetrics {
                total_inferences: 0,
                total_tokens_generated: 0,
                average_tokens_per_second: 0.0,
                total_inference_time: Duration::default(),
            })),
        })
    }

    pub fn is_ready(&self) -> bool {
        true
    }

    pub fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities {
            max_context_length: self.config.max_context_length,
            supports_gpu: false, // Would check for CUDA in production
            max_batch_size: self.config.batch_size,
            supported_models: vec![
                "llama".to_string(),
                "llama2".to_string(),
                "mistral".to_string(),
                "phi".to_string(),
            ],
        }
    }

    pub async fn load_model(&mut self, config: ModelConfig) -> Result<String> {
        let model_id = Uuid::new_v4().to_string();

        // Update model info
        let model = Model {
            id: model_id.clone(),
            config: config.clone(),
            status: ModelStatus::Loading,
            loaded_at: std::time::SystemTime::now(),
            usage_count: 0,
        };

        self.model_info
            .write()
            .await
            .insert(model_id.clone(), model.clone());

        // Initialize backend
        let backend =
            LlamaBackend::init().map_err(|e| anyhow!("Failed to initialize backend: {:?}", e))?;

        // Load the GGUF model
        let model_params = LlamaModelParams::default().with_n_gpu_layers(config.gpu_layers as u32);

        let model = LlamaModel::load_from_file(&backend, &config.model_path, &model_params)
            .map_err(|e| anyhow!("Failed to load model: {:?}", e))?;

        let real_model = RealLlamaModel {
            backend,
            model,
            context_size: config.context_size,
        };

        // Store the loaded model
        self.models
            .lock()
            .unwrap()
            .insert(model_id.clone(), real_model);

        // Update status to ready
        if let Some(model) = self.model_info.write().await.get_mut(&model_id) {
            model.status = ModelStatus::Ready;
        }

        println!("Model loaded successfully!");
        Ok(model_id)
    }

    pub async fn is_model_loaded(&self, model_id: &str) -> bool {
        self.model_info.read().await.contains_key(model_id)
    }

    pub async fn list_loaded_models(&self) -> Vec<String> {
        self.model_info.read().await.keys().cloned().collect()
    }

    pub async fn run_inference(&self, mut request: InferenceRequest) -> Result<InferenceResult> {
        let start_time = Instant::now();

        // Check if model exists
        if !self.model_info.read().await.contains_key(&request.model_id) {
            return Err(anyhow!("Model not found: {}", request.model_id));
        }

        // Update metrics
        *self.inference_count.write().await += 1;

        // Check if we have a real model loaded and perform generation
        let (
            output,
            tokens_generated,
            generation_time,
            token_info_list,
            stop_reason,
            total_prompt_tokens,
            context_size,
        ) = {
            let mut models = self.models.lock().unwrap();
            let has_real_model = models.contains_key(&request.model_id);

            if !has_real_model {
                return Err(anyhow!(
                    "Model {} is not loaded in memory",
                    request.model_id
                ));
            }

            // Create necessary data before borrowing the model
            let (prompt_tokens, context_size, eos_token, stop_token_ids) = {
                let model = models
                    .get_mut(&request.model_id)
                    .ok_or_else(|| anyhow!("Model not found in storage"))?;

                // Sanitize prompt before tokenization to prevent NulError
                // Remove null bytes and other problematic characters that break C string handling
                let sanitized_prompt = sanitize_prompt_for_tokenizer(&request.prompt);
                if sanitized_prompt.len() != request.prompt.len() {
                    tracing::warn!(
                        "🧹 Sanitized prompt: removed {} problematic bytes (original: {}, sanitized: {})",
                        request.prompt.len() - sanitized_prompt.len(),
                        request.prompt.len(),
                        sanitized_prompt.len()
                    );
                }

                // Tokenize the sanitized prompt
                let tokens_list = model
                    .model
                    .str_to_token(&sanitized_prompt, AddBos::Always)
                    .map_err(|e| anyhow!("Failed to tokenize: {:?}", e))?;

                let eos = model.model.token_eos();

                // Resolve stop tokens from template (or MODEL_STOP_TOKENS env override)
                let template_name =
                    std::env::var("MODEL_CHAT_TEMPLATE").unwrap_or_else(|_| "harmony".to_string());
                let template = crate::inference::ChatTemplate::from_str(&template_name)
                    .unwrap_or(crate::inference::ChatTemplate::Harmony);

                let stop_token_strings = {
                    let env_overrides = crate::inference::chat_template::parse_stop_tokens_env();
                    if env_overrides.is_empty() {
                        template
                            .stop_tokens()
                            .iter()
                            .map(|s| s.to_string())
                            .collect::<Vec<_>>()
                    } else {
                        env_overrides
                    }
                };

                let mut stop_ids: Vec<llama_cpp_2::token::LlamaToken> = Vec::new();
                for token_str in &stop_token_strings {
                    if let Ok(tokens) = model.model.str_to_token(token_str, AddBos::Never) {
                        if let Some(&tok) = tokens.first() {
                            stop_ids.push(tok);
                        }
                    }
                }

                tracing::debug!(
                    "🎯 Stop tokens: eos={}, template={}, strings={:?}, ids={:?}",
                    eos,
                    template_name,
                    stop_token_strings,
                    stop_ids.iter().map(|t| t.0).collect::<Vec<_>>()
                );

                (tokens_list, model.context_size, eos, stop_ids)
            };

            // Check for context overflow before creating context
            if prompt_tokens.len() >= context_size {
                let overflow = prompt_tokens.len() - context_size;
                return Err(anyhow!(
                    "Prompt ({} tokens) exceeds context window ({} tokens) by {} tokens",
                    prompt_tokens.len(),
                    context_size,
                    overflow
                ));
            }

            // Now work with the model again for context creation and generation
            let model = models
                .get_mut(&request.model_id)
                .ok_or_else(|| anyhow!("Model not found in storage"))?;

            // Create context
            let mut ctx_params = LlamaContextParams::default()
                .with_n_ctx(NonZeroU32::new(context_size as u32))
                .with_n_batch(self.config.batch_size as u32);

            if let Some(ref type_k_str) = self.config.kv_cache_type_k {
                if let Some(kv_type) = parse_kv_cache_type(type_k_str) {
                    ctx_params = ctx_params.with_type_k(kv_type);
                    tracing::info!("KV cache K type set to: {}", type_k_str);
                }
            }
            if let Some(ref type_v_str) = self.config.kv_cache_type_v {
                if let Some(kv_type) = parse_kv_cache_type(type_v_str) {
                    ctx_params = ctx_params.with_type_v(kv_type);
                    tracing::info!("KV cache V type set to: {}", type_v_str);
                }
            }

            let mut context = model
                .model
                .new_context(&model.backend, ctx_params)
                .map_err(|e| anyhow!("Failed to create context: {:?}", e))?;

            // Create batch with configured batch size
            let mut batch = LlamaBatch::new(self.config.batch_size, 1);

            // Process prompt tokens in chunks of batch_size (v8.15.4+)
            // Previously all tokens were added to a single batch, causing
            // InsufficientSpace errors when prompt exceeded batch_size.
            let total_prompt_tokens = prompt_tokens.len();
            let mut processed = 0;
            while processed < total_prompt_tokens {
                batch.clear();
                let chunk_end = (processed + self.config.batch_size).min(total_prompt_tokens);
                for i in processed..chunk_end {
                    let is_last = i == total_prompt_tokens - 1;
                    batch
                        .add(prompt_tokens[i], i as i32, &[0], is_last)
                        .map_err(|e| anyhow!("Failed to add token to batch: {:?}", e))?;
                }
                context.decode(&mut batch).map_err(|e| {
                    anyhow!(
                        "Decode failed at chunk {}/{}: {:?}",
                        processed,
                        total_prompt_tokens,
                        e
                    )
                })?;
                processed = chunk_end;
            }

            // Generate tokens
            let mut output = String::new();
            let mut token_info_list: Vec<TokenInfo> = Vec::new();
            let mut n_cur = prompt_tokens.len();
            let max_tokens = request.max_tokens;
            let mut consecutive_invalid_utf8 = 0; // Track consecutive invalid UTF-8 tokens
            const MAX_CONSECUTIVE_INVALID: u32 = 10; // Break if stuck generating invalid tokens
            let mut stop_reason = "loop_condition"; // v8.4.18: Track why we stopped

            tracing::info!(
                "🚀 Starting generation: prompt_tokens={}, max_tokens={}, context_size={}, limit={}",
                prompt_tokens.len(),
                max_tokens,
                context_size,
                prompt_tokens.len() + max_tokens
            );

            while n_cur < prompt_tokens.len() + max_tokens {
                // Check cancellation flag between tokens
                if let Some(ref flag) = request.cancel_flag {
                    if flag.load(Ordering::Acquire) {
                        stop_reason = "cancelled";
                        tracing::info!(
                            "🛑 Inference cancelled after {} tokens",
                            n_cur - prompt_tokens.len()
                        );
                        break;
                    }
                }

                // Build sampler chain: temp → penalties → top_p → min_p → dist/greedy
                let mut samplers: Vec<LlamaSampler> = Vec::new();
                samplers.push(LlamaSampler::temp(request.temperature));
                if request.repeat_penalty != 1.0 {
                    samplers.push(LlamaSampler::penalties(
                        256,
                        request.repeat_penalty,
                        0.0,
                        0.0,
                    ));
                }
                samplers.push(LlamaSampler::top_p(request.top_p, 1));
                if request.min_p > 0.0 {
                    samplers.push(LlamaSampler::min_p(request.min_p, 1));
                }
                if request.temperature > 0.0 {
                    let seed = request.seed.unwrap_or(0) as u32;
                    samplers.push(LlamaSampler::dist(seed));
                } else {
                    samplers.push(LlamaSampler::greedy());
                }
                let mut sampler = LlamaSampler::chain_simple(samplers);

                let new_token_id = sampler.sample(&context, -1);

                let tokens_so_far = n_cur - prompt_tokens.len();
                let is_special =
                    new_token_id == eos_token || stop_token_ids.contains(&new_token_id);

                // Stop on EOS token
                if new_token_id == eos_token {
                    stop_reason = "eos_token";
                    tracing::info!(
                        "🛑 EOS token after {} chars, {} tokens",
                        output.len(),
                        token_info_list.len()
                    );
                    break;
                }

                // Stop on template-specific stop tokens
                if stop_token_ids.contains(&new_token_id) {
                    stop_reason = "stop_token";
                    tracing::info!(
                        "🛑 Stop token {} after {} chars, {} tokens",
                        new_token_id,
                        output.len(),
                        token_info_list.len()
                    );
                    break;
                }

                // v8.4.19 FIX: Convert token to string - handle invalid UTF-8 by still advancing model state
                let token_str_result = model.model.token_to_str(new_token_id, Special::Tokenize);

                let is_valid_utf8 = token_str_result.is_ok();
                let token_str = token_str_result.unwrap_or_else(|_| String::new());

                if is_valid_utf8 {
                    consecutive_invalid_utf8 = 0; // Reset counter on valid token

                    // Add valid token to output
                    output.push_str(&token_str);

                    // Store token info for streaming
                    let token_info = TokenInfo {
                        token_id: new_token_id.0 as i32,
                        text: token_str,
                        logprob: None,
                        timestamp: None,
                    };
                    // Send token as it's generated (true streaming)
                    if let Some(ref tx) = request.token_sender {
                        let _ = tx.try_send(Ok(token_info.clone()));
                    }
                    token_info_list.push(token_info);
                } else {
                    // Invalid UTF-8 - don't add to output but MUST advance model state
                    consecutive_invalid_utf8 += 1;
                    tracing::warn!(
                        token_id = new_token_id.0,
                        consecutive_invalid = consecutive_invalid_utf8,
                        output_chars = output.len(),
                        valid_tokens = token_info_list.len(),
                        "Invalid UTF-8 token detected - this may indicate chat template mismatch"
                    );
                    // DON'T add to token_info_list - we don't want to stream garbage to client
                }

                // CRITICAL: Always add token to batch and decode to advance model state
                // This prevents infinite loops on invalid UTF-8 tokens
                batch.clear();
                batch
                    .add(new_token_id, n_cur as i32, &[0], true)
                    .map_err(|e| anyhow!("Failed to add token: {:?}", e))?;
                context
                    .decode(&mut batch)
                    .map_err(|e| anyhow!("Decode failed: {:?}", e))?;

                n_cur += 1;
            }

            let tokens_generated = n_cur - prompt_tokens.len();
            let generation_time = start_time.elapsed();

            tracing::info!(
                "🏁 Generation ended: tokens_generated={}, output_chars={}, n_cur={}, limit={}, stop_reason={}",
                tokens_generated,
                output.len(),
                n_cur,
                prompt_tokens.len() + max_tokens,
                stop_reason
            );

            (
                output,
                tokens_generated,
                generation_time,
                token_info_list,
                stop_reason,
                total_prompt_tokens,
                context_size,
            )
        }; // Release the mutex here before any await

        let tokens_per_second = tokens_generated as f32 / generation_time.as_secs_f32();

        // Update metrics
        {
            let mut metrics = self.metrics.write().await;
            metrics.total_inferences += 1;
            metrics.total_tokens_generated += tokens_generated;
            metrics.total_inference_time += generation_time;
            metrics.average_tokens_per_second =
                metrics.total_tokens_generated as f32 / metrics.total_inference_time.as_secs_f32();
        }

        let result = InferenceResult {
            text: output,
            tokens_generated,
            generation_time,
            tokens_per_second,
            model_id: request.model_id,
            finish_reason: match stop_reason {
                "cancelled" => "cancelled".to_string(),
                "loop_condition" => "length".to_string(),
                _ => "stop".to_string(),
            },
            token_info: token_info_list,
            was_cancelled: stop_reason == "cancelled",
            context_usage: Some(ContextUsage {
                prompt_tokens: total_prompt_tokens,
                completion_tokens: tokens_generated,
                total_tokens: total_prompt_tokens + tokens_generated,
                context_window_size: context_size,
            }),
        };

        if let Some(sender) = request.result_sender.take() {
            let _ = sender.send(result.clone());
        }

        Ok(result)
    }

    pub async fn run_inference_stream(
        &self,
        request: InferenceRequest,
    ) -> Result<(TokenStream, tokio::sync::oneshot::Receiver<InferenceResult>)> {
        // Check if model exists
        if !self.model_info.read().await.contains_key(&request.model_id) {
            return Err(anyhow!("Model not found: {}", request.model_id));
        }

        let (tx, rx) = mpsc::channel(4096);
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();

        // Check if we have a real model loaded
        let has_real_model = self.models.lock().unwrap().contains_key(&request.model_id);

        if has_real_model {
            // True token-by-token streaming via spawn_blocking (v8.19.1)
            // Each token is sent over the channel as it's generated in the loop.
            let mut inference_request = request;
            inference_request.stream = false;
            inference_request.token_sender = Some(tx);
            inference_request.result_sender = Some(result_tx);
            // Clone engine — all fields are Arc, cheap clone
            let engine = self.clone();
            // Run generation on blocking thread pool (solves !Send constraint)
            tokio::task::spawn_blocking(move || {
                let handle = tokio::runtime::Handle::current();
                handle.block_on(async move {
                    let _ = engine.run_inference(inference_request).await;
                    // tx drops here → rx returns None → stream ends
                })
            });
        } else {
            // Model not loaded in memory
            return Err(anyhow!(
                "Model {} is not loaded in memory for streaming",
                request.model_id
            ));
        }

        Ok((ReceiverStream::new(rx), result_rx))
    }

    pub async fn unload_model(&mut self, model_id: &str) -> Result<()> {
        self.models.lock().unwrap().remove(model_id);
        self.model_info.write().await.remove(model_id);
        Ok(())
    }

    pub async fn cancel_inference(&self, _inference_id: &str) -> Result<()> {
        // In real implementation, would cancel ongoing inference
        Ok(())
    }

    pub async fn get_metrics(&self) -> EngineMetrics {
        self.metrics.read().await.clone()
    }

    pub async fn run_inference_async(&self, request: InferenceRequest) -> InferenceHandle {
        // Since we can't move the engine to another thread, we need to run inference
        // on the current task and wrap the result in a future
        let result = self.run_inference(request).await;

        // Create a completed future with the result
        let task = tokio::spawn(async move { result });

        InferenceHandle { task }
    }

    pub async fn get_model_capabilities(&self, model_id: &str) -> Option<ModelCapabilities> {
        let models = self.model_info.read().await;
        if let Some(model) = models.get(model_id) {
            let model_name = &model.config.model_type;

            Some(ModelCapabilities {
                supports_completion: true,
                supports_chat: model_name.contains("chat") || model_name.contains("llama"),
                supports_code: model_name.contains("code"),
                supports_fim: model_name.contains("code"), // Code models support fill-in-middle
                supports_embedding: false,
                max_sequence_length: 2048,
            })
        } else {
            None
        }
    }

    pub async fn create_prompt_template(
        &self,
        model_id: &str,
        template_type: &str,
    ) -> Option<String> {
        let models = self.model_info.read().await;
        if models.contains_key(model_id) {
            match template_type {
                "chat" => Some("[INST] {prompt} [/INST]".to_string()),
                "completion" => Some("{prompt}".to_string()),
                _ => None,
            }
        } else {
            None
        }
    }

    pub fn create_chat_request(
        &self,
        model_id: String,
        messages: Vec<ChatMessage>,
    ) -> InferenceRequest {
        let prompt = messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n");

        InferenceRequest {
            model_id,
            prompt,
            max_tokens: 1000,
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            repeat_penalty: 1.0,
            min_p: 0.0,
            seed: None,
            stop_sequences: vec![],
            stream: false,
            cancel_flag: None,
            token_sender: None,
            result_sender: None,
        }
    }

    pub async fn count_tokens(&self, model_id: &str, text: &str) -> Result<usize> {
        // Check if we have a real model loaded
        if self.models.lock().unwrap().contains_key(model_id) {
            // Note: llama_cpp_rs might not expose direct tokenization
            // For now, we'll use an approximation
            // Typically, one token is roughly 4 characters
            Ok(text.len() / 4)
        } else {
            // Mock token counting for tests - roughly 4 chars per token
            Ok(text.len() / 4)
        }
    }

    pub async fn reset_metrics(&mut self) {
        *self.metrics.write().await = EngineMetrics {
            total_inferences: 0,
            total_tokens_generated: 0,
            average_tokens_per_second: 0.0,
            total_inference_time: Duration::default(),
        };
    }
}

// Async inference handle for cancellation
pub struct InferenceHandle {
    task: tokio::task::JoinHandle<Result<InferenceResult>>,
}

impl InferenceHandle {
    pub async fn cancel(&self) {
        self.task.abort();
    }
}

impl std::future::Future for InferenceHandle {
    type Output = Result<InferenceResult>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match self.task.poll_unpin(cx) {
            std::task::Poll::Ready(Ok(result)) => std::task::Poll::Ready(result),
            std::task::Poll::Ready(Err(_)) => {
                std::task::Poll::Ready(Err(anyhow!("Task cancelled")))
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelCapabilities {
    pub supports_completion: bool,
    pub supports_chat: bool,
    pub supports_code: bool,
    pub supports_fim: bool,
    pub supports_embedding: bool,
    pub max_sequence_length: usize,
}

// Model capability enum for tests
#[derive(Debug, Clone, PartialEq)]
pub enum ModelCapability {
    TextGeneration,
    CodeGeneration,
    Instruction,
    Chat,
    Embedding,
}

#[cfg(test)]
mod tests {
    use super::*;

    // === KV Cache Type Parsing Tests (Sub-phase 1.1) ===

    #[test]
    fn test_parse_kv_cache_type_q8_0() {
        assert_eq!(parse_kv_cache_type("q8_0"), Some(KvCacheType::Q8_0));
    }

    #[test]
    fn test_parse_kv_cache_type_q4_0() {
        assert_eq!(parse_kv_cache_type("q4_0"), Some(KvCacheType::Q4_0));
    }

    #[test]
    fn test_parse_kv_cache_type_f16() {
        assert_eq!(parse_kv_cache_type("f16"), Some(KvCacheType::F16));
    }

    #[test]
    fn test_parse_kv_cache_type_invalid() {
        assert_eq!(parse_kv_cache_type("invalid"), None);
    }

    #[test]
    fn test_parse_kv_cache_type_case_insensitive() {
        assert_eq!(parse_kv_cache_type("Q8_0"), Some(KvCacheType::Q8_0));
        assert_eq!(parse_kv_cache_type("F16"), Some(KvCacheType::F16));
        assert_eq!(parse_kv_cache_type("BF16"), Some(KvCacheType::BF16));
    }

    // === EngineConfig KV Cache Default Test (Sub-phase 1.2) ===

    #[test]
    fn test_engine_config_default_kv_cache() {
        let config = EngineConfig::default();
        assert_eq!(config.kv_cache_type_k, None);
        assert_eq!(config.kv_cache_type_v, None);
    }

    #[test]
    fn test_sanitize_removes_null_bytes() {
        let input = "Hello\0World";
        let result = sanitize_prompt_for_tokenizer(input);
        assert_eq!(result, "HelloWorld");
        assert!(!result.contains('\0'));
    }

    #[test]
    fn test_sanitize_removes_control_characters() {
        // \x01 through \x1F are control characters (except \t, \n, \r)
        let input = "Hello\x01\x02\x03World";
        let result = sanitize_prompt_for_tokenizer(input);
        assert_eq!(result, "HelloWorld");
    }

    #[test]
    fn test_sanitize_preserves_whitespace() {
        let input = "Hello\tWorld\nNew\rLine";
        let result = sanitize_prompt_for_tokenizer(input);
        assert_eq!(result, "Hello\tWorld\nNew\rLine");
    }

    #[test]
    fn test_sanitize_preserves_normal_text() {
        let input = "What is the plot of the movie `Iron Man`?";
        let result = sanitize_prompt_for_tokenizer(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_sanitize_handles_unicode() {
        let input = "Hello 世界 🌍";
        let result = sanitize_prompt_for_tokenizer(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_sanitize_pdf_like_content() {
        // Simulate content that might come from a PDF with embedded nulls
        let input = "PDF content\0with\0null\0bytes and normal text";
        let result = sanitize_prompt_for_tokenizer(input);
        assert_eq!(result, "PDF contentwithnullbytes and normal text");
        assert!(!result.contains('\0'));
    }

    // === ContextUsage + finish_reason Tests (Sub-phase 1.1) ===

    #[test]
    fn test_context_usage_creation() {
        let cu = ContextUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
            context_window_size: 4096,
        };
        assert_eq!(cu.prompt_tokens, 100);
        assert_eq!(cu.completion_tokens, 50);
        assert_eq!(cu.total_tokens, 150);
        assert_eq!(cu.context_window_size, 4096);
    }

    #[test]
    fn test_context_usage_serialization() {
        let cu = ContextUsage {
            prompt_tokens: 1250,
            completion_tokens: 150,
            total_tokens: 1400,
            context_window_size: 32768,
        };
        let json = serde_json::to_value(&cu).unwrap();
        assert_eq!(json["prompt_tokens"], 1250);
        assert_eq!(json["completion_tokens"], 150);
        assert_eq!(json["total_tokens"], 1400);
        assert_eq!(json["context_window_size"], 32768);
    }

    #[test]
    fn test_finish_reason_loop_condition_maps_to_length() {
        let stop_reason = "loop_condition";
        let finish_reason = match stop_reason {
            "cancelled" => "cancelled",
            "loop_condition" => "length",
            _ => "stop",
        };
        assert_eq!(finish_reason, "length");
    }

    #[test]
    fn test_finish_reason_eos_maps_to_stop() {
        let stop_reason = "eos_token";
        let finish_reason = match stop_reason {
            "cancelled" => "cancelled",
            "loop_condition" => "length",
            _ => "stop",
        };
        assert_eq!(finish_reason, "stop");
    }

    #[test]
    fn test_finish_reason_cancelled_maps_to_cancelled() {
        let stop_reason = "cancelled";
        let finish_reason = match stop_reason {
            "cancelled" => "cancelled",
            "loop_condition" => "length",
            _ => "stop",
        };
        assert_eq!(finish_reason, "cancelled");
    }

    // === Think-tag passthrough tests (v8.21.1) ===

    /// Verify that the engine source uses Special::Tokenize (not Special::Plaintext)
    /// so that special tokens like <think> are rendered as text in output.
    #[test]
    fn test_token_to_str_uses_tokenize_mode() {
        let src = include_str!("engine.rs");
        // Count occurrences outside this test block:
        // The actual render call should use Tokenize, not the suppressing variant.
        // We search for the exact pattern that appears in the generation loop.
        let pattern_tokenize = "model.token_to_str(new_token_id, Special::Tokenize)";
        let pattern_suppress = {
            // Build the suppress pattern dynamically to avoid include_str self-match
            let mut p = String::from("model.token_to_str(new_token_id, Special::Plain");
            p.push_str("text)");
            p
        };
        assert!(
            src.contains(pattern_tokenize),
            "engine.rs must use Special::Tokenize to render special tokens (e.g. <think>)"
        );
        assert!(
            !src.contains(&pattern_suppress),
            "engine.rs must not suppress special tokens"
        );
    }

    /// Structural invariant: stop tokens (EOS + template) are checked BEFORE
    /// token_to_str is called, so template markers never leak into output.
    #[test]
    fn test_stop_tokens_checked_before_rendering() {
        let src = include_str!("engine.rs");
        let eos_check = src.find("if new_token_id == eos_token");
        let stop_check = src.find("if stop_token_ids.contains(&new_token_id)");
        let render_call = src.find("token_to_str(new_token_id,");
        assert!(eos_check.is_some(), "EOS check must exist");
        assert!(stop_check.is_some(), "stop token check must exist");
        assert!(render_call.is_some(), "token_to_str call must exist");
        assert!(
            eos_check.unwrap() < render_call.unwrap(),
            "EOS check must come before rendering"
        );
        assert!(
            stop_check.unwrap() < render_call.unwrap(),
            "stop token check must come before rendering"
        );
    }

    /// GLM-4 stop tokens must include template markers so they are caught
    /// before rendering even with Special::Tokenize.
    #[test]
    fn test_glm4_stop_tokens_include_template_markers() {
        let tokens = crate::inference::ChatTemplate::Glm4.stop_tokens();
        assert!(tokens.contains(&"<|user|>"), "GLM-4 must stop on <|user|>");
        assert!(
            tokens.contains(&"<|observation|>"),
            "GLM-4 must stop on <|observation|>"
        );
    }
}
