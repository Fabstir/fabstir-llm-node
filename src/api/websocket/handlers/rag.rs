// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// RAG (Retrieval-Augmented Generation) message handlers

use crate::api::websocket::message_types::{
    SearchVectorsRequest, SearchVectorsResponse, UploadVectorsRequest, UploadVectorsResponse,
    VectorSearchResult,
};
use crate::api::websocket::session::{VectorLoadingStatus, WebSocketSession};
use anyhow::{anyhow, Result};
use tracing::info;
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
    let (vector_store, session_id) = {
        let session_lock = session.lock().unwrap();
        let sid = session_lock.id().clone();
        let vs = session_lock.get_vector_store();
        info!("📦 handle_upload_vectors: session_id={}, vector_store={}",
              sid, if vs.is_some() { "Some" } else { "None" });
        (
            vs.ok_or_else(|| anyhow!("RAG not enabled for this session"))?,
            sid,
        )
    };

    // Log Arc pointer for debugging
    let arc_ptr = Arc::as_ptr(&vector_store);
    info!("📦 handle_upload_vectors: Arc ptr={:?}", arc_ptr);

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

    // Log final vector count
    {
        let store = vector_store.lock().unwrap();
        info!("📦 handle_upload_vectors: session_id={}, final_count={}, uploaded={}, rejected={}",
              session_id, store.count(), uploaded, rejected);
    }

    // Determine status: "success" if all uploaded, "partial" if some rejected, "error" if all failed
    let status = if rejected == 0 {
        "success".to_string()
    } else if uploaded > 0 {
        "partial".to_string()
    } else {
        "error".to_string()
    };

    Ok(UploadVectorsResponse {
        msg_type: "uploadVectorsResponse".to_string(),
        request_id: request.request_id,
        status,
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
    let (has_vector_database, vector_loading_status, vector_index, session_id, has_vector_store) = {
        let session_lock = session.lock().unwrap();
        let sid = session_lock.id().clone();
        let has_vs = session_lock.get_vector_store().is_some();
        info!("🔍 handle_search_vectors: session_id={}, has_vector_database={}, has_vector_store={}",
              sid, session_lock.vector_database.is_some(), has_vs);
        (
            session_lock.vector_database.is_some(),
            session_lock.vector_loading_status.clone(),
            session_lock.get_vector_index(),
            sid,
            has_vs,
        )
    };

    // Log if no vector store (helps debug why vectors not found)
    if !has_vector_store && !has_vector_database {
        info!("⚠️ handle_search_vectors: session_id={} has NO vector_store and NO vector_database!", session_id);
    }

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
        let vs = session_lock.get_vector_store();
        info!("🔍 handle_search_vectors PATH 2: session_id={}, vector_store={}",
              session_id, if vs.is_some() { "Some" } else { "None" });
        vs.ok_or_else(|| anyhow!("RAG not enabled for this session"))?
    };

    // Log Arc pointer and vector count for debugging
    let arc_ptr = Arc::as_ptr(&vector_store);
    {
        let store = vector_store.lock().unwrap();
        info!("🔍 handle_search_vectors: Arc ptr={:?}, vector_count={}", arc_ptr, store.count());
    }

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

    /// Test that mimics production behavior:
    /// - Different Arc<Mutex<WebSocketSession>> for upload and search
    /// - Verifies vector_store Arc is properly shared between clones
    #[test]
    fn test_vector_store_shared_between_session_clones() {
        // Create a session with RAG enabled
        let mut session = WebSocketSession::with_config("test-clone".to_string(), SessionConfig::default());
        session.enable_rag(100);

        // Clone the session (simulating what get_or_create_rag_session returns)
        let session_clone1 = session.clone();

        // Create DIFFERENT Arc<Mutex> wrappers (simulating production behavior)
        let arc1 = Arc::new(Mutex::new(session_clone1));

        // Upload vectors using first Arc
        let upload_req = UploadVectorsRequest {
            request_id: Some("upload-1".to_string()),
            vectors: vec![crate::api::websocket::message_types::VectorUpload {
                id: "shared-doc".to_string(),
                vector: vec![0.5; 384],
                metadata: json!({"test": "shared"}),
            }],
            replace: false,
        };
        let upload_response = handle_upload_vectors(&arc1, upload_req).unwrap();
        assert_eq!(upload_response.uploaded, 1);

        // Now clone the ORIGINAL session again (simulating what get_session returns)
        let session_clone2 = session.clone();

        // Create ANOTHER DIFFERENT Arc<Mutex> wrapper
        let arc2 = Arc::new(Mutex::new(session_clone2));

        // Search using second Arc - should find the vector uploaded through first Arc
        let search_req = SearchVectorsRequest {
            request_id: Some("search-1".to_string()),
            query_vector: vec![0.5; 384],
            k: 5,
            threshold: None,
            metadata_filter: None,
        };
        let search_response = handle_search_vectors(&arc2, search_req).unwrap();

        // THIS IS THE KEY ASSERTION: vectors uploaded through arc1 should be found through arc2
        // because both clones share the same vector_store Arc<Mutex<SessionVectorStore>>
        assert_eq!(search_response.results.len(), 1, "Should find 1 vector that was uploaded via different Arc");
        assert_eq!(search_response.total_vectors, 1, "Total vectors should be 1");
        assert_eq!(search_response.results[0].id, "shared-doc");
    }
}
