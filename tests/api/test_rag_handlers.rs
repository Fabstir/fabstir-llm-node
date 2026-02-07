// TDD Tests for RAG Message Handlers (Sub-phase 2.3)
// Written FIRST before implementation

use fabstir_llm_node::api::websocket::message_types::{
    SearchVectorsRequest, SearchVectorsResponse, UploadVectorsRequest, UploadVectorsResponse,
    VectorUpload,
};
use fabstir_llm_node::api::websocket::session::{SessionConfig, WebSocketSession};
use serde_json::json;
use std::sync::{Arc, Mutex};

// Helper function to create a test session with RAG enabled
fn create_test_session_with_rag(max_vectors: usize) -> Arc<Mutex<WebSocketSession>> {
    let mut session =
        WebSocketSession::with_config("test-session".to_string(), SessionConfig::default());
    session.enable_rag(max_vectors);
    Arc::new(Mutex::new(session))
}

// Helper function to create a test session without RAG
fn create_test_session_without_rag() -> Arc<Mutex<WebSocketSession>> {
    let session =
        WebSocketSession::with_config("test-session".to_string(), SessionConfig::default());
    Arc::new(Mutex::new(session))
}

#[test]
fn test_upload_handler_success() {
    use fabstir_llm_node::api::websocket::handlers::rag::handle_upload_vectors;

    let session = create_test_session_with_rag(1000);

    let request = UploadVectorsRequest {
        request_id: Some("req-1".to_string()),
        vectors: vec![
            VectorUpload {
                id: "doc1".to_string(),
                vector: vec![0.1; 384],
                metadata: json!({"title": "Test Doc 1"}),
            },
            VectorUpload {
                id: "doc2".to_string(),
                vector: vec![0.2; 384],
                metadata: json!({"title": "Test Doc 2"}),
            },
        ],
        replace: false,
    };

    let response = handle_upload_vectors(&session, request).unwrap();

    assert_eq!(response.uploaded, 2);
    assert_eq!(response.rejected, 0);
    assert_eq!(response.errors.len(), 0);
    assert_eq!(response.request_id, Some("req-1".to_string()));
}

#[test]
fn test_upload_handler_validates_dimensions() {
    use fabstir_llm_node::api::websocket::handlers::rag::handle_upload_vectors;

    let session = create_test_session_with_rag(1000);

    let request = UploadVectorsRequest {
        request_id: None,
        vectors: vec![
            VectorUpload {
                id: "doc1".to_string(),
                vector: vec![0.1; 384], // Valid
                metadata: json!({}),
            },
            VectorUpload {
                id: "doc2".to_string(),
                vector: vec![0.2; 256], // Invalid dimensions
                metadata: json!({}),
            },
        ],
        replace: false,
    };

    let response = handle_upload_vectors(&session, request).unwrap();

    assert_eq!(response.uploaded, 1);
    assert_eq!(response.rejected, 1);
    assert_eq!(response.errors.len(), 1);
    assert!(response.errors[0].contains("384"));
}

#[test]
fn test_upload_handler_replace_clears() {
    use fabstir_llm_node::api::websocket::handlers::rag::handle_upload_vectors;

    let session = create_test_session_with_rag(1000);

    // First upload
    let request1 = UploadVectorsRequest {
        request_id: None,
        vectors: vec![VectorUpload {
            id: "doc1".to_string(),
            vector: vec![0.1; 384],
            metadata: json!({}),
        }],
        replace: false,
    };
    handle_upload_vectors(&session, request1).unwrap();

    // Second upload with replace=true
    let request2 = UploadVectorsRequest {
        request_id: None,
        vectors: vec![VectorUpload {
            id: "doc2".to_string(),
            vector: vec![0.2; 384],
            metadata: json!({}),
        }],
        replace: true,
    };
    let response2 = handle_upload_vectors(&session, request2).unwrap();

    assert_eq!(response2.uploaded, 1);

    // Verify only doc2 exists (doc1 was cleared)
    let store = session.lock().unwrap().get_vector_store().unwrap();
    let store_lock = store.lock().unwrap();
    assert_eq!(store_lock.count(), 1);
    assert!(store_lock.get("doc2").is_some());
    assert!(store_lock.get("doc1").is_none());
}

#[test]
fn test_upload_handler_rag_disabled_error() {
    use fabstir_llm_node::api::websocket::handlers::rag::handle_upload_vectors;

    let session = create_test_session_without_rag();

    let request = UploadVectorsRequest {
        request_id: None,
        vectors: vec![VectorUpload {
            id: "doc1".to_string(),
            vector: vec![0.1; 384],
            metadata: json!({}),
        }],
        replace: false,
    };

    let result = handle_upload_vectors(&session, request);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("RAG") || err_msg.contains("not enabled"));
}

#[test]
fn test_upload_handler_batch_processing() {
    use fabstir_llm_node::api::websocket::handlers::rag::handle_upload_vectors;

    let session = create_test_session_with_rag(1000);

    // Create batch of 50 vectors
    let vectors: Vec<VectorUpload> = (0..50)
        .map(|i| VectorUpload {
            id: format!("doc{}", i),
            vector: vec![0.1; 384],
            metadata: json!({"index": i}),
        })
        .collect();

    let request = UploadVectorsRequest {
        request_id: Some("batch-1".to_string()),
        vectors,
        replace: false,
    };

    let response = handle_upload_vectors(&session, request).unwrap();

    assert_eq!(response.uploaded, 50);
    assert_eq!(response.rejected, 0);
    assert_eq!(response.request_id, Some("batch-1".to_string()));
}

#[test]
fn test_upload_handler_partial_success() {
    use fabstir_llm_node::api::websocket::handlers::rag::handle_upload_vectors;

    let session = create_test_session_with_rag(1000);

    let request = UploadVectorsRequest {
        request_id: None,
        vectors: vec![
            VectorUpload {
                id: "doc1".to_string(),
                vector: vec![0.1; 384], // Valid
                metadata: json!({}),
            },
            VectorUpload {
                id: "doc2".to_string(),
                vector: vec![f32::NAN; 384], // Invalid (NaN)
                metadata: json!({}),
            },
            VectorUpload {
                id: "doc3".to_string(),
                vector: vec![0.3; 384], // Valid
                metadata: json!({}),
            },
        ],
        replace: false,
    };

    let response = handle_upload_vectors(&session, request).unwrap();

    assert_eq!(response.uploaded, 2);
    assert_eq!(response.rejected, 1);
    assert_eq!(response.errors.len(), 1);
    assert!(response.errors[0].contains("doc2"));
}

#[test]
fn test_search_handler_success() {
    use fabstir_llm_node::api::websocket::handlers::rag::{
        handle_search_vectors, handle_upload_vectors,
    };

    let session = create_test_session_with_rag(1000);

    // First upload some vectors
    let upload_request = UploadVectorsRequest {
        request_id: None,
        vectors: vec![
            VectorUpload {
                id: "doc1".to_string(),
                vector: vec![0.1; 384],
                metadata: json!({"title": "Machine Learning"}),
            },
            VectorUpload {
                id: "doc2".to_string(),
                vector: vec![0.2; 384],
                metadata: json!({"title": "Deep Learning"}),
            },
        ],
        replace: false,
    };
    handle_upload_vectors(&session, upload_request).unwrap();

    // Now search
    let search_request = SearchVectorsRequest {
        request_id: Some("search-1".to_string()),
        query_vector: vec![0.15; 384],
        k: 2,
        threshold: None,
        metadata_filter: None,
    };

    let response = handle_search_vectors(&session, search_request).unwrap();

    assert_eq!(response.results.len(), 2);
    assert_eq!(response.total_vectors, 2);
    assert!(response.search_time_ms > 0.0);
    assert_eq!(response.request_id, Some("search-1".to_string()));
}

#[test]
fn test_search_handler_empty_store() {
    use fabstir_llm_node::api::websocket::handlers::rag::handle_search_vectors;

    let session = create_test_session_with_rag(1000);

    let request = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.5; 384],
        k: 10,
        threshold: None,
        metadata_filter: None,
    };

    let response = handle_search_vectors(&session, request).unwrap();

    assert_eq!(response.results.len(), 0);
    assert_eq!(response.total_vectors, 0);
}

#[test]
fn test_search_handler_with_threshold() {
    use fabstir_llm_node::api::websocket::handlers::rag::{
        handle_search_vectors, handle_upload_vectors,
    };

    let session = create_test_session_with_rag(1000);

    // Upload vectors with different directions
    let mut vec1 = vec![1.0; 384];
    let mut vec2 = vec![0.0; 384];
    vec2[0] = 1.0; // Orthogonal to vec1

    let upload_request = UploadVectorsRequest {
        request_id: None,
        vectors: vec![
            VectorUpload {
                id: "doc1".to_string(),
                vector: vec1.clone(),
                metadata: json!({}),
            },
            VectorUpload {
                id: "doc2".to_string(),
                vector: vec2.clone(),
                metadata: json!({}),
            },
        ],
        replace: false,
    };
    handle_upload_vectors(&session, upload_request).unwrap();

    // Search with query similar to doc1, with high threshold
    let search_request = SearchVectorsRequest {
        request_id: None,
        query_vector: vec1, // Same as doc1
        k: 10,
        threshold: Some(0.99), // High threshold (only exact/near matches)
        metadata_filter: None,
    };

    let response = handle_search_vectors(&session, search_request).unwrap();

    // Should only return doc1 (similarity ~1.0), not doc2 (similarity ~0.05)
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].id, "doc1");
}

#[test]
fn test_search_handler_with_filter() {
    use fabstir_llm_node::api::websocket::handlers::rag::{
        handle_search_vectors, handle_upload_vectors,
    };

    let session = create_test_session_with_rag(1000);

    // Upload vectors with different categories
    let upload_request = UploadVectorsRequest {
        request_id: None,
        vectors: vec![
            VectorUpload {
                id: "doc1".to_string(),
                vector: vec![0.1; 384],
                metadata: json!({"category": "science"}),
            },
            VectorUpload {
                id: "doc2".to_string(),
                vector: vec![0.2; 384],
                metadata: json!({"category": "history"}),
            },
        ],
        replace: false,
    };
    handle_upload_vectors(&session, upload_request).unwrap();

    // Search with metadata filter
    let search_request = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.15; 384],
        k: 10,
        threshold: None,
        metadata_filter: Some(json!({"category": {"$eq": "science"}})),
    };

    let response = handle_search_vectors(&session, search_request).unwrap();

    // Should only return science category
    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].id, "doc1");
}

#[test]
fn test_search_handler_rag_disabled_error() {
    use fabstir_llm_node::api::websocket::handlers::rag::handle_search_vectors;

    let session = create_test_session_without_rag();

    let request = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.5; 384],
        k: 10,
        threshold: None,
        metadata_filter: None,
    };

    let result = handle_search_vectors(&session, request);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("RAG") || err_msg.contains("not enabled"));
}

#[test]
fn test_search_handler_timing_accurate() {
    use fabstir_llm_node::api::websocket::handlers::rag::{
        handle_search_vectors, handle_upload_vectors,
    };

    let session = create_test_session_with_rag(1000);

    // Upload 100 vectors
    let vectors: Vec<VectorUpload> = (0..100)
        .map(|i| VectorUpload {
            id: format!("doc{}", i),
            vector: vec![i as f32 / 100.0; 384],
            metadata: json!({}),
        })
        .collect();

    let upload_request = UploadVectorsRequest {
        request_id: None,
        vectors,
        replace: false,
    };
    handle_upload_vectors(&session, upload_request).unwrap();

    // Search and verify timing
    let search_request = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.5; 384],
        k: 10,
        threshold: None,
        metadata_filter: None,
    };

    let response = handle_search_vectors(&session, search_request).unwrap();

    // Timing should be positive and reasonable (<100ms for 100 vectors)
    assert!(response.search_time_ms > 0.0);
    assert!(response.search_time_ms < 100.0);
}
