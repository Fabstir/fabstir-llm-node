use axum::{
    extract::{State, Json},
    http::{StatusCode, header},
    response::{IntoResponse, Response, Sse},
    routing::{get, post},
    Router,
};
use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use futures::stream::StreamExt;
use serde_json::json;
use std::{
    convert::Infallible,
    net::SocketAddr,
    sync::Arc,
};
use tower_http::cors::{CorsLayer, Any};

use super::{
    ApiServer, ApiError,
    InferenceRequest,
    ModelsResponse,
};

#[derive(Clone)]
struct AppState {
    api_server: Arc<ApiServer>,
}

pub async fn start_server(api_server: ApiServer) -> Result<(), Box<dyn std::error::Error>> {
    let state = AppState {
        api_server: Arc::new(api_server),
    };
    
    let app = Router::new()
        // Health check
        .route("/health", get(health_handler))
        // Models endpoint
        .route("/v1/models", get(models_handler))
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
                .allow_headers(Any)
        )
        .with_state(state);

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

async fn models_handler(State(state): State<AppState>) -> Result<axum::response::Json<ModelsResponse>, ApiErrorResponse> {
    state.api_server.get_available_models().await
        .map(axum::response::Json)
        .map_err(|e| ApiErrorResponse(e))
}

async fn inference_handler(
    State(state): State<AppState>,
    Json(request): Json<InferenceRequest>,
) -> impl IntoResponse {
    let client_ip = "127.0.0.1".to_string(); // In production, extract from request
    
    if request.stream {
        // Streaming response
        match state.api_server.handle_streaming_request(request, client_ip).await {
            Ok(receiver) => {
                let stream = tokio_stream::wrappers::ReceiverStream::new(receiver);
                let sse_stream = stream.map(|response| {
                    Ok::<_, Infallible>(axum::response::sse::Event::default()
                        .data(serde_json::to_string(&response).unwrap_or_default()))
                });
                
                Sse::new(sse_stream).into_response()
            }
            Err(e) => ApiErrorResponse(e).into_response()
        }
    } else {
        // Non-streaming response
        match state.api_server.handle_inference_request(request, client_ip).await {
            Ok(response) => axum::response::Json(response).into_response(),
            Err(e) => ApiErrorResponse(e).into_response()
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
    while let Some(msg) = socket.recv().await {
        match msg {
            Ok(axum::extract::ws::Message::Text(text)) => {
                // Parse WebSocket message
                if let Ok(json_msg) = serde_json::from_str::<serde_json::Value>(&text) {
                    if json_msg["type"] == "inference" {
                        // Debug: Log the entire request
                        tracing::info!("🔍 WebSocket inference request received: {:?}", json_msg["request"]);

                        if let Ok(request) = serde_json::from_value::<InferenceRequest>(json_msg["request"].clone()) {
                            // Log job_id for payment tracking visibility
                            if let Some(job_id) = request.job_id {
                                tracing::info!("📋 Processing inference request for blockchain job_id: {}", job_id);
                            } else {
                                tracing::info!("⚠️  No job_id in WebSocket request");
                            }

                            // Handle streaming inference
                            match state.api_server.handle_streaming_request(request, "ws-client".to_string()).await {
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

impl IntoResponse for ApiErrorResponse {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.0.status_code()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let error_response = self.0.to_response(None);
        
        (status, axum::response::Json(error_response)).into_response()
    }
}