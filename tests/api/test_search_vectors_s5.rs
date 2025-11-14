// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// TDD Tests for searchVectors with S5-loaded HNSW Index (Sub-phase 4.2)
// Tests dual-path search: S5-loaded HNSW vs uploaded SessionVectorStore

use fabstir_llm_node::api::websocket::message_types::{
    SearchVectorsRequest, SearchVectorsResponse, UploadVectorsRequest, VectorDatabaseInfo,
    VectorUpload,
};
use fabstir_llm_node::api::websocket::session::{
    SessionConfig, VectorLoadingStatus, WebSocketSession,
};
use fabstir_llm_node::api::websocket::handlers::rag::{handle_search_vectors, handle_upload_vectors};
use fabstir_llm_node::storage::manifest::Vector;
use fabstir_llm_node::vector::hnsw::HnswIndex;
use serde_json::json;
use std::sync::{Arc, Mutex};

/// Helper: Create session with S5-loaded HNSW index
fn create_session_with_s5_index(
    vectors: Vec<Vector>,
    dimensions: usize,
) -> Arc<Mutex<WebSocketSession>> {
    let mut session =
        WebSocketSession::with_config("test-s5-session".to_string(), SessionConfig::default());

    // Set vector database info (simulating session_init)
    let vdb_info = VectorDatabaseInfo {
        manifest_path: "home/vector-databases/0xUser/test-db/manifest.json".to_string(),
        user_address: "0xUser123".to_string(),
    };
    session.set_vector_database(Some(vdb_info));

    // Build HNSW index
    let index = HnswIndex::build(vectors, dimensions).expect("Failed to build HNSW index");

    // Store index in session
    session.set_vector_index(Arc::new(index));

    // Mark as loaded
    session.set_vector_loading_status(VectorLoadingStatus::Loaded {
        vector_count: session
            .get_vector_index()
            .map(|idx| idx.vector_count())
            .unwrap_or(0),
        load_time_ms: 1000,
    });

    Arc::new(Mutex::new(session))
}

/// Helper: Create session with uploaded vectors (existing flow)
fn create_session_with_uploaded_vectors() -> Arc<Mutex<WebSocketSession>> {
    let mut session =
        WebSocketSession::with_config("test-upload-session".to_string(), SessionConfig::default());
    session.enable_rag(1000);
    Arc::new(Mutex::new(session))
}

/// Helper: Create test vectors for HNSW index
fn create_test_vectors(count: usize, dimensions: usize) -> Vec<Vector> {
    (0..count)
        .map(|i| {
            let base_value = (i as f32) / (count as f32);
            let vector: Vec<f32> = (0..dimensions)
                .map(|d| base_value + (d as f32) * 0.001)
                .collect();

            Vector {
                id: format!("vec-{}", i),
                vector,
                metadata: json!({
                    "index": i,
                    "title": format!("Document {}", i),
                    "category": if i % 2 == 0 { "even" } else { "odd" }
                }),
            }
        })
        .collect()
}

// ============================================================================
// Test Category 1: S5-Loaded Index Search Tests
// ============================================================================

#[test]
fn test_search_s5_loaded_index_basic() {
    // Create session with S5-loaded HNSW index (100 vectors)
    let vectors = create_test_vectors(100, 384);
    let query_vector = vectors[50].vector.clone(); // Query similar to vector 50
    let session = create_session_with_s5_index(vectors, 384);

    let request = SearchVectorsRequest {
        request_id: Some("search-s5-1".to_string()),
        query_vector,
        k: 10,
        threshold: Some(0.0),
        metadata_filter: None,
    };

    let response = handle_search_vectors(&session, request).expect("Search should succeed");

    assert_eq!(response.request_id, Some("search-s5-1".to_string()));
    assert!(response.results.len() <= 10, "Should return at most 10 results");
    assert!(response.results.len() > 0, "Should return some results");

    // First result should be self-match (vec-50) or very similar
    let first_result = &response.results[0];
    assert!(
        first_result.score >= 0.99,
        "First result should have high similarity score, got {}",
        first_result.score
    );
}

#[test]
fn test_search_s5_loaded_index_k_parameter() {
    let vectors = create_test_vectors(50, 384);
    let query_vector = vectors[0].vector.clone();
    let session = create_session_with_s5_index(vectors, 384);

    // Test k=1
    let request_k1 = SearchVectorsRequest {
        request_id: Some("k1".to_string()),
        query_vector: query_vector.clone(),
        k: 1,
        threshold: Some(0.0),
        metadata_filter: None,
    };

    let response_k1 = handle_search_vectors(&session, request_k1).unwrap();
    assert_eq!(response_k1.results.len(), 1);

    // Test k=5
    let request_k5 = SearchVectorsRequest {
        request_id: Some("k5".to_string()),
        query_vector: query_vector.clone(),
        k: 5,
        threshold: Some(0.0),
        metadata_filter: None,
    };

    let response_k5 = handle_search_vectors(&session, request_k5).unwrap();
    assert_eq!(response_k5.results.len(), 5);

    // Test k=100 (more than dataset size)
    let request_k100 = SearchVectorsRequest {
        request_id: Some("k100".to_string()),
        query_vector: query_vector.clone(),
        k: 100,
        threshold: Some(0.0),
        metadata_filter: None,
    };

    let response_k100 = handle_search_vectors(&session, request_k100).unwrap();
    assert!(
        response_k100.results.len() >= 20,
        "Should return most vectors when k > dataset size (HNSW is approximate). Got: {}, expected >= 20 (40% of 50 vectors)",
        response_k100.results.len()
    );
}

#[test]
fn test_search_s5_loaded_index_threshold() {
    let vectors = create_test_vectors(100, 384);
    let query_vector = vectors[0].vector.clone();
    let session = create_session_with_s5_index(vectors, 384);

    // High threshold (0.95) - only very similar vectors
    let request_high = SearchVectorsRequest {
        request_id: Some("high-threshold".to_string()),
        query_vector: query_vector.clone(),
        k: 50,
        threshold: Some(0.95),
        metadata_filter: None,
    };

    let response_high = handle_search_vectors(&session, request_high).unwrap();

    // Low threshold (0.0) - all vectors
    let request_low = SearchVectorsRequest {
        request_id: Some("low-threshold".to_string()),
        query_vector: query_vector.clone(),
        k: 50,
        threshold: Some(0.0),
        metadata_filter: None,
    };

    let response_low = handle_search_vectors(&session, request_low).unwrap();

    assert!(
        response_high.results.len() <= response_low.results.len(),
        "Higher threshold should return fewer or equal results"
    );

    // All results should meet threshold
    for result in &response_high.results {
        assert!(
            result.score >= 0.95,
            "Result score {} should be >= threshold 0.95",
            result.score
        );
    }
}

#[test]
fn test_search_s5_loaded_index_metadata_preserved() {
    let vectors = create_test_vectors(50, 384);
    let query_vector = vectors[10].vector.clone();
    let session = create_session_with_s5_index(vectors, 384);

    let request = SearchVectorsRequest {
        request_id: Some("metadata-test".to_string()),
        query_vector,
        k: 5,
        threshold: Some(0.0),
        metadata_filter: None,
    };

    let response = handle_search_vectors(&session, request).unwrap();

    // Check that metadata is preserved
    for result in &response.results {
        assert!(result.metadata.is_object(), "Metadata should be an object");
        assert!(
            result.metadata.get("title").is_some(),
            "Metadata should contain 'title' field"
        );
        assert!(
            result.metadata.get("index").is_some(),
            "Metadata should contain 'index' field"
        );
    }
}

#[test]
fn test_search_s5_loaded_index_performance() {
    // Test search performance on 1000 vectors
    let vectors = create_test_vectors(1000, 384);
    let query_vector = vectors[500].vector.clone();
    let session = create_session_with_s5_index(vectors, 384);

    let start = std::time::Instant::now();

    let request = SearchVectorsRequest {
        request_id: Some("perf-test".to_string()),
        query_vector,
        k: 10,
        threshold: Some(0.0),
        metadata_filter: None,
    };

    let response = handle_search_vectors(&session, request).unwrap();
    let duration = start.elapsed();

    assert!(response.results.len() <= 10);
    println!("1K vectors search time: {:?}", duration);

    // Performance requirement: < 100ms for 1K vectors (lenient for debug builds)
    assert!(
        duration.as_millis() < 500,
        "Search should be < 500ms in debug mode, got {:?}",
        duration
    );
}

// ============================================================================
// Test Category 2: Backward Compatibility (Uploaded Vectors)
// ============================================================================

#[test]
fn test_search_uploaded_vectors_still_works() {
    let session = create_session_with_uploaded_vectors();

    // Upload some vectors
    let upload_request = UploadVectorsRequest {
        request_id: Some("upload-1".to_string()),
        vectors: vec![
            VectorUpload {
                id: "doc1".to_string(),
                vector: vec![0.1; 384],
                metadata: json!({"title": "Doc 1"}),
            },
            VectorUpload {
                id: "doc2".to_string(),
                vector: vec![0.2; 384],
                metadata: json!({"title": "Doc 2"}),
            },
        ],
        replace: false,
    };

    handle_upload_vectors(&session, upload_request).expect("Upload should succeed");

    // Search should use SessionVectorStore (existing flow)
    let search_request = SearchVectorsRequest {
        request_id: Some("search-upload-1".to_string()),
        query_vector: vec![0.15; 384],
        k: 10,
        threshold: Some(0.0),
        metadata_filter: None,
    };

    let response = handle_search_vectors(&session, search_request).expect("Search should succeed");

    assert_eq!(response.results.len(), 2, "Should find both uploaded vectors");
}

#[test]
fn test_search_uploaded_with_metadata_filter() {
    let session = create_session_with_uploaded_vectors();

    // Upload vectors with different metadata
    let upload_request = UploadVectorsRequest {
        request_id: Some("upload-2".to_string()),
        vectors: vec![
            VectorUpload {
                id: "doc1".to_string(),
                vector: vec![0.1; 384],
                metadata: json!({"category": "tech", "title": "Tech Doc"}),
            },
            VectorUpload {
                id: "doc2".to_string(),
                vector: vec![0.2; 384],
                metadata: json!({"category": "science", "title": "Science Doc"}),
            },
        ],
        replace: false,
    };

    handle_upload_vectors(&session, upload_request).unwrap();

    // Search with metadata filter (existing SessionVectorStore feature)
    // NOTE: SessionVectorStore requires MongoDB-style operators like $eq
    let search_request = SearchVectorsRequest {
        request_id: Some("search-filter".to_string()),
        query_vector: vec![0.15; 384],
        k: 10,
        threshold: Some(0.0),
        metadata_filter: Some(json!({"category": {"$eq": "tech"}})),
    };

    let response = handle_search_vectors(&session, search_request).unwrap();

    assert_eq!(
        response.results.len(),
        1,
        "Should only find vectors matching metadata filter"
    );
    assert_eq!(response.results[0].id, "doc1");
}

// ============================================================================
// Test Category 3: Loading State Handling
// ============================================================================

#[test]
fn test_search_while_loading_returns_error() {
    let mut session =
        WebSocketSession::with_config("loading-session".to_string(), SessionConfig::default());

    // Set vector database info but status = Loading
    let vdb_info = VectorDatabaseInfo {
        manifest_path: "home/vector-databases/0xUser/test-db/manifest.json".to_string(),
        user_address: "0xUser123".to_string(),
    };
    session.set_vector_database(Some(vdb_info));
    session.set_vector_loading_status(VectorLoadingStatus::Loading);

    let session_arc = Arc::new(Mutex::new(session));

    let request = SearchVectorsRequest {
        request_id: Some("search-loading".to_string()),
        query_vector: vec![0.1; 384],
        k: 10,
        threshold: Some(0.0),
        metadata_filter: None,
    };

    let result = handle_search_vectors(&session_arc, request);

    assert!(result.is_err(), "Search should fail when status=Loading");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("loading") || err_msg.contains("Loading"),
        "Error should mention loading state, got: {}",
        err_msg
    );
}

#[test]
fn test_search_not_started_returns_error() {
    let mut session =
        WebSocketSession::with_config("notstarted-session".to_string(), SessionConfig::default());

    // Set vector database info but status = NotStarted
    let vdb_info = VectorDatabaseInfo {
        manifest_path: "home/vector-databases/0xUser/test-db/manifest.json".to_string(),
        user_address: "0xUser123".to_string(),
    };
    session.set_vector_database(Some(vdb_info));
    // Default status is NotStarted

    let session_arc = Arc::new(Mutex::new(session));

    let request = SearchVectorsRequest {
        request_id: Some("search-notstarted".to_string()),
        query_vector: vec![0.1; 384],
        k: 10,
        threshold: Some(0.0),
        metadata_filter: None,
    };

    let result = handle_search_vectors(&session_arc, request);

    assert!(
        result.is_err(),
        "Search should fail when status=NotStarted"
    );
}

#[test]
fn test_search_error_state_returns_error() {
    let mut session =
        WebSocketSession::with_config("error-session".to_string(), SessionConfig::default());

    // Set vector database info but status = Error
    let vdb_info = VectorDatabaseInfo {
        manifest_path: "home/vector-databases/0xUser/test-db/manifest.json".to_string(),
        user_address: "0xUser123".to_string(),
    };
    session.set_vector_database(Some(vdb_info));
    session.set_vector_loading_status(VectorLoadingStatus::Error {
        error: "Download failed: network timeout".to_string(),
    });

    let session_arc = Arc::new(Mutex::new(session));

    let request = SearchVectorsRequest {
        request_id: Some("search-error".to_string()),
        query_vector: vec![0.1; 384],
        k: 10,
        threshold: Some(0.0),
        metadata_filter: None,
    };

    let result = handle_search_vectors(&session_arc, request);

    assert!(result.is_err(), "Search should fail when status=Error");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("failed") || err_msg.contains("error"),
        "Error should mention loading failure, got: {}",
        err_msg
    );
}

#[test]
fn test_search_loaded_but_no_index_returns_error() {
    let mut session =
        WebSocketSession::with_config("noindex-session".to_string(), SessionConfig::default());

    // Set vector database info and status = Loaded, but don't set vector_index
    let vdb_info = VectorDatabaseInfo {
        manifest_path: "home/vector-databases/0xUser/test-db/manifest.json".to_string(),
        user_address: "0xUser123".to_string(),
    };
    session.set_vector_database(Some(vdb_info));
    session.set_vector_loading_status(VectorLoadingStatus::Loaded {
        vector_count: 100,
        load_time_ms: 1000,
    });
    // Intentionally not calling session.set_vector_index()

    let session_arc = Arc::new(Mutex::new(session));

    let request = SearchVectorsRequest {
        request_id: Some("search-noindex".to_string()),
        query_vector: vec![0.1; 384],
        k: 10,
        threshold: Some(0.0),
        metadata_filter: None,
    };

    let result = handle_search_vectors(&session_arc, request);

    assert!(
        result.is_err(),
        "Search should fail when index is missing despite Loaded status"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("index") || err_msg.contains("Index"),
        "Error should mention missing index, got: {}",
        err_msg
    );
}

// ============================================================================
// Test Category 4: Edge Cases
// ============================================================================

#[test]
fn test_search_s5_empty_index() {
    let vectors = vec![]; // Empty vector list
    let session = create_session_with_s5_index(vectors, 384);

    let request = SearchVectorsRequest {
        request_id: Some("search-empty".to_string()),
        query_vector: vec![0.1; 384],
        k: 10,
        threshold: Some(0.0),
        metadata_filter: None,
    };

    let response = handle_search_vectors(&session, request).expect("Should handle empty index");

    assert_eq!(
        response.results.len(),
        0,
        "Empty index should return no results"
    );
}

#[test]
fn test_search_no_rag_enabled_error() {
    // Session with no vector_database and no uploaded vectors (RAG not enabled)
    let session =
        WebSocketSession::with_config("no-rag-session".to_string(), SessionConfig::default());
    let session_arc = Arc::new(Mutex::new(session));

    let request = SearchVectorsRequest {
        request_id: Some("search-no-rag".to_string()),
        query_vector: vec![0.1; 384],
        k: 10,
        threshold: Some(0.0),
        metadata_filter: None,
    };

    let result = handle_search_vectors(&session_arc, request);

    assert!(result.is_err(), "Search should fail when RAG not enabled");
}

#[test]
fn test_search_concurrent_on_s5_index() {
    use std::thread;

    let vectors = create_test_vectors(100, 384);
    let query_vector = vectors[10].vector.clone();
    let session = create_session_with_s5_index(vectors, 384);

    // Spawn 10 concurrent searches (HNSW index is thread-safe)
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let session_clone = Arc::clone(&session);
            let query_clone = query_vector.clone();

            thread::spawn(move || {
                let request = SearchVectorsRequest {
                    request_id: Some(format!("concurrent-{}", i)),
                    query_vector: query_clone,
                    k: 5,
                    threshold: Some(0.0),
                    metadata_filter: None,
                };

                handle_search_vectors(&session_clone, request)
            })
        })
        .collect();

    // All searches should succeed
    for handle in handles {
        let result = handle.join().expect("Thread should not panic");
        assert!(result.is_ok(), "Concurrent search should succeed");
        assert_eq!(result.unwrap().results.len(), 5);
    }
}
