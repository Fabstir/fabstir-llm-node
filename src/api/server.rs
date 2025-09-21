use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, Mutex, oneshot, mpsc};
use anyhow::Result;
use axum::{
    Router,
    routing::{get, post},
    extract::{State, Json, ws::{WebSocket, WebSocketUpgrade}},
    response::{IntoResponse, Response},
    http::StatusCode,
};
use tower_http::cors::CorsLayer;
use tracing::{info, error};

use crate::p2p::Node;
use crate::inference::LlmEngine;
use crate::utils::context::{build_prompt_with_context, count_context_tokens};
use crate::api::token_tracker::TokenTracker;
use crate::contracts::checkpoint_manager::CheckpointManager;
use super::{ApiError, InferenceRequest, InferenceResponse, StreamingResponse};
use super::handlers::{ModelInfo, ModelsResponse, HealthResponse};
use super::pool::{ConnectionPool, ConnectionStats, PoolConfig};

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
    shutdown_tx: Option<oneshot::Sender<()>>,
    listener: Option<tokio::net::TcpListener>,
}

#[derive(Default)]
struct Metrics {
    total_requests: u64,
    total_errors: u64,
    request_durations: Vec<Duration>,
}

impl ApiServer {
    pub async fn new(config: ApiConfig) -> Result<Self> {
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
                
                let serve_future = axum::serve(listener, app)
                    .with_graceful_shutdown(async move {
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
        let engine = engine_guard.as_ref()
            .ok_or_else(|| ApiError::ServiceUnavailable("inference engine not initialized".to_string()))?;
        
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
        let full_prompt = build_prompt_with_context(
            &request.conversation_context,
            &request.prompt
        );

        if !request.conversation_context.is_empty() {
            info!("Processing with {} context messages, ~{} tokens",
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
        let result = engine.run_inference(engine_request).await
            .map_err(|e| ApiError::InternalError(format!("Inference failed: {}", e)))?;
        
        // Convert to API response
        let response = InferenceResponse {
            model: request.model,
            content: result.text,
            tokens_used: result.tokens_generated as u32,
            finish_reason: result.finish_reason,
            request_id: request.request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        };
        
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
        let engine = engine_guard.as_ref()
            .ok_or_else(|| ApiError::ServiceUnavailable("inference engine not initialized".to_string()))?;

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
        let full_prompt = build_prompt_with_context(
            &request.conversation_context,
            &request.prompt
        );

        if !request.conversation_context.is_empty() {
            info!("Processing streaming request with {} context messages",
                request.conversation_context.len()
            );
        }

        // DEBUG: Log the actual prompt
        println!("DEBUG STREAMING: Sending prompt to engine: {:?}", full_prompt);

        // Log the request for debugging
        info!("Streaming inference request: model={}, prompt_len={}, max_tokens={}",
            model_id, full_prompt.len(), request.max_tokens);

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
            stream: true,  // Enable streaming!
        };

        // Run streaming inference with real model
        let token_stream = engine.run_inference_stream(engine_request).await
            .map_err(|e| {
                error!("Failed to start streaming inference: {}", e);
                ApiError::InternalError(format!("Streaming inference failed: {}", e))
            })?;

        let (tx, rx) = mpsc::channel(100);

        // Clone values for the spawned task
        let job_id = request.job_id;
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
                                let _ = cm.track_tokens(jid, 1, session_id.clone()).await;
                            } else {
                                token_tracker.track_tokens(Some(jid), 1, session_id.clone()).await;
                            }
                        }

                        let response = StreamingResponse {
                            content: token_info.text.clone(),
                            tokens: 1,
                            finish_reason: None,
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

            // Force checkpoint and cleanup if session ends with a job_id
            if let Some(jid) = job_id {
                if let Some(cm) = checkpoint_manager.as_ref() {
                    let _ = cm.force_checkpoint(jid).await;
                    // Clean up to prevent memory leak
                    cm.cleanup_job(jid).await;
                } else {
                    token_tracker.force_checkpoint(jid).await;
                    // Clean up the simple tracker too
                    token_tracker.cleanup_job(jid).await;
                }
            }

            // Send final message with finish reason
            let final_response = StreamingResponse {
                content: String::new(),
                tokens: 0,
                finish_reason: Some("stop".to_string()),
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
        let node = node_guard.as_ref()
            .ok_or_else(|| ApiError::ServiceUnavailable("no available nodes".to_string()))?;
        
        let capabilities = node.capabilities();
        let models = capabilities.into_iter().map(|id| ModelInfo {
            id: id.clone(),
            name: id,
            description: None,
        }).collect();
        
        Ok(ModelsResponse { models })
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
            issues: if issues.is_empty() { None } else { Some(issues) },
        }
    }
    
    fn create_router(server: Arc<Self>) -> Router {
        Router::new()
            .route("/health", get(health_handler))
            .route("/v1/models", get(models_handler))
            .route("/v1/inference", post(simple_inference_handler))
            .route("/v1/ws", get(websocket_handler))
            .route("/metrics", get(metrics_handler))
            .layer(
                CorsLayer::permissive()
            )
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
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        metrics,
    )
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(server): State<Arc<ApiServer>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_websocket(socket, server))
}

async fn handle_websocket(mut socket: WebSocket, server: Arc<ApiServer>) {
    use serde_json::json;
    
    // Send connection acknowledgment
    let welcome_msg = json!({
        "type": "connected",
        "message": "WebSocket connected successfully"
    });
    if socket.send(axum::extract::ws::Message::Text(welcome_msg.to_string())).await.is_err() {
        return;
    }
    
    while let Some(msg) = socket.recv().await {
        match msg {
            Ok(axum::extract::ws::Message::Text(text)) => {
                // Parse WebSocket message
                if let Ok(json_msg) = serde_json::from_str::<serde_json::Value>(&text) {
                    if json_msg["type"] == "inference" {
                        // Debug: Log the entire request
                        info!("🔍 WebSocket inference request received: {:?}", json_msg["request"]);

                        if let Ok(request) = serde_json::from_value::<InferenceRequest>(json_msg["request"].clone()) {
                            // Log job_id for payment tracking visibility
                            if let Some(job_id) = request.job_id {
                                info!("📋 Processing inference request for blockchain job_id: {}", job_id);
                            } else {
                                info!("⚠️  No job_id in WebSocket request");
                            }

                            // Handle streaming inference
                            match server.handle_streaming_request(request, "ws-client".to_string()).await {
                                Ok(mut receiver) => {
                                    while let Some(response) = receiver.recv().await {
                                        let ws_msg = json!({
                                            "type": "stream_chunk",
                                            "content": response.content,
                                            "tokens": response.tokens,
                                        });
                                        
                                        if socket.send(axum::extract::ws::Message::Text(ws_msg.to_string())).await.is_err() {
                                            break;
                                        }
                                        
                                        if response.finish_reason.is_some() {
                                            let end_msg = json!({"type": "stream_end"});
                                            let _ = socket.send(axum::extract::ws::Message::Text(end_msg.to_string())).await;
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    let error_msg = json!({
                                        "type": "error",
                                        "error": e.to_string()
                                    });
                                    let _ = socket.send(axum::extract::ws::Message::Text(error_msg.to_string())).await;
                                }
                            }
                        }
                    }
                }
            }
            Ok(axum::extract::ws::Message::Ping(data)) => {
                if socket.send(axum::extract::ws::Message::Pong(data)).await.is_err() {
                    break;
                }
            }
            Ok(axum::extract::ws::Message::Close(_)) => break,
            _ => {}
        }
    }
}

impl ApiServer {
    fn error_response(error: ApiError) -> Response {
        let status = StatusCode::from_u16(error.status_code())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
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