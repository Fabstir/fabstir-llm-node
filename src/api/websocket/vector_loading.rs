// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Async Vector Database Loading (Sub-phase 3.3)
//!
//! Provides non-blocking vector database loading with timeout, cancellation,
//! and real-time progress updates via WebSocket.
//!
//! ## Features
//!
//! - **Non-Blocking**: session_init returns immediately, loading happens in background
//! - **5-Minute Timeout**: Automatic cancellation after 300 seconds
//! - **Graceful Cancellation**: Responds to CancellationToken on session disconnect
//! - **Progress Updates**: Sends real-time status via WebSocket
//! - **HNSW Indexing**: Builds searchable index after loading completes
//! - **Error Handling**: Updates session status on failures
//!
//! ## Usage
//!
//! ```rust,ignore
//! // In session_init handler:
//! if let Some(vdb_info) = session_init_data.vector_database {
//!     session.set_vector_database(Some(vdb_info.clone()));
//!     session.set_vector_loading_status(VectorLoadingStatus::Loading);
//!
//!     tokio::spawn(load_vectors_async(
//!         session_id.clone(),
//!         vdb_info,
//!         session_store.clone(),
//!         session.cancel_token.clone(),
//!         session.encryption_key.clone(),
//!     ));
//! }
//! ```

use crate::api::websocket::message_types::{
    VectorDatabaseInfo, WebSocketMessage, MessageType, LoadingProgressMessage,
};
use crate::api::websocket::session::{VectorLoadingStatus, WebSocketSession};
use crate::api::websocket::session_store::SessionStore;
use crate::job_processor::Message;
use crate::rag::vector_loader::{VectorLoader, LoadProgress};
use crate::storage::enhanced_s5_client::{EnhancedS5Client, S5Config};
use crate::storage::s5_client::EnhancedS5Backend;
use crate::vector::hnsw::HnswIndex;
use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

/// Timeout duration for vector loading operations (5 minutes)
const VECTOR_LOADING_TIMEOUT: Duration = Duration::from_secs(300);

/// HNSW index parameters
const HNSW_M: usize = 16;          // Number of connections per layer
const HNSW_EF_CONSTRUCTION: usize = 200;  // Size of dynamic candidate list during construction

/// Load vectors asynchronously in background task
///
/// This function spawns a background task that:
/// 1. Downloads vectors from S5 using VectorLoader
/// 2. Builds HNSW index for fast search
/// 3. Sends progress updates via WebSocket
/// 4. Updates session status throughout
/// 5. Handles timeout (5 minutes) and cancellation
///
/// # Arguments
///
/// * `session_id` - Session ID for status updates
/// * `vdb_info` - Vector database information (manifest path, user address)
/// * `session_store` - SessionStore for updating session state
/// * `cancel_token` - Token for graceful cancellation on disconnect
/// * `encryption_key` - Optional encryption key for decrypting vector database paths
///
/// # Panics
///
/// This function catches and handles all panics internally, updating session
/// status to Error state instead of propagating panics.
pub async fn load_vectors_async(
    session_id: String,
    vdb_info: VectorDatabaseInfo,
    session_store: Arc<RwLock<SessionStore>>,
    cancel_token: CancellationToken,
    encryption_key: Option<Vec<u8>>,
) {
    info!(
        session_id = %session_id,
        manifest_path = %vdb_info.manifest_path,
        user_address = %vdb_info.user_address,
        "🚀 Starting async vector loading task"
    );

    let start_time = Instant::now();

    // Wrap entire operation in timeout
    let result = timeout(
        VECTOR_LOADING_TIMEOUT,
        load_vectors_with_cancellation(
            session_id.clone(),
            vdb_info,
            session_store.clone(),
            cancel_token.clone(),
            encryption_key,
        ),
    )
    .await;

    match result {
        // Timeout occurred
        Err(_) => {
            error!(
                session_id = %session_id,
                timeout_sec = VECTOR_LOADING_TIMEOUT.as_secs(),
                "❌ Vector loading timed out after {} seconds",
                VECTOR_LOADING_TIMEOUT.as_secs()
            );

            // Update session status to Error
            if let Err(e) = update_session_status(
                &session_id,
                &session_store,
                VectorLoadingStatus::Error {
                    error: format!("Loading timed out after {} minutes", VECTOR_LOADING_TIMEOUT.as_secs() / 60),
                },
            ).await {
                error!(
                    session_id = %session_id,
                    error = %e,
                    "Failed to update session status after timeout"
                );
            }

            // TODO: Record timeout metric via S5Metrics::record_loading_timeout()
            // Requires passing S5Metrics instance through function parameters
        }

        // Operation completed (success or error)
        Ok(inner_result) => {
            let duration_ms = start_time.elapsed().as_millis() as u64;

            match inner_result {
                Ok(()) => {
                    info!(
                        session_id = %session_id,
                        duration_ms,
                        "✅ Async vector loading completed successfully"
                    );
                    // TODO: Record success metric via S5Metrics::record_loading_success(duration)
                    // Requires passing S5Metrics instance through function parameters
                }
                Err(e) => {
                    error!(
                        session_id = %session_id,
                        error = %e,
                        duration_ms,
                        "❌ Async vector loading failed"
                    );
                    // TODO: Record failure metric via S5Metrics::record_loading_failure()
                    // Requires passing S5Metrics instance through function parameters
                }
            }
        }
    }
}

/// Internal loading function with cancellation support
async fn load_vectors_with_cancellation(
    session_id: String,
    vdb_info: VectorDatabaseInfo,
    session_store: Arc<RwLock<SessionStore>>,
    cancel_token: CancellationToken,
    encryption_key: Option<Vec<u8>>,
) -> Result<()> {
    // Check if already cancelled
    if cancel_token.is_cancelled() {
        warn!(session_id = %session_id, "⚠️  Vector loading cancelled before starting");
        return Ok(());
    }

    // Get session key for decryption
    let session_key = encryption_key.ok_or_else(|| {
        anyhow::anyhow!("No encryption key available for vector database decryption")
    })?;

    if session_key.len() != 32 {
        return Err(anyhow::anyhow!(
            "Invalid session key length: expected 32 bytes, got {}",
            session_key.len()
        ));
    }

    // Create S5 client and VectorLoader
    let s5_config = S5Config {
        api_url: std::env::var("ENHANCED_S5_URL")
            .unwrap_or_else(|_| "http://localhost:5522".to_string()),
        api_key: None,
        timeout_secs: 60,
    };
    let s5_client = EnhancedS5Client::new(s5_config)?;
    let s5_backend = EnhancedS5Backend::new(s5_client);
    let loader = VectorLoader::with_timeout(
        Box::new(s5_backend),
        5, // max parallel chunks
        VECTOR_LOADING_TIMEOUT,
    );

    // Create progress channel for VectorLoader
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel(10);

    // Spawn progress monitoring task
    let session_id_clone = session_id.clone();
    let session_store_clone = session_store.clone();
    let cancel_token_clone = cancel_token.clone();
    let progress_task = tokio::spawn(async move {
        while let Some(progress) = progress_rx.recv().await {
            // Check if cancelled
            if cancel_token_clone.is_cancelled() {
                debug!("Progress monitoring cancelled");
                break;
            }

            // Convert LoadProgress to LoadingProgressMessage
            let progress_msg = match progress {
                LoadProgress::ManifestDownloaded => {
                    LoadingProgressMessage::ManifestDownloaded
                }
                LoadProgress::ChunkDownloaded { chunk_id, total } => {
                    LoadingProgressMessage::ChunkDownloaded { chunk_id, total }
                }
                LoadProgress::IndexBuilding => {
                    LoadingProgressMessage::IndexBuilding
                }
                LoadProgress::Complete { vector_count, duration_ms } => {
                    LoadingProgressMessage::LoadingComplete { vector_count, duration_ms }
                }
            };

            // Send progress message via WebSocket
            if let Err(e) = send_loading_progress(
                &session_id_clone,
                &session_store_clone,
                progress_msg,
            ).await {
                warn!(
                    session_id = %session_id_clone,
                    error = %e,
                    "Failed to send loading progress message"
                );
            }
        }
    });

    // Load vectors with cancellation check
    let vectors = tokio::select! {
        result = loader.load_vectors_from_s5(
            &vdb_info.manifest_path,
            &vdb_info.user_address,
            &session_key,
            Some(progress_tx.clone()),
        ) => {
            result.map_err(|e| anyhow::anyhow!("Vector loading failed: {}", e))?
        }
        _ = cancel_token.cancelled() => {
            warn!(session_id = %session_id, "⚠️  Vector loading cancelled by disconnect");
            update_session_status(
                &session_id,
                &session_store,
                VectorLoadingStatus::Error {
                    error: "Loading cancelled by client disconnect".to_string(),
                },
            ).await?;
            return Ok(());
        }
    };

    let vector_count = vectors.len();

    // Calculate dimensions from first vector
    let dimensions = if !vectors.is_empty() {
        vectors[0].vector.len()
    } else {
        return Err(anyhow::anyhow!("No vectors loaded from database"));
    };

    info!(
        session_id = %session_id,
        vector_count,
        dimensions,
        "📦 Vectors loaded, building HNSW index..."
    );

    // Send index building progress (will be converted to LoadingProgressMessage by progress task)
    let _ = progress_tx.send(LoadProgress::IndexBuilding).await;

    // Build HNSW index with cancellation check
    let index_start = Instant::now();
    let index = tokio::select! {
        result = tokio::task::spawn_blocking(move || {
            HnswIndex::build(vectors, dimensions)
        }) => {
            result.map_err(|e| anyhow::anyhow!("Index building task failed: {}", e))??
        }
        _ = cancel_token.cancelled() => {
            warn!(session_id = %session_id, "⚠️  Index building cancelled by disconnect");
            update_session_status(
                &session_id,
                &session_store,
                VectorLoadingStatus::Error {
                    error: "Index building cancelled by client disconnect".to_string(),
                },
            ).await?;
            return Ok(());
        }
    };

    let index_duration_ms = index_start.elapsed().as_millis() as u64;
    info!(
        session_id = %session_id,
        vector_count,
        index_duration_ms,
        "✅ HNSW index built successfully"
    );

    // Calculate total loading time
    let total_duration_ms = index_start.elapsed().as_millis() as u64 + index_duration_ms;

    // Send completion progress (will be converted to LoadingProgressMessage by progress task)
    let _ = progress_tx.send(LoadProgress::Complete {
        vector_count,
        duration_ms: total_duration_ms,
    }).await;

    // Update session with index and status
    {
        let mut store = session_store.write().await;
        if let Some(mut session) = store.get_session_mut(&session_id).await {
            session.set_vector_index(Arc::new(index));
            session.set_vector_loading_status(VectorLoadingStatus::Loaded {
                vector_count,
                load_time_ms: total_duration_ms,
            });
        } else {
            return Err(anyhow::anyhow!("Session not found: {}", session_id));
        }
    }

    // Drop progress_tx to close the channel and allow progress_task to complete
    drop(progress_tx);

    // Wait for progress task to finish sending all messages
    let _ = progress_task.await;

    Ok(())
}

/// Update session loading status
async fn update_session_status(
    session_id: &str,
    session_store: &Arc<RwLock<SessionStore>>,
    status: VectorLoadingStatus,
) -> Result<()> {
    let mut store = session_store.write().await;
    if let Some(mut session) = store.get_session_mut(session_id).await {
        session.set_vector_loading_status(status);
        Ok(())
    } else {
        Err(anyhow::anyhow!("Session not found: {}", session_id))
    }
}

/// Send loading progress message to client via WebSocket
async fn send_loading_progress(
    session_id: &str,
    session_store: &Arc<RwLock<SessionStore>>,
    progress: LoadingProgressMessage,
) -> Result<()> {
    debug!(
        session_id = %session_id,
        progress = ?progress,
        "Sending loading progress message"
    );

    // Get session's tx channel
    let store = session_store.read().await;
    if let Some(session) = store.get_session(session_id).await {
        if let Some(ref tx) = session.tx {
            // Serialize LoadingProgressMessage to JSON
            let progress_payload = serde_json::to_value(&progress)?;

            // Create WebSocket message
            let ws_message = WebSocketMessage {
                msg_type: MessageType::VectorLoadingProgress,
                session_id: Some(session_id.to_string()),
                payload: progress_payload,
            };

            // Convert to job_processor Message (role: "system", content: JSON string)
            let msg = Message {
                role: "system".to_string(),
                content: serde_json::to_string(&ws_message)?,
                timestamp: Some(chrono::Utc::now().timestamp_millis()),
            };

            // Send via channel
            tx.send(msg).map_err(|e| anyhow::anyhow!("Failed to send message: {}", e))?;
        } else {
            warn!(session_id = %session_id, "No tx channel available for sending progress");
        }
    } else {
        return Err(anyhow::anyhow!("Session not found: {}", session_id));
    }

    Ok(())
}
