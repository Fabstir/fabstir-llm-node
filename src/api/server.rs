// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use axum::{
    extract::{
        ws::{WebSocket, WebSocketUpgrade},
        Json, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};

use super::handlers::{HealthResponse, ModelInfo, ModelsResponse};
use super::pool::{ConnectionPool, ConnectionStats, PoolConfig};
use super::{ApiError, InferenceRequest, InferenceResponse, StreamingResponse};
use crate::api::token_tracker::TokenTracker;
use crate::contracts::checkpoint_manager::CheckpointManager;
use crate::crypto::SessionKeyStore;
use crate::inference::LlmEngine;
use crate::p2p::Node;
use crate::utils::context::{build_prompt_with_context, count_context_tokens};

// TODO: Implement full HTTP server using axum framework
// See tests/client/ for expected functionality

#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub listen_addr: String,
    pub max_connections: usize,
    pub max_connections_per_ip: usize,
    pub request_timeout: Duration,
    pub cors_allowed_origins: Vec<String>,
    pub enable_websocket: bool,
    pub require_api_key: bool,
    pub api_keys: Vec<String>,
    pub rate_limit_per_minute: usize,
    pub enable_http2: bool,
    pub enable_auto_retry: bool,
    pub max_retries: usize,
    pub enable_circuit_breaker: bool,
    pub circuit_breaker_threshold: usize,
    pub circuit_breaker_timeout: Duration,
    pub enable_error_details: bool,
    pub connection_idle_timeout: Duration,
    pub websocket_ping_interval: Duration,
    pub websocket_pong_timeout: Duration,
    pub max_concurrent_streams: usize,
    pub connection_retry_count: usize,
    pub connection_retry_backoff: Duration,
    pub shutdown_timeout: Duration,
    pub enable_connection_health_checks: bool,
    pub health_check_interval: Duration,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:8080".to_string(),
            max_connections: 1000,
            max_connections_per_ip: 10,
            request_timeout: Duration::from_secs(30),
            cors_allowed_origins: vec!["*".to_string()],
            enable_websocket: false,
            require_api_key: false,
            api_keys: Vec::new(),
            rate_limit_per_minute: 60,
            enable_http2: false,
            enable_auto_retry: false,
            max_retries: 3,
            enable_circuit_breaker: false,
            circuit_breaker_threshold: 5,
            circuit_breaker_timeout: Duration::from_secs(30),
            enable_error_details: false,
            connection_idle_timeout: Duration::from_secs(60),
            websocket_ping_interval: Duration::from_secs(30),
            websocket_pong_timeout: Duration::from_secs(10),
            max_concurrent_streams: 100,
            connection_retry_count: 3,
            connection_retry_backoff: Duration::from_millis(100),
            shutdown_timeout: Duration::from_secs(30),
            enable_connection_health_checks: false,
            health_check_interval: Duration::from_secs(10),
        }
    }
}

struct RateLimiter {
    requests: Arc<RwLock<HashMap<String, Vec<Instant>>>>,
    limit: usize,
}

impl RateLimiter {
    fn new(limit: usize) -> Self {
        Self {
            requests: Arc::new(RwLock::new(HashMap::new())),
            limit,
        }
    }

    async fn check_rate_limit(&self, key: &str) -> Result<(), ApiError> {
        let now = Instant::now();
        let one_minute_ago = now - Duration::from_secs(60);

        let mut requests = self.requests.write().await;
        let entry = requests.entry(key.to_string()).or_insert_with(Vec::new);

        // Remove old requests
        entry.retain(|&t| t > one_minute_ago);

        if entry.len() >= self.limit {
            return Err(ApiError::RateLimitExceeded { retry_after: 60 });
        }

        entry.push(now);
        Ok(())
    }
}

struct CircuitBreaker {
    failures: Arc<Mutex<usize>>,
    last_failure: Arc<Mutex<Option<Instant>>>,
    threshold: usize,
    timeout: Duration,
}

impl CircuitBreaker {
    fn new(threshold: usize, timeout: Duration) -> Self {
        Self {
            failures: Arc::new(Mutex::new(0)),
            last_failure: Arc::new(Mutex::new(None)),
            threshold,
            timeout,
        }
    }

    async fn is_open(&self) -> bool {
        let failures = *self.failures.lock().await;
        if failures < self.threshold {
            return false;
        }

        if let Some(last_failure) = *self.last_failure.lock().await {
            if Instant::now().duration_since(last_failure) > self.timeout {
                // Reset circuit breaker
                *self.failures.lock().await = 0;
                *self.last_failure.lock().await = None;
                return false;
            }
        }

        true
    }

    async fn record_success(&self) {
        *self.failures.lock().await = 0;
        *self.last_failure.lock().await = None;
    }

    async fn record_failure(&self) {
        let mut failures = self.failures.lock().await;
        *failures += 1;
        *self.last_failure.lock().await = Some(Instant::now());
    }
}

pub struct ApiServer {
    config: ApiConfig,
    addr: SocketAddr,
    node: Arc<RwLock<Option<Node>>>,
    engine: Arc<RwLock<Option<Arc<LlmEngine>>>>,
    default_model_id: Arc<RwLock<String>>,
    rate_limiter: Arc<RateLimiter>,
    circuit_breaker: Arc<CircuitBreaker>,
    connection_pool: Arc<ConnectionPool>,
    active_connections: Arc<RwLock<HashMap<String, usize>>>,
    metrics: Arc<RwLock<Metrics>>,
    token_tracker: Arc<TokenTracker>,
    checkpoint_manager: Arc<RwLock<Option<Arc<CheckpointManager>>>>,
    session_key_store: Arc<SessionKeyStore>,
    node_private_key: Option<[u8; 32]>,
    embedding_model_manager: Arc<RwLock<Option<Arc<crate::embeddings::EmbeddingModelManager>>>>,
    vision_model_manager: Arc<RwLock<Option<Arc<crate::vision::VisionModelManager>>>>,
    session_store: Arc<RwLock<crate::api::websocket::session_store::SessionStore>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    listener: Option<tokio::net::TcpListener>,
}

#[derive(Default)]
struct Metrics {
    total_requests: u64,
    total_errors: u64,
    request_durations: Vec<Duration>,
}

/// Session key metrics for monitoring
#[derive(Debug, Clone)]
pub struct SessionKeyMetrics {
    pub active_sessions: usize,
}

impl ApiServer {
    pub fn new_for_test() -> Self {
        let config = ApiConfig::default();
        let addr = "127.0.0.1:0".parse().unwrap();

        let session_store_config = crate::api::websocket::session_store::SessionStoreConfig::default();
        let session_store = Arc::new(RwLock::new(
            crate::api::websocket::session_store::SessionStore::new(session_store_config)
        ));

        ApiServer {
            config,
            addr,
            node: Arc::new(RwLock::new(None)),
            engine: Arc::new(RwLock::new(None)),
            default_model_id: Arc::new(RwLock::new("test-model".to_string())),
            rate_limiter: Arc::new(RateLimiter::new(100)),
            circuit_breaker: Arc::new(CircuitBreaker::new(5, Duration::from_secs(60))),
            connection_pool: Arc::new(ConnectionPool::new_for_test(PoolConfig::default())),
            active_connections: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(Metrics {
                total_requests: 0,
                total_errors: 0,
                request_durations: Vec::new(),
            })),
            token_tracker: Arc::new(TokenTracker::new()),
            checkpoint_manager: Arc::new(RwLock::new(None)),
            session_key_store: Arc::new(SessionKeyStore::new()),
            node_private_key: None,
            embedding_model_manager: Arc::new(RwLock::new(None)),
            vision_model_manager: Arc::new(RwLock::new(None)),
            session_store,
            shutdown_tx: None,
            listener: None,
        }
    }

    pub async fn new(config: ApiConfig) -> Result<Self> {
        // Version stamp for deployment verification
        eprintln!("🚀 API SERVER VERSION: {}", crate::version::VERSION);
        eprintln!("✅ Multi-chain support enabled (Base Sepolia + opBNB Testnet)");
        eprintln!("✅ Auto-settlement on disconnect enabled");
        eprintln!("🔍 Enhanced diagnostic logging for settlement debugging");

        // Parse the address
        let addr: SocketAddr = config.listen_addr.parse()?;

        // Bind to the address
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let actual_addr = listener.local_addr()?;

        let pool_config = PoolConfig {
            min_connections: 2,
            max_connections: config.max_connections,
            connection_timeout: config.request_timeout,
            idle_timeout: config.connection_idle_timeout,
            ..Default::default()
        };

        let connection_pool = Arc::new(ConnectionPool::new(pool_config).await?);

        // Extract node private key for encrypted sessions (Phase 6.2.1, Sub-phase 6.2)
        // If HOST_PRIVATE_KEY is not set, node will operate in plaintext-only mode
        let node_private_key = match crate::crypto::extract_node_private_key() {
            Ok(key) => {
                info!("🔐 Node private key loaded - encrypted sessions enabled");
                Some(key)
            }
            Err(e) => {
                warn!("⚠️ Failed to load node private key: {}", e);
                warn!("   Node will operate in plaintext-only mode");
                warn!("   Set HOST_PRIVATE_KEY environment variable to enable encrypted sessions");
                None
            }
        };

        // Initialize session store for RAG functionality
        let session_store_config = crate::api::websocket::session_store::SessionStoreConfig {
            max_sessions: 1000,
            cleanup_interval_seconds: 300,
            enable_metrics: true,
            enable_persistence: false,
        };
        let session_store = Arc::new(RwLock::new(
            crate::api::websocket::session_store::SessionStore::new(session_store_config)
        ));

        let mut server = Self {
            addr: actual_addr,
            node: Arc::new(RwLock::new(None)),
            engine: Arc::new(RwLock::new(None)),
            default_model_id: Arc::new(RwLock::new("tiny-vicuna".to_string())),
            rate_limiter: Arc::new(RateLimiter::new(config.rate_limit_per_minute)),
            circuit_breaker: Arc::new(CircuitBreaker::new(
                config.circuit_breaker_threshold,
                config.circuit_breaker_timeout,
            )),
            connection_pool,
            active_connections: Arc::new(RwLock::new(HashMap::new())),
            metrics: Arc::new(RwLock::new(Metrics::default())),
            token_tracker: Arc::new(TokenTracker::new()),
            checkpoint_manager: Arc::new(RwLock::new(None)),
            session_key_store: Arc::new(SessionKeyStore::new()),
            node_private_key,
            embedding_model_manager: Arc::new(RwLock::new(None)),
            vision_model_manager: Arc::new(RwLock::new(None)),
            session_store,
            shutdown_tx: None,
            listener: Some(listener),
            config,
        };

        // Start the HTTP server in the background
        server.start_http_server().await;

        Ok(server)
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    async fn start_http_server(&mut self) {
        if let Some(listener) = self.listener.take() {
            let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
            self.shutdown_tx = Some(shutdown_tx);

            let server = self.clone_for_http();

            tokio::spawn(async move {
                let app = Self::create_router(server);

                let serve_future = axum::serve(listener, app).with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                });

                let _ = serve_future.await;
            });
        }
    }

    fn clone_for_http(&self) -> Arc<Self> {
        Arc::new(Self {
            config: self.config.clone(),
            addr: self.addr,
            node: self.node.clone(),
            engine: self.engine.clone(),
            default_model_id: self.default_model_id.clone(),
            rate_limiter: self.rate_limiter.clone(),
            circuit_breaker: self.circuit_breaker.clone(),
            connection_pool: self.connection_pool.clone(),
            active_connections: self.active_connections.clone(),
            metrics: self.metrics.clone(),
            token_tracker: self.token_tracker.clone(),
            checkpoint_manager: self.checkpoint_manager.clone(),
            session_key_store: self.session_key_store.clone(),
            node_private_key: self.node_private_key,
            embedding_model_manager: self.embedding_model_manager.clone(),
            vision_model_manager: self.vision_model_manager.clone(),
            session_store: self.session_store.clone(),
            shutdown_tx: None,
            listener: None,
        })
    }

    pub fn set_node(&mut self, node: Node) {
        *self.node.blocking_write() = Some(node);
    }

    pub async fn set_engine(&self, engine: Arc<LlmEngine>) {
        *self.engine.write().await = Some(engine);
    }

    pub async fn set_default_model_id(&self, model_id: String) {
        *self.default_model_id.write().await = model_id;
    }

    pub async fn set_checkpoint_manager(&self, checkpoint_manager: Arc<CheckpointManager>) {
        *self.checkpoint_manager.write().await = Some(checkpoint_manager);
    }

    pub async fn get_checkpoint_manager(&self) -> Option<Arc<CheckpointManager>> {
        self.checkpoint_manager.read().await.clone()
    }

    pub async fn set_embedding_model_manager(&self, manager: Arc<crate::embeddings::EmbeddingModelManager>) {
        *self.embedding_model_manager.write().await = Some(manager);
    }

    pub async fn get_embedding_model_manager(&self) -> Option<Arc<crate::embeddings::EmbeddingModelManager>> {
        self.embedding_model_manager.read().await.clone()
    }

    pub async fn set_vision_model_manager(&self, manager: Arc<crate::vision::VisionModelManager>) {
        *self.vision_model_manager.write().await = Some(manager);
    }

    pub async fn get_vision_model_manager(&self) -> Option<Arc<crate::vision::VisionModelManager>> {
        self.vision_model_manager.read().await.clone()
    }

    /// Get the session key store for encryption/decryption operations
    pub fn get_session_key_store(&self) -> Arc<SessionKeyStore> {
        self.session_key_store.clone()
    }

    /// Get the node's private key for encrypted session initialization (Phase 6.2.1, Sub-phase 6.2)
    ///
    /// Returns `Some([u8; 32])` if the node has a private key configured (encrypted mode enabled),
    /// or `None` if operating in plaintext-only mode.
    ///
    /// The private key is used for ECDH key exchange during encrypted_session_init handshake.
    pub fn get_node_private_key(&self) -> Option<[u8; 32]> {
        self.node_private_key
    }

    /// Get session key metrics
    pub async fn session_key_metrics(&self) -> SessionKeyMetrics {
        SessionKeyMetrics {
            active_sessions: self.session_key_store.count().await,
        }
    }

    pub async fn connection_stats(&self) -> ConnectionStats {
        self.connection_pool.stats().await
    }

    pub async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }

    pub async fn handle_inference_request(
        &self,
        request: InferenceRequest,
        client_ip: String,
    ) -> Result<InferenceResponse, ApiError> {
        // Validate request
        request.validate()?;

        // Check rate limit
        if self.config.require_api_key {
            // Rate limit by API key if available
        } else {
            self.rate_limiter.check_rate_limit(&client_ip).await?;
        }

        // Check circuit breaker
        if self.config.enable_circuit_breaker && self.circuit_breaker.is_open().await {
            return Err(ApiError::CircuitBreakerOpen);
        }

        // Get engine
        let engine_guard = self.engine.read().await;
        let engine = engine_guard.as_ref().ok_or_else(|| {
            ApiError::ServiceUnavailable("inference engine not initialized".to_string())
        })?;

        // Use default model ID if model field is "tiny-vicuna" or similar
        let model_id = if request.model == "tiny-vicuna" || request.model.is_empty() {
            self.default_model_id.read().await.clone()
        } else {
            // Check if this specific model ID is loaded
            let loaded_models = engine.list_loaded_models().await;
            if loaded_models.contains(&request.model) {
                request.model.clone()
            } else {
                // Fall back to default
                self.default_model_id.read().await.clone()
            }
        };

        // Build prompt (always use the formatter for consistency)
        let full_prompt = build_prompt_with_context(&request.conversation_context, &request.prompt);

        if !request.conversation_context.is_empty() {
            info!(
                "Processing with {} context messages, ~{} tokens",
                request.conversation_context.len(),
                count_context_tokens(&request.conversation_context)
            );
        }

        // DEBUG: Log the actual prompt
        println!("DEBUG API: Sending prompt to engine: {:?}", full_prompt);

        // Create inference request for the engine
        let engine_request = crate::inference::InferenceRequest {
            model_id: model_id.clone(),
            prompt: full_prompt,
            max_tokens: request.max_tokens as usize,
            temperature: request.temperature,
            top_p: 0.9,
            top_k: 40,
            repeat_penalty: 1.1,
            seed: None,
            stop_sequences: vec![],
            stream: false,
        };

        // Run inference with real model
        let result = engine
            .run_inference(engine_request)
            .await
            .map_err(|e| ApiError::InternalError(format!("Inference failed: {}", e)))?;

        // Convert to API response
        let response = InferenceResponse {
            model: request.model.clone(),
            content: result.text,
            tokens_used: result.tokens_generated as u32,
            finish_reason: result.finish_reason,
            request_id: request
                .request_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            chain_id: request.chain_id,
            chain_name: None,
            native_token: None,
        };

        // Track tokens for checkpoint submission (non-streaming path)
        eprintln!("\n🔍 INFERENCE REQUEST RECEIVED:");
        eprintln!("   request.job_id: {:?}", request.job_id);
        eprintln!("   request.session_id: {:?}", request.session_id);
        eprintln!("   request.model: {}", request.model);
        eprintln!("   tokens to be used: {}", response.tokens_used);

        let job_id = request.job_id.or_else(|| {
            request.session_id.as_ref().and_then(|sid| {
                let parsed = sid.trim_end_matches('n').parse::<u64>().ok();
                eprintln!("   Parsing session_id '{}' -> job_id: {:?}", sid, parsed);
                parsed
            })
        });

        eprintln!("   FINAL job_id for tracking: {:?}", job_id);

        if let Some(jid) = job_id {
            if let Some(cm) = self.checkpoint_manager.read().await.as_ref() {
                eprintln!(
                    "📊 HTTP: Tracking {} tokens for job {} (session_id: {:?})",
                    response.tokens_used, jid, request.session_id
                );
                match cm
                    .track_tokens(jid, response.tokens_used as u64, request.session_id.clone())
                    .await
                {
                    Ok(_) => eprintln!("   ✅ Token tracking successful for job {}", jid),
                    Err(e) => eprintln!("   ❌ Token tracking failed for job {}: {}", jid, e),
                }
            } else {
                eprintln!("❌ CRITICAL: No checkpoint manager available!");
                eprintln!("   HOST_PRIVATE_KEY probably not set");
                eprintln!("   Tokens will NOT be tracked for settlement!");
                eprintln!(
                    "📊 Using simple token tracker for job {} (no checkpoint manager)",
                    jid
                );
                self.token_tracker
                    .track_tokens(
                        Some(jid),
                        response.tokens_used as usize,
                        request.session_id.clone(),
                    )
                    .await;
            }
        } else {
            eprintln!("⚠️ No job_id available for token tracking in non-streaming request");
        }

        // Record success
        if self.config.enable_circuit_breaker {
            self.circuit_breaker.record_success().await;
        }

        Ok(response)
    }

    pub async fn handle_streaming_request(
        &self,
        request: InferenceRequest,
        client_ip: String,
    ) -> Result<mpsc::Receiver<StreamingResponse>, ApiError> {
        // Validate and check limits (same as non-streaming)
        request.validate()?;
        self.rate_limiter.check_rate_limit(&client_ip).await?;

        if self.config.enable_circuit_breaker && self.circuit_breaker.is_open().await {
            return Err(ApiError::CircuitBreakerOpen);
        }

        // Get engine (same as non-streaming)
        let engine_guard = self.engine.read().await;
        let engine = engine_guard.as_ref().ok_or_else(|| {
            ApiError::ServiceUnavailable("inference engine not initialized".to_string())
        })?;

        // Use default model ID if model field is "tiny-vicuna" or similar
        let model_id = if request.model == "tiny-vicuna" || request.model.is_empty() {
            self.default_model_id.read().await.clone()
        } else {
            // Check if this specific model ID is loaded
            let loaded_models = engine.list_loaded_models().await;
            if loaded_models.contains(&request.model) {
                request.model.clone()
            } else {
                // Fall back to default
                self.default_model_id.read().await.clone()
            }
        };

        // Build prompt (always use the formatter for consistency)
        let full_prompt = build_prompt_with_context(&request.conversation_context, &request.prompt);

        if !request.conversation_context.is_empty() {
            info!(
                "Processing streaming request with {} context messages",
                request.conversation_context.len()
            );
        }

        // DEBUG: Log the actual prompt
        println!(
            "DEBUG STREAMING: Sending prompt to engine: {:?}",
            full_prompt
        );

        // Log the request for debugging
        info!(
            "Streaming inference request: model={}, prompt_len={}, max_tokens={}",
            model_id,
            full_prompt.len(),
            request.max_tokens
        );

        // Create inference request for the engine with stream=true
        let engine_request = crate::inference::InferenceRequest {
            model_id: model_id.clone(),
            prompt: full_prompt,
            max_tokens: request.max_tokens as usize,
            temperature: request.temperature,
            top_p: 0.9,
            top_k: 40,
            repeat_penalty: 1.1,
            seed: None,
            stop_sequences: vec![],
            stream: true, // Enable streaming!
        };

        // Run streaming inference with real model
        let token_stream = engine
            .run_inference_stream(engine_request)
            .await
            .map_err(|e| {
                error!("Failed to start streaming inference: {}", e);
                ApiError::InternalError(format!("Streaming inference failed: {}", e))
            })?;

        let (tx, rx) = mpsc::channel(100);

        // Clone values for the spawned task
        // If job_id is not provided but session_id is, try to parse session_id as job_id
        let job_id = request.job_id.or_else(|| {
            request.session_id.as_ref().and_then(|sid| {
                // Try to parse session_id as a number (SDK sends it as "139n" or just "139")
                let parsed = sid.trim_end_matches('n').parse::<u64>().ok();
                eprintln!(
                    "DEBUG: Parsing session_id '{}' -> job_id: {:?}",
                    sid, parsed
                );
                parsed
            })
        });

        // Log the job/session tracking
        if let Some(jid) = job_id {
            eprintln!("📝 TRACKING TOKENS for job_id/session_id: {}", jid);
            info!("📝 Tracking tokens for job_id/session_id: {}", jid);
        } else {
            eprintln!(
                "⚠️ NO JOB_ID - session_id: {:?}, job_id: {:?}",
                request.session_id, request.job_id
            );
        }

        let session_id = request.session_id.clone();
        let token_tracker = self.token_tracker.clone();
        let checkpoint_manager = self.checkpoint_manager.read().await.clone();

        // Spawn task to convert token stream to streaming responses
        tokio::spawn(async move {
            use futures::StreamExt;
            futures::pin_mut!(token_stream);

            let mut accumulated_text = String::new();
            let mut total_tokens = 0;
            let mut got_any_tokens = false;

            while let Some(token_result) = token_stream.next().await {
                match token_result {
                    Ok(token_info) => {
                        got_any_tokens = true;
                        accumulated_text.push_str(&token_info.text);
                        total_tokens += 1;

                        // Skip empty tokens except for the first one
                        if token_info.text.is_empty() && total_tokens > 1 {
                            continue;
                        }

                        // Track only non-empty tokens for checkpoint submission
                        if let Some(jid) = job_id {
                            // Use checkpoint manager if available, otherwise use simple token tracker
                            if let Some(cm) = checkpoint_manager.as_ref() {
                                eprintln!(
                                    "📊 Calling checkpoint_manager.track_tokens for job {}",
                                    jid
                                );
                                let _ = cm.track_tokens(jid, 1, session_id.clone()).await;
                            } else {
                                eprintln!("📊 Using simple token tracker (no checkpoint manager) for job {}", jid);
                                token_tracker
                                    .track_tokens(Some(jid), 1, session_id.clone())
                                    .await;
                            }
                        } else {
                            eprintln!("⚠️ No job_id available for token tracking");
                        }

                        let response = StreamingResponse {
                            content: token_info.text.clone(),
                            tokens: 1,
                            finish_reason: None,
                            chain_id: request.chain_id,
                            chain_name: None,
                            native_token: None,
                        };

                        if tx.send(response).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Token stream error: {}", e);
                        // Send error message to client
                        let error_response = StreamingResponse {
                            content: format!("Error: {}", e),
                            tokens: 0,
                            finish_reason: Some("error".to_string()),
                            chain_id: request.chain_id,
                            chain_name: None,
                            native_token: None,
                        };
                        let _ = tx.send(error_response).await;
                        break;
                    }
                }
            }

            // Log if we got no tokens
            if !got_any_tokens {
                error!("Stream completed with no tokens generated");
            }

            // Try to submit checkpoint if we have enough tokens
            // BUT DON'T CLEANUP - the session might continue!
            if let Some(jid) = job_id {
                if let Some(cm) = checkpoint_manager.as_ref() {
                    let _ = cm.force_checkpoint(jid).await;
                    // DON'T cleanup here - session continues across multiple prompts!
                    // Cleanup should only happen when websocket disconnects
                } else {
                    token_tracker.force_checkpoint(jid).await;
                    // DON'T cleanup here either
                }
            }

            // Send final message with finish reason
            let final_response = StreamingResponse {
                content: String::new(),
                tokens: 0,
                finish_reason: Some("stop".to_string()),
                chain_id: request.chain_id,
                chain_name: None,
                native_token: None,
            };
            let _ = tx.send(final_response).await;
        });

        // Record success
        if self.config.enable_circuit_breaker {
            self.circuit_breaker.record_success().await;
        }

        Ok(rx)
    }

    pub async fn get_available_models(&self) -> Result<ModelsResponse, ApiError> {
        let node_guard = self.node.read().await;
        let node = node_guard
            .as_ref()
            .ok_or_else(|| ApiError::ServiceUnavailable("no available nodes".to_string()))?;

        let capabilities = node.capabilities();
        let models = capabilities
            .into_iter()
            .map(|id| ModelInfo {
                id: id.clone(),
                name: id,
                description: None,
            })
            .collect();

        Ok(ModelsResponse {
            models,
            chain_id: None,
            chain_name: None,
        })
    }

    pub async fn health_check(&self) -> HealthResponse {
        let mut issues = Vec::new();

        // Check node availability
        let node_available = self.node.read().await.is_some();
        if !node_available {
            issues.push("No P2P node available".to_string());
        }

        // Check circuit breaker
        if self.config.enable_circuit_breaker && self.circuit_breaker.is_open().await {
            issues.push("Circuit breaker is open".to_string());
        }

        let status = if issues.is_empty() {
            "healthy"
        } else if issues.len() == 1 {
            "degraded"
        } else {
            "unhealthy"
        };

        HealthResponse {
            status: status.to_string(),
            issues: if issues.is_empty() {
                None
            } else {
                Some(issues)
            },
        }
    }

    fn create_router(server: Arc<Self>) -> Router {
        Router::new()
            .route("/health", get(health_handler))
            .route("/v1/models", get(models_handler))
            .route("/v1/inference", post(simple_inference_handler))
            .route("/v1/embed", post(embed_handler_wrapper))
            .route("/v1/ocr", post(ocr_handler_wrapper))
            .route("/v1/describe-image", post(describe_image_handler_wrapper))
            .route("/v1/ws", get(websocket_handler))
            .route("/metrics", get(metrics_handler))
            .layer(CorsLayer::permissive())
            .with_state(server)
    }
}

// Handler functions as free functions
async fn health_handler(State(server): State<Arc<ApiServer>>) -> impl IntoResponse {
    axum::response::Json(server.health_check().await)
}

async fn models_handler(State(server): State<Arc<ApiServer>>) -> impl IntoResponse {
    match server.get_available_models().await {
        Ok(models) => (StatusCode::OK, axum::response::Json(models)).into_response(),
        Err(e) => ApiServer::error_response(e),
    }
}

// Inference handler that properly uses axum extractors
async fn simple_inference_handler(
    State(server): State<Arc<ApiServer>>,
    Json(request): Json<InferenceRequest>,
) -> impl IntoResponse {
    let client_ip = "127.0.0.1".to_string();

    match server.handle_inference_request(request, client_ip).await {
        Ok(response) => (StatusCode::OK, axum::response::Json(response)).into_response(),
        Err(e) => ApiServer::error_response(e),
    }
}

async fn metrics_handler() -> impl IntoResponse {
    let metrics = "# HELP http_requests_total Total HTTP requests\n\
                  # TYPE http_requests_total counter\n\
                  http_requests_total 0\n\
                  # HELP http_request_duration_seconds Request duration\n\
                  # TYPE http_request_duration_seconds histogram\n\
                  http_request_duration_seconds_bucket{le=\"0.1\"} 0\n";

    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        metrics,
    )
}

// Embedding handler wrapper that converts ApiServer state to AppState
async fn embed_handler_wrapper(
    State(server): State<Arc<ApiServer>>,
    Json(request): Json<crate::api::EmbedRequest>,
) -> impl IntoResponse {
    use crate::blockchain::ChainRegistry;
    use crate::api::http_server::AppState;

    // Create AppState from ApiServer
    let app_state = AppState {
        api_server: server.clone(),
        chain_registry: Arc::new(ChainRegistry::new()),
        sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        chain_stats: Arc::new(RwLock::new(std::collections::HashMap::new())),
        embedding_model_manager: server.embedding_model_manager.clone(),
        vision_model_manager: server.vision_model_manager.clone(),
    };

    // Call the actual embed_handler
    match crate::api::embed_handler(axum::extract::State(app_state), Json(request)).await {
        Ok(response) => (StatusCode::OK, axum::response::Json(response.0)).into_response(),
        Err((status, message)) => (status, axum::response::Json(serde_json::json!({
            "error": message
        }))).into_response(),
    }
}

// OCR handler wrapper that converts ApiServer state to AppState
async fn ocr_handler_wrapper(
    State(server): State<Arc<ApiServer>>,
    Json(request): Json<crate::api::ocr::OcrRequest>,
) -> impl IntoResponse {
    use crate::blockchain::ChainRegistry;
    use crate::api::http_server::AppState;

    // Create AppState from ApiServer
    let app_state = AppState {
        api_server: server.clone(),
        chain_registry: Arc::new(ChainRegistry::new()),
        sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        chain_stats: Arc::new(RwLock::new(std::collections::HashMap::new())),
        embedding_model_manager: server.embedding_model_manager.clone(),
        vision_model_manager: server.vision_model_manager.clone(),
    };

    // Call the actual ocr_handler
    match crate::api::ocr_handler(axum::extract::State(app_state), Json(request)).await {
        Ok(response) => (StatusCode::OK, axum::response::Json(response.0)).into_response(),
        Err((status, message)) => (status, axum::response::Json(serde_json::json!({
            "error": message
        }))).into_response(),
    }
}

// Describe image handler wrapper that converts ApiServer state to AppState
async fn describe_image_handler_wrapper(
    State(server): State<Arc<ApiServer>>,
    Json(request): Json<crate::api::describe_image::DescribeImageRequest>,
) -> impl IntoResponse {
    use crate::blockchain::ChainRegistry;
    use crate::api::http_server::AppState;

    // Create AppState from ApiServer
    let app_state = AppState {
        api_server: server.clone(),
        chain_registry: Arc::new(ChainRegistry::new()),
        sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        chain_stats: Arc::new(RwLock::new(std::collections::HashMap::new())),
        embedding_model_manager: server.embedding_model_manager.clone(),
        vision_model_manager: server.vision_model_manager.clone(),
    };

    // Call the actual describe_image_handler
    match crate::api::describe_image_handler(axum::extract::State(app_state), Json(request)).await {
        Ok(response) => (StatusCode::OK, axum::response::Json(response.0)).into_response(),
        Err((status, message)) => (status, axum::response::Json(serde_json::json!({
            "error": message
        }))).into_response(),
    }
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(server): State<Arc<ApiServer>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_websocket(socket, server))
}

async fn handle_websocket(mut socket: WebSocket, server: Arc<ApiServer>) {
    use serde_json::json;

    // Track session information for settlement
    let mut session_id: Option<String> = None;
    let mut job_id: Option<u64> = None;
    let mut chain_id: Option<u64> = None;

    // Send connection acknowledgment
    let welcome_msg = json!({
        "type": "connected",
        "message": "WebSocket connected successfully"
    });
    if socket
        .send(axum::extract::ws::Message::Text(welcome_msg.to_string()))
        .await
        .is_err()
    {
        return;
    }

    while let Some(msg) = socket.recv().await {
        match msg {
            Ok(axum::extract::ws::Message::Text(text)) => {
                // Parse WebSocket message
                if let Ok(json_msg) = serde_json::from_str::<serde_json::Value>(&text) {
                    // Track session initialization
                    if json_msg["type"] == "session_init" {
                        // Handle session_id or sessionId
                        session_id = json_msg["session_id"]
                            .as_str()
                            .or_else(|| json_msg["sessionId"].as_str())
                            .map(String::from);

                        // Handle job_id (Rust) or jobId (SDK/contracts) as either string or number
                        job_id = json_msg["job_id"]
                            .as_u64()
                            .or_else(|| {
                                json_msg["job_id"]
                                    .as_str()
                                    .and_then(|s| s.parse::<u64>().ok())
                            })
                            .or_else(|| json_msg["jobId"].as_u64())
                            .or_else(|| {
                                json_msg["jobId"]
                                    .as_str()
                                    .and_then(|s| s.parse::<u64>().ok())
                            });

                        // Handle chain_id or chainId
                        chain_id = json_msg["chain_id"]
                            .as_u64()
                            .or_else(|| json_msg["chainId"].as_u64());

                        // DEPRECATED: Plaintext session (Phase 6.2.1, Sub-phase 5.4)
                        // SDK v6.2+ uses encryption by default. Plaintext is a fallback for clients with `encryption: false`.
                        warn!(
                            "⚠️ DEPRECATED: Plaintext session_init detected for session_id: {:?}. \
                            Encryption is strongly recommended for privacy and security. \
                            Update your SDK to use encrypted sessions or enable encryption: true in session options.",
                            session_id
                        );

                        info!("🎯 WebSocket session_init received:");
                        info!("   session_id: {:?}", session_id);
                        info!("   job_id: {:?}", job_id);
                        info!("   chain_id: {:?}", chain_id);

                        info!("📝 WebSocket session initialized - session_id: {:?}, job_id: {:?}, chain_id: {:?}",
                              session_id, job_id, chain_id);
                        info!("🔍 Raw job_id value from message: {:?}", json_msg["job_id"]);

                        // CRITICAL: Send response to session_init so SDK doesn't timeout!
                        // Must echo back the 'id' field for request-response correlation
                        let mut response = serde_json::json!({
                            "type": "session_init_ack",
                            "status": "success",
                            "session_id": session_id.clone().unwrap_or_else(|| "unknown".to_string()),
                            "job_id": job_id,
                            "chain_id": chain_id,
                            "message": "Session initialized successfully"
                        });

                        // Echo back the message ID if present (SDK uses this for request correlation)
                        if let Some(msg_id) = json_msg.get("id") {
                            response["id"] = msg_id.clone();
                        }

                        if let Err(e) = socket
                            .send(axum::extract::ws::Message::Text(response.to_string()))
                            .await
                        {
                            error!("Failed to send session_init response: {}", e);
                        } else {
                            info!("✅ Sent session_init_ack response to client");
                        }
                    }

                    // Handle encrypted session initialization (Phase 6.2.1, Sub-phase 6.2)
                    if json_msg["type"] == "encrypted_session_init" {
                        info!("🔐 Encrypted session_init received");

                        // Extract session_id and chain_id
                        session_id = json_msg["session_id"]
                            .as_str()
                            .or_else(|| json_msg["sessionId"].as_str())
                            .map(String::from);

                        chain_id = json_msg["chain_id"]
                            .as_u64()
                            .or_else(|| json_msg["chainId"].as_u64())
                            .or(Some(84532)); // Default to Base Sepolia

                        // Get node's private key from ApiServer (Phase 6.2.1, Sub-phase 6.2)
                        let node_private_key_opt = server.get_node_private_key();

                        if let Some(node_private_key) = node_private_key_opt {
                            // Node has private key - can handle encrypted sessions
                            info!(
                                "✅ Node private key available - processing encrypted session init"
                            );

                            // Parse encrypted payload (Phase 6.2.1, Sub-phase 6.3)
                            if let Some(payload_obj) = json_msg.get("payload") {
                                // Extract hex fields from payload
                                let eph_pub_hex = payload_obj["ephPubHex"].as_str();
                                let ciphertext_hex = payload_obj["ciphertextHex"].as_str();
                                let signature_hex = payload_obj["signatureHex"].as_str();
                                let nonce_hex = payload_obj["nonceHex"].as_str();
                                let aad_hex = payload_obj["aadHex"].as_str();

                                // Validate all required fields are present
                                if let (
                                    Some(eph_pub),
                                    Some(ciphertext),
                                    Some(signature),
                                    Some(nonce),
                                    Some(aad),
                                ) = (
                                    eph_pub_hex,
                                    ciphertext_hex,
                                    signature_hex,
                                    nonce_hex,
                                    aad_hex,
                                ) {
                                    // Strip "0x" prefix if present
                                    let eph_pub = eph_pub.strip_prefix("0x").unwrap_or(eph_pub);
                                    let ciphertext =
                                        ciphertext.strip_prefix("0x").unwrap_or(ciphertext);
                                    let signature =
                                        signature.strip_prefix("0x").unwrap_or(signature);
                                    let nonce = nonce.strip_prefix("0x").unwrap_or(nonce);
                                    let aad = aad.strip_prefix("0x").unwrap_or(aad);

                                    // Decode hex fields
                                    match (
                                        hex::decode(eph_pub),
                                        hex::decode(ciphertext),
                                        hex::decode(signature),
                                        hex::decode(nonce),
                                        hex::decode(aad),
                                    ) {
                                        (
                                            Ok(eph_pub_bytes),
                                            Ok(ciphertext_bytes),
                                            Ok(signature_bytes),
                                            Ok(nonce_bytes),
                                            Ok(aad_bytes),
                                        ) => {
                                            // Validate nonce size (must be 24 bytes for XChaCha20)
                                            if nonce_bytes.len() != 24 {
                                                let mut error_msg = json!({
                                                    "type": "error",
                                                    "code": "INVALID_NONCE_SIZE",
                                                    "message": format!("Invalid nonce size: expected 24 bytes, got {}", nonce_bytes.len()),
                                                    "session_id": session_id.clone().unwrap_or_else(|| "unknown".to_string())
                                                });

                                                if let Some(msg_id) = json_msg.get("id") {
                                                    error_msg["id"] = msg_id.clone();
                                                }

                                                let _ = socket
                                                    .send(axum::extract::ws::Message::Text(
                                                        error_msg.to_string(),
                                                    ))
                                                    .await;
                                                continue;
                                            }

                                            // Build EncryptedSessionPayload for decryption
                                            let encrypted_payload =
                                                crate::crypto::EncryptedSessionPayload {
                                                    eph_pub: eph_pub_bytes,
                                                    ciphertext: ciphertext_bytes,
                                                    signature: signature_bytes,
                                                    nonce: nonce_bytes,
                                                    aad: aad_bytes,
                                                };

                                            // Decrypt session init payload
                                            match crate::crypto::decrypt_session_init(
                                                &encrypted_payload,
                                                &node_private_key,
                                            ) {
                                                Ok(session_init_data) => {
                                                    info!("✅ Successfully decrypted session init payload");

                                                    // Extract session data
                                                    let extracted_session_key =
                                                        session_init_data.session_key;
                                                    let extracted_job_id_str =
                                                        session_init_data.job_id;
                                                    let model_name = session_init_data.model_name;
                                                    let price_per_token =
                                                        session_init_data.price_per_token;
                                                    let client_address =
                                                        session_init_data.client_address;

                                                    // Update tracked session/job info - parse job_id from string
                                                    job_id =
                                                        extracted_job_id_str.parse::<u64>().ok();

                                                    info!("🔐 Session init data:");
                                                    info!(
                                                        "   job_id: {} (parsed to {:?})",
                                                        extracted_job_id_str, job_id
                                                    );
                                                    info!("   model_name: {}", model_name);
                                                    info!(
                                                        "   price_per_token: {}",
                                                        price_per_token
                                                    );
                                                    info!("   client_address: {}", client_address);

                                                    // Store session key in SessionKeyStore
                                                    if let Some(sid) = &session_id {
                                                        server
                                                            .session_key_store
                                                            .store_key(
                                                                sid.clone(),
                                                                extracted_session_key,
                                                            )
                                                            .await;

                                                        info!("✅ Session key stored for session_id: {}", sid);

                                                        // Handle vector_database if provided (Sub-phase 3.3)
                                                        if let Some(vdb_info) = session_init_data.vector_database.clone() {
                                                            info!(
                                                                "📦 Vector database requested: {}",
                                                                vdb_info.manifest_path
                                                            );

                                                            // Get session from store and update it
                                                            let mut store = server.session_store.write().await;
                                                            if let Some(mut session) = store.get_session_mut(sid).await {
                                                                // Store encryption key in session
                                                                session.encryption_key = Some(extracted_session_key.to_vec());

                                                                // Set vector_database info
                                                                session.set_vector_database(Some(vdb_info.clone()));

                                                                // Set status to Loading
                                                                session.set_vector_loading_status(
                                                                    crate::api::websocket::session::VectorLoadingStatus::Loading
                                                                );

                                                                // Get cancel_token for background task
                                                                let cancel_token = session.cancel_token.clone();

                                                                info!("🚀 Spawning async vector loading task for session: {}", sid);

                                                                // Spawn background task
                                                                let sid_clone = sid.clone();
                                                                let session_store_clone = server.session_store.clone();
                                                                let encryption_key_clone = Some(extracted_session_key.to_vec());

                                                                tokio::spawn(async move {
                                                                    crate::api::websocket::vector_loading::load_vectors_async(
                                                                        sid_clone,
                                                                        vdb_info,
                                                                        session_store_clone,
                                                                        cancel_token,
                                                                        encryption_key_clone,
                                                                    ).await;
                                                                });
                                                            } else {
                                                                warn!("⚠️ Session not found in store: {}", sid);
                                                            }
                                                        }
                                                    } else {
                                                        warn!("⚠️ No session_id provided - session key not stored");
                                                    }

                                                    // Send session_init_ack response
                                                    let mut response = json!({
                                                        "type": "session_init_ack",
                                                        "status": "success",
                                                        "session_id": session_id.clone().unwrap_or_else(|| "unknown".to_string()),
                                                        "job_id": job_id,
                                                        "chain_id": chain_id,
                                                        "client_address": client_address,
                                                        "message": "Encrypted session initialized successfully"
                                                    });

                                                    if let Some(msg_id) = json_msg.get("id") {
                                                        response["id"] = msg_id.clone();
                                                    }

                                                    if let Err(e) = socket
                                                        .send(axum::extract::ws::Message::Text(
                                                            response.to_string(),
                                                        ))
                                                        .await
                                                    {
                                                        error!("Failed to send encrypted session_init_ack: {}", e);
                                                    } else {
                                                        info!("✅ Sent encrypted session_init_ack to client");
                                                    }
                                                }
                                                Err(e) => {
                                                    error!("Failed to decrypt session init: {}", e);
                                                    let mut error_msg = json!({
                                                        "type": "error",
                                                        "code": "DECRYPTION_FAILED",
                                                        "message": format!("Failed to decrypt session init payload: {}", e),
                                                        "session_id": session_id.clone().unwrap_or_else(|| "unknown".to_string())
                                                    });

                                                    if let Some(msg_id) = json_msg.get("id") {
                                                        error_msg["id"] = msg_id.clone();
                                                    }

                                                    let _ = socket
                                                        .send(axum::extract::ws::Message::Text(
                                                            error_msg.to_string(),
                                                        ))
                                                        .await;
                                                }
                                            }
                                        }
                                        _ => {
                                            let mut error_msg = json!({
                                                "type": "error",
                                                "code": "INVALID_HEX_ENCODING",
                                                "message": "Failed to decode hex fields in encrypted session init payload",
                                                "session_id": session_id.clone().unwrap_or_else(|| "unknown".to_string())
                                            });

                                            if let Some(msg_id) = json_msg.get("id") {
                                                error_msg["id"] = msg_id.clone();
                                            }

                                            let _ = socket
                                                .send(axum::extract::ws::Message::Text(
                                                    error_msg.to_string(),
                                                ))
                                                .await;
                                        }
                                    }
                                } else {
                                    let mut error_msg = json!({
                                        "type": "error",
                                        "code": "INVALID_PAYLOAD",
                                        "message": "Missing required fields in encrypted session init payload (ephPubHex, ciphertextHex, signatureHex, nonceHex, aadHex)",
                                        "session_id": session_id.clone().unwrap_or_else(|| "unknown".to_string())
                                    });

                                    if let Some(msg_id) = json_msg.get("id") {
                                        error_msg["id"] = msg_id.clone();
                                    }

                                    let _ = socket
                                        .send(axum::extract::ws::Message::Text(
                                            error_msg.to_string(),
                                        ))
                                        .await;
                                }
                            } else {
                                let mut error_msg = json!({
                                    "type": "error",
                                    "code": "MISSING_PAYLOAD",
                                    "message": "encrypted_session_init must include payload object",
                                    "session_id": session_id.clone().unwrap_or_else(|| "unknown".to_string())
                                });

                                if let Some(msg_id) = json_msg.get("id") {
                                    error_msg["id"] = msg_id.clone();
                                }

                                let _ = socket
                                    .send(axum::extract::ws::Message::Text(error_msg.to_string()))
                                    .await;
                            }
                        } else {
                            // No private key - node operates in plaintext-only mode
                            warn!("⚠️ Encrypted session init requested but node private key not configured");
                            warn!(
                                "   Set HOST_PRIVATE_KEY environment variable to enable encryption"
                            );

                            // Send error response directing client to use plaintext
                            let mut response = json!({
                                "type": "error",
                                "code": "ENCRYPTION_NOT_SUPPORTED",
                                "message": "Node does not have encryption key configured. Please use plaintext session_init or configure HOST_PRIVATE_KEY.",
                                "session_id": session_id.clone().unwrap_or_else(|| "unknown".to_string())
                            });

                            if let Some(msg_id) = json_msg.get("id") {
                                response["id"] = msg_id.clone();
                            }

                            if let Err(e) = socket
                                .send(axum::extract::ws::Message::Text(response.to_string()))
                                .await
                            {
                                error!(
                                    "Failed to send encrypted_session_init error response: {}",
                                    e
                                );
                            }
                        }
                    }

                    // Handle encrypted messages (Phase 6.2.1, Sub-phase 5.2)
                    if json_msg["type"] == "encrypted_message" {
                        info!("🔐 Encrypted message received");

                        // Extract session_id
                        let current_session_id = json_msg["session_id"]
                            .as_str()
                            .or_else(|| json_msg["sessionId"].as_str())
                            .map(String::from)
                            .or(session_id.clone());

                        if let Some(sid) = &current_session_id {
                            // Try to retrieve session key from store
                            let session_key_result = server.session_key_store.get_key(sid).await;

                            if let Some(session_key) = session_key_result {
                                // Parse encrypted payload
                                if let Some(payload_obj) = json_msg.get("payload") {
                                    let ciphertext_hex = payload_obj["ciphertextHex"].as_str();
                                    let nonce_hex = payload_obj["nonceHex"].as_str();
                                    let aad_hex = payload_obj["aadHex"].as_str();

                                    if let (Some(ct_hex), Some(n_hex), Some(a_hex)) =
                                        (ciphertext_hex, nonce_hex, aad_hex)
                                    {
                                        // Strip "0x" prefix if present
                                        let ct_hex = ct_hex.strip_prefix("0x").unwrap_or(ct_hex);
                                        let n_hex = n_hex.strip_prefix("0x").unwrap_or(n_hex);
                                        let a_hex = a_hex.strip_prefix("0x").unwrap_or(a_hex);

                                        // Decode hex fields
                                        match (
                                            hex::decode(ct_hex),
                                            hex::decode(n_hex),
                                            hex::decode(a_hex),
                                        ) {
                                            (Ok(ciphertext), Ok(nonce_bytes), Ok(aad_bytes)) => {
                                                // Validate nonce size
                                                if nonce_bytes.len() != 24 {
                                                    let mut error_msg = json!({
                                                        "type": "error",
                                                        "code": "INVALID_NONCE_SIZE",
                                                        "message": format!(
                                                            "Invalid nonce size: expected 24 bytes, got {}",
                                                            nonce_bytes.len()
                                                        )
                                                    });

                                                    if let Some(msg_id) = json_msg.get("id") {
                                                        error_msg["id"] = msg_id.clone();
                                                    }

                                                    let _ = socket
                                                        .send(axum::extract::ws::Message::Text(
                                                            error_msg.to_string(),
                                                        ))
                                                        .await;
                                                    continue;
                                                }

                                                // Convert nonce to array
                                                let mut nonce = [0u8; 24];
                                                nonce.copy_from_slice(&nonce_bytes);

                                                // Decrypt message
                                                match crate::crypto::decrypt_with_aead(
                                                    &ciphertext,
                                                    &nonce,
                                                    &aad_bytes,
                                                    &session_key,
                                                ) {
                                                    Ok(plaintext_bytes) => {
                                                        // Convert plaintext to string
                                                        match String::from_utf8(plaintext_bytes) {
                                                            Ok(plaintext_str) => {
                                                                info!(
                                                                    "✅ Decrypted message: {}",
                                                                    plaintext_str
                                                                );

                                                                // Try to parse decrypted content as JSON (SDK v6.2+)
                                                                // Falls back to treating it as plain prompt string
                                                                let decrypted_json: serde_json::Value =
                                                                    serde_json::from_str(&plaintext_str)
                                                                        .unwrap_or_else(|_| {
                                                                            // If not JSON, treat as plain prompt
                                                                            json!({"prompt": plaintext_str})
                                                                        });

                                                                // Extract prompt from decrypted JSON or use entire string
                                                                let plaintext_prompt = decrypted_json
                                                                    .get("prompt")
                                                                    .and_then(|v| v.as_str())
                                                                    .unwrap_or(&plaintext_str)
                                                                    .to_string();

                                                                // Extract model (priority: decrypted > outer message > default)
                                                                let model = decrypted_json
                                                                    .get("model")
                                                                    .and_then(|v| v.as_str())
                                                                    .or_else(|| json_msg.get("model").and_then(|v| v.as_str()))
                                                                    .unwrap_or("tiny-vicuna")
                                                                    .to_string();

                                                                // Extract max_tokens (priority: decrypted > outer message > default)
                                                                let max_tokens = decrypted_json
                                                                    .get("max_tokens")
                                                                    .and_then(|v| v.as_u64())
                                                                    .or_else(|| json_msg.get("max_tokens").and_then(|v| v.as_u64()))
                                                                    .unwrap_or(4000);  // Increased default to 4000

                                                                // Extract temperature (priority: decrypted > outer message > default)
                                                                let temperature = decrypted_json
                                                                    .get("temperature")
                                                                    .and_then(|v| v.as_f64())
                                                                    .or_else(|| json_msg.get("temperature").and_then(|v| v.as_f64()))
                                                                    .unwrap_or(0.7);

                                                                // Extract stream (priority: decrypted > outer message > default)
                                                                let stream = decrypted_json
                                                                    .get("stream")
                                                                    .and_then(|v| v.as_bool())
                                                                    .or_else(|| json_msg.get("stream").and_then(|v| v.as_bool()))
                                                                    .unwrap_or(true);

                                                                let request_value = json!({
                                                                    "model": model,
                                                                    "prompt": plaintext_prompt,
                                                                    "job_id": job_id,
                                                                    "session_id": current_session_id,
                                                                    "max_tokens": max_tokens,
                                                                    "temperature": temperature,
                                                                    "stream": stream
                                                                });

                                                                // Extract message ID for response correlation
                                                                let message_id =
                                                                    json_msg.get("id").cloned();

                                                                if let Ok(request) =
                                                                    serde_json::from_value::<
                                                                        InferenceRequest,
                                                                    >(
                                                                        request_value
                                                                    )
                                                                {
                                                                    info!(
                                                                        "📋 Processing encrypted inference request for job_id: {:?}",
                                                                        request.job_id
                                                                    );

                                                                    // Handle streaming inference (same as plaintext)
                                                                    match server
                                                                        .handle_streaming_request(
                                                                            request,
                                                                            "ws-client".to_string(),
                                                                        )
                                                                        .await
                                                                    {
                                                                        Ok(mut receiver) => {
                                                                            let mut total_tokens =
                                                                                0u64;

                                                                            let mut chunk_index =
                                                                                0u32;

                                                                            while let Some(
                                                                                response,
                                                                            ) = receiver
                                                                                .recv()
                                                                                .await
                                                                            {
                                                                                // Count tokens for logging only - producer already tracks for checkpoints
                                                                                if response.tokens > 0 {
                                                                                    total_tokens += response.tokens as u64;
                                                                                }

                                                                                // Encrypt response chunks with session key
                                                                                // Generate random 24-byte nonce using CSPRNG
                                                                                let mut nonce =
                                                                                    [0u8; 24];
                                                                                use rand::RngCore;
                                                                                rand::thread_rng()
                                                                                    .fill_bytes(
                                                                                        &mut nonce,
                                                                                    );

                                                                                // Prepare AAD with chunk index for ordering validation
                                                                                let aad = format!(
                                                                                    "chunk_{}",
                                                                                    chunk_index
                                                                                );
                                                                                let aad_bytes =
                                                                                    aad.as_bytes();

                                                                                // Encrypt the response content
                                                                                match crate::crypto::encrypt_with_aead(
                                                                                    response.content.as_bytes(),
                                                                                    &nonce,
                                                                                    aad_bytes,
                                                                                    &session_key,
                                                                                ) {
                                                                                    Ok(ciphertext) => {
                                                                                        // Build encrypted_chunk message
                                                                                        let mut ws_msg = json!({
                                                                                            "type": "encrypted_chunk",
                                                                                            "tokens": response.tokens,
                                                                                            "payload": {
                                                                                                "ciphertextHex": hex::encode(&ciphertext),
                                                                                                "nonceHex": hex::encode(&nonce),
                                                                                                "aadHex": hex::encode(aad_bytes),
                                                                                                "index": chunk_index
                                                                                            }
                                                                                        });

                                                                                        // Include message ID for correlation
                                                                                        if let Some(ref msg_id) = message_id {
                                                                                            ws_msg["id"] = msg_id.clone();
                                                                                        }

                                                                                        // Include session_id
                                                                                        if let Some(ref sid) = current_session_id {
                                                                                            ws_msg["session_id"] = json!(sid);
                                                                                        }

                                                                                        // CRITICAL: Add "final": true to last chunk for mobile browser compatibility
                                                                                        // Mobile browsers buffer small WebSocket messages (<8KB) and may not flush
                                                                                        // the tiny encrypted_response/stream_end messages
                                                                                        if response.finish_reason.is_some() {
                                                                                            ws_msg["final"] = json!(true);
                                                                                        }

                                                                                        // Send encrypted chunk
                                                                                        match socket
                                                                                            .send(
                                                                                                axum::extract::ws::Message::Text(
                                                                                                    ws_msg.to_string(),
                                                                                                ),
                                                                                            )
                                                                                            .await
                                                                                        {
                                                                                            Ok(_) => {
                                                                                                if response.finish_reason.is_some() {
                                                                                                    info!("✅ Sent encrypted_chunk {} (tokens: {}, final: true)", chunk_index, response.tokens);
                                                                                                } else {
                                                                                                    info!("✅ Sent encrypted_chunk {} (tokens: {})", chunk_index, response.tokens);
                                                                                                }
                                                                                            }
                                                                                            Err(e) => {
                                                                                                error!("❌ Failed to send encrypted_chunk {}: {}", chunk_index, e);
                                                                                                break;
                                                                                            }
                                                                                        }

                                                                                        chunk_index += 1;

                                                                                        // Handle streaming completion
                                                                                        if response.finish_reason.is_some() {
                                                                                            // Send final encrypted_response message
                                                                                            // Generate new nonce for final message
                                                                                            let mut final_nonce = [0u8; 24];
                                                                                            rand::thread_rng().fill_bytes(&mut final_nonce);

                                                                                            // AAD for final message
                                                                                            let final_aad = b"encrypted_response_final";

                                                                                            // Encrypt finish_reason
                                                                                            let finish_reason_str = response.finish_reason.as_ref().unwrap();
                                                                                            match crate::crypto::encrypt_with_aead(
                                                                                                finish_reason_str.as_bytes(),
                                                                                                &final_nonce,
                                                                                                final_aad,
                                                                                                &session_key,
                                                                                            ) {
                                                                                                Ok(final_ciphertext) => {
                                                                                                    let mut end_msg = json!({
                                                                                                        "type": "encrypted_response",
                                                                                                        "payload": {
                                                                                                            "ciphertextHex": hex::encode(&final_ciphertext),
                                                                                                            "nonceHex": hex::encode(&final_nonce),
                                                                                                            "aadHex": hex::encode(final_aad),
                                                                                                        }
                                                                                                    });

                                                                                                    // Include message ID
                                                                                                    if let Some(ref msg_id) = message_id {
                                                                                                        end_msg["id"] = msg_id.clone();
                                                                                                    }

                                                                                                    // Include session_id
                                                                                                    if let Some(ref sid) = current_session_id {
                                                                                                        end_msg["session_id"] = json!(sid);
                                                                                                    }

                                                                                                    // Send final encrypted_response
                                                                                                    match socket
                                                                                                        .send(
                                                                                                            axum::extract::ws::Message::Text(
                                                                                                                end_msg.to_string(),
                                                                                                            ),
                                                                                                        )
                                                                                                        .await
                                                                                                    {
                                                                                                        Ok(_) => {
                                                                                                            info!("🏁 Sent final encrypted_response (finish_reason: {})", finish_reason_str);

                                                                                                            // CRITICAL: Also send stream_end for SDK compatibility
                                                                                                            let mut stream_end_msg = json!({"type": "stream_end"});
                                                                                                            if let Some(ref msg_id) = message_id {
                                                                                                                stream_end_msg["id"] = msg_id.clone();
                                                                                                            }
                                                                                                            if let Some(ref sid) = current_session_id {
                                                                                                                stream_end_msg["session_id"] = json!(sid);
                                                                                                            }
                                                                                                            let _ = socket.send(axum::extract::ws::Message::Text(stream_end_msg.to_string())).await;
                                                                                                            info!("🏁 Sent stream_end for SDK compatibility");
                                                                                                        }
                                                                                                        Err(e) => {
                                                                                                            error!("❌ Failed to send final encrypted_response: {}", e);
                                                                                                        }
                                                                                                    }
                                                                                                }
                                                                                                Err(e) => {
                                                                                                    error!("Failed to encrypt final response: {}", e);
                                                                                                }
                                                                                            }
                                                                                            break;
                                                                                        }
                                                                                    }
                                                                                    Err(e) => {
                                                                                        error!("Failed to encrypt response chunk: {}", e);
                                                                                        // Send error message
                                                                                        let mut error_msg = json!({
                                                                                            "type": "error",
                                                                                            "code": "ENCRYPTION_FAILED",
                                                                                            "message": format!("Failed to encrypt response: {}", e)
                                                                                        });

                                                                                        if let Some(ref msg_id) = message_id {
                                                                                            error_msg["id"] = msg_id.clone();
                                                                                        }

                                                                                        let _ = socket
                                                                                            .send(axum::extract::ws::Message::Text(
                                                                                                error_msg.to_string(),
                                                                                            ))
                                                                                            .await;
                                                                                        // Send stream_end after error
                                                                                        let mut stream_end_msg = json!({"type": "stream_end"});
                                                                                        if let Some(ref msg_id) = message_id {
                                                                                            stream_end_msg["id"] = msg_id.clone();
                                                                                        }
                                                                                        let _ = socket.send(axum::extract::ws::Message::Text(stream_end_msg.to_string())).await;
                                                                                        break;
                                                                                    }
                                                                                }
                                                                            }

                                                                            info!(
                                                                                "📊 Encrypted session complete - Total tokens: {}",
                                                                                total_tokens
                                                                            );
                                                                        }
                                                                        Err(e) => {
                                                                            let mut error_msg = json!({
                                                                                "type": "error",
                                                                                "error": e.to_string()
                                                                            });

                                                                            if let Some(
                                                                                ref msg_id,
                                                                            ) = message_id
                                                                            {
                                                                                error_msg["id"] =
                                                                                    msg_id.clone();
                                                                            }

                                                                            let _ = socket
                                                                                .send(
                                                                                    axum::extract::ws::Message::Text(
                                                                                        error_msg.to_string(),
                                                                                    ),
                                                                                )
                                                                                .await;

                                                                            // CRITICAL: Send stream_end even on error so SDK knows stream is done
                                                                            let mut stream_end_msg = json!({"type": "stream_end"});
                                                                            if let Some(ref msg_id) = message_id {
                                                                                stream_end_msg["id"] = msg_id.clone();
                                                                            }
                                                                            let _ = socket.send(axum::extract::ws::Message::Text(stream_end_msg.to_string())).await;
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                            Err(_) => {
                                                                let mut error_msg = json!({
                                                                    "type": "error",
                                                                    "code": "INVALID_UTF8",
                                                                    "message": "Decrypted plaintext is not valid UTF-8"
                                                                });

                                                                if let Some(msg_id) =
                                                                    json_msg.get("id")
                                                                {
                                                                    error_msg["id"] =
                                                                        msg_id.clone();
                                                                }

                                                                let _ = socket
                                                                    .send(
                                                                        axum::extract::ws::Message::Text(
                                                                            error_msg.to_string(),
                                                                        ),
                                                                    )
                                                                    .await;
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        let mut error_msg = json!({
                                                            "type": "error",
                                                            "code": "DECRYPTION_FAILED",
                                                            "message": format!("Failed to decrypt message: {}", e)
                                                        });

                                                        if let Some(msg_id) = json_msg.get("id") {
                                                            error_msg["id"] = msg_id.clone();
                                                        }

                                                        let _ = socket
                                                            .send(axum::extract::ws::Message::Text(
                                                                error_msg.to_string(),
                                                            ))
                                                            .await;
                                                    }
                                                }
                                            }
                                            _ => {
                                                let mut error_msg = json!({
                                                    "type": "error",
                                                    "code": "INVALID_HEX_ENCODING",
                                                    "message": "Failed to decode hex fields in payload"
                                                });

                                                if let Some(msg_id) = json_msg.get("id") {
                                                    error_msg["id"] = msg_id.clone();
                                                }

                                                let _ = socket
                                                    .send(axum::extract::ws::Message::Text(
                                                        error_msg.to_string(),
                                                    ))
                                                    .await;
                                            }
                                        }
                                    } else {
                                        let mut error_msg = json!({
                                            "type": "error",
                                            "code": "MISSING_PAYLOAD_FIELDS",
                                            "message": "Payload must contain ciphertextHex, nonceHex, and aadHex"
                                        });

                                        if let Some(msg_id) = json_msg.get("id") {
                                            error_msg["id"] = msg_id.clone();
                                        }

                                        let _ = socket
                                            .send(axum::extract::ws::Message::Text(
                                                error_msg.to_string(),
                                            ))
                                            .await;
                                    }
                                } else {
                                    let mut error_msg = json!({
                                        "type": "error",
                                        "code": "MISSING_PAYLOAD",
                                        "message": "encrypted_message must include payload object"
                                    });

                                    if let Some(msg_id) = json_msg.get("id") {
                                        error_msg["id"] = msg_id.clone();
                                    }

                                    let _ = socket
                                        .send(axum::extract::ws::Message::Text(
                                            error_msg.to_string(),
                                        ))
                                        .await;
                                }
                            } else {
                                let mut error_msg = json!({
                                    "type": "error",
                                    "code": "SESSION_KEY_NOT_FOUND",
                                    "message": format!("No session key found for session_id: {}", sid)
                                });

                                if let Some(msg_id) = json_msg.get("id") {
                                    error_msg["id"] = msg_id.clone();
                                }

                                let _ = socket
                                    .send(axum::extract::ws::Message::Text(error_msg.to_string()))
                                    .await;
                            }
                        } else {
                            let mut error_msg = json!({
                                "type": "error",
                                "code": "MISSING_SESSION_ID",
                                "message": "encrypted_message requires session_id"
                            });

                            if let Some(msg_id) = json_msg.get("id") {
                                error_msg["id"] = msg_id.clone();
                            }

                            let _ = socket
                                .send(axum::extract::ws::Message::Text(error_msg.to_string()))
                                .await;
                        }
                    }

                    // Handle both "prompt" and "inference" messages
                    if json_msg["type"] == "prompt" || json_msg["type"] == "inference" {
                        // DEPRECATED: Plaintext prompt/inference (Phase 6.2.1, Sub-phase 5.4)
                        // SDK v6.2+ uses encryption by default. Plaintext is a fallback for clients with `encryption: false`.
                        warn!(
                            "⚠️ DEPRECATED: Plaintext {} message detected for session_id: {:?}. \
                            Encryption is strongly recommended for privacy and security. \
                            Update your SDK to use encrypted_message or enable encryption: true in session options.",
                            json_msg["type"], session_id
                        );

                        // Extract message ID for response correlation
                        let message_id = json_msg.get("id").cloned();

                        // Extract job_id from messages if not already set
                        if job_id.is_none() {
                            // Try to get job_id (Rust) or jobId (SDK/contracts)
                            job_id = json_msg["job_id"]
                                .as_u64()
                                .or_else(|| {
                                    json_msg["job_id"]
                                        .as_str()
                                        .and_then(|s| s.parse::<u64>().ok())
                                })
                                .or_else(|| json_msg["jobId"].as_u64())
                                .or_else(|| {
                                    json_msg["jobId"]
                                        .as_str()
                                        .and_then(|s| s.parse::<u64>().ok())
                                });

                            if job_id.is_some() {
                                info!(
                                    "📋 Got job_id from {} message: {:?}",
                                    json_msg["type"], job_id
                                );
                            }
                        }

                        // Log the message for debugging
                        info!(
                            "💬 {} message received with job_id: {:?}, message_id: {:?}",
                            json_msg["type"], job_id, message_id
                        );

                        // Build InferenceRequest from either prompt or inference message
                        let request_value = if json_msg["type"] == "prompt" {
                            // For prompt messages, use the nested request object if available
                            if json_msg.get("request").is_some() {
                                // SDK sends a nested request object with all parameters
                                let mut req = json_msg["request"].clone();
                                // Add job_id and session_id to the request
                                if let Some(obj) = req.as_object_mut() {
                                    obj.insert("job_id".to_string(), json!(job_id));
                                    obj.insert("session_id".to_string(), json!(session_id));
                                }
                                req
                            } else {
                                // Fallback: build request from message fields
                                json!({
                                    "model": json_msg["model"].as_str().unwrap_or("tiny-vicuna"),
                                    "prompt": json_msg["prompt"].as_str().unwrap_or(""),
                                    "job_id": job_id,
                                    "session_id": session_id.clone(),
                                    "max_tokens": json_msg["max_tokens"].as_u64().unwrap_or(4000),
                                    "temperature": json_msg["temperature"].as_f64().unwrap_or(0.7),
                                    "stream": json_msg["stream"].as_bool().unwrap_or(true)
                                })
                            }
                        } else {
                            // For inference messages, use the nested request object
                            json_msg["request"].clone()
                        };

                        // Debug: Log the entire request
                        info!(
                            "🔍 WebSocket inference request received: {:?}",
                            request_value
                        );

                        if let Ok(request) =
                            serde_json::from_value::<InferenceRequest>(request_value)
                        {
                            // Log job_id for payment tracking visibility
                            if let Some(req_job_id) = request.job_id {
                                info!(
                                    "📋 Processing inference request for blockchain job_id: {}",
                                    req_job_id
                                );
                                // Update tracked job_id if not already set
                                if job_id.is_none() {
                                    job_id = Some(req_job_id);
                                }
                            } else {
                                info!("⚠️  No job_id in WebSocket request");
                            }

                            // Handle streaming inference
                            match server
                                .handle_streaming_request(request, "ws-client".to_string())
                                .await
                            {
                                Ok(mut receiver) => {
                                    let mut total_tokens = 0u64;

                                    while let Some(response) = receiver.recv().await {
                                        // Count tokens for logging - producer already tracks for checkpoints
                                        if response.tokens > 0 {
                                            total_tokens += response.tokens as u64;
                                        }

                                        let mut ws_msg = json!({
                                            "type": "stream_chunk",
                                            "content": response.content,
                                            "tokens": response.tokens,
                                        });

                                        // Include message ID if present for correlation
                                        if let Some(ref msg_id) = message_id {
                                            ws_msg["id"] = msg_id.clone();
                                        }

                                        if socket
                                            .send(axum::extract::ws::Message::Text(
                                                ws_msg.to_string(),
                                            ))
                                            .await
                                            .is_err()
                                        {
                                            break;
                                        }

                                        if response.finish_reason.is_some() {
                                            let mut end_msg = json!({"type": "stream_end"});

                                            // Include message ID in end message too
                                            if let Some(ref msg_id) = message_id {
                                                end_msg["id"] = msg_id.clone();
                                            }

                                            let _ = socket
                                                .send(axum::extract::ws::Message::Text(
                                                    end_msg.to_string(),
                                                ))
                                                .await;
                                            break;
                                        }
                                    }

                                    // Log total tokens tracked for this session
                                    if total_tokens > 0 {
                                        info!("📊 WebSocket session complete - Total tokens tracked for job {:?}: {}",
                                              job_id, total_tokens);
                                    }
                                }
                                Err(e) => {
                                    let mut error_msg = json!({
                                        "type": "error",
                                        "error": e.to_string()
                                    });

                                    // Include message ID in error message
                                    if let Some(ref msg_id) = message_id {
                                        error_msg["id"] = msg_id.clone();
                                    }

                                    let _ = socket
                                        .send(axum::extract::ws::Message::Text(
                                            error_msg.to_string(),
                                        ))
                                        .await;

                                    // CRITICAL: Send stream_end even on error so SDK knows stream is done
                                    let mut stream_end_msg = json!({"type": "stream_end"});
                                    if let Some(ref msg_id) = message_id {
                                        stream_end_msg["id"] = msg_id.clone();
                                    }
                                    let _ = socket.send(axum::extract::ws::Message::Text(stream_end_msg.to_string())).await;
                                }
                            }
                        }
                    }

                    // Handle RAG uploadVectors message (Phase 3.4)
                    if json_msg["type"] == "uploadVectors" {
                        info!("📤 uploadVectors message received, WS session_id={:?}", session_id);

                        match serde_json::from_value::<crate::api::websocket::message_types::UploadVectorsRequest>(json_msg.clone()) {
                            Ok(request) => {
                                // Get or create session with RAG enabled
                                let sid = session_id.clone().unwrap_or_else(|| "default-rag-session".to_string());
                                info!("📤 uploadVectors using session: {} (from WS session_id={:?})", sid, session_id);

                                // Use the new helper method
                                let rag_session = {
                                    let mut store = server.session_store.write().await;
                                    match store.get_or_create_rag_session(sid.clone(), 100_000).await {
                                        Ok(sess) => {
                                            info!("✅ Session ready with RAG enabled: {}", sid);
                                            sess
                                        }
                                        Err(e) => {
                                            error!("Failed to create RAG session: {}", e);
                                            let error_msg = json!({
                                                "type": "error",
                                                "error": format!("Failed to create RAG session: {}", e)
                                            });
                                            if socket.send(axum::extract::ws::Message::Text(error_msg.to_string())).await.is_err() {
                                                break;
                                            }
                                            continue;
                                        }
                                    }
                                };

                                let rag_session_arc = Arc::new(std::sync::Mutex::new(rag_session));

                                // Call the RAG handler
                                match crate::api::websocket::handlers::rag::handle_upload_vectors(&rag_session_arc, request) {
                                    Ok(response) => {
                                        match serde_json::to_string(&response) {
                                            Ok(response_json) => {
                                                info!("✅ uploadVectors response: {} uploaded, {} rejected",
                                                      response.uploaded, response.rejected);
                                                if socket.send(axum::extract::ws::Message::Text(response_json)).await.is_err() {
                                                    error!("Failed to send uploadVectors response");
                                                    break;
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to serialize uploadVectors response: {}", e);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("uploadVectors handler error: {}", e);
                                        let error_msg = json!({
                                            "type": "error",
                                            "error": format!("Upload vectors failed: {}", e)
                                        });
                                        if socket.send(axum::extract::ws::Message::Text(error_msg.to_string())).await.is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Invalid uploadVectors request: {}", e);
                                let error_msg = json!({
                                    "type": "error",
                                    "error": format!("Invalid uploadVectors request: {}", e)
                                });
                                if socket.send(axum::extract::ws::Message::Text(error_msg.to_string())).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }

                    // Handle RAG searchVectors message (Phase 3.4)
                    if json_msg["type"] == "searchVectors" {
                        info!("🔍 searchVectors message received, WS session_id={:?}", session_id);

                        match serde_json::from_value::<crate::api::websocket::message_types::SearchVectorsRequest>(json_msg.clone()) {
                            Ok(request) => {
                                // Get existing session with RAG (should already exist from uploadVectors)
                                let sid = session_id.clone().unwrap_or_else(|| "default-rag-session".to_string());
                                info!("🔍 searchVectors using session: {} (from WS session_id={:?})", sid, session_id);

                                // Get session from store
                                let rag_session = {
                                    let store = server.session_store.read().await;
                                    match store.get_session(&sid).await {
                                        Some(sess) => {
                                            info!("✅ Found session for search: {}", sid);
                                            if sess.get_vector_store().is_none() {
                                                warn!("⚠️  Session {} exists but RAG not enabled!", sid);
                                                let error_msg = json!({
                                                    "type": "error",
                                                    "error": format!("Session {} found but RAG not enabled. Upload vectors first.", sid)
                                                });
                                                if socket.send(axum::extract::ws::Message::Text(error_msg.to_string())).await.is_err() {
                                                    break;
                                                }
                                                continue;
                                            }
                                            sess
                                        }
                                        None => {
                                            error!("❌ Session {} not found for search!", sid);
                                            let error_msg = json!({
                                                "type": "error",
                                                "error": format!("Session {} not found. Upload vectors first.", sid)
                                            });
                                            if socket.send(axum::extract::ws::Message::Text(error_msg.to_string())).await.is_err() {
                                                break;
                                            }
                                            continue;
                                        }
                                    }
                                };

                                let rag_session_arc = Arc::new(std::sync::Mutex::new(rag_session));

                                // Call the RAG handler
                                match crate::api::websocket::handlers::rag::handle_search_vectors(&rag_session_arc, request) {
                                    Ok(response) => {
                                        match serde_json::to_string(&response) {
                                            Ok(response_json) => {
                                                info!("✅ searchVectors response: {} results in {:.2}ms",
                                                      response.results.len(), response.search_time_ms);
                                                if socket.send(axum::extract::ws::Message::Text(response_json)).await.is_err() {
                                                    error!("Failed to send searchVectors response");
                                                    break;
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to serialize searchVectors response: {}", e);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("searchVectors handler error: {}", e);
                                        let error_msg = json!({
                                            "type": "error",
                                            "error": format!("Search vectors failed: {}", e)
                                        });
                                        if socket.send(axum::extract::ws::Message::Text(error_msg.to_string())).await.is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Invalid searchVectors request: {}", e);
                                let error_msg = json!({
                                    "type": "error",
                                    "error": format!("Invalid searchVectors request: {}", e)
                                });
                                if socket.send(axum::extract::ws::Message::Text(error_msg.to_string())).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            Ok(axum::extract::ws::Message::Ping(data)) => {
                if socket
                    .send(axum::extract::ws::Message::Pong(data))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Ok(axum::extract::ws::Message::Close(frame)) => {
                info!("📴 WebSocket closed by client - Close frame: {:?}", frame);
                info!(
                    "🔍 Current tracked job_id: {:?}, session_id: {:?}",
                    job_id, session_id
                );
                break;
            }
            Err(e) => {
                info!(
                    "⚠️ WebSocket error: {} - job_id: {:?}, session_id: {:?}",
                    e, job_id, session_id
                );
                break;
            }
            _ => {}
        }
    }

    // CRITICAL FIX: Trigger settlement on disconnect
    info!("🔚 WebSocket connection ended - Checking for settlement...");
    info!("   Session ID: {:?}", session_id);
    info!("   Job ID: {:?}", job_id);
    info!("   Chain ID: {:?}", chain_id);

    // Cancel background vector loading task if active (Phase 5)
    if let Some(sid) = &session_id {
        let store = server.session_store.read().await;
        if let Some(session) = store.get_session(sid).await {
            // Cancel the background task
            session.cancel_token.cancel();
            info!("🛑 Cancelled background vector loading task for session: {}", sid);
        }
    }

    if let Some(jid) = job_id {
        info!("\n🚨 WEBSOCKET DISCONNECTED - STARTING SETTLEMENT PROCESS");
        info!("   Job ID from WebSocket session: {}", jid);
        info!("   Session ID: {:?}", session_id);
        info!("   Chain ID: {:?}", chain_id);

        // Get checkpoint manager and complete the session job
        let cm = server.checkpoint_manager.read().await;
        info!("   Checkpoint manager available: {}", cm.is_some());

        if let Some(checkpoint_manager) = cm.clone() {
            info!("✅ Spawning complete_session_job in background for job_id: {}", jid);
            drop(cm); // Release lock before spawning

            // ASYNC: Spawn session completion in background to avoid blocking
            tokio::spawn(async move {
                info!("[WS-BG] 🚀 Starting background session completion for job_id: {}", jid);

                match checkpoint_manager.complete_session_job(jid).await {
                    Ok(()) => {
                        info!("[WS-BG] 💰 Settlement completed successfully for job_id: {}", jid);
                    }
                    Err(e) => {
                        error!("[WS-BG] ❌ Failed to complete session job {}: {}", jid, e);
                    }
                }
            });
        } else {
            drop(cm);
            warn!("⚠️ No checkpoint manager available for settlement");
            warn!("   This means the node is running without blockchain integration");
            warn!("   Check if RPC_URL and HOST_PRIVATE_KEY are configured");
        }
    } else {
        info!("ℹ️ WebSocket closed without job_id - no settlement needed");
        info!("   Session might not have been properly initialized");
        info!("   Ensure SDK sends job_id in session_init or prompt messages");
    }
}

impl ApiServer {
    fn error_response(error: ApiError) -> Response {
        let status =
            StatusCode::from_u16(error.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = error.to_response(None);

        (status, axum::response::Json(body)).into_response()
    }
}

// Add uuid to dependencies
use uuid;

/// Test server for integration tests
pub struct TestServer {
    pub port: u16,
}

pub async fn create_test_server() -> Result<TestServer> {
    // Find an available port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    // Create minimal config for testing
    let config = ApiConfig {
        listen_addr: format!("127.0.0.1:{}", port),
        max_connections: 100,
        max_connections_per_ip: 10,
        request_timeout: Duration::from_secs(30),
        cors_allowed_origins: vec!["*".to_string()],
        enable_websocket: true,
        require_api_key: false,
        api_keys: vec![],
        rate_limit_per_minute: 100,
        enable_http2: false,
        enable_auto_retry: false,
        max_retries: 0,
        enable_circuit_breaker: false,
        circuit_breaker_threshold: 10,
        circuit_breaker_timeout: Duration::from_secs(60),
        enable_error_details: true,
        connection_idle_timeout: Duration::from_secs(60),
        websocket_ping_interval: Duration::from_secs(30),
        websocket_pong_timeout: Duration::from_secs(10),
        max_concurrent_streams: 100,
        connection_retry_count: 0,
        connection_retry_backoff: Duration::from_millis(100),
        enable_connection_health_checks: false,
        health_check_interval: Duration::from_secs(60),
        shutdown_timeout: Duration::from_secs(30),
    };

    // Create server and start in background
    let server = Arc::new(ApiServer::new(config).await?);

    // Note: ApiServer doesn't have a run() method yet
    // This would need to be implemented to actually start the server

    // Wait for server to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    Ok(TestServer { port })
}
