// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// RAG (Retrieval-Augmented Generation) message handlers

use crate::api::websocket::message_types::{
    SearchVectorsRequest, SearchVectorsResponse, UploadVectorsRequest, UploadVectorsResponse,
    VectorSearchResult,
};
use crate::api::websocket::session::{VectorLoadingStatus, WebSocketSession};
use anyhow::{anyhow, Result};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Handles vector upload requests
///
/// Processes batch uploads of vectors to the session's vector store.
/// Returns counts of uploaded/rejected vectors and error details.
pub fn handle_upload_vectors(
    session: &Arc<Mutex<WebSocketSession>>,
    request: UploadVectorsRequest,
) -> Result<UploadVectorsResponse> {
    // Get vector store from session
    let vector_store = {
        let session_lock = session.lock().unwrap();
        session_lock
            .get_vector_store()
            .ok_or_else(|| anyhow!("RAG not enabled for this session"))?
    };

    // If replace=true, clear existing vectors first
    if request.replace {
        let mut store = vector_store.lock().unwrap();
        store.clear();
    }

    // Process each vector in the batch
    let mut uploaded = 0;
    let mut rejected = 0;
    let mut errors = Vec::new();

    for upload in request.vectors {
        let mut store = vector_store.lock().unwrap();
        match store.add(upload.id.clone(), upload.vector, upload.metadata) {
            Ok(_) => uploaded += 1,
            Err(e) => {
                rejected += 1;
                errors.push(format!("{}: {}", upload.id, e));
            }
        }
    }

    Ok(UploadVectorsResponse {
        msg_type: "uploadVectorsResponse".to_string(),
        request_id: request.request_id,
        uploaded,
        rejected,
        errors,
    })
}

/// Handles vector search requests (Sub-phase 4.2: Dual-path routing)
///
/// Supports two search paths:
/// 1. S5-loaded vectors via HNSW index (when vector_database is present)
/// 2. Uploaded vectors via SessionVectorStore (backward compatibility)
///
/// Returns top-k results with similarity scores and timing information.
pub fn handle_search_vectors(
    session: &Arc<Mutex<WebSocketSession>>,
    request: SearchVectorsRequest,
) -> Result<SearchVectorsResponse> {
    // Start timer for performance tracking
    let start = Instant::now();

    // Check if we have S5-loaded vectors (HNSW index path)
    let (has_vector_database, vector_loading_status, vector_index) = {
        let session_lock = session.lock().unwrap();
        (
            session_lock.vector_database.is_some(),
            session_lock.vector_loading_status.clone(),
            session_lock.get_vector_index(),
        )
    };

    // PATH 1: S5-loaded vectors via HNSW index
    if has_vector_database {
        // Check loading status
        match vector_loading_status {
            VectorLoadingStatus::Loading => {
                return Err(anyhow!(
                    "Vector database is still loading, please wait and try again"
                ));
            }
            VectorLoadingStatus::NotStarted => {
                return Err(anyhow!(
                    "Vector database loading has not started yet"
                ));
            }
            VectorLoadingStatus::Error { error } => {
                return Err(anyhow!(
                    "Vector database failed to load: {}",
                    error
                ));
            }
            VectorLoadingStatus::Loaded { .. } => {
                // Status is Loaded, proceed to use index
            }
        }

        // Get the HNSW index
        let index = vector_index.ok_or_else(|| {
            anyhow!("Vector database is marked as loaded but index is not available")
        })?;

        // Perform HNSW search
        let threshold = request.threshold.unwrap_or(0.0);
        let search_results = index.search(&request.query_vector, request.k, threshold)?;

        // Calculate search time
        let search_time_ms = start.elapsed().as_secs_f64() * 1000.0;

        // Get total vector count
        let total_vectors = index.vector_count();

        // Convert to response format
        let results: Vec<VectorSearchResult> = search_results
            .into_iter()
            .map(|r| VectorSearchResult {
                id: r.id,
                score: r.score,
                metadata: r.metadata,
            })
            .collect();

        return Ok(SearchVectorsResponse {
            msg_type: "searchVectorsResponse".to_string(),
            request_id: request.request_id,
            results,
            total_vectors,
            search_time_ms,
        });
    }

    // PATH 2: Uploaded vectors via SessionVectorStore (backward compatibility)
    let vector_store = {
        let session_lock = session.lock().unwrap();
        session_lock
            .get_vector_store()
            .ok_or_else(|| anyhow!("RAG not enabled for this session"))?
    };

    // Perform search
    let search_results = {
        let store = vector_store.lock().unwrap();

        // Use search_with_filter if metadata filter provided
        if let Some(ref filter) = request.metadata_filter {
            store.search_with_filter(request.query_vector, request.k, filter.clone())?
        } else {
            store.search(request.query_vector, request.k, request.threshold)?
        }
    };

    // Calculate search time
    let search_time_ms = start.elapsed().as_secs_f64() * 1000.0;

    // Get total vector count
    let total_vectors = {
        let store = vector_store.lock().unwrap();
        store.count()
    };

    // Convert to response format
    let results: Vec<VectorSearchResult> = search_results
        .into_iter()
        .map(|r| VectorSearchResult {
            id: r.id,
            score: r.score,
            metadata: r.metadata,
        })
        .collect();

    Ok(SearchVectorsResponse {
        msg_type: "searchVectorsResponse".to_string(),
        request_id: request.request_id,
        results,
        total_vectors,
        search_time_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::websocket::session::{SessionConfig, WebSocketSession};
    use serde_json::json;

    #[test]
    fn test_handle_upload_vectors_basic() {
        let mut session = WebSocketSession::with_config("test".to_string(), SessionConfig::default());
        session.enable_rag(100);
        let session = Arc::new(Mutex::new(session));

        let request = UploadVectorsRequest {
            request_id: Some("req-1".to_string()),
            vectors: vec![crate::api::websocket::message_types::VectorUpload {
                id: "doc1".to_string(),
                vector: vec![0.5; 384],
                metadata: json!({}),
            }],
            replace: false,
        };

        let response = handle_upload_vectors(&session, request).unwrap();
        assert_eq!(response.uploaded, 1);
        assert_eq!(response.rejected, 0);
    }

    #[test]
    fn test_handle_search_vectors_basic() {
        let mut session = WebSocketSession::with_config("test".to_string(), SessionConfig::default());
        session.enable_rag(100);
        let session = Arc::new(Mutex::new(session));

        // Upload a vector first
        let upload_req = UploadVectorsRequest {
            request_id: None,
            vectors: vec![crate::api::websocket::message_types::VectorUpload {
                id: "doc1".to_string(),
                vector: vec![0.5; 384],
                metadata: json!({}),
            }],
            replace: false,
        };
        handle_upload_vectors(&session, upload_req).unwrap();

        // Now search
        let search_req = SearchVectorsRequest {
            request_id: Some("search-1".to_string()),
            query_vector: vec![0.5; 384],
            k: 1,
            threshold: None,
            metadata_filter: None,
        };

        let response = handle_search_vectors(&session, search_req).unwrap();
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.total_vectors, 1);
        assert!(response.search_time_ms >= 0.0);
    }

    #[test]
    fn test_handle_upload_rag_not_enabled() {
        let session = WebSocketSession::with_config("test".to_string(), SessionConfig::default());
        let session = Arc::new(Mutex::new(session));

        let request = UploadVectorsRequest {
            request_id: None,
            vectors: vec![crate::api::websocket::message_types::VectorUpload {
                id: "doc1".to_string(),
                vector: vec![0.5; 384],
                metadata: json!({}),
            }],
            replace: false,
        };

        let result = handle_upload_vectors(&session, request);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("RAG not enabled"));
    }

    #[test]
    fn test_handle_search_rag_not_enabled() {
        let session = WebSocketSession::with_config("test".to_string(), SessionConfig::default());
        let session = Arc::new(Mutex::new(session));

        let request = SearchVectorsRequest {
            request_id: None,
            query_vector: vec![0.5; 384],
            k: 1,
            threshold: None,
            metadata_filter: None,
        };

        let result = handle_search_vectors(&session, request);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("RAG not enabled"));
    }
}
