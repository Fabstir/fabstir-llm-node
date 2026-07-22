// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::Result;
use axum::{
    extract::{
        ws::{WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Json, Path, State,
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
use tracing::{debug, error, info, warn};

use super::handlers::{HealthResponse, ModelInfo, ModelsResponse};
use super::pool::{ConnectionPool, ConnectionStats, PoolConfig};
use super::{ApiError, InferenceRequest, InferenceResponse, StreamingResponse, UsageInfo};
use crate::api::token_tracker::TokenTracker;
use crate::contracts::checkpoint_manager::CheckpointManager;
use crate::crypto::SessionKeyStore;
use crate::inference::LlmEngine;
use crate::p2p::Node;
use crate::utils::context::{
    build_prompt_with_context, count_context_tokens, extract_latest_user_message,
};
use sha2::{Digest, Sha256};

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
    search_service: Arc<RwLock<Option<Arc<crate::search::SearchService>>>>,
    diffusion_client: Arc<RwLock<Option<Arc<crate::diffusion::DiffusionClient>>>>,
    image_gen_tracker: Arc<crate::diffusion::billing::ImageGenerationTracker>,
    image_gen_rate_limiter: Arc<crate::diffusion::ImageGenerationRateLimiter>,
    transcoder_client: Arc<RwLock<Option<Arc<crate::transcoder::TranscoderClient>>>>,
    transcoding_tracker: Arc<crate::transcoder::billing::TranscodingTracker>,
    /// Host-reachable seam-#2 moderation verdicts (job_id → result). Absent ⇒ HOLD.
    moderation_store: Arc<crate::moderation::verdict_store::VerdictStore>,
    /// Dark-launch switch (MODERATION_ENFORCE) for the transcode moderation gate.
    /// Default off so merging the gate doesn't brick transcoding before seam #1 is
    /// wired; the gate logic itself is always fail-closed when this is on.
    moderation_enforce: bool,
    /// Encrypted, append-only-audited evidence store for matched material (B6).
    moderation_quarantine: Arc<std::sync::Mutex<crate::moderation::csam::quarantine::Quarantine>>,
    /// NCMEC report sink (mock at launch; the real CyberTipline client swaps in at go-live).
    moderation_report_sink: Arc<dyn crate::moderation::csam::report::ReportSink + Send + Sync>,
    /// Moderation observability counters (§8 #7).
    moderation_metrics: Arc<crate::monitoring::moderation_metrics::ModerationMetrics>,
    /// Seam-#1 `task_id → job_id` map (the transcoder's own task id → the node's u64
    /// job id). Recorded at transcode submit; an unknown `task_id` at
    /// `/v1/moderate/frames` ⇒ 404 ⇒ the transcoder HOLDs (fail-closed). In-memory
    /// like `moderation_store` (a node restart loses it ⇒ unknown ⇒ hold).
    moderation_task_jobs: Arc<std::sync::Mutex<HashMap<String, u64>>>,
    /// Shared secret authenticating `/v1/moderate/frames` (MODERATION_INGEST_TOKEN).
    /// `None`/empty ⇒ every frame POST is rejected (401): an unset secret never means
    /// "accept all" (fail-closed, R3-C1).
    moderation_ingest_token: Option<String>,
    transcoding_rate_limiter: Arc<crate::transcoder::rate_limiter::TranscodingRateLimiter>,
    sidecar_capacity_cache: Arc<crate::transcoder::capacity::CachedSidecarStatus>,
    // LTX 2.3 generation sidecar (mirror of the transcoder fields).
    ltx_client: Arc<RwLock<Option<Arc<crate::ltx::ComfyClient>>>>,
    ltx_template_store: Arc<RwLock<Option<Arc<crate::ltx::TemplateStore>>>>,
    ltx_tracker: Arc<crate::ltx::billing::LtxTracker>,
    ltx_rate_limiter: Arc<crate::ltx::rate_limiter::LtxRateLimiter>,
    /// VRAM admission: `MAX_CONCURRENT_GENERATIONS` slots. ComfyUI has no status
    /// endpoint, so LTX gates locally rather than via the transcoder capacity cache.
    ltx_semaphore: Arc<tokio::sync::Semaphore>,
    auto_image_routing: bool,
    /// FC1.6: platform vault depositor addresses (lowercase). EMPTY (the
    /// default) ⇒ the vault-session auth gate is skipped entirely — every
    /// current deployment behaves exactly as before.
    fiat_vault_addresses: Vec<String>,
    /// FC1.6: the credits backend's AUTH key address (NOT its funds key).
    /// None ⇒ POST /v1/session-auth 404s (pre-hardening shape).
    fiat_backend_auth_address: Option<String>,
    /// FC1.6: jobId → backend-authorised client address (in-memory; a restart
    /// clears it and the helper simply re-presents before its next submit).
    session_auth_store: Arc<crate::api::session_auth::SessionAuthStore>,
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

        let session_store_config =
            crate::api::websocket::session_store::SessionStoreConfig::default();
        let session_store = Arc::new(RwLock::new(
            crate::api::websocket::session_store::SessionStore::new(session_store_config),
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
            search_service: Arc::new(RwLock::new(None)),
            diffusion_client: Arc::new(RwLock::new(None)),
            image_gen_tracker: Arc::new(crate::diffusion::billing::ImageGenerationTracker::new()),
            image_gen_rate_limiter: Arc::new(crate::diffusion::ImageGenerationRateLimiter::new(10)),
            transcoder_client: Arc::new(RwLock::new(None)),
            transcoding_tracker: Arc::new(crate::transcoder::billing::TranscodingTracker::new()),
            moderation_store: Arc::new(crate::moderation::verdict_store::VerdictStore::new()),
            moderation_enforce: false,
            moderation_quarantine: Arc::new(std::sync::Mutex::new(
                crate::moderation::csam::quarantine::Quarantine::new(
                    b"test-quarantine-key".to_vec(),
                    90,
                ),
            )),
            moderation_report_sink: Arc::new(crate::moderation::csam::report::MockReportSink::new()),
            moderation_metrics: Arc::new(
                crate::monitoring::moderation_metrics::ModerationMetrics::new(),
            ),
            moderation_task_jobs: Arc::new(std::sync::Mutex::new(HashMap::new())),
            moderation_ingest_token: None,
            transcoding_rate_limiter: Arc::new(
                crate::transcoder::rate_limiter::TranscodingRateLimiter::new(3),
            ),
            sidecar_capacity_cache: Arc::new(
                crate::transcoder::capacity::CachedSidecarStatus::new(Duration::from_secs(2)),
            ),
            ltx_client: Arc::new(RwLock::new(None)),
            ltx_template_store: Arc::new(RwLock::new(None)),
            ltx_tracker: Arc::new(crate::ltx::billing::LtxTracker::new()),
            ltx_rate_limiter: Arc::new(crate::ltx::rate_limiter::LtxRateLimiter::new(3)),
            ltx_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
            auto_image_routing: false,
            fiat_vault_addresses: Vec::new(),
            fiat_backend_auth_address: None,
            session_auth_store: Arc::new(std::sync::Mutex::new(HashMap::new())),
            session_store,
            shutdown_tx: None,
            listener: None,
        }
    }

    pub async fn new(config: ApiConfig) -> Result<Self> {
        // Version stamp for deployment verification
        eprintln!("BUILD VERSION: {}", crate::version::VERSION);
        info!("🚀 API Server {} started", crate::version::VERSION);
        let (rp, fp, pp, pln) = crate::inference::get_penalty_defaults();
        info!(
            "🎛️ Penalty defaults: repeat={}, frequency={}, presence={}, last_n={}",
            rp, fp, pp, pln
        );

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
            crate::api::websocket::session_store::SessionStore::new(session_store_config),
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
            search_service: Arc::new(RwLock::new(None)),
            diffusion_client: Arc::new(RwLock::new(None)),
            image_gen_tracker: Arc::new(crate::diffusion::billing::ImageGenerationTracker::new()),
            image_gen_rate_limiter: Arc::new(crate::diffusion::ImageGenerationRateLimiter::new(
                std::env::var("IMAGE_GEN_RATE_LIMIT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(5),
            )),
            transcoder_client: Arc::new(RwLock::new(None)),
            transcoding_tracker: Arc::new(crate::transcoder::billing::TranscodingTracker::new()),
            moderation_store: Arc::new(crate::moderation::verdict_store::VerdictStore::new()),
            moderation_enforce: std::env::var("MODERATION_ENFORCE")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
            moderation_quarantine: Arc::new(std::sync::Mutex::new(
                crate::moderation::csam::quarantine::Quarantine::new(
                    std::env::var("MODERATION_QUARANTINE_KEY")
                        .map(|s| s.into_bytes())
                        .unwrap_or_else(|_| b"launch-quarantine-key".to_vec()),
                    90,
                ),
            )),
            moderation_report_sink: Arc::new(crate::moderation::csam::report::MockReportSink::new()),
            moderation_metrics: Arc::new(
                crate::monitoring::moderation_metrics::ModerationMetrics::new(),
            ),
            moderation_task_jobs: Arc::new(std::sync::Mutex::new(HashMap::new())),
            // Unset OR empty ⇒ None ⇒ /v1/moderate/frames rejects every POST (401).
            moderation_ingest_token: std::env::var("MODERATION_INGEST_TOKEN")
                .ok()
                .filter(|s| !s.is_empty()),
            transcoding_rate_limiter: Arc::new(
                crate::transcoder::rate_limiter::TranscodingRateLimiter::new(
                    std::env::var("TRANSCODE_RATE_LIMIT")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(3),
                ),
            ),
            sidecar_capacity_cache: Arc::new(
                crate::transcoder::capacity::CachedSidecarStatus::new(Duration::from_secs(2)),
            ),
            ltx_client: Arc::new(RwLock::new(None)),
            ltx_template_store: Arc::new(RwLock::new(None)),
            ltx_tracker: Arc::new(crate::ltx::billing::LtxTracker::new()),
            ltx_rate_limiter: Arc::new(crate::ltx::rate_limiter::ltx_rate_limiter()),
            ltx_semaphore: Arc::new(tokio::sync::Semaphore::new(
                std::env::var("MAX_CONCURRENT_GENERATIONS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1),
            )),
            auto_image_routing: match std::env::var("AUTO_IMAGE_ROUTING") {
                Ok(v) => v == "true",
                Err(_) => std::env::var("DIFFUSION_ENDPOINT")
                    .map(|v| !v.is_empty())
                    .unwrap_or(false),
            },
            // FC1.6 (same optional-env posture as MODERATION_INGEST_TOKEN):
            // unset ⇒ feature off ⇒ behaviour identical to pre-FC1.6 builds.
            fiat_vault_addresses: std::env::var("FIAT_VAULT_ADDRESSES")
                .unwrap_or_default()
                .split(',')
                .map(|a| a.trim().to_lowercase())
                .filter(|a| !a.is_empty())
                .collect(),
            fiat_backend_auth_address: std::env::var("FIAT_BACKEND_AUTH_ADDRESS")
                .ok()
                .map(|a| a.trim().to_lowercase())
                .filter(|a| !a.is_empty()),
            session_auth_store: Arc::new(std::sync::Mutex::new(HashMap::new())),
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
            search_service: self.search_service.clone(),
            diffusion_client: self.diffusion_client.clone(),
            image_gen_tracker: self.image_gen_tracker.clone(),
            image_gen_rate_limiter: self.image_gen_rate_limiter.clone(),
            transcoder_client: self.transcoder_client.clone(),
            transcoding_tracker: self.transcoding_tracker.clone(),
            moderation_store: self.moderation_store.clone(),
            moderation_enforce: self.moderation_enforce,
            moderation_quarantine: self.moderation_quarantine.clone(),
            moderation_report_sink: self.moderation_report_sink.clone(),
            moderation_metrics: self.moderation_metrics.clone(),
            moderation_task_jobs: self.moderation_task_jobs.clone(),
            moderation_ingest_token: self.moderation_ingest_token.clone(),
            transcoding_rate_limiter: self.transcoding_rate_limiter.clone(),
            sidecar_capacity_cache: self.sidecar_capacity_cache.clone(),
            ltx_client: self.ltx_client.clone(),
            ltx_template_store: self.ltx_template_store.clone(),
            ltx_tracker: self.ltx_tracker.clone(),
            ltx_rate_limiter: self.ltx_rate_limiter.clone(),
            ltx_semaphore: self.ltx_semaphore.clone(),
            auto_image_routing: self.auto_image_routing,
            fiat_vault_addresses: self.fiat_vault_addresses.clone(),
            fiat_backend_auth_address: self.fiat_backend_auth_address.clone(),
            session_auth_store: self.session_auth_store.clone(),
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

    /// FC1.6 accessors (fields are private to this module; the session-auth
    /// handler lives in api::session_auth).
    pub fn fiat_backend_auth_address(&self) -> Option<&str> {
        self.fiat_backend_auth_address.as_deref()
    }

    pub fn session_auth_store(&self) -> &Arc<crate::api::session_auth::SessionAuthStore> {
        &self.session_auth_store
    }

    pub async fn get_checkpoint_manager(&self) -> Option<Arc<CheckpointManager>> {
        self.checkpoint_manager.read().await.clone()
    }

    pub async fn set_embedding_model_manager(
        &self,
        manager: Arc<crate::embeddings::EmbeddingModelManager>,
    ) {
        *self.embedding_model_manager.write().await = Some(manager);
    }

    pub async fn get_embedding_model_manager(
        &self,
    ) -> Option<Arc<crate::embeddings::EmbeddingModelManager>> {
        self.embedding_model_manager.read().await.clone()
    }

    pub async fn set_vision_model_manager(&self, manager: Arc<crate::vision::VisionModelManager>) {
        *self.vision_model_manager.write().await = Some(manager);
    }

    pub async fn get_vision_model_manager(&self) -> Option<Arc<crate::vision::VisionModelManager>> {
        self.vision_model_manager.read().await.clone()
    }

    /// Set the search service for web search functionality (v8.7.0+)
    pub async fn set_search_service(&self, service: Arc<crate::search::SearchService>) {
        *self.search_service.write().await = Some(service);
    }

    /// Get the search service for web search functionality
    pub async fn get_search_service(&self) -> Option<Arc<crate::search::SearchService>> {
        self.search_service.read().await.clone()
    }

    /// Set the diffusion client for image generation (v8.16.0+)
    pub async fn set_diffusion_client(&self, client: Arc<crate::diffusion::DiffusionClient>) {
        *self.diffusion_client.write().await = Some(client);
    }

    /// Get the diffusion client for image generation
    pub async fn get_diffusion_client(&self) -> Option<Arc<crate::diffusion::DiffusionClient>> {
        self.diffusion_client.read().await.clone()
    }

    /// Get the image generation rate limiter (v8.16.0+)
    pub fn image_gen_rate_limiter(&self) -> &crate::diffusion::ImageGenerationRateLimiter {
        &self.image_gen_rate_limiter
    }

    /// Get the image generation billing tracker (v8.16.0+)
    pub fn image_gen_tracker(&self) -> &crate::diffusion::billing::ImageGenerationTracker {
        &self.image_gen_tracker
    }

    /// Set the transcoder client (v8.25.0+)
    pub async fn set_transcoder_client(&self, client: Arc<crate::transcoder::TranscoderClient>) {
        *self.transcoder_client.write().await = Some(client);
    }

    /// Get the transcoder client
    pub async fn get_transcoder_client(&self) -> Option<Arc<crate::transcoder::TranscoderClient>> {
        self.transcoder_client.read().await.clone()
    }

    /// Get the transcoding rate limiter (v8.25.0+)
    pub fn transcoding_rate_limiter(
        &self,
    ) -> &crate::transcoder::rate_limiter::TranscodingRateLimiter {
        &self.transcoding_rate_limiter
    }

    /// Get the transcoding billing tracker (v8.25.0+)
    pub fn transcoding_tracker(&self) -> &crate::transcoder::billing::TranscodingTracker {
        &self.transcoding_tracker
    }

    /// Set the LTX (ComfyUI) generation client.
    pub async fn set_ltx_client(&self, client: Arc<crate::ltx::ComfyClient>) {
        *self.ltx_client.write().await = Some(client);
    }

    /// Get the LTX generation client (None ⇒ sidecar unconfigured ⇒ 503).
    pub async fn get_ltx_client(&self) -> Option<Arc<crate::ltx::ComfyClient>> {
        self.ltx_client.read().await.clone()
    }

    /// Set the pinned LTX template store.
    pub async fn set_ltx_template_store(&self, store: Arc<crate::ltx::TemplateStore>) {
        *self.ltx_template_store.write().await = Some(store);
    }

    /// Get the pinned LTX template store.
    pub async fn get_ltx_template_store(&self) -> Option<Arc<crate::ltx::TemplateStore>> {
        self.ltx_template_store.read().await.clone()
    }

    /// Publish the pinned LTX allow-list bundle to S5 so clients can fetch and
    /// authenticate it; the content-addressed CID is the `bundleCID` advertised
    /// on-chain / used for the paid E2E. Returns the CID, or `None` if LTX or S5
    /// isn't configured. Idempotent: identical bundle bytes yield the same CID.
    pub async fn publish_ltx_bundle(&self) -> Option<String> {
        use crate::storage::s5_client::S5Storage;
        let store = self.get_ltx_template_store().await?;
        let cm = self.get_checkpoint_manager().await?;
        let bundle = store.bundle();
        let bytes = match serde_json::to_vec_pretty(bundle) {
            Ok(b) => b,
            Err(e) => {
                println!("⚠️  LTX bundle serialise failed: {e}");
                return None;
            }
        };
        match cm
            .get_s5_storage()
            .put("home/ltx/allowlist-bundle.json", bytes)
            .await
        {
            Ok(cid) => {
                println!(
                    "📦 LTX allow-list bundle published to S5: bundleCID={} (bundleHash={}, allowListVersion={})",
                    cid, bundle.bundle_hash, bundle.allow_list_version
                );
                Some(cid)
            }
            Err(e) => {
                println!("⚠️  LTX bundle publish to S5 failed: {e}");
                None
            }
        }
    }

    /// Get the LTX per-session rate limiter.
    pub fn ltx_rate_limiter(&self) -> &crate::ltx::rate_limiter::LtxRateLimiter {
        &self.ltx_rate_limiter
    }

    /// Get the LTX billing tracker.
    pub fn ltx_tracker(&self) -> &crate::ltx::billing::LtxTracker {
        &self.ltx_tracker
    }

    /// VRAM admission semaphore (`MAX_CONCURRENT_GENERATIONS`). Acquire an owned
    /// permit for the generation's lifetime; `try_acquire_owned().is_err()` ⇒ full.
    pub fn ltx_semaphore(&self) -> Arc<tokio::sync::Semaphore> {
        self.ltx_semaphore.clone()
    }

    /// Host-reachable seam-#2 moderation verdict store (job_id → result). An absent
    /// verdict ⇒ the transcode gate HOLDs (fail-closed, §3.2).
    pub fn moderation_store(&self) -> &crate::moderation::verdict_store::VerdictStore {
        &self.moderation_store
    }

    /// Whether the transcode moderation gate is enforced (MODERATION_ENFORCE).
    /// Default off (dark-launch) until seam-#1 ingest is wired; see [`Self::moderation_store`].
    pub fn moderation_enforce(&self) -> bool {
        self.moderation_enforce
    }

    /// Build the launch asset moderator (B8). Until the real NCMEC list is wired at
    /// go-live (Phase-7 glue), the NCMEC snapshot is `Unavailable` ⇒ image assets
    /// HOLD (fail-closed); the subtitle text-scan list is the launch mock (§4-Q2).
    pub fn build_asset_moderator(&self) -> crate::moderation::asset::AssetModerator {
        crate::moderation::asset::AssetModerator::new(
            crate::moderation::csam::hashlist::HashListSnapshot::unavailable(),
            crate::moderation::csam::ownhash::OwnHashList::new(),
            31,
            crate::moderation::asset::TextScanList::launch_mock(),
        )
    }

    /// The CSAM evidence quarantine (B6). Populated when blocking is wired (Phase 7).
    pub fn moderation_quarantine(
        &self,
    ) -> &std::sync::Mutex<crate::moderation::csam::quarantine::Quarantine> {
        &self.moderation_quarantine
    }

    /// The NCMEC report sink (B7). Mock at launch; real client swaps in at go-live.
    pub fn moderation_report_sink(
        &self,
    ) -> Arc<dyn crate::moderation::csam::report::ReportSink + Send + Sync> {
        self.moderation_report_sink.clone()
    }

    /// Moderation observability counters (§8 #7), exposable at `/metrics`.
    pub fn moderation_metrics(&self) -> &crate::monitoring::moderation_metrics::ModerationMetrics {
        &self.moderation_metrics
    }

    /// Record the seam-#1 `task_id → job_id` mapping at transcode submit (C1). A
    /// `task_id` is single-use (a fresh one is minted per submit), so a re-alias to a
    /// DIFFERENT `job_id` is rejected (logged + ignored) — never silently re-pointed,
    /// which could make a `/frames` POST write `VerdictStore[wrong_job]` (R2-F5).
    pub fn record_task_job(&self, task_id: String, job_id: u64) {
        let mut map = self
            .moderation_task_jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match map.get(&task_id) {
            Some(&existing) if existing != job_id => {
                warn!(
                    "refusing to re-alias moderation task_id {} from job {} to job {}",
                    task_id, existing, job_id
                );
            }
            _ => {
                map.insert(task_id, job_id);
            }
        }
    }

    /// Resolve a transcoder `task_id` to the node's `job_id` (C1). Unknown ⇒ `None`
    /// ⇒ `/v1/moderate/frames` returns 404 ⇒ the transcoder HOLDs (fail-closed).
    pub fn job_for_task(&self, task_id: &str) -> Option<u64> {
        let map = self
            .moderation_task_jobs
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        map.get(task_id).copied()
    }

    /// The configured `/v1/moderate/frames` ingest secret, if set (non-empty).
    pub fn moderation_ingest_token(&self) -> Option<&str> {
        self.moderation_ingest_token.as_deref()
    }

    /// Verify a presented `/v1/moderate/frames` ingest token (C2 / R3-C1). Returns
    /// `false` (reject) when the server's token is unset/empty OR the presented token
    /// is empty; otherwise a plain `==` (a long random shared service secret,
    /// mirroring `reviewer_token`; no constant-time dep — R4-D2). 🔒 An unset secret
    /// NEVER means "accept all".
    pub fn verify_ingest_token(&self, presented: &str) -> bool {
        match self.moderation_ingest_token.as_deref() {
            Some(configured) if !configured.is_empty() && !presented.is_empty() => {
                configured == presented
            }
            _ => false,
        }
    }

    /// Build the launch frames match-state (C5): the SAME `Unavailable` NCMEC
    /// snapshot, own-hash list, and PDQ `max_distance` (31) as
    /// [`Self::build_asset_moderator`], so `/frames` cannot drift from `/asset`.
    pub fn build_frames_match_state(
        &self,
    ) -> (
        crate::moderation::csam::hashlist::HashListSnapshot,
        crate::moderation::csam::ownhash::OwnHashList,
        u32,
    ) {
        (
            crate::moderation::csam::hashlist::HashListSnapshot::unavailable(),
            crate::moderation::csam::ownhash::OwnHashList::new(),
            31,
        )
    }

    /// 🧪 Test-only: set the ingest token after construction (mirrors `set_node`'s
    /// `&mut self` pattern). NOT `#[cfg(test)]` — the integration-test crate compiles
    /// the lib without the test cfg; `#[doc(hidden)]` keeps it off the public surface.
    #[doc(hidden)]
    pub fn set_ingest_token(&mut self, token: Option<String>) {
        self.moderation_ingest_token = token;
    }

    pub async fn has_sidecar_capacity(&self) -> bool {
        match self.get_transcoder_client().await {
            Some(client) => self.sidecar_capacity_cache.has_capacity(&client).await,
            None => false,
        }
    }

    pub async fn get_sidecar_status(&self) -> Option<crate::transcoder::types::SidecarStatus> {
        match self.get_transcoder_client().await {
            Some(client) => self.sidecar_capacity_cache.get_or_fetch(&client).await,
            None => None,
        }
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

        // Web search integration (v8.7.0+)
        let mut search_metadata: Option<(bool, u32, String)> = None;
        let mut search_context = String::new();

        if request.web_search {
            info!("Web search requested for inference");

            // Get search service
            let search_service_guard = self.search_service.read().await;
            if let Some(search_service) = search_service_guard.as_ref() {
                if search_service.is_enabled() {
                    // Extract search queries from request or derive from prompt (v8.7.11+)
                    // Use extract_last_user_query to strip Harmony markers and get actual query
                    let queries = if let Some(ref custom_queries) = request.search_queries {
                        custom_queries.clone()
                    } else {
                        // Extract last user query, stripping Harmony chat markers
                        let query = crate::search::query_extractor::extract_last_user_query(
                            &request.prompt,
                        );
                        vec![query]
                    };

                    // Limit number of searches
                    let max_searches = std::cmp::min(request.max_searches, 20) as usize;
                    let queries_to_search: Vec<_> =
                        queries.into_iter().take(max_searches).collect();
                    let queries_count = queries_to_search.len() as u32;

                    // Log the actual search query for debugging
                    debug!("🔍 Search queries (cleaned): {:?}", queries_to_search);

                    // Perform searches with content fetching (Phase 9)
                    let mut all_results = Vec::new();
                    let mut provider_name = String::new();
                    let mut content_fetched_count = 0usize;

                    for query in &queries_to_search {
                        // Use search_with_content to fetch actual page content
                        match search_service.search_with_content(query, Some(5)).await {
                            Ok(result) => {
                                provider_name = result.provider.clone();
                                content_fetched_count += result.content_fetched_count;
                                all_results.extend(result.results);
                            }
                            Err(e) => {
                                warn!("Search failed for query '{}': {}", query, e);
                            }
                        }
                    }

                    if !all_results.is_empty() {
                        // Format search results with content (Phase 9)
                        // Uses actual page content when available, falls back to snippets
                        search_context = format!(
                            "\n{}\n",
                            crate::search::query_extractor::format_results_with_content_for_prompt(
                                &all_results,
                                8000
                            )
                        );
                        search_metadata = Some((true, queries_count, provider_name));
                        info!(
                            "Web search completed: {} results ({} with content) from {} queries",
                            all_results.len(),
                            content_fetched_count,
                            queries_count
                        );
                    }
                } else {
                    warn!("Web search requested but search service is disabled");
                }
            } else {
                warn!("Web search requested but search service is not available");
            }
        }

        // Build prompt with search context (if any) and conversation context
        let prompt_with_search = if !search_context.is_empty() {
            format!("{}{}", search_context, request.prompt)
        } else {
            request.prompt.clone()
        };
        let full_prompt = build_prompt_with_context(
            &request.conversation_context,
            &prompt_with_search,
            request.thinking.as_deref(),
        );

        if !request.conversation_context.is_empty() {
            info!(
                "Processing with {} context messages, ~{} tokens",
                request.conversation_context.len(),
                count_context_tokens(&request.conversation_context)
            );
        }

        // Parse job_id early (needed for prompt hash storage)
        let job_id = request.job_id.or_else(|| {
            request
                .session_id
                .as_ref()
                .and_then(|sid| sid.trim_end_matches('n').parse::<u64>().ok())
        });

        // Phase 4: Compute prompt hash for proof binding (v8.10.0+)
        let prompt_hash = {
            let hash = Sha256::digest(full_prompt.as_bytes());
            let mut hash_bytes = [0u8; 32];
            hash_bytes.copy_from_slice(&hash);
            hash_bytes
        };

        // Store prompt hash in checkpoint manager (non-streaming path)
        if let Some(jid) = job_id {
            if let Some(cm) = self.checkpoint_manager.read().await.as_ref() {
                cm.set_prompt_hash(jid, prompt_hash).await;
            }
        }

        // Phase 3: Track user message for checkpoint publishing (v8.11.0)
        // Extract just the last user message from Harmony format (v8.12.0 - Bug fix)
        if let Some(ref session_id) = request.session_id {
            if let Some(cm) = self.checkpoint_manager.read().await.as_ref() {
                let user_content = crate::checkpoint::extract_last_user_message(&request.prompt);
                cm.track_conversation_message(session_id, "user", &user_content, false)
                    .await;
            }
        }

        // Create inference request for the engine
        let (repeat_pen, freq_pen, pres_pen, _) = crate::inference::get_penalty_defaults();
        let engine_request = crate::inference::InferenceRequest {
            model_id: model_id.clone(),
            prompt: full_prompt,
            max_tokens: request.max_tokens as usize,
            temperature: request.temperature,
            top_p: 0.9,
            top_k: 40,
            repeat_penalty: repeat_pen,
            frequency_penalty: freq_pen,
            presence_penalty: pres_pen,
            min_p: 0.0,
            seed: None,
            stop_sequences: vec![],
            stream: false,
            cancel_flag: None,
            token_sender: None,
            result_sender: None,
        };

        // Run inference with real model
        let result = engine.run_inference(engine_request).await.map_err(|e| {
            let msg = format!("{}", e);
            if msg.contains("exceeds context window") {
                ApiError::InvalidRequest(msg)
            } else {
                ApiError::InternalError(format!("Inference failed: {}", e))
            }
        })?;

        // Convert to API response (include search metadata if search was performed)
        let (web_search_performed, search_queries_count, search_provider) =
            if let Some((performed, count, provider)) = search_metadata {
                (Some(performed), Some(count), Some(provider))
            } else {
                (None, None, None)
            };

        let response = InferenceResponse {
            model: request.model.clone(),
            content: result.text.clone(),
            tokens_used: result.tokens_generated as u32,
            finish_reason: result.finish_reason,
            request_id: request
                .request_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            chain_id: request.chain_id,
            chain_name: None,
            native_token: None,
            web_search_performed,
            search_queries_count,
            search_provider,
            usage: result.context_usage.map(|cu| UsageInfo {
                prompt_tokens: cu.prompt_tokens as u32,
                completion_tokens: cu.completion_tokens as u32,
                total_tokens: cu.total_tokens as u32,
                context_window_size: cu.context_window_size as u32,
            }),
        };

        // Phase 4: Store response hash for proof binding (non-streaming path - v8.10.0+)
        // In non-streaming, we have the complete response immediately
        if let Some(jid) = job_id {
            if let Some(cm) = self.checkpoint_manager.read().await.as_ref() {
                // Append entire response and finalize in one go
                cm.append_response(jid, &result.text).await;
                let _ = cm.finalize_response_hash(jid).await;
            }
        }

        // Phase 4.2: Track assistant response for checkpoint publishing (v8.11.0)
        if let Some(ref session_id) = request.session_id {
            if let Some(cm) = self.checkpoint_manager.read().await.as_ref() {
                cm.track_conversation_message(session_id, "assistant", &result.text, false)
                    .await;
            }
        }

        if let Some(jid) = job_id {
            info!("📊 Job {} completed: {} tokens", jid, response.tokens_used);
            if let Some(cm) = self.checkpoint_manager.read().await.as_ref() {
                if let Err(e) = cm
                    .track_tokens(jid, response.tokens_used as u64, request.session_id.clone())
                    .await
                {
                    warn!("Token tracking failed for job {}: {}", jid, e);
                }
            } else {
                self.token_tracker
                    .track_tokens(
                        Some(jid),
                        response.tokens_used as usize,
                        request.session_id.clone(),
                    )
                    .await;
            }
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
        cancel_flag: Option<Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<
        (
            mpsc::Receiver<StreamingResponse>,
            tokio::sync::oneshot::Receiver<crate::inference::InferenceResult>,
        ),
        ApiError,
    > {
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

        // Web search integration for streaming (v8.7.5+)
        // Auto-detect search intent from prompt if not explicitly requested (v8.7.8+)
        let mut search_context = String::new();
        let should_search =
            request.web_search || crate::search::query_extractor::needs_web_search(&request.prompt);

        if should_search {
            info!(
                "🔍 Web search triggered for streaming inference (explicit={}, auto-detected={})",
                request.web_search,
                !request.web_search && should_search
            );

            // Get search service
            let search_service_guard = self.search_service.read().await;
            if let Some(search_service) = search_service_guard.as_ref() {
                if search_service.is_enabled() {
                    // Extract search queries from request or derive from prompt (v8.7.11+)
                    // Use extract_last_user_query to strip Harmony markers and get actual query
                    let queries = if let Some(ref custom_queries) = request.search_queries {
                        custom_queries.clone()
                    } else {
                        // Extract last user query, stripping Harmony chat markers
                        let query = crate::search::query_extractor::extract_last_user_query(
                            &request.prompt,
                        );
                        vec![query]
                    };

                    // Limit number of searches
                    let max_searches = std::cmp::min(request.max_searches, 20) as usize;
                    let queries_to_search: Vec<_> =
                        queries.into_iter().take(max_searches).collect();

                    // Log the actual search query for debugging
                    debug!("🔍 Search queries (cleaned): {:?}", queries_to_search);

                    // Perform searches with content fetching (Phase 9)
                    let mut all_results = Vec::new();
                    let mut content_fetched_count = 0usize;

                    for query in &queries_to_search {
                        // Use search_with_content to fetch actual page content
                        match search_service.search_with_content(query, Some(5)).await {
                            Ok(result) => {
                                content_fetched_count += result.content_fetched_count;
                                all_results.extend(result.results);
                            }
                            Err(e) => {
                                warn!("Search failed for streaming query '{}': {}", query, e);
                            }
                        }
                    }

                    if !all_results.is_empty() {
                        // Format search results with content (Phase 9)
                        search_context = format!(
                            "\n{}\n",
                            crate::search::query_extractor::format_results_with_content_for_prompt(
                                &all_results,
                                8000
                            )
                        );
                        info!(
                            "🔍 Web search completed for streaming: {} results ({} with content)",
                            all_results.len(),
                            content_fetched_count
                        );
                    }
                } else {
                    warn!("🔍 Web search requested but search service is disabled");
                }
            } else {
                warn!("🔍 Web search requested but search service is not available");
            }
        }

        // Build prompt with search context (if any) and conversation context
        let prompt_with_search = if !search_context.is_empty() {
            format!("{}{}", search_context, request.prompt)
        } else {
            request.prompt.clone()
        };
        let full_prompt = build_prompt_with_context(
            &request.conversation_context,
            &prompt_with_search,
            request.thinking.as_deref(),
        );

        if !request.conversation_context.is_empty() {
            info!(
                "Processing streaming request with {} context messages",
                request.conversation_context.len()
            );
        }

        // Parse job_id early (needed for prompt hash storage)
        // SDK sends "139n" format, so we strip trailing 'n'
        let job_id = request.job_id.or_else(|| {
            request
                .session_id
                .as_ref()
                .and_then(|sid| sid.trim_end_matches('n').parse::<u64>().ok())
        });

        // Phase 4: Compute prompt hash for proof binding (v8.10.0+)
        let prompt_hash = {
            let hash = Sha256::digest(full_prompt.as_bytes());
            let mut hash_bytes = [0u8; 32];
            hash_bytes.copy_from_slice(&hash);
            hash_bytes
        };

        // Store prompt hash in checkpoint manager
        let checkpoint_manager = self.checkpoint_manager.read().await.clone();
        if let Some(jid) = job_id {
            if let Some(cm) = checkpoint_manager.as_ref() {
                cm.set_prompt_hash(jid, prompt_hash).await;
            }
        }

        // Phase 3: Track user message for checkpoint publishing (v8.11.0)
        // Extract just the last user message from Harmony format (v8.12.0 - Bug fix)
        if let Some(ref session_id) = request.session_id {
            if let Some(cm) = checkpoint_manager.as_ref() {
                let user_content = crate::checkpoint::extract_last_user_message(&request.prompt);
                cm.track_conversation_message(session_id, "user", &user_content, false)
                    .await;
            }
        }

        // Log the request for debugging
        info!(
            "Streaming inference request: model={}, prompt_len={}, max_tokens={}",
            model_id,
            full_prompt.len(),
            request.max_tokens
        );

        // Create inference request for the engine with stream=true
        let (repeat_pen, freq_pen, pres_pen, _) = crate::inference::get_penalty_defaults();
        let engine_request = crate::inference::InferenceRequest {
            model_id: model_id.clone(),
            prompt: full_prompt,
            max_tokens: request.max_tokens as usize,
            temperature: request.temperature,
            top_p: 0.9,
            top_k: 40,
            repeat_penalty: repeat_pen,
            frequency_penalty: freq_pen,
            presence_penalty: pres_pen,
            min_p: 0.0,
            seed: None,
            stop_sequences: vec![],
            stream: true, // Enable streaming!
            cancel_flag,
            token_sender: None,
            result_sender: None,
        };

        // Run streaming inference with real model
        let (token_stream, result_rx) =
            engine
                .run_inference_stream(engine_request)
                .await
                .map_err(|e| {
                    error!("Failed to start streaming inference: {}", e);
                    ApiError::InternalError(format!("Streaming inference failed: {}", e))
                })?;

        let (tx, rx) = mpsc::channel(100);

        // Log job tracking once at start
        if let Some(jid) = job_id {
            info!("📝 Streaming job {} started", jid);
        }

        let session_id = request.session_id.clone();
        let token_tracker = self.token_tracker.clone();

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

                        // Track tokens for checkpoint submission (silent - logs only on checkpoint trigger)
                        // Phase 4: Also append token to response buffer for hash computation (v8.10.0+)
                        if let Some(jid) = job_id {
                            if let Some(cm) = checkpoint_manager.as_ref() {
                                // Append token to response buffer for proof binding
                                cm.append_response(jid, &token_info.text).await;
                                let _ = cm.track_tokens(jid, 1, session_id.clone()).await;
                            } else {
                                token_tracker
                                    .track_tokens(Some(jid), 1, session_id.clone())
                                    .await;
                            }
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

            // Log completion
            if !got_any_tokens {
                error!("Stream completed with no tokens generated");
            } else if let Some(jid) = job_id {
                eprintln!(
                    "✅ Streaming job {} completed: {} tokens",
                    jid, total_tokens
                );
            }

            // Try to submit checkpoint if we have enough tokens
            // BUT DON'T CLEANUP - the session might continue!
            if let Some(jid) = job_id {
                if let Some(cm) = checkpoint_manager.as_ref() {
                    // Phase 4: Finalize response hash before checkpoint (v8.10.0+)
                    let _ = cm.finalize_response_hash(jid).await;
                    let _ = cm.force_checkpoint(jid).await;
                    // DON'T cleanup here - session continues across multiple prompts!
                    // Cleanup should only happen when websocket disconnects
                } else {
                    token_tracker.force_checkpoint(jid).await;
                    // DON'T cleanup here either
                }
            }

            // Phase 4.2: Track assistant response for checkpoint publishing (v8.11.0)
            if let Some(ref session_id) = session_id {
                if let Some(cm) = checkpoint_manager.as_ref() {
                    cm.track_conversation_message(
                        session_id,
                        "assistant",
                        &accumulated_text,
                        false,
                    )
                    .await;
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

        Ok((rx, result_rx))
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

    /// Maximum body size for vision endpoints (20MB to support ~15MB raw images after base64 encoding)
    const VISION_BODY_LIMIT: usize = 20 * 1024 * 1024;

    pub fn create_router(server: Arc<Self>) -> Router {
        // Vision routes need higher body limit for large images
        let vision_routes = Router::new()
            .route("/ocr", post(ocr_handler_wrapper))
            .route("/describe-image", post(describe_image_handler_wrapper))
            .layer(DefaultBodyLimit::max(Self::VISION_BODY_LIMIT))
            .with_state(server.clone());

        // Moderation asset endpoint (B8) — same large body limit as vision (images).
        let moderation_routes = Router::new()
            .route(
                "/asset",
                post(crate::api::moderation::moderate_asset_handler),
            )
            .route(
                "/review",
                post(crate::api::moderation::moderate_review_handler),
            )
            // Seam #1: the transcoder's keyframe handoff. The handler checks the
            // body-field `ingestToken` itself (no global auth layer on this nest).
            .route(
                "/frames",
                post(crate::api::moderation::moderate_frames_handler),
            )
            .layer(DefaultBodyLimit::max(Self::VISION_BODY_LIMIT))
            .with_state(server.clone());

        Router::new()
            .route("/health", get(health_handler))
            // Alias: the SDK browser build probes /v1/health (ClientManager
            // discovery + the per-prompt host-health check). Without it those
            // 404 and read as "unreachable" for healthy hosts. Same handler as
            // /health; keeps the path consistent with the rest of /v1/*.
            .route("/v1/health", get(health_handler))
            .route("/v1/version", get(version_handler))
            .route("/v1/models", get(models_handler))
            .route("/v1/checkpoints/:session_id", get(checkpoints_handler))
            .route("/v1/inference", post(simple_inference_handler))
            .route("/v1/embed", post(embed_handler_wrapper))
            .route("/v1/search", post(search_handler_wrapper))
            .route("/v1/images/generate", post(generate_image_handler_wrapper))
            .route(
                "/v1/transcode",
                post(crate::api::transcode::handler::transcode_submit_handler),
            )
            .route("/v1/transcode/capacity", get(transcode_capacity_handler))
            .route(
                "/v1/transcode/:task_id",
                get(crate::api::transcode::handler::transcode_status_handler),
            )
            .nest("/v1", vision_routes)
            .nest("/v1/moderate", moderation_routes)
            // FC1.6: the fiat helper presents its backend-signed client
            // authorisation here before its WS submit (self-authenticating;
            // 404 when the feature is unconfigured).
            .route(
                "/v1/session-auth",
                post(crate::api::session_auth::session_auth_handler),
            )
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

async fn transcode_capacity_handler(State(server): State<Arc<ApiServer>>) -> impl IntoResponse {
    match server.get_sidecar_status().await {
        Some(status) => axum::response::Json(serde_json::json!({
            "active": status.active_jobs,
            "max": status.max_concurrent,
            "queued": status.queued_jobs,
            "available": status.available(),
            "sidecarConnected": true,
        })),
        None => axum::response::Json(serde_json::json!({
            "active": 0,
            "max": 0,
            "queued": 0,
            "available": 0,
            "sidecarConnected": false,
        })),
    }
}

async fn models_handler(State(server): State<Arc<ApiServer>>) -> impl IntoResponse {
    match server.get_available_models().await {
        Ok(models) => (StatusCode::OK, axum::response::Json(models)).into_response(),
        Err(e) => ApiServer::error_response(e),
    }
}

async fn version_handler() -> impl IntoResponse {
    axum::response::Json(crate::version::get_version_info())
}

/// GET /v1/checkpoints/:session_id - Returns checkpoint index for SDK conversation recovery
async fn checkpoints_handler(
    State(server): State<Arc<ApiServer>>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    use crate::checkpoint::index::CheckpointIndex;

    tracing::info!(
        "🔍 checkpoints_handler called for session_id: {}",
        session_id
    );

    // Get checkpoint_manager from server
    let checkpoint_manager = match server.get_checkpoint_manager().await {
        Some(cm) => cm,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                axum::response::Json(serde_json::json!({
                    "error": "Checkpoint service unavailable"
                })),
            )
                .into_response()
        }
    };

    // Get host address and S5 storage
    let host_address = checkpoint_manager.get_host_address();
    let s5_storage = checkpoint_manager.get_s5_storage();

    // Build S5 path and fetch
    let index_path = CheckpointIndex::s5_path(&host_address, &session_id);
    tracing::info!("🔍 Fetching checkpoint index from: {}", index_path);

    match s5_storage.get(&index_path).await {
        Ok(bytes) => {
            tracing::info!("🔍 Got {} bytes from S5", bytes.len());
            match serde_json::from_slice::<CheckpointIndex>(&bytes) {
                Ok(index) => {
                    tracing::info!(
                        "🔍 Returning checkpoint index: sessionId={}, hostAddress={}, checkpoints={}, hostSignature_len={}",
                        index.session_id,
                        index.host_address,
                        index.checkpoints.len(),
                        index.host_signature.len()
                    );
                    let response = serde_json::to_value(&index).unwrap();
                    tracing::debug!(
                        "🔍 Response JSON: {}",
                        serde_json::to_string(&response).unwrap_or_default()
                    );
                    (StatusCode::OK, axum::response::Json(response)).into_response()
                }
                Err(e) => {
                    tracing::error!("🔍 Failed to parse checkpoint index: {}", e);
                    tracing::error!("🔍 Raw bytes: {}", String::from_utf8_lossy(&bytes));
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        axum::response::Json(serde_json::json!({
                            "error": format!("Failed to parse checkpoint index: {}", e)
                        })),
                    )
                        .into_response()
                }
            }
        }
        Err(crate::storage::StorageError::NotFound(_)) => {
            tracing::warn!("🔍 No checkpoints found for session {}", session_id);
            (
                StatusCode::NOT_FOUND,
                axum::response::Json(serde_json::json!({
                    "error": format!("No checkpoints found for session {}", session_id)
                })),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("🔍 Failed to fetch checkpoint index: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                axum::response::Json(serde_json::json!({
                    "error": format!("Failed to fetch checkpoint index: {}", e)
                })),
            )
                .into_response()
        }
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

async fn metrics_handler(State(server): State<Arc<ApiServer>>) -> impl IntoResponse {
    // Surface the moderation observability counters (§8 #7) in Prometheus text format —
    // they are incremented in-process by the moderation handlers/gate and must be visible
    // to operators (cleared/blocked/flagged verdicts, fail-closed holds, Track-1 matches,
    // NCMEC reports filed).
    let m = server.moderation_metrics().snapshot();
    let metrics = format!(
        "# HELP http_requests_total Total HTTP requests\n\
         # TYPE http_requests_total counter\n\
         http_requests_total 0\n\
         # HELP http_request_duration_seconds Request duration\n\
         # TYPE http_request_duration_seconds histogram\n\
         http_request_duration_seconds_bucket{{le=\"0.1\"}} 0\n\
         # HELP moderation_verdicts_total Moderation verdicts by outcome\n\
         # TYPE moderation_verdicts_total counter\n\
         moderation_verdicts_total{{outcome=\"cleared\"}} {}\n\
         moderation_verdicts_total{{outcome=\"blocked\"}} {}\n\
         moderation_verdicts_total{{outcome=\"flagged\"}} {}\n\
         # HELP moderation_holds_total Fail-closed moderation holds\n\
         # TYPE moderation_holds_total counter\n\
         moderation_holds_total {}\n\
         # HELP moderation_matches_total Track-1 content matches\n\
         # TYPE moderation_matches_total counter\n\
         moderation_matches_total {}\n\
         # HELP moderation_reports_filed_total NCMEC reports filed\n\
         # TYPE moderation_reports_filed_total counter\n\
         moderation_reports_filed_total {}\n",
        m.cleared, m.blocked, m.flagged, m.held, m.matches, m.reports_filed
    );

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
    use crate::api::http_server::AppState;
    use crate::blockchain::ChainRegistry;

    // Create AppState from ApiServer
    let app_state = AppState {
        api_server: server.clone(),
        chain_registry: Arc::new(ChainRegistry::new()),
        sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        chain_stats: Arc::new(RwLock::new(std::collections::HashMap::new())),
        embedding_model_manager: server.embedding_model_manager.clone(),
        vision_model_manager: server.vision_model_manager.clone(),
        search_service: server.search_service.clone(),
        diffusion_client: server.diffusion_client.clone(),
        transcoder_client: server.transcoder_client.clone(),
    };

    // Call the actual embed_handler
    match crate::api::embed_handler(axum::extract::State(app_state), Json(request)).await {
        Ok(response) => (StatusCode::OK, axum::response::Json(response.0)).into_response(),
        Err((status, message)) => (
            status,
            axum::response::Json(serde_json::json!({
                "error": message
            })),
        )
            .into_response(),
    }
}

// OCR handler wrapper that converts ApiServer state to AppState
async fn ocr_handler_wrapper(
    State(server): State<Arc<ApiServer>>,
    Json(request): Json<crate::api::ocr::OcrRequest>,
) -> impl IntoResponse {
    use crate::api::http_server::AppState;
    use crate::blockchain::ChainRegistry;

    // Create AppState from ApiServer
    let app_state = AppState {
        api_server: server.clone(),
        chain_registry: Arc::new(ChainRegistry::new()),
        sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        chain_stats: Arc::new(RwLock::new(std::collections::HashMap::new())),
        embedding_model_manager: server.embedding_model_manager.clone(),
        vision_model_manager: server.vision_model_manager.clone(),
        search_service: server.search_service.clone(),
        diffusion_client: server.diffusion_client.clone(),
        transcoder_client: server.transcoder_client.clone(),
    };

    // Call the actual ocr_handler
    match crate::api::ocr_handler(axum::extract::State(app_state), Json(request)).await {
        Ok(response) => (StatusCode::OK, axum::response::Json(response.0)).into_response(),
        Err((status, message)) => (
            status,
            axum::response::Json(serde_json::json!({
                "error": message
            })),
        )
            .into_response(),
    }
}

// Describe image handler wrapper that converts ApiServer state to AppState
async fn describe_image_handler_wrapper(
    State(server): State<Arc<ApiServer>>,
    Json(request): Json<crate::api::describe_image::DescribeImageRequest>,
) -> impl IntoResponse {
    use crate::api::http_server::AppState;
    use crate::blockchain::ChainRegistry;

    // Create AppState from ApiServer
    let app_state = AppState {
        api_server: server.clone(),
        chain_registry: Arc::new(ChainRegistry::new()),
        sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        chain_stats: Arc::new(RwLock::new(std::collections::HashMap::new())),
        embedding_model_manager: server.embedding_model_manager.clone(),
        vision_model_manager: server.vision_model_manager.clone(),
        search_service: server.search_service.clone(),
        diffusion_client: server.diffusion_client.clone(),
        transcoder_client: server.transcoder_client.clone(),
    };

    // Call the actual describe_image_handler
    match crate::api::describe_image_handler(axum::extract::State(app_state), Json(request)).await {
        Ok(response) => (StatusCode::OK, axum::response::Json(response.0)).into_response(),
        Err((status, message)) => (
            status,
            axum::response::Json(serde_json::json!({
                "error": message
            })),
        )
            .into_response(),
    }
}

// Search handler wrapper that converts ApiServer state to AppState (v8.7.0+)
async fn search_handler_wrapper(
    State(server): State<Arc<ApiServer>>,
    Json(request): Json<crate::api::search::SearchApiRequest>,
) -> impl IntoResponse {
    use crate::api::http_server::AppState;
    use crate::blockchain::ChainRegistry;

    // Create AppState from ApiServer
    let app_state = AppState {
        api_server: server.clone(),
        chain_registry: Arc::new(ChainRegistry::new()),
        sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        chain_stats: Arc::new(RwLock::new(std::collections::HashMap::new())),
        embedding_model_manager: server.embedding_model_manager.clone(),
        vision_model_manager: server.vision_model_manager.clone(),
        search_service: server.search_service.clone(),
        diffusion_client: server.diffusion_client.clone(),
        transcoder_client: server.transcoder_client.clone(),
    };

    // Call the actual search_handler
    match crate::api::search::search_handler(axum::extract::State(app_state), Json(request)).await {
        Ok(response) => (StatusCode::OK, axum::response::Json(response.0)).into_response(),
        Err((status, message)) => (
            status,
            axum::response::Json(serde_json::json!({
                "error": message
            })),
        )
            .into_response(),
    }
}

// Generate image handler wrapper that converts ApiServer state to AppState (v8.16.0+)
async fn generate_image_handler_wrapper(
    State(server): State<Arc<ApiServer>>,
    Json(request): Json<crate::api::generate_image::GenerateImageRequest>,
) -> impl IntoResponse {
    use crate::api::http_server::AppState;
    use crate::blockchain::ChainRegistry;

    // Create AppState from ApiServer
    let app_state = AppState {
        api_server: server.clone(),
        chain_registry: Arc::new(ChainRegistry::new()),
        sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        chain_stats: Arc::new(RwLock::new(std::collections::HashMap::new())),
        embedding_model_manager: server.embedding_model_manager.clone(),
        vision_model_manager: server.vision_model_manager.clone(),
        search_service: server.search_service.clone(),
        diffusion_client: server.diffusion_client.clone(),
        transcoder_client: server.transcoder_client.clone(),
    };

    // Call the actual generate_image_handler
    match crate::api::generate_image::generate_image_handler(
        axum::extract::State(app_state),
        Json(request),
    )
    .await
    {
        Ok(response) => (StatusCode::OK, axum::response::Json(response.0)).into_response(),
        Err((status, message)) => (
            status,
            axum::response::Json(serde_json::json!({
                "error": message
            })),
        )
            .into_response(),
    }
}

/// Process images via VLM sidecar, return augmented prompt (v8.15.3+)
///
/// Strip SDK UI markers from prompt before sending to LLM (v8.15.4+)
///
/// The SDK chat UI embeds `<<DISPLAY>>...<</DISPLAY>>` and `<<ATTACHMENTS>>...<</ATTACHMENTS>>`
/// markers in the prompt text. These contain filenames that cause the LLM to hallucinate
/// (e.g., seeing "Book1 - Excel.png" and describing a spreadsheet instead of the actual image).
/// This function removes those markers so the LLM only sees the actual user text and
/// the `[Image Analysis]` block from VLM processing.
fn strip_ui_markers(prompt: &str) -> String {
    use regex::Regex;
    // Strip <<ATTACHMENTS>>...<</ATTACHMENTS>> entirely (filenames cause hallucinations)
    let re_attach = Regex::new(r"<<ATTACHMENTS>>.*?<</ATTACHMENTS>>").unwrap();
    let cleaned = re_attach.replace_all(prompt, "");
    // Strip <<DISPLAY>> and <</DISPLAY>> markers but keep content between them
    let cleaned = cleaned
        .replace("<<DISPLAY>>", "")
        .replace("<</DISPLAY>>", "");
    // Also strip [Attached image: ...] lines that may reference filenames
    let re_attached = Regex::new(r"\[Attached image: [^\]]*\]").unwrap();
    let cleaned = re_attached.replace_all(&cleaned, "");
    // Collapse multiple newlines
    let re_newlines = Regex::new(r"\n{3,}").unwrap();
    re_newlines.replace_all(&cleaned, "\n\n").trim().to_string()
}

/// For each image in the array, calls VlmClient::describe() to get a text description.
/// Returns the prompt augmented with `[Image Analysis]...[/Image Analysis]` context,
/// or the original prompt if VLM is unavailable or all images fail.
async fn process_vision_images(
    server: &ApiServer,
    images: &[serde_json::Value],
    user_prompt: &str,
) -> (String, u64) {
    let manager_guard = server.vision_model_manager.read().await;
    if let Some(manager) = manager_guard.as_ref() {
        if let Some(vlm_client) = manager.get_vlm_client() {
            let mut descriptions = Vec::new();
            let mut vlm_tokens: u64 = 0;
            for (i, image) in images.iter().enumerate() {
                let data = match image["data"].as_str() {
                    Some(d) if !d.is_empty() => d,
                    _ => continue,
                };
                let format = image["format"].as_str().unwrap_or("png");
                info!(
                    "Processing image {}/{} via VLM sidecar (OCR + describe)",
                    i + 1,
                    images.len()
                );

                // Step 1: OCR - extract all text (4096 tokens, temp 0.1)
                let mut parts = Vec::new();
                match vlm_client.ocr(data, format).await {
                    Ok(result) => {
                        let text = result.text.trim().to_string();
                        info!(
                            "VLM OCR image {} in {}ms ({} chars, {} tokens): {:?}",
                            i + 1,
                            result.processing_time_ms,
                            text.len(),
                            result.tokens_used,
                            &text[..text.len().min(200)]
                        );
                        vlm_tokens += result.tokens_used as u64;
                        if !text.is_empty() {
                            parts.push(format!("Text content:\n{}", text));
                        }
                    }
                    Err(e) => warn!("VLM OCR failed for image {}: {}", i + 1, e),
                }

                // Step 2: Brief visual description (100 tokens, temp 0.3)
                match vlm_client.describe(data, format, "brief", None).await {
                    Ok(result) => {
                        let desc = result.description.trim().to_string();
                        info!(
                            "VLM described image {} in {}ms ({} tokens): {:?}",
                            i + 1,
                            result.processing_time_ms,
                            result.tokens_used,
                            &desc[..desc.len().min(200)]
                        );
                        vlm_tokens += result.tokens_used as u64;
                        if !desc.is_empty() {
                            parts.push(format!("Visual: {}", desc));
                        }
                    }
                    Err(e) => warn!("VLM describe failed for image {}: {}", i + 1, e),
                }

                if !parts.is_empty() {
                    descriptions.push(parts.join("\n\n"));
                }
            }
            // Strip UI markers (<<ATTACHMENTS>>, <<DISPLAY>>, [Attached image:]) that
            // contain filenames causing LLM hallucinations (v8.15.4+)
            let clean_prompt = strip_ui_markers(user_prompt);
            let augmented = crate::vision::augment_prompt_with_vision(&descriptions, &clean_prompt);
            info!("VLM vision processing used {} tokens total", vlm_tokens);
            info!(
                "Vision-augmented prompt preview (first 500 chars): {:?}",
                &augmented[..augmented.len().min(500)]
            );
            return (augmented, vlm_tokens);
        }
    }
    (user_prompt.to_string(), 0)
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(server): State<Arc<ApiServer>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_websocket(socket, server))
}

async fn handle_websocket(socket: WebSocket, server: Arc<ApiServer>) {
    use futures::{SinkExt, StreamExt};
    use serde_json::json;

    // Split ws_sender into sender + receiver for concurrent access (stream_cancel support)
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Track session information for settlement
    let mut session_id: Option<String> = None;
    let mut job_id: Option<u64> = None;
    let mut chain_id: Option<u64> = None;

    // Send connection acknowledgment
    let welcome_msg = json!({
        "type": "connected",
        "message": "WebSocket connected successfully"
    });
    if ws_sender
        .send(axum::extract::ws::Message::Text(welcome_msg.to_string()))
        .await
        .is_err()
    {
        return;
    }

    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(axum::extract::ws::Message::Text(text)) => {
                // Parse WebSocket message
                if let Ok(json_msg) = serde_json::from_str::<serde_json::Value>(&text) {
                    // Handle stream_cancel (always plaintext, processed before all other types)
                    if json_msg["type"] == "stream_cancel" {
                        let cancel_sid = json_msg["session_id"]
                            .as_str()
                            .or_else(|| json_msg["sessionId"].as_str())
                            .map(String::from)
                            .or(session_id.clone());
                        if let Some(sid) = &cancel_sid {
                            let store = server.session_store.read().await;
                            if let Some(session) = store.get_session(sid).await {
                                session
                                    .inference_cancel_flag
                                    .store(true, std::sync::atomic::Ordering::Release);
                                info!("🛑 stream_cancel received for session {}", sid);
                            } else {
                                debug!("stream_cancel for unknown session {}, ignoring", sid);
                            }
                        }
                        continue;
                    }

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

                        eprintln!("📝 Session init: job={:?} session={:?}", job_id, session_id);

                        // FIX: Create session in session_store (was missing - caused "Session not found" errors)
                        if let Some(sid) = &session_id {
                            let mut store = server.session_store.write().await;
                            match store
                                .create_session_with_chain(
                                    sid.clone(),
                                    crate::api::websocket::session::SessionConfig::default(),
                                    chain_id.unwrap_or(84532), // Default to Base Sepolia
                                )
                                .await
                            {
                                Ok(_) => {
                                    info!("✅ Session created in store: {}", sid);
                                }
                                Err(e) => {
                                    error!("❌ Failed to create session in store: {}", e);
                                }
                            }
                        }

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

                        if let Err(e) = ws_sender
                            .send(axum::extract::ws::Message::Text(response.to_string()))
                            .await
                        {
                            error!("Failed to send session_init response: {}", e);
                        }
                    }

                    // Handle encrypted session initialization
                    if json_msg["type"] == "encrypted_session_init" {
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

                                                let _ = ws_sender
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

                                                    // FC1.6: vault-paid sessions serve only the depositor or
                                                    // a backend-authorised client. With no vault configured
                                                    // (every pre-FC1 deployment) this whole block is skipped.
                                                    // With the gate ON, an unreadable depositor fails CLOSED:
                                                    // a node serving vault money must not serve blind.
                                                    if !server.fiat_vault_addresses.is_empty() {
                                                        let denial: Option<String> = match job_id {
                                                            None => Some("job id unparseable".to_string()),
                                                            Some(jid) => {
                                                                let depositor = match server.get_checkpoint_manager().await {
                                                                    Some(cm) => cm.query_session_depositor(jid).await,
                                                                    None => Err(anyhow::anyhow!("no checkpoint manager")),
                                                                };
                                                                match depositor {
                                                                    Err(e) => Some(format!("depositor unavailable: {}", e)),
                                                                    Ok(dep) => {
                                                                        let authorised = server
                                                                            .session_auth_store
                                                                            .lock()
                                                                            .ok()
                                                                            .and_then(|m| m.get(&jid).cloned());
                                                                        if crate::api::session_auth::authorise_session_client(
                                                                            &dep,
                                                                            &client_address,
                                                                            &server.fiat_vault_addresses,
                                                                            authorised.as_deref(),
                                                                        ) {
                                                                            None
                                                                        } else {
                                                                            Some(format!(
                                                                                "client {} is not authorised for vault-paid job {}",
                                                                                client_address, jid
                                                                            ))
                                                                        }
                                                                    }
                                                                }
                                                            }
                                                        };
                                                        if let Some(reason) = denial {
                                                            error!("🚫 SESSION_AUTH_DENIED: {}", reason);
                                                            let mut error_msg = json!({
                                                                "type": "error",
                                                                "code": "SESSION_AUTH_DENIED",
                                                                "message": format!("session authorisation denied: {}", reason),
                                                                "session_id": session_id.clone().unwrap_or_else(|| "unknown".to_string())
                                                            });
                                                            if let Some(msg_id) = json_msg.get("id") {
                                                                error_msg["id"] = msg_id.clone();
                                                            }
                                                            let _ = ws_sender
                                                                .send(axum::extract::ws::Message::Text(
                                                                    error_msg.to_string(),
                                                                ))
                                                                .await;
                                                            continue;
                                                        }
                                                    }

                                                    eprintln!("🔐 Encrypted session started: job={:?} session={:?} model={}", job_id, session_id, model_name);

                                                    // Store session key in SessionKeyStore
                                                    if let Some(sid) = &session_id {
                                                        server
                                                            .session_key_store
                                                            .store_key(
                                                                sid.clone(),
                                                                extracted_session_key,
                                                            )
                                                            .await;

                                                        // Ensure session exists without replacing (preserves vectors/history on re-init)
                                                        {
                                                            let mut store =
                                                                server.session_store.write().await;
                                                            match store.ensure_session_exists_with_chain(
                                                                sid.clone(),
                                                                crate::api::websocket::session::SessionConfig::default(),
                                                                chain_id.unwrap_or(84532),
                                                            ).await {
                                                                Ok(true) => {
                                                                    info!("✅ Encrypted session created in store: {}", sid);
                                                                }
                                                                Ok(false) => {
                                                                    info!("✅ Session re-init, preserving existing state: {}", sid);
                                                                }
                                                                Err(e) => {
                                                                    error!("❌ Failed to ensure session exists: {}", e);
                                                                }
                                                            }
                                                        }

                                                        // Set recovery public key in checkpoint manager (for encrypted checkpoint deltas)
                                                        if let Some(recovery_pubkey) =
                                                            &session_init_data.recovery_public_key
                                                        {
                                                            if let Some(cm) = server
                                                                .get_checkpoint_manager()
                                                                .await
                                                            {
                                                                cm.set_session_recovery_public_key(
                                                                    sid,
                                                                    recovery_pubkey.clone(),
                                                                )
                                                                .await;
                                                                info!("🔐 Recovery public key set for session {} (encrypted checkpoints enabled)", sid);
                                                            } else {
                                                                warn!("⚠️ Recovery public key provided but checkpoint manager not available");
                                                            }
                                                        } else {
                                                            debug!("ℹ️ No recovery public key in session init - checkpoints will be plaintext");
                                                        }

                                                        // Handle vector_database if provided (Sub-phase 3.3)
                                                        if let Some(vdb_info) = session_init_data
                                                            .vector_database
                                                            .clone()
                                                        {
                                                            info!(
                                                                "📦 Vector database requested: {}",
                                                                vdb_info.manifest_path
                                                            );

                                                            // Get session from store and update it
                                                            let mut store =
                                                                server.session_store.write().await;
                                                            if let Some(mut session) =
                                                                store.get_session_mut(sid).await
                                                            {
                                                                // Store encryption key in session
                                                                session.encryption_key = Some(
                                                                    extracted_session_key.to_vec(),
                                                                );

                                                                // Set vector_database info
                                                                session.set_vector_database(Some(
                                                                    vdb_info.clone(),
                                                                ));

                                                                // Set status to Loading
                                                                session.set_vector_loading_status(
                                                                    crate::api::websocket::session::VectorLoadingStatus::Loading
                                                                );

                                                                // Get cancel_token for background task
                                                                let cancel_token =
                                                                    session.cancel_token.clone();

                                                                info!("🚀 Spawning async vector loading task for session: {}", sid);

                                                                // Spawn background task
                                                                let sid_clone = sid.clone();
                                                                let session_store_clone =
                                                                    server.session_store.clone();
                                                                let encryption_key_clone = Some(
                                                                    extracted_session_key.to_vec(),
                                                                );

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

                                                    if let Err(e) = ws_sender
                                                        .send(axum::extract::ws::Message::Text(
                                                            response.to_string(),
                                                        ))
                                                        .await
                                                    {
                                                        error!("Failed to send encrypted session_init_ack: {}", e);
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

                                                    let _ = ws_sender
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

                                            let _ = ws_sender
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

                                    let _ = ws_sender
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

                                let _ = ws_sender
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

                            if let Err(e) = ws_sender
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

                    // Handle encrypted messages
                    if json_msg["type"] == "encrypted_message" {
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

                                                    let _ = ws_sender
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
                                                                    "✅ Decrypted message ({} chars)",
                                                                    plaintext_str.len()
                                                                );

                                                                // Try to parse decrypted content as JSON (SDK v6.2+)
                                                                // Falls back to treating it as plain prompt string
                                                                let decrypted_json: serde_json::Value =
                                                                    serde_json::from_str(&plaintext_str)
                                                                        .unwrap_or_else(|_| {
                                                                            // If not JSON, treat as plain prompt
                                                                            json!({"prompt": plaintext_str})
                                                                        });

                                                                // Log decrypted JSON keys for debugging (v8.15.3+)
                                                                if let Some(obj) =
                                                                    decrypted_json.as_object()
                                                                {
                                                                    let keys: Vec<&String> =
                                                                        obj.keys().collect();
                                                                    info!(
                                                                        "Decrypted JSON keys: {:?}",
                                                                        keys
                                                                    );
                                                                    if let Some(images_val) =
                                                                        obj.get("images")
                                                                    {
                                                                        info!("images field type: {}, is_array={}, is_null={}",
                                                                            match images_val {
                                                                                serde_json::Value::Array(a) => format!("array(len={})", a.len()),
                                                                                serde_json::Value::String(_) => "string".to_string(),
                                                                                serde_json::Value::Null => "null".to_string(),
                                                                                serde_json::Value::Object(_) => "object".to_string(),
                                                                                _ => format!("{:?}", images_val),
                                                                            },
                                                                            images_val.is_array(),
                                                                            images_val.is_null()
                                                                        );
                                                                    } else {
                                                                        info!("No 'images' field in decrypted JSON");
                                                                    }
                                                                }

                                                                // Image generation routing (v8.16.0+)
                                                                if decrypted_json
                                                                    .get("action")
                                                                    .and_then(|v| v.as_str())
                                                                    == Some("image_generation")
                                                                {
                                                                    info!("Routing encrypted message to image generation handler");
                                                                    let response_msg = crate::api::websocket::handlers::image_generation::handle_encrypted_image_generation(
                                                                        &server,
                                                                        &decrypted_json,
                                                                        &session_key,
                                                                        current_session_id.as_deref().unwrap_or("unknown"),
                                                                        job_id,
                                                                        json_msg.get("id"),
                                                                    ).await;
                                                                    let _ = ws_sender.send(axum::extract::ws::Message::Text(response_msg.to_string())).await;
                                                                    continue;
                                                                }

                                                                // Transcoding routing (v8.25.0+)
                                                                if decrypted_json
                                                                    .get("action")
                                                                    .and_then(|v| v.as_str())
                                                                    == Some("transcode")
                                                                {
                                                                    info!("Routing encrypted message to transcode handler");
                                                                    let (ack, progress_task) = crate::api::websocket::handlers::transcode::handle_encrypted_transcode(
                                                                        &server,
                                                                        &decrypted_json,
                                                                        &session_key,
                                                                        current_session_id.as_deref().unwrap_or("unknown"),
                                                                        job_id,
                                                                        json_msg.get("id"),
                                                                    ).await;
                                                                    let _ = ws_sender.send(axum::extract::ws::Message::Text(ack.to_string())).await;

                                                                    // If a progress task was returned, spawn it and stream progress
                                                                    if let Some(task) =
                                                                        progress_task
                                                                    {
                                                                        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel::<serde_json::Value>(32);
                                                                        let tc = server
                                                                            .get_transcoder_client()
                                                                            .await
                                                                            .unwrap();
                                                                        let cancel_task_id =
                                                                            task.task_id.clone();
                                                                        let cancel_tc = tc.clone();
                                                                        let server_arc =
                                                                            server.clone();
                                                                        // Clone formats from decrypted_json for the background task
                                                                        let formats: Vec<crate::transcoder::VideoFormat> = decrypted_json.get("mediaFormats")
                                                                            .and_then(|v| serde_json::from_value(v.clone()).ok())
                                                                            .unwrap_or_default();
                                                                        let is_encrypted_flag =
                                                                            decrypted_json
                                                                                .get("isEncrypted")
                                                                                .and_then(|v| {
                                                                                    v.as_bool()
                                                                                })
                                                                                .unwrap_or(true);

                                                                        task.spawn(
                                                                            tc,
                                                                            session_key,
                                                                            current_session_id
                                                                                .clone()
                                                                                .unwrap_or_default(
                                                                                ),
                                                                            job_id,
                                                                            server_arc,
                                                                            progress_tx,
                                                                            formats,
                                                                            is_encrypted_flag,
                                                                        );

                                                                        // Drain progress messages until task completes
                                                                        loop {
                                                                            tokio::select! {
                                                                                Some(msg) = progress_rx.recv() => {
                                                                                    let _ = ws_sender.send(axum::extract::ws::Message::Text(msg.to_string())).await;
                                                                                }
                                                                                ws_msg = ws_receiver.next() => {
                                                                                    match ws_msg {
                                                                                        Some(Ok(axum::extract::ws::Message::Text(txt))) => {
                                                                                            // Check for cancel
                                                                                            if let Ok(cancel_json) = serde_json::from_str::<serde_json::Value>(&txt) {
                                                                                                if cancel_json.get("type").and_then(|v| v.as_str()) == Some("transcode_cancel") {
                                                                                                    info!("Transcode cancel received for task {}", cancel_task_id);
                                                                                                    // Best-effort sidecar cancel
                                                                                                    match cancel_tc.cancel_transcode(&cancel_task_id).await {
                                                                                                        Ok(true) => info!("Sidecar cancel acknowledged for {}", cancel_task_id),
                                                                                                        Ok(false) => debug!("Sidecar cancel endpoint not supported for {}", cancel_task_id),
                                                                                                        Err(e) => warn!("Sidecar cancel failed for {}: {}", cancel_task_id, e),
                                                                                                    }
                                                                                                    break;
                                                                                                }
                                                                                            }
                                                                                        }
                                                                                        Some(Ok(axum::extract::ws::Message::Close(_))) | None => break,
                                                                                        _ => {}
                                                                                    }
                                                                                }
                                                                                else => break,
                                                                            }
                                                                        }
                                                                    }
                                                                    continue;
                                                                }

                                                                // LTX 2.3 generation routing
                                                                if decrypted_json
                                                                    .get("action")
                                                                    .and_then(|v| v.as_str())
                                                                    == Some("ltx_generate")
                                                                {
                                                                    info!("Routing encrypted message to ltx_generate handler");
                                                                    let (ack, gen_task) = crate::api::websocket::handlers::ltx::handle_encrypted_ltx_generate(
                                                                        &server,
                                                                        &decrypted_json,
                                                                        &session_key,
                                                                        current_session_id.as_deref().unwrap_or("unknown"),
                                                                        job_id,
                                                                        json_msg.get("id"),
                                                                    ).await;
                                                                    let _ = ws_sender.send(axum::extract::ws::Message::Text(ack.to_string())).await;

                                                                    if let Some(task) = gen_task {
                                                                        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel::<serde_json::Value>(32);
                                                                        // Graceful re-fetch (never None in practice; client is set once
                                                                        // at startup). If it vanished, give the client a terminal error
                                                                        // rather than silence (it already got the "processing" ack).
                                                                        let Some(lc) = server
                                                                            .get_ltx_client()
                                                                            .await
                                                                        else {
                                                                            // The task dies unspawned: resolve the pending the
                                                                            // handler marked at accept, or a disconnect would
                                                                            // defer completion to a task that never runs.
                                                                            if task.pending_marked {
                                                                                if let Some(jid) =
                                                                                    task.job_id
                                                                                {
                                                                                    server.ltx_tracker().mark_proof_forfeited(jid).await;
                                                                                }
                                                                            }
                                                                            let err = crate::api::websocket::handlers::ltx::build_ltx_error(
                                                                                "SIDECAR_UNAVAILABLE",
                                                                                "LTX sidecar became unavailable",
                                                                                &session_key,
                                                                                current_session_id.as_deref().unwrap_or("unknown"),
                                                                                decrypted_json.get("requestId").and_then(|v| v.as_str()),
                                                                                json_msg.get("id"),
                                                                            );
                                                                            let _ = ws_sender.send(axum::extract::ws::Message::Text(err.to_string())).await;
                                                                            continue;
                                                                        };
                                                                        let cancel_lc = lc.clone();
                                                                        let server_arc =
                                                                            server.clone();
                                                                        task.spawn(
                                                                            lc,
                                                                            session_key,
                                                                            current_session_id
                                                                                .clone()
                                                                                .unwrap_or_else(|| "unknown".to_string()),
                                                                            server_arc,
                                                                            progress_tx,
                                                                        );

                                                                        // Drain progress until the generation task completes.
                                                                        // Breaking this loop drops progress_rx, which is what the
                                                                        // spawn's disconnect gates detect — so a failed WS write and
                                                                        // a read error must BREAK (not be swallowed), or the spawn
                                                                        // would bill a clip whose ltx_complete provably cannot land.
                                                                        loop {
                                                                            tokio::select! {
                                                                                msg = progress_rx.recv() => {
                                                                                    match msg {
                                                                                        Some(m) => {
                                                                                            if ws_sender.send(axum::extract::ws::Message::Text(m.to_string())).await.is_err() {
                                                                                                info!("LTX progress write failed — client gone, dropping progress channel");
                                                                                                break;
                                                                                            }
                                                                                        }
                                                                                        None => break, // task finished (sender dropped)
                                                                                    }
                                                                                }
                                                                                ws_msg = ws_receiver.next() => {
                                                                                    match ws_msg {
                                                                                        Some(Ok(axum::extract::ws::Message::Text(txt))) => {
                                                                                            if let Ok(cancel_json) = serde_json::from_str::<serde_json::Value>(&txt) {
                                                                                                if cancel_json.get("type").and_then(|v| v.as_str()) == Some("ltx_cancel") {
                                                                                                    info!("LTX cancel received");
                                                                                                    let _ = cancel_lc.interrupt().await;
                                                                                                    break;
                                                                                                }
                                                                                            }
                                                                                        }
                                                                                        Some(Ok(axum::extract::ws::Message::Close(_))) | Some(Err(_)) | None => break,
                                                                                        _ => {}
                                                                                    }
                                                                                }
                                                                                else => break,
                                                                            }
                                                                        }
                                                                    }
                                                                    continue;
                                                                }

                                                                // Extract prompt from decrypted JSON or use entire string
                                                                let plaintext_prompt =
                                                                    decrypted_json
                                                                        .get("prompt")
                                                                        .and_then(|v| v.as_str())
                                                                        .unwrap_or(&plaintext_str)
                                                                        .to_string();

                                                                // Vision pre-processing: route images to VLM sidecar (v8.15.3+)
                                                                let mut vlm_tokens_used: u64 = 0;
                                                                let plaintext_prompt =
                                                                    match decrypted_json
                                                                        .get("images")
                                                                        .and_then(|v| v.as_array())
                                                                    {
                                                                        Some(imgs)
                                                                            if !imgs.is_empty() =>
                                                                        {
                                                                            info!("Found {} image(s) in encrypted message, routing to VLM sidecar", imgs.len());
                                                                            let (augmented, vlm_tokens) = process_vision_images(&server, imgs, &plaintext_prompt).await;
                                                                            vlm_tokens_used =
                                                                                vlm_tokens;
                                                                            // Track VLM tokens for billing (v8.15.4+)
                                                                            if vlm_tokens > 0 {
                                                                                if let Some(jid) =
                                                                                    job_id
                                                                                {
                                                                                    info!("📊 VLM vision used {} tokens for job {}", vlm_tokens, jid);
                                                                                    if let Some(cm) = server.checkpoint_manager.read().await.as_ref() {
                                                                                    let _ = cm.track_tokens(jid, vlm_tokens, current_session_id.clone()).await;
                                                                                } else {
                                                                                    server.token_tracker.track_tokens(Some(jid), vlm_tokens as usize, current_session_id.clone()).await;
                                                                                }
                                                                                }
                                                                            }
                                                                            augmented
                                                                        }
                                                                        _ => plaintext_prompt,
                                                                    };

                                                                // Auto-route image intent to diffusion sidecar (v8.16.1+)
                                                                if server.auto_image_routing
                                                                    && server
                                                                        .get_diffusion_client()
                                                                        .await
                                                                        .is_some()
                                                                {
                                                                    let last_user_msg = crate::search::query_extractor::extract_last_user_query(&plaintext_prompt);
                                                                    if crate::search::query_extractor::needs_image_generation(&last_user_msg) {
                                                                    info!("🎨 Auto-routing prompt to image generation (intent detected)");
                                                                    let auto_json = serde_json::json!({
                                                                        "action": "image_generation",
                                                                        "prompt": last_user_msg,
                                                                    });
                                                                    let response_msg = crate::api::websocket::handlers::image_generation::handle_encrypted_image_generation(
                                                                        &server,
                                                                        &auto_json,
                                                                        &session_key,
                                                                        current_session_id.as_deref().unwrap_or("unknown"),
                                                                        job_id,
                                                                        json_msg.get("id"),
                                                                    ).await;
                                                                    let _ = ws_sender.send(axum::extract::ws::Message::Text(response_msg.to_string())).await;
                                                                    continue;
                                                                    }
                                                                }

                                                                // Extract model (priority: decrypted > outer message > default)
                                                                let model = decrypted_json
                                                                    .get("model")
                                                                    .and_then(|v| v.as_str())
                                                                    .or_else(|| {
                                                                        json_msg
                                                                            .get("model")
                                                                            .and_then(|v| {
                                                                                v.as_str()
                                                                            })
                                                                    })
                                                                    .unwrap_or("tiny-vicuna")
                                                                    .to_string();

                                                                // Extract max_tokens (priority: decrypted > outer message > default)
                                                                let max_tokens = decrypted_json
                                                                    .get("max_tokens")
                                                                    .and_then(|v| v.as_u64())
                                                                    .or_else(|| {
                                                                        json_msg
                                                                            .get("max_tokens")
                                                                            .and_then(|v| {
                                                                                v.as_u64()
                                                                            })
                                                                    })
                                                                    .unwrap_or(4000); // Increased default to 4000

                                                                // Extract temperature (priority: decrypted > outer message > default)
                                                                let temperature = decrypted_json
                                                                    .get("temperature")
                                                                    .and_then(|v| v.as_f64())
                                                                    .or_else(|| {
                                                                        json_msg
                                                                            .get("temperature")
                                                                            .and_then(|v| {
                                                                                v.as_f64()
                                                                            })
                                                                    })
                                                                    .unwrap_or(0.7);

                                                                // Extract stream (priority: decrypted > outer message > default)
                                                                let stream = decrypted_json
                                                                    .get("stream")
                                                                    .and_then(|v| v.as_bool())
                                                                    .or_else(|| {
                                                                        json_msg
                                                                            .get("stream")
                                                                            .and_then(|v| {
                                                                                v.as_bool()
                                                                            })
                                                                    })
                                                                    .unwrap_or(true);

                                                                // Extract web_search fields from outer message (v8.7.9+)
                                                                // SDK sends these at message level, not inside encrypted payload
                                                                let web_search = decrypted_json
                                                                    .get("web_search")
                                                                    .and_then(|v| v.as_bool())
                                                                    .or_else(|| {
                                                                        json_msg
                                                                            .get("web_search")
                                                                            .and_then(|v| {
                                                                                v.as_bool()
                                                                            })
                                                                    })
                                                                    .unwrap_or(false);

                                                                let max_searches = decrypted_json
                                                                    .get("max_searches")
                                                                    .and_then(|v| v.as_i64())
                                                                    .or_else(|| {
                                                                        json_msg
                                                                            .get("max_searches")
                                                                            .and_then(|v| {
                                                                                v.as_i64()
                                                                            })
                                                                    })
                                                                    .unwrap_or(5);

                                                                let search_queries: Option<
                                                                    Vec<String>,
                                                                > = decrypted_json
                                                                    .get("search_queries")
                                                                    .or_else(|| {
                                                                        json_msg
                                                                            .get("search_queries")
                                                                    })
                                                                    .and_then(|v| {
                                                                        serde_json::from_value(
                                                                            v.clone(),
                                                                        )
                                                                        .ok()
                                                                    });

                                                                // Extract thinking mode (v8.17.0+)
                                                                let thinking: Option<String> =
                                                                    decrypted_json
                                                                        .get("thinking")
                                                                        .and_then(|v| v.as_str())
                                                                        .or_else(|| {
                                                                            json_msg
                                                                                .get("thinking")
                                                                                .and_then(|v| {
                                                                                    v.as_str()
                                                                                })
                                                                        })
                                                                        .map(|s| s.to_string());

                                                                // Log thinking mode for debugging
                                                                info!(
                                                                    "🧠 Thinking mode: {:?} (raw JSON value: {:?})",
                                                                    thinking,
                                                                    decrypted_json.get("thinking")
                                                                );

                                                                // Fetch session conversation history for proper multi-turn formatting (v8.22.5+)
                                                                let (
                                                                    effective_prompt,
                                                                    conversation_context_json,
                                                                ) = if let Some(ref sid) =
                                                                    current_session_id
                                                                {
                                                                    let store = server
                                                                        .session_store
                                                                        .read()
                                                                        .await;
                                                                    if let Some(session) =
                                                                        store.get_session(sid).await
                                                                    {
                                                                        let history = session
                                                                            .get_context_messages();
                                                                        if !history.is_empty() {
                                                                            let latest = extract_latest_user_message(&plaintext_prompt, &history);
                                                                            info!(
                                                                                "📝 Multi-turn: extracted latest user message ({} chars) from prompt ({} chars), {} history messages",
                                                                                latest.len(), plaintext_prompt.len(), history.len()
                                                                            );
                                                                            (latest, serde_json::to_value(&history).unwrap_or(json!([])))
                                                                        } else {
                                                                            (
                                                                                plaintext_prompt
                                                                                    .clone(),
                                                                                json!([]),
                                                                            )
                                                                        }
                                                                    } else {
                                                                        (
                                                                            plaintext_prompt
                                                                                .clone(),
                                                                            json!([]),
                                                                        )
                                                                    }
                                                                } else {
                                                                    (
                                                                        plaintext_prompt.clone(),
                                                                        json!([]),
                                                                    )
                                                                };

                                                                let mut request_value = json!({
                                                                    "model": model,
                                                                    "prompt": effective_prompt,
                                                                    "job_id": job_id,
                                                                    "session_id": current_session_id,
                                                                    "max_tokens": max_tokens,
                                                                    "temperature": temperature,
                                                                    "stream": stream,
                                                                    "web_search": web_search,
                                                                    "max_searches": max_searches,
                                                                    "thinking": thinking,
                                                                    "conversation_context": conversation_context_json
                                                                });

                                                                // Add search_queries if present
                                                                if let Some(queries) =
                                                                    search_queries
                                                                {
                                                                    request_value
                                                                        ["search_queries"] =
                                                                        json!(queries);
                                                                }

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
                                                                    // Reset and clone cancel flag for this inference
                                                                    let cancel_flag = if let Some(
                                                                        ref sid,
                                                                    ) =
                                                                        current_session_id
                                                                    {
                                                                        let store = server
                                                                            .session_store
                                                                            .read()
                                                                            .await;
                                                                        if let Some(session) = store
                                                                            .get_session(sid)
                                                                            .await
                                                                        {
                                                                            session.inference_cancel_flag.store(false, std::sync::atomic::Ordering::Release);
                                                                            Some(session.inference_cancel_flag.clone())
                                                                        } else {
                                                                            None
                                                                        }
                                                                    } else {
                                                                        None
                                                                    };

                                                                    // Handle streaming inference
                                                                    match server
                                                                        .handle_streaming_request(
                                                                            request,
                                                                            "ws-client".to_string(),
                                                                            cancel_flag.clone(),
                                                                        )
                                                                        .await
                                                                    {
                                                                        Ok((
                                                                            mut receiver,
                                                                            mut result_rx,
                                                                        )) => {
                                                                            let mut total_tokens =
                                                                                0u64;
                                                                            let mut
                                                                            accumulated_text =
                                                                                String::new();

                                                                            let mut chunk_index =
                                                                                0u32;

                                                                            loop {
                                                                                let response = tokio::select! {
                                                                                    resp = receiver.recv() => {
                                                                                        match resp {
                                                                                            Some(r) => r,
                                                                                            None => break, // channel closed
                                                                                        }
                                                                                    }
                                                                                    ws_msg = ws_receiver.next() => {
                                                                                        match ws_msg {
                                                                                            Some(Ok(axum::extract::ws::Message::Text(text))) => {
                                                                                                if let Ok(cj) = serde_json::from_str::<serde_json::Value>(&text) {
                                                                                                    if cj["type"] == "stream_cancel" {
                                                                                                        if let Some(ref flag) = cancel_flag {
                                                                                                            flag.store(true, std::sync::atomic::Ordering::Release);
                                                                                                        }
                                                                                                        info!("🛑 stream_cancel during encrypted streaming");
                                                                                                        let mut end_msg = json!({"type": "stream_end", "reason": "cancelled", "tokens_used": total_tokens, "finish_reason": "cancelled"});
                                                                                                        if let Some(ref msg_id) = message_id { end_msg["id"] = msg_id.clone(); }
                                                                                                        if let Some(ref sid) = current_session_id { end_msg["session_id"] = json!(sid); }
                                                                                                        let _ = ws_sender.send(axum::extract::ws::Message::Text(end_msg.to_string())).await;
                                                                                                        break;
                                                                                                    }
                                                                                                }
                                                                                                continue; // non-cancel message, keep streaming
                                                                                            }
                                                                                            Some(Ok(axum::extract::ws::Message::Close(_))) | None => break,
                                                                                            _ => continue,
                                                                                        }
                                                                                    }
                                                                                };
                                                                                {
                                                                                    // Count tokens for logging only - producer already tracks for checkpoints
                                                                                    if response
                                                                                        .tokens
                                                                                        > 0
                                                                                    {
                                                                                        total_tokens +=
                                                                                        response
                                                                                            .tokens
                                                                                            as u64;
                                                                                    }
                                                                                    // Accumulate text for session history (v8.22.5+)
                                                                                    accumulated_text.push_str(&response.content);

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
                                                                                        match ws_sender
                                                                                            .send(
                                                                                                axum::extract::ws::Message::Text(
                                                                                                    ws_msg.to_string(),
                                                                                                ),
                                                                                            )
                                                                                            .await
                                                                                        {
                                                                                            Ok(_) => {}
                                                                                            Err(e) => {
                                                                                                error!("Failed to send chunk {}: {}", chunk_index, e);
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
                                                                                                    match ws_sender
                                                                                                        .send(
                                                                                                            axum::extract::ws::Message::Text(
                                                                                                                end_msg.to_string(),
                                                                                                            ),
                                                                                                        )
                                                                                                        .await
                                                                                                    {
                                                                                                        Ok(_) => {
                                                                                                            // Send stream_end for SDK compatibility
                                                                                                            let mut stream_end_msg = json!({"type": "stream_end", "reason": "complete", "tokens_used": total_tokens});
                                                                                                            if let Some(ref msg_id) = message_id {
                                                                                                                stream_end_msg["id"] = msg_id.clone();
                                                                                                            }
                                                                                                            if let Some(ref sid) = current_session_id {
                                                                                                                stream_end_msg["session_id"] = json!(sid);
                                                                                                            }
                                                                                                            if vlm_tokens_used > 0 {
                                                                                                                stream_end_msg["vlm_tokens"] = json!(vlm_tokens_used);
                                                                                                            }
                                                                                                            // Add usage and finish_reason from inference result (v8.21.0)
                                                                                                            if let Ok(meta) = result_rx.try_recv() {
                                                                                                                stream_end_msg["finish_reason"] = json!(&meta.finish_reason);
                                                                                                                if let Some(ref cu) = meta.context_usage {
                                                                                                                    stream_end_msg["usage"] = json!({
                                                                                                                        "prompt_tokens": cu.prompt_tokens,
                                                                                                                        "completion_tokens": cu.completion_tokens,
                                                                                                                        "total_tokens": cu.total_tokens,
                                                                                                                        "context_window_size": cu.context_window_size
                                                                                                                    });
                                                                                                                }
                                                                                                            }
                                                                                                            let _ = ws_sender.send(axum::extract::ws::Message::Text(stream_end_msg.to_string())).await;
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

                                                                                        let _ = ws_sender
                                                                                            .send(axum::extract::ws::Message::Text(
                                                                                                error_msg.to_string(),
                                                                                            ))
                                                                                            .await;
                                                                                        // Send stream_end after error
                                                                                        let mut stream_end_msg = json!({"type": "stream_end", "reason": "error", "tokens_used": total_tokens});
                                                                                        if let Some(ref msg_id) = message_id {
                                                                                            stream_end_msg["id"] = msg_id.clone();
                                                                                        }
                                                                                        let _ = ws_sender.send(axum::extract::ws::Message::Text(stream_end_msg.to_string())).await;
                                                                                        break;
                                                                                    }
                                                                                }
                                                                                }
                                                                                // close tokio::select! response block
                                                                            } // close loop

                                                                            info!(
                                                                                "📊 Encrypted session complete - Total tokens: {}",
                                                                                total_tokens
                                                                            );

                                                                            // Store user prompt and assistant response in session history (v8.22.5+)
                                                                            if let Some(ref sid) =
                                                                                current_session_id
                                                                            {
                                                                                if !accumulated_text
                                                                                    .is_empty()
                                                                                {
                                                                                    let mut store = server.session_store.write().await;
                                                                                    let _ = store.update_session(sid, crate::job_processor::Message {
                                                                                        role: "user".to_string(),
                                                                                        content: effective_prompt.clone(),
                                                                                        timestamp: None,
                                                                                    }).await;
                                                                                    let _ = store.update_session(sid, crate::job_processor::Message {
                                                                                        role: "assistant".to_string(),
                                                                                        content: accumulated_text.clone(),
                                                                                        timestamp: None,
                                                                                    }).await;
                                                                                    info!("💾 Stored multi-turn context: user ({} chars) + assistant ({} chars)", effective_prompt.len(), accumulated_text.len());
                                                                                }
                                                                            }
                                                                        }
                                                                        Err(e) => {
                                                                            let error_str =
                                                                                e.to_string();
                                                                            let mut error_msg = if error_str.contains("exceeds context window") {
                                                                                // Parse token counts from error message for structured error
                                                                                let mut em = json!({
                                                                                    "type": "error",
                                                                                    "code": "TOKEN_LIMIT_EXCEEDED",
                                                                                    "message": error_str,
                                                                                });
                                                                                // Extract prompt_tokens and context_window_size from message pattern
                                                                                // "Prompt (N tokens) exceeds context window (M tokens) by K tokens"
                                                                                if let Some(start) = error_str.find("Prompt (") {
                                                                                    let after = &error_str[start + 8..];
                                                                                    if let Some(end) = after.find(" tokens)") {
                                                                                        if let Ok(pt) = after[..end].parse::<u64>() { em["prompt_tokens"] = json!(pt); }
                                                                                    }
                                                                                }
                                                                                if let Some(start) = error_str.find("context window (") {
                                                                                    let after = &error_str[start + 16..];
                                                                                    if let Some(end) = after.find(" tokens)") {
                                                                                        if let Ok(cw) = after[..end].parse::<u64>() { em["context_window_size"] = json!(cw); }
                                                                                    }
                                                                                }
                                                                                em
                                                                            } else {
                                                                                json!({
                                                                                    "type": "error",
                                                                                    "error": error_str
                                                                                })
                                                                            };

                                                                            if let Some(
                                                                                ref msg_id,
                                                                            ) = message_id
                                                                            {
                                                                                error_msg["id"] =
                                                                                    msg_id.clone();
                                                                            }

                                                                            let _ = ws_sender
                                                                                .send(
                                                                                    axum::extract::ws::Message::Text(
                                                                                        error_msg.to_string(),
                                                                                    ),
                                                                                )
                                                                                .await;

                                                                            // CRITICAL: Send stream_end even on error so SDK knows stream is done
                                                                            let mut stream_end_msg = json!({"type": "stream_end", "reason": "error", "tokens_used": 0});
                                                                            if let Some(
                                                                                ref msg_id,
                                                                            ) = message_id
                                                                            {
                                                                                stream_end_msg
                                                                                    ["id"] =
                                                                                    msg_id.clone();
                                                                            }
                                                                            let _ = ws_sender.send(axum::extract::ws::Message::Text(stream_end_msg.to_string())).await;
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

                                                                let _ = ws_sender
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

                                                        let _ = ws_sender
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

                                                let _ = ws_sender
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

                                        let _ = ws_sender
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

                                    let _ = ws_sender
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

                                let _ = ws_sender
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

                            let _ = ws_sender
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
                                    "stream": json_msg["stream"].as_bool().unwrap_or(true),
                                    "thinking": json_msg["thinking"].as_str()
                                })
                            }
                        } else {
                            // For inference messages, use the nested request object
                            json_msg["request"].clone()
                        };

                        // Vision pre-processing for plaintext messages (v8.15.3+)
                        let mut vlm_tokens_used: u64 = 0;
                        let request_value = if let Some(imgs) =
                            json_msg.get("images").and_then(|v| v.as_array())
                        {
                            if !imgs.is_empty() {
                                info!("Found {} image(s) in plaintext message, routing to VLM sidecar", imgs.len());
                                let prompt = request_value
                                    .get("prompt")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                let (augmented, vlm_tokens) =
                                    process_vision_images(&server, imgs, prompt).await;
                                vlm_tokens_used = vlm_tokens;
                                // Track VLM tokens for billing (v8.15.4+)
                                if vlm_tokens > 0 {
                                    if let Some(jid) = job_id {
                                        info!(
                                            "📊 VLM vision used {} tokens for job {}",
                                            vlm_tokens, jid
                                        );
                                        if let Some(cm) =
                                            server.checkpoint_manager.read().await.as_ref()
                                        {
                                            let _ = cm
                                                .track_tokens(jid, vlm_tokens, session_id.clone())
                                                .await;
                                        } else {
                                            server
                                                .token_tracker
                                                .track_tokens(
                                                    Some(jid),
                                                    vlm_tokens as usize,
                                                    session_id.clone(),
                                                )
                                                .await;
                                        }
                                    }
                                }
                                let mut rv = request_value;
                                rv["prompt"] = serde_json::Value::String(augmented);
                                rv
                            } else {
                                request_value
                            }
                        } else {
                            request_value
                        };

                        // Debug: Log the entire request
                        info!(
                            "🔍 WebSocket inference request received: {:?}",
                            request_value
                        );

                        if let Ok(request) =
                            serde_json::from_value::<InferenceRequest>(request_value)
                        {
                            // Update tracked job_id if not already set
                            if let Some(req_job_id) = request.job_id {
                                if job_id.is_none() {
                                    job_id = Some(req_job_id);
                                }
                            }

                            // Reset and clone cancel flag for this inference
                            let cancel_flag = if let Some(ref sid) = session_id {
                                let store = server.session_store.read().await;
                                if let Some(session) = store.get_session(sid).await {
                                    session
                                        .inference_cancel_flag
                                        .store(false, std::sync::atomic::Ordering::Release);
                                    Some(session.inference_cancel_flag.clone())
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            // Handle streaming inference
                            match server
                                .handle_streaming_request(
                                    request,
                                    "ws-client".to_string(),
                                    cancel_flag.clone(),
                                )
                                .await
                            {
                                Ok((mut receiver, mut result_rx)) => {
                                    let mut total_tokens = 0u64;

                                    loop {
                                        let response = tokio::select! {
                                            resp = receiver.recv() => {
                                                match resp {
                                                    Some(r) => r,
                                                    None => break,
                                                }
                                            }
                                            ws_msg = ws_receiver.next() => {
                                                match ws_msg {
                                                    Some(Ok(axum::extract::ws::Message::Text(text))) => {
                                                        if let Ok(cj) = serde_json::from_str::<serde_json::Value>(&text) {
                                                            if cj["type"] == "stream_cancel" {
                                                                if let Some(ref flag) = cancel_flag {
                                                                    flag.store(true, std::sync::atomic::Ordering::Release);
                                                                }
                                                                info!("🛑 stream_cancel during plaintext streaming");
                                                                let mut end_msg = json!({"type": "stream_end", "reason": "cancelled", "tokens_used": total_tokens, "finish_reason": "cancelled"});
                                                                if let Some(ref sid) = session_id { end_msg["session_id"] = json!(sid); }
                                                                let _ = ws_sender.send(axum::extract::ws::Message::Text(end_msg.to_string())).await;
                                                                break;
                                                            }
                                                        }
                                                        continue;
                                                    }
                                                    Some(Ok(axum::extract::ws::Message::Close(_))) | None => break,
                                                    _ => continue,
                                                }
                                            }
                                        };
                                        {
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

                                            if ws_sender
                                                .send(axum::extract::ws::Message::Text(
                                                    ws_msg.to_string(),
                                                ))
                                                .await
                                                .is_err()
                                            {
                                                break;
                                            }

                                            if response.finish_reason.is_some() {
                                                let mut end_msg = json!({"type": "stream_end", "reason": "complete", "tokens_used": total_tokens});

                                                // Include message ID in end message too
                                                if let Some(ref msg_id) = message_id {
                                                    end_msg["id"] = msg_id.clone();
                                                }
                                                if vlm_tokens_used > 0 {
                                                    end_msg["vlm_tokens"] = json!(vlm_tokens_used);
                                                }
                                                // Add usage and finish_reason from inference result (v8.21.0)
                                                if let Ok(meta) = result_rx.try_recv() {
                                                    end_msg["finish_reason"] =
                                                        json!(&meta.finish_reason);
                                                    if let Some(ref cu) = meta.context_usage {
                                                        end_msg["usage"] = json!({
                                                            "prompt_tokens": cu.prompt_tokens,
                                                            "completion_tokens": cu.completion_tokens,
                                                            "total_tokens": cu.total_tokens,
                                                            "context_window_size": cu.context_window_size
                                                        });
                                                    }
                                                }

                                                let _ = ws_sender
                                                    .send(axum::extract::ws::Message::Text(
                                                        end_msg.to_string(),
                                                    ))
                                                    .await;
                                                break;
                                            }
                                        } // close tokio::select! response block
                                    } // close loop

                                    // Log total tokens tracked for this session
                                    if total_tokens > 0 {
                                        info!("📊 WebSocket session complete - Total tokens tracked for job {:?}: {}",
                                              job_id, total_tokens);
                                    }
                                }
                                Err(e) => {
                                    let error_str = e.to_string();
                                    let mut error_msg = if error_str
                                        .contains("exceeds context window")
                                    {
                                        let mut em = json!({
                                            "type": "error",
                                            "code": "TOKEN_LIMIT_EXCEEDED",
                                            "message": error_str,
                                        });
                                        if let Some(start) = error_str.find("Prompt (") {
                                            let after = &error_str[start + 8..];
                                            if let Some(end) = after.find(" tokens)") {
                                                if let Ok(pt) = after[..end].parse::<u64>() {
                                                    em["prompt_tokens"] = json!(pt);
                                                }
                                            }
                                        }
                                        if let Some(start) = error_str.find("context window (") {
                                            let after = &error_str[start + 16..];
                                            if let Some(end) = after.find(" tokens)") {
                                                if let Ok(cw) = after[..end].parse::<u64>() {
                                                    em["context_window_size"] = json!(cw);
                                                }
                                            }
                                        }
                                        em
                                    } else {
                                        json!({
                                            "type": "error",
                                            "error": error_str
                                        })
                                    };

                                    // Include message ID in error message
                                    if let Some(ref msg_id) = message_id {
                                        error_msg["id"] = msg_id.clone();
                                    }

                                    let _ = ws_sender
                                        .send(axum::extract::ws::Message::Text(
                                            error_msg.to_string(),
                                        ))
                                        .await;

                                    // CRITICAL: Send stream_end even on error so SDK knows stream is done
                                    let mut stream_end_msg = json!({"type": "stream_end", "reason": "error", "tokens_used": 0});
                                    if let Some(ref msg_id) = message_id {
                                        stream_end_msg["id"] = msg_id.clone();
                                    }
                                    let _ = ws_sender
                                        .send(axum::extract::ws::Message::Text(
                                            stream_end_msg.to_string(),
                                        ))
                                        .await;
                                }
                            }
                        }
                    }

                    // Handle RAG uploadVectors message (Phase 3.4)
                    if json_msg["type"] == "uploadVectors" {
                        info!(
                            "📤 uploadVectors message received, WS session_id={:?}",
                            session_id
                        );

                        match serde_json::from_value::<
                            crate::api::websocket::message_types::UploadVectorsRequest,
                        >(json_msg.clone())
                        {
                            Ok(request) => {
                                // Get or create session with RAG enabled
                                let sid = session_id
                                    .clone()
                                    .unwrap_or_else(|| "default-rag-session".to_string());
                                info!(
                                    "📤 uploadVectors using session: {} (from WS session_id={:?})",
                                    sid, session_id
                                );

                                // Use the new helper method
                                let rag_session = {
                                    let mut store = server.session_store.write().await;
                                    match store
                                        .get_or_create_rag_session(sid.clone(), 100_000)
                                        .await
                                    {
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
                                            if ws_sender
                                                .send(axum::extract::ws::Message::Text(
                                                    error_msg.to_string(),
                                                ))
                                                .await
                                                .is_err()
                                            {
                                                break;
                                            }
                                            continue;
                                        }
                                    }
                                };

                                let rag_session_arc = Arc::new(std::sync::Mutex::new(rag_session));

                                // Call the RAG handler
                                match crate::api::websocket::handlers::rag::handle_upload_vectors(
                                    &rag_session_arc,
                                    request,
                                ) {
                                    Ok(response) => {
                                        match serde_json::to_string(&response) {
                                            Ok(response_json) => {
                                                info!("✅ uploadVectors response: {} uploaded, {} rejected",
                                                      response.uploaded, response.rejected);
                                                if ws_sender
                                                    .send(axum::extract::ws::Message::Text(
                                                        response_json,
                                                    ))
                                                    .await
                                                    .is_err()
                                                {
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
                                        if ws_sender
                                            .send(axum::extract::ws::Message::Text(
                                                error_msg.to_string(),
                                            ))
                                            .await
                                            .is_err()
                                        {
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
                                if ws_sender
                                    .send(axum::extract::ws::Message::Text(error_msg.to_string()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }

                    // Handle RAG searchVectors message (Phase 3.4)
                    if json_msg["type"] == "searchVectors" {
                        info!(
                            "🔍 searchVectors message received, WS session_id={:?}",
                            session_id
                        );

                        match serde_json::from_value::<
                            crate::api::websocket::message_types::SearchVectorsRequest,
                        >(json_msg.clone())
                        {
                            Ok(request) => {
                                // Get existing session with RAG (should already exist from uploadVectors)
                                let sid = session_id
                                    .clone()
                                    .unwrap_or_else(|| "default-rag-session".to_string());
                                info!(
                                    "🔍 searchVectors using session: {} (from WS session_id={:?})",
                                    sid, session_id
                                );

                                // Get session from store
                                let rag_session = {
                                    let store = server.session_store.read().await;
                                    match store.get_session(&sid).await {
                                        Some(sess) => {
                                            info!("✅ Found session for search: {}", sid);
                                            if sess.get_vector_store().is_none() {
                                                warn!(
                                                    "⚠️  Session {} exists but RAG not enabled!",
                                                    sid
                                                );
                                                let error_msg = json!({
                                                    "type": "error",
                                                    "error": format!("Session {} found but RAG not enabled. Upload vectors first.", sid)
                                                });
                                                if ws_sender
                                                    .send(axum::extract::ws::Message::Text(
                                                        error_msg.to_string(),
                                                    ))
                                                    .await
                                                    .is_err()
                                                {
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
                                            if ws_sender
                                                .send(axum::extract::ws::Message::Text(
                                                    error_msg.to_string(),
                                                ))
                                                .await
                                                .is_err()
                                            {
                                                break;
                                            }
                                            continue;
                                        }
                                    }
                                };

                                let rag_session_arc = Arc::new(std::sync::Mutex::new(rag_session));

                                // Call the RAG handler
                                match crate::api::websocket::handlers::rag::handle_search_vectors(
                                    &rag_session_arc,
                                    request,
                                ) {
                                    Ok(response) => {
                                        match serde_json::to_string(&response) {
                                            Ok(response_json) => {
                                                info!("✅ searchVectors response: {} results in {:.2}ms",
                                                      response.results.len(), response.search_time_ms);
                                                if ws_sender
                                                    .send(axum::extract::ws::Message::Text(
                                                        response_json,
                                                    ))
                                                    .await
                                                    .is_err()
                                                {
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
                                        if ws_sender
                                            .send(axum::extract::ws::Message::Text(
                                                error_msg.to_string(),
                                            ))
                                            .await
                                            .is_err()
                                        {
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
                                if ws_sender
                                    .send(axum::extract::ws::Message::Text(error_msg.to_string()))
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            Ok(axum::extract::ws::Message::Ping(data)) => {
                if ws_sender
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
            info!(
                "🛑 Cancelled background vector loading task for session: {}",
                sid
            );
        }
    }

    if let Some(jid) = job_id {
        info!("\n🚨 WEBSOCKET DISCONNECTED - STARTING SETTLEMENT PROCESS");
        info!("   Job ID from WebSocket session: {}", jid);
        info!("   Session ID: {:?}", session_id);
        info!("   Chain ID: {:?}", chain_id);

        // M1 economics: an LTX proof in flight means completing NOW would
        // settle at 0 tokens under a rendering/submitting clip. Defer — the
        // finishing generation task (single-exit cleanup) owns completion.
        // LLM-only sessions have no LTX entry and never defer.
        if server.ltx_tracker().defer_completion(jid).await {
            info!(
                "[WS-BG] ⏸ LTX proof in flight for job {} — completion deferred to the \
                 generation task",
                jid
            );
            return;
        }

        // Get checkpoint manager and complete the session job
        let cm = server.checkpoint_manager.read().await;
        info!("   Checkpoint manager available: {}", cm.is_some());

        if let Some(checkpoint_manager) = cm.clone() {
            info!(
                "✅ Spawning complete_session_job in background for job_id: {}",
                jid
            );
            drop(cm); // Release lock before spawning

            // LTX: a proof landed shortly before this disconnect ⇒ the host
            // caller reverts "Dispute wait" until window (+ buffer) elapses
            // since that proof. Snapshot the remaining wait now; the spawned
            // task sleeps it out first. Zero for LLM-only sessions (the LLM
            // wait machinery lives inside complete_session_job).
            let window_secs = checkpoint_manager.dispute_window_secs()
                + crate::contracts::checkpoint_manager::DISPUTE_WINDOW_BUFFER_SECS;
            let ltx_wait = server
                .ltx_tracker()
                .proof_wait_remaining(jid, window_secs)
                .await;

            // ASYNC: Spawn session completion in background to avoid blocking
            let ltx_tracker = server.ltx_tracker.clone();
            tokio::spawn(async move {
                info!(
                    "[WS-BG] 🚀 Starting background session completion for job_id: {}",
                    jid
                );
                if !ltx_wait.is_zero() {
                    info!(
                        "[WS-BG] ⏳ LTX dispute window: waiting {}s before completing job {}",
                        ltx_wait.as_secs(),
                        jid
                    );
                    tokio::time::sleep(ltx_wait).await;
                }

                // Atomic pre-dispatch guard: if a clip was accepted (on a fast
                // RECONNECT) since the defer check — including mid-sleep — its
                // lifecycle owns completion; otherwise set the completing
                // latch so the accept path rejects new clips for the tx
                // duration. No-op effect on LLM-only sessions (nothing
                // consults the latch there).
                if !ltx_tracker.mark_completing_if_idle(jid).await {
                    info!(
                        "[WS-BG] ⏸ New LTX clip in flight for job {} — completion ownership \
                         moved to its lifecycle",
                        jid
                    );
                    return;
                }

                // Stringify errors: complete_session_job's Box<dyn Error> is
                // not Send across the retry-sleep await.
                match checkpoint_manager
                    .complete_session_job(jid)
                    .await
                    .map_err(|e| e.to_string())
                {
                    Ok(()) => {
                        info!(
                            "[WS-BG] 💰 Settlement completed successfully for job_id: {}",
                            jid
                        );
                    }
                    Err(e) => {
                        error!("[WS-BG] ❌ Failed to complete session job {}: {}", jid, e);
                        // Belt-and-braces for the LTX path (cm may itself retry
                        // internally): wait one window and retry ONCE.
                        warn!(
                            "[WS-BG] Retrying completion for job {} once after {}s",
                            jid, window_secs
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(window_secs)).await;
                        // Atomically re-latch for the retry; false = a newer
                        // clip owns completion now.
                        if !ltx_tracker.mark_completing_if_idle(jid).await {
                            info!(
                                "[WS-BG] ⏸ New LTX clip in flight for job {} — abandoning the \
                                 completion retry to its lifecycle",
                                jid
                            );
                            return;
                        }
                        if let Err(e2) = checkpoint_manager
                            .complete_session_job(jid)
                            .await
                            .map_err(|e| e.to_string())
                        {
                            error!("[WS-BG] ❌ Completion retry failed for job {}: {}", jid, e2);
                        }
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
