// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::{anyhow, Result};
use futures::FutureExt;
use llama_cpp_2::{
    context::params::LlamaContextParams,
    llama_backend::LlamaBackend,
    llama_batch::LlamaBatch,
    model::{params::LlamaModelParams, AddBos, LlamaModel, Special},
    sampling::LlamaSampler,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

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
                .unwrap_or(2048),  // Increased default from 512 to 2048
            use_mmap: true,
            use_mlock: false,
            max_concurrent_inferences: 4,
            model_eviction_policy: "lru".to_string(),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceRequest {
    pub model_id: String,
    pub prompt: String,
    pub max_tokens: usize,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: usize,
    pub repeat_penalty: f32,
    pub seed: Option<u64>,
    pub stop_sequences: Vec<String>,
    pub stream: bool,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub token_id: i32,
    pub text: String,
    pub logprob: Option<f32>,
    pub timestamp: Option<f32>,
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

    pub async fn run_inference(&self, request: InferenceRequest) -> Result<InferenceResult> {
        let start_time = Instant::now();

        // Debug log the prompt
        println!("DEBUG: Inference prompt: {:?}", request.prompt);

        // Check if model exists
        if !self.model_info.read().await.contains_key(&request.model_id) {
            return Err(anyhow!("Model not found: {}", request.model_id));
        }

        // Update metrics
        *self.inference_count.write().await += 1;

        // Check if we have a real model loaded and perform generation
        let (output, tokens_generated, generation_time, token_info_list) = {
            let mut models = self.models.lock().unwrap();
            let has_real_model = models.contains_key(&request.model_id);

            if !has_real_model {
                return Err(anyhow!(
                    "Model {} is not loaded in memory",
                    request.model_id
                ));
            }

            // Create necessary data before borrowing the model
            let (prompt_tokens, context_size, eos_token, return_token) = {
                let model = models
                    .get_mut(&request.model_id)
                    .ok_or_else(|| anyhow!("Model not found in storage"))?;

                // Tokenize the prompt
                let tokens_list = model
                    .model
                    .str_to_token(&request.prompt, AddBos::Always)
                    .map_err(|e| anyhow!("Failed to tokenize: {:?}", e))?;

                let eos = model.model.token_eos();

                // Get token ID for "<|return|>" special token (GPT-OSS-20B Harmony format stop token)
                // Official spec: https://cookbook.openai.com/articles/openai-harmony
                // <|return|> (200002) is the proper stop token for Harmony format
                let return_tok = model
                    .model
                    .str_to_token("<|return|>", AddBos::Never)
                    .ok()
                    .and_then(|tokens| tokens.first().copied())
                    .unwrap_or_else(|| {
                        // Fallback: create LlamaToken from known ID for GPT-OSS-20B
                        use llama_cpp_2::token::LlamaToken;
                        unsafe { LlamaToken::new(200002) }
                    });

                tracing::debug!("🎯 Token IDs: eos_token={}, return_token={}", eos, return_tok);

                (tokens_list, model.context_size, eos, return_tok)
            };

            // Now work with the model again for context creation and generation
            let model = models
                .get_mut(&request.model_id)
                .ok_or_else(|| anyhow!("Model not found in storage"))?;

            // Create context
            let ctx_params = LlamaContextParams::default()
                .with_n_ctx(NonZeroU32::new(context_size as u32))
                .with_n_batch(self.config.batch_size as u32);

            let mut context = model
                .model
                .new_context(&model.backend, ctx_params)
                .map_err(|e| anyhow!("Failed to create context: {:?}", e))?;

            // Create batch
            let mut batch = LlamaBatch::new(512, 1);

            // Add all tokens to batch with only last one requesting logits
            for (i, &token) in prompt_tokens.iter().enumerate() {
                let is_last = i == prompt_tokens.len() - 1;
                batch
                    .add(token, i as i32, &[0], is_last)
                    .map_err(|e| anyhow!("Failed to add token to batch: {:?}", e))?;
            }

            context
                .decode(&mut batch)
                .map_err(|e| anyhow!("Decode failed: {:?}", e))?;

            // Generate tokens
            let mut output = String::new();
            let mut token_info_list: Vec<TokenInfo> = Vec::new();
            let mut n_cur = prompt_tokens.len();
            let max_tokens = request.max_tokens;
            let mut recent_text = String::new(); // Track recent text for pattern detection
            let mut seen_first_response = false; // Track if we've started generating assistant response

            while n_cur < prompt_tokens.len() + max_tokens {
                // Sample next token using sampler chain
                let mut sampler = LlamaSampler::chain_simple([
                    LlamaSampler::temp(request.temperature),
                    LlamaSampler::top_p(request.top_p, 1),
                    LlamaSampler::greedy(),
                ]);

                let new_token_id = sampler.sample(&context, -1);

                tracing::debug!("🔤 Generated token: {} (comparing to eos={}, return={})", new_token_id, eos_token, return_token);

                // Check for EOS or <|return|> token (GPT-OSS-20B Harmony format stop token)
                if new_token_id == eos_token || new_token_id == return_token {
                    tracing::debug!("🛑 Stop token detected: {} (eos={}, return={})", new_token_id, eos_token, return_token);
                    break;
                }

                // Convert token to string
                let token_str = model
                    .model
                    .token_to_str(new_token_id, Special::Plaintext)
                    .map_err(|e| anyhow!("Token to string failed: {:?}", e))?;

                // Update recent text buffer (keep last 50 chars for pattern detection)
                recent_text.push_str(&token_str);
                if recent_text.len() > 50 {
                    recent_text = recent_text.chars().skip(recent_text.len() - 50).collect();
                }

                // Mark that we've started generating content
                if !output.is_empty() {
                    seen_first_response = true;
                }

                // Check for conversation patterns that indicate we should stop
                // This prevents the model from continuing to generate conversation history
                // BUT only after we've generated at least some response

                let mut should_stop = false;
                if output.len() > 5 {
                    // After we have at least some content
                    // Check various conversation continuation patterns
                    let stop_patterns = [
                        // Our format
                        "\nUser:",
                        "\nAssistant:",
                        ". User:",
                        ". Assistant:",
                        // Common continuations
                        "\nuser:",
                        "\nassistant:",
                        // Q&A patterns
                        "\nQ:",
                        "\nA:",
                        ". Q:",
                        ". A:",
                    ];

                    // Check if output contains any of these patterns
                    for pattern in &stop_patterns {
                        if output.contains(pattern) {
                            // Find where the pattern starts and truncate there
                            if let Some(pos) = output.rfind(pattern) {
                                output.truncate(pos);

                                // Also truncate token_info_list to match
                                // Count how many tokens to keep
                                let mut kept_len = 0;
                                let mut tokens_to_keep = 0;
                                for (i, token) in token_info_list.iter().enumerate() {
                                    kept_len += token.text.len();
                                    if kept_len >= output.len() {
                                        tokens_to_keep = i + 1;
                                        break;
                                    }
                                }
                                token_info_list.truncate(tokens_to_keep);

                                // Stop generation completely
                                should_stop = true;
                                break;
                            }
                        }
                    }
                }

                if should_stop {
                    break; // Break from the main generation loop
                }

                // Pattern 2: Check if we're about to start a conversation turn
                // (when we have a newline and the current token starts a role)
                // Only check this after we've generated some response
                if seen_first_response
                    && output.len() > 10
                    && (output.ends_with('\n') || output.ends_with("\n\n"))
                {
                    // Check for Vicuna format role markers (uppercase)
                    if token_str.starts_with("USER")
                        || token_str.starts_with("ASSISTANT")
                        || token_str.starts_with("User")
                        || token_str.starts_with("Assistant")
                    {
                        // Don't include this token, stop here
                        break;
                    }
                }

                // Pattern 3: Stop if output is getting repetitive or too long for a single response
                // This helps when the model starts hallucinating conversations
                if output.len() > 500 {
                    // Reasonable response length
                    // Check if we're starting to repeat conversation patterns
                    if output.contains("\nuser:") && output.contains("\nassistant:") {
                        // We already have a full exchange, stop here
                        break;
                    }
                }

                // Additional check: Stop if model starts asking questions
                // This is a common pattern when models continue conversations
                if output.len() > 20 {
                    // Check if we're starting a new question
                    // Use char_indices to avoid panicking on UTF-8 boundaries
                    let last_30 = if output.chars().count() > 30 {
                        // Find the byte index 30 characters from the end
                        let char_count = output.chars().count();
                        let start_char_idx = char_count.saturating_sub(30);
                        output
                            .char_indices()
                            .nth(start_char_idx)
                            .map(|(byte_idx, _)| &output[byte_idx..])
                            .unwrap_or(&output)
                    } else {
                        &output
                    };

                    // If we just finished a sentence and starting a question
                    if (last_30.contains(". What ")
                        || last_30.contains("? What ")
                        || last_30.contains("! What ")
                        || last_30.contains("\nWhat "))
                    {
                        // Stop before "What"
                        if let Some(pos) = output.rfind(" What") {
                            output.truncate(pos);
                            // Adjust token list
                            let mut kept_len = 0;
                            let mut tokens_to_keep = 0;
                            for (i, token) in token_info_list.iter().enumerate() {
                                kept_len += token.text.len();
                                if kept_len >= output.len() {
                                    tokens_to_keep = i + 1;
                                    break;
                                }
                            }
                            token_info_list.truncate(tokens_to_keep);
                            break;
                        }
                    }
                }

                // If we get here, the token is safe to add
                output.push_str(&token_str);

                // Store token info for streaming
                // Clone the token_str since we're moving it into TokenInfo
                token_info_list.push(TokenInfo {
                    token_id: new_token_id.0 as i32,
                    text: token_str.clone(),
                    logprob: None,
                    timestamp: None,
                });

                // Add token to batch for next iteration
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

            (output, tokens_generated, generation_time, token_info_list)
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

        Ok(InferenceResult {
            text: output,
            tokens_generated,
            generation_time,
            tokens_per_second,
            model_id: request.model_id,
            finish_reason: "stop".to_string(),
            token_info: token_info_list, // Use the collected tokens!
            was_cancelled: false,
        })
    }

    pub async fn run_inference_stream(&self, request: InferenceRequest) -> Result<TokenStream> {
        // Check if model exists
        if !self.model_info.read().await.contains_key(&request.model_id) {
            return Err(anyhow!("Model not found: {}", request.model_id));
        }

        let (tx, rx) = mpsc::channel(100);

        // Check if we have a real model loaded
        let has_real_model = self.models.lock().unwrap().contains_key(&request.model_id);

        if has_real_model {
            // For real models, we need to run synchronously due to !Send constraint
            // We'll generate all tokens at once and then stream them
            // Make sure stream is false for the actual inference
            let mut inference_request = request;
            inference_request.stream = false;
            let result = self.run_inference(inference_request).await;

            // Spawn a task to stream the already-generated tokens
            tokio::spawn(async move {
                match result {
                    Ok(inference_result) => {
                        for token_info in inference_result.token_info {
                            if tx.send(Ok(token_info)).await.is_err() {
                                break;
                            }
                            // Add a small delay to simulate streaming
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e)).await;
                    }
                }
            });
        } else {
            // Model not loaded in memory
            return Err(anyhow!(
                "Model {} is not loaded in memory for streaming",
                request.model_id
            ));
        }

        Ok(ReceiverStream::new(rx))
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
            seed: None,
            stop_sequences: vec![],
            stream: false,
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
