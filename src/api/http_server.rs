use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use axum::{
    extract::{Json, Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response, Sse},
    routing::{get, post},
    Router,
};
use futures::stream::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::HashMap, convert::Infallible, net::SocketAddr, sync::Arc};
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

use super::{
    ApiError, ApiServer, ChainInfo, ChainStatistics, ChainStatsResponse, ChainsResponse,
    InferenceRequest, InferenceResponse, ModelInfo, ModelsResponse, SessionInfo,
    SessionInfoResponse, SessionStatus, TotalStatistics,
};
use crate::blockchain::{ChainConfig, ChainRegistry};

#[derive(Deserialize)]
struct ChainQuery {
    chain_id: Option<u64>,
}

#[derive(Clone)]
pub struct AppState {
    pub api_server: Arc<ApiServer>,
    pub chain_registry: Arc<ChainRegistry>,
    pub sessions: Arc<RwLock<HashMap<u64, SessionInfo>>>,
    pub chain_stats: Arc<RwLock<HashMap<u64, ChainStatistics>>>,
}

impl AppState {
    pub fn new_for_test() -> Self {
        AppState {
            api_server: Arc::new(ApiServer::new_for_test()),
            chain_registry: Arc::new(ChainRegistry::new()),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            chain_stats: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

pub fn create_app(state: Arc<AppState>) -> Router {
    Router::new()
        // Health check
        .route("/health", get(health_handler))
        // Models endpoint with chain support
        .route("/v1/models", get(models_handler))
        // Chain endpoints
        .route("/v1/chains", get(chains_handler))
        .route("/v1/chains/stats", get(chain_stats_handler))
        .route(
            "/v1/chains/:chain_id/stats",
            get(chain_specific_stats_handler),
        )
        // Session endpoints
        .route("/v1/session/:session_id/info", get(session_info_handler))
        // Inference endpoint
        .route("/v1/inference", post(inference_handler))
        // WebSocket endpoint
        .route("/v1/ws", get(websocket_handler))
        // Metrics endpoint
        .route("/metrics", get(metrics_handler))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state((*state).clone())
}

pub async fn start_server(api_server: ApiServer) -> Result<(), Box<dyn std::error::Error>> {
    let state = Arc::new(AppState {
        api_server: Arc::new(api_server),
        chain_registry: Arc::new(ChainRegistry::new()),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        chain_stats: Arc::new(RwLock::new(HashMap::new())),
    });

    let app = create_app(state);

    let addr = "127.0.0.1:8080".parse::<SocketAddr>()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("API server listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let health = state.api_server.health_check().await;
    axum::response::Json(health)
}

async fn models_handler(
    State(state): State<AppState>,
    Query(query): Query<ChainQuery>,
) -> Result<axum::response::Json<ModelsResponse>, ApiErrorResponse> {
    let chain_id = query
        .chain_id
        .unwrap_or_else(|| state.chain_registry.get_default_chain_id());

    // Get chain info
    let chain = state
        .chain_registry
        .get_chain(chain_id)
        .ok_or(ApiError::InvalidRequest("Invalid chain ID".to_string()))?;

    // Get models (in real implementation, this would query chain-specific models)
    let mut response = state
        .api_server
        .get_available_models()
        .await
        .map_err(|e| ApiErrorResponse(e))?;

    // Add chain information to response
    response.chain_id = Some(chain_id);
    response.chain_name = Some(chain.name.clone());

    Ok(axum::response::Json(response))
}

async fn chains_handler(State(state): State<AppState>) -> impl IntoResponse {
    let chains: Vec<ChainInfo> = state
        .chain_registry
        .get_all_chains()
        .iter()
        .map(|config| ChainInfo {
            chain_id: config.chain_id,
            name: config.name.clone(),
            native_token: config.native_token.symbol.clone(),
            rpc_url: config.rpc_url.clone(),
            contracts: config.contracts.clone(),
        })
        .collect();

    let response = ChainsResponse {
        chains,
        default_chain: state.chain_registry.get_default_chain_id(),
    };

    axum::response::Json(response)
}

async fn session_info_handler(
    State(state): State<AppState>,
    Path(session_id): Path<u64>,
) -> Result<axum::response::Json<SessionInfoResponse>, ApiErrorResponse> {
    let sessions = state.sessions.read().await;
    let session = sessions
        .get(&session_id)
        .ok_or(ApiError::NotFound("Session not found".to_string()))?;

    let chain_id = session
        .chain_id
        .unwrap_or(state.chain_registry.get_default_chain_id());
    let chain = state
        .chain_registry
        .get_chain(chain_id)
        .ok_or(ApiError::InvalidRequest("Invalid chain ID".to_string()))?;

    let response = SessionInfoResponse {
        session_id,
        chain_id,
        chain_name: chain.name.clone(),
        native_token: chain.native_token.symbol.clone(),
        status: match session.status {
            SessionStatus::Active => "active".to_string(),
            SessionStatus::Completed => "completed".to_string(),
            SessionStatus::Failed => "failed".to_string(),
        },
        tokens_used: session.tokens_used,
    };

    Ok(axum::response::Json(response))
}

async fn chain_stats_handler(State(state): State<AppState>) -> impl IntoResponse {
    let stats = state.chain_stats.read().await;
    let chains: Vec<ChainStatistics> = stats.values().cloned().collect();

    // Calculate totals
    let total = TotalStatistics {
        total_sessions: chains.iter().map(|s| s.total_sessions).sum(),
        active_sessions: chains.iter().map(|s| s.active_sessions).sum(),
        total_tokens_processed: chains.iter().map(|s| s.total_tokens_processed).sum(),
    };

    let response = ChainStatsResponse { chains, total };

    axum::response::Json(response)
}

async fn chain_specific_stats_handler(
    State(state): State<AppState>,
    Path(chain_id): Path<u64>,
) -> Result<axum::response::Json<ChainStatistics>, ApiErrorResponse> {
    let stats = state.chain_stats.read().await;
    let chain_stats = stats
        .get(&chain_id)
        .ok_or(ApiError::NotFound("Chain statistics not found".to_string()))?;

    Ok(axum::response::Json(chain_stats.clone()))
}

async fn inference_handler(
    State(state): State<AppState>,
    Json(mut request): Json<InferenceRequest>,
) -> impl IntoResponse {
    let client_ip = "127.0.0.1".to_string(); // In production, extract from request

    // Use chain_id from request or default
    let chain_id = request
        .chain_id
        .unwrap_or(state.chain_registry.get_default_chain_id());

    // Validate chain exists
    if let Some(chain) = state.chain_registry.get_chain(chain_id) {
        // Add chain information to response when created
        request.chain_id = Some(chain_id);
    }

    if request.stream {
        // Streaming response
        match state
            .api_server
            .handle_streaming_request(request, client_ip)
            .await
        {
            Ok(receiver) => {
                let stream = tokio_stream::wrappers::ReceiverStream::new(receiver);
                let sse_stream = stream.map(|response| {
                    Ok::<_, Infallible>(
                        axum::response::sse::Event::default()
                            .data(serde_json::to_string(&response).unwrap_or_default()),
                    )
                });

                Sse::new(sse_stream).into_response()
            }
            Err(e) => ApiErrorResponse(e).into_response(),
        }
    } else {
        // Non-streaming response
        match state
            .api_server
            .handle_inference_request(request.clone(), client_ip)
            .await
        {
            Ok(mut response) => {
                // Add chain information to response
                if let Some(chain_id) = request.chain_id {
                    if let Some(chain) = state.chain_registry.get_chain(chain_id) {
                        response.chain_id = Some(chain_id);
                        response.chain_name = Some(chain.name.clone());
                        response.native_token = Some(chain.native_token.symbol.clone());
                    }
                }
                axum::response::Json(response).into_response()
            }
            Err(e) => ApiErrorResponse(e).into_response(),
        }
    }
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_websocket(socket, state))
}

async fn handle_websocket(mut socket: WebSocket, state: AppState) {
    let mut current_job_id: Option<u64> = None;
    let mut current_chain_id: Option<u64> = None;

    while let Some(msg) = socket.recv().await {
        match msg {
            Ok(axum::extract::ws::Message::Text(text)) => {
                // Parse WebSocket message
                if let Ok(json_msg) = serde_json::from_str::<serde_json::Value>(&text) {
                    if json_msg["type"] == "inference" {
                        // Debug: Log the entire request
                        tracing::info!(
                            "🔍 WebSocket inference request received: {:?}",
                            json_msg["request"]
                        );

                        if let Ok(mut request) =
                            serde_json::from_value::<InferenceRequest>(json_msg["request"].clone())
                        {
                            eprintln!(
                                "🔍 RAW REQUEST - job_id: {:?}, session_id: {:?}, chain_id: {:?}",
                                request.job_id, request.session_id, request.chain_id
                            );

                            // Track chain_id
                            current_chain_id = request
                                .chain_id
                                .or(Some(state.chain_registry.get_default_chain_id()));

                            // If job_id is not provided but session_id is, try to parse session_id as job_id
                            if request.job_id.is_none() && request.session_id.is_some() {
                                if let Some(ref sid) = request.session_id {
                                    // Try to parse session_id as a number (SDK sends it as "139n" or just "139")
                                    if let Ok(parsed_id) = sid.trim_end_matches('n').parse::<u64>()
                                    {
                                        request.job_id = Some(parsed_id);
                                        current_job_id = Some(parsed_id); // Track current job ID
                                        eprintln!(
                                            "📋 CONVERTED session_id {} to job_id {}",
                                            sid, parsed_id
                                        );
                                        tracing::info!("📋 Using session_id {} as job_id for checkpoint tracking", parsed_id);

                                        // Create session tracking
                                        if let Some(chain_id) = current_chain_id {
                                            let mut sessions = state.sessions.write().await;
                                            sessions.insert(
                                                parsed_id,
                                                SessionInfo {
                                                    job_id: parsed_id,
                                                    chain_id: Some(chain_id),
                                                    user_address: "unknown".to_string(), // Would be from auth
                                                    start_time: chrono::Utc::now(),
                                                    tokens_used: 0,
                                                    status: SessionStatus::Active,
                                                },
                                            );
                                        }
                                    } else {
                                        eprintln!(
                                            "❌ FAILED to parse session_id '{}' as number",
                                            sid
                                        );
                                    }
                                }
                            } else if let Some(jid) = request.job_id {
                                current_job_id = Some(jid); // Track current job ID
                            }

                            // Log job_id for payment tracking visibility
                            if let Some(job_id) = request.job_id {
                                tracing::info!("📋 Processing inference request for blockchain job_id: {} on chain: {}",
                                    job_id, current_chain_id.unwrap_or(0));
                            } else {
                                tracing::info!("⚠️  No job_id or session_id in WebSocket request");
                            }

                            // Handle streaming inference
                            match state
                                .api_server
                                .handle_streaming_request(request, "ws-client".to_string())
                                .await
                            {
                                Ok(mut receiver) => {
                                    // Get chain info for formatting
                                    let (chain_name, native_token) =
                                        if let Some(chain_id) = current_chain_id {
                                            if let Some(chain) =
                                                state.chain_registry.get_chain(chain_id)
                                            {
                                                (
                                                    Some(chain.name.clone()),
                                                    Some(chain.native_token.symbol.clone()),
                                                )
                                            } else {
                                                (None, None)
                                            }
                                        } else {
                                            (None, None)
                                        };

                                    while let Some(response) = receiver.recv().await {
                                        let ws_msg = json!({
                                            "type": "stream_chunk",
                                            "content": response.content,
                                            "tokens": response.tokens,
                                            "chain_id": current_chain_id,
                                            "chain_name": chain_name.clone(),
                                            "native_token": native_token.clone(),
                                        });

                                        // Update session tokens
                                        if let Some(job_id) = current_job_id {
                                            let mut sessions = state.sessions.write().await;
                                            if let Some(session) = sessions.get_mut(&job_id) {
                                                session.tokens_used += response.tokens as u64;
                                            }
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
                                            let end_msg = json!({"type": "stream_end"});
                                            let _ = socket
                                                .send(axum::extract::ws::Message::Text(
                                                    end_msg.to_string(),
                                                ))
                                                .await;
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    let error_msg = json!({
                                        "type": "error",
                                        "error": e.to_string()
                                    });
                                    let _ = socket
                                        .send(axum::extract::ws::Message::Text(
                                            error_msg.to_string(),
                                        ))
                                        .await;
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
            Ok(axum::extract::ws::Message::Close(_)) => {
                // Trigger payment settlement before closing
                if let Some(job_id) = current_job_id {
                    tracing::info!("💰 WebSocket closing - triggering payment settlement for job {} on chain {}",
                        job_id, current_chain_id.unwrap_or(0));

                    // Update session status
                    {
                        let mut sessions = state.sessions.write().await;
                        if let Some(session) = sessions.get_mut(&job_id) {
                            session.status = SessionStatus::Completed;
                        }
                    }

                    // Get checkpoint manager and complete the session
                    tracing::info!("[HTTP-WS] 💰 === Session End Detected - Initiating Payment Settlement ===");
                    tracing::info!("[HTTP-WS] Job ID: {}, Chain: {}", job_id, current_chain_id.unwrap_or(0));

                    if let Some(checkpoint_manager) =
                        state.api_server.get_checkpoint_manager().await
                    {
                        tracing::info!("[HTTP-WS] ✓ Checkpoint manager available, calling complete_session_job...");
                        if let Err(e) = checkpoint_manager.complete_session_job(job_id).await {
                            tracing::error!(
                                "[HTTP-WS] ❌ Failed to complete session job {}: {:?}",
                                job_id,
                                e
                            );
                            tracing::error!("[HTTP-WS] ⚠️ Payment settlement may not have occurred!");
                        } else {
                            tracing::info!(
                                "[HTTP-WS] ✅ Session job {} completed successfully",
                                job_id
                            );
                            tracing::info!("[HTTP-WS] 💸 Payments should now be distributed to:");
                            tracing::info!("[HTTP-WS]   - Host earnings (90%)");
                            tracing::info!("[HTTP-WS]   - Treasury fee (10%)");
                            tracing::info!("[HTTP-WS]   - User refund (unused tokens)");
                        }
                    } else {
                        tracing::error!(
                            "[HTTP-WS] ⚠️ NO CHECKPOINT MANAGER AVAILABLE!"
                        );
                        tracing::error!(
                            "[HTTP-WS] ⚠️ Cannot complete session job {} - PAYMENTS WILL NOT BE SETTLED!",
                            job_id
                        );
                    }
                }
                break;
            }
            _ => {}
        }
    }

    // Also trigger payment settlement when connection drops unexpectedly
    if let Some(job_id) = current_job_id {
        tracing::info!(
            "[HTTP-WS-DISCONNECT] 🔌 === WebSocket Disconnected Unexpectedly ==="
        );
        tracing::info!(
            "[HTTP-WS-DISCONNECT] 💰 Triggering emergency payment settlement for job {} on chain {}",
            job_id,
            current_chain_id.unwrap_or(0)
        );

        // Update session status
        {
            let mut sessions = state.sessions.write().await;
            if let Some(session) = sessions.get_mut(&job_id) {
                session.status = SessionStatus::Failed;
                tracing::info!("[HTTP-WS-DISCONNECT] Session status updated to Failed");
            }
        }

        if let Some(checkpoint_manager) = state.api_server.get_checkpoint_manager().await {
            tracing::info!("[HTTP-WS-DISCONNECT] ✓ Checkpoint manager available, initiating settlement...");
            if let Err(e) = checkpoint_manager.complete_session_job(job_id).await {
                tracing::error!(
                    "[HTTP-WS-DISCONNECT] ❌ CRITICAL: Failed to complete session job {} on disconnect!",
                    job_id
                );
                tracing::error!(
                    "[HTTP-WS-DISCONNECT]   Error: {:?}",
                    e
                );
                tracing::error!(
                    "[HTTP-WS-DISCONNECT] ⚠️ PAYMENTS NOT SETTLED - Manual intervention may be required!"
                );
            } else {
                tracing::info!(
                    "[HTTP-WS-DISCONNECT] ✅ Session job {} completed on disconnect",
                    job_id
                );
                tracing::info!(
                    "[HTTP-WS-DISCONNECT] 💸 Payments should be distributed despite unexpected disconnect"
                );
            }
        } else {
            tracing::error!(
                "[HTTP-WS-DISCONNECT] ⚠️ CRITICAL: No checkpoint manager available!"
            );
            tracing::error!(
                "[HTTP-WS-DISCONNECT] ⚠️ Session {} cannot be settled - payments stuck!",
                job_id
            );
        }
    } else {
        tracing::info!("[HTTP-WS-DISCONNECT] No active job to settle on disconnect");
    }
}

async fn metrics_handler() -> impl IntoResponse {
    // Simple Prometheus-style metrics
    let metrics = format!(
        "# HELP http_requests_total Total number of HTTP requests\n\
         # TYPE http_requests_total counter\n\
         http_requests_total 0\n\
         # HELP http_request_duration_seconds HTTP request latency\n\
         # TYPE http_request_duration_seconds histogram\n\
         http_request_duration_seconds_bucket{{le=\"0.1\"}} 0\n"
    );

    Response::builder()
        .header(header::CONTENT_TYPE, "text/plain; version=0.0.4")
        .body(metrics)
        .unwrap()
}

// Error response wrapper
struct ApiErrorResponse(ApiError);

impl From<ApiError> for ApiErrorResponse {
    fn from(error: ApiError) -> Self {
        ApiErrorResponse(error)
    }
}

impl IntoResponse for ApiErrorResponse {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let error_response = self.0.to_response(None);

        (status, axum::response::Json(error_response)).into_response()
    }
}
