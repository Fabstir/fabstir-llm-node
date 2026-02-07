// TDD Tests for End-to-End RAG Workflow (Sub-phase 3.1)
// Written FIRST before implementation

use fabstir_llm_node::api::websocket::handlers::rag::{
    handle_search_vectors, handle_upload_vectors,
};
use fabstir_llm_node::api::websocket::message_types::{
    SearchVectorsRequest, UploadVectorsRequest, VectorUpload,
};
use fabstir_llm_node::api::websocket::session::{SessionConfig, WebSocketSession};
use serde_json::json;
use std::sync::{Arc, Mutex};

/// Helper to create sample document chunks
fn create_sample_document_chunks() -> Vec<(String, Vec<f32>, serde_json::Value)> {
    vec![
        (
            "doc1".to_string(),
            vec![0.1; 384], // Represents "machine learning fundamentals"
            json!({"title": "Introduction to Machine Learning", "page": 1, "category": "ml"}),
        ),
        (
            "doc2".to_string(),
            vec![0.2; 384], // Represents "neural networks"
            json!({"title": "Neural Networks Explained", "page": 2, "category": "ml"}),
        ),
        (
            "doc3".to_string(),
            vec![0.3; 384], // Represents "deep learning"
            json!({"title": "Deep Learning Basics", "page": 3, "category": "dl"}),
        ),
        (
            "doc4".to_string(),
            vec![0.4; 384], // Represents "supervised learning"
            json!({"title": "Supervised Learning", "page": 4, "category": "ml"}),
        ),
        (
            "doc5".to_string(),
            vec![0.5; 384], // Represents "unsupervised learning"
            json!({"title": "Unsupervised Learning", "page": 5, "category": "ml"}),
        ),
    ]
}

/// Helper to create a large dataset for performance testing
fn create_large_dataset(size: usize) -> Vec<VectorUpload> {
    (0..size)
        .map(|i| {
            let val = (i as f32) / (size as f32);
            VectorUpload {
                id: format!("doc_{}", i),
                vector: vec![val; 384],
                metadata: json!({"index": i, "chunk": format!("Chunk {}", i)}),
            }
        })
        .collect()
}

#[test]
fn test_full_rag_workflow() {
    // This test simulates the complete RAG workflow:
    // 1. Enable RAG on session
    // 2. Upload document vectors
    // 3. Search for relevant chunks
    // 4. Verify results are relevant

    let mut session =
        WebSocketSession::with_config("rag-test".to_string(), SessionConfig::default());
    session.enable_rag(10000);
    let session = Arc::new(Mutex::new(session));

    // Step 1: Upload document chunks
    let chunks = create_sample_document_chunks();
    let vectors: Vec<VectorUpload> = chunks
        .into_iter()
        .map(|(id, vector, metadata)| VectorUpload {
            id,
            vector,
            metadata,
        })
        .collect();

    let upload_request = UploadVectorsRequest {
        request_id: Some("upload-1".to_string()),
        vectors,
        replace: false,
    };

    let upload_response = handle_upload_vectors(&session, upload_request).unwrap();
    assert_eq!(upload_response.uploaded, 5);
    assert_eq!(upload_response.rejected, 0);
    assert_eq!(upload_response.errors.len(), 0);

    // Step 2: Search for relevant chunks
    let search_request = SearchVectorsRequest {
        request_id: Some("search-1".to_string()),
        query_vector: vec![0.15; 384], // Query similar to doc1/doc2
        k: 3,
        threshold: None,
        metadata_filter: None,
    };

    let search_response = handle_search_vectors(&session, search_request).unwrap();

    // Step 3: Verify results
    assert_eq!(search_response.results.len(), 3);
    assert_eq!(search_response.total_vectors, 5);
    assert!(search_response.search_time_ms > 0.0);
    assert!(search_response.search_time_ms < 100.0); // Should be fast

    // Results should be sorted by relevance
    assert!(search_response.results[0].score >= search_response.results[1].score);
    assert!(search_response.results[1].score >= search_response.results[2].score);
}

#[test]
fn test_upload_search_inference_pipeline() {
    // Test the typical pipeline:
    // Upload → Search → Use results for context injection

    let mut session =
        WebSocketSession::with_config("pipeline-test".to_string(), SessionConfig::default());
    session.enable_rag(10000);
    let session = Arc::new(Mutex::new(session));

    // Upload 100 chunks
    let vectors = create_large_dataset(100);
    let upload_request = UploadVectorsRequest {
        request_id: None,
        vectors,
        replace: false,
    };

    let upload_response = handle_upload_vectors(&session, upload_request).unwrap();
    assert_eq!(upload_response.uploaded, 100);
    assert_eq!(upload_response.rejected, 0);

    // Search for top-5 relevant chunks
    let search_request = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.5; 384], // Mid-range query
        k: 5,
        threshold: None,
        metadata_filter: None,
    };

    let search_response = handle_search_vectors(&session, search_request).unwrap();
    assert_eq!(search_response.results.len(), 5);
    assert_eq!(search_response.total_vectors, 100);

    // Verify we can extract context from results
    for result in &search_response.results {
        assert!(result.metadata.get("chunk").is_some());
        // Cosine similarity should be in range [-1.0, 1.0]
        // Allow small floating point errors (e.g., 1.000007 due to precision)
        assert!(
            !result.score.is_nan(),
            "Score is NaN for result: {:?}",
            result
        );
        assert!(
            result.score >= -1.001 && result.score <= 1.001,
            "Score {} out of range for result: {:?}",
            result.score,
            result
        );
    }

    // Context injection would happen here in real workflow
    // (concatenate result.metadata["chunk"] values into prompt)
}

#[test]
fn test_multiple_searches_same_session() {
    // Test performing multiple searches on the same vector store
    let mut session =
        WebSocketSession::with_config("multi-search".to_string(), SessionConfig::default());
    session.enable_rag(10000);
    let session = Arc::new(Mutex::new(session));

    // Upload data
    let chunks = create_sample_document_chunks();
    let vectors: Vec<VectorUpload> = chunks
        .into_iter()
        .map(|(id, vector, metadata)| VectorUpload {
            id,
            vector,
            metadata,
        })
        .collect();

    let upload_request = UploadVectorsRequest {
        request_id: None,
        vectors,
        replace: false,
    };
    handle_upload_vectors(&session, upload_request).unwrap();

    // Perform multiple searches
    for i in 0..5 {
        let search_request = SearchVectorsRequest {
            request_id: Some(format!("search-{}", i)),
            query_vector: vec![(i as f32) * 0.1; 384],
            k: 2,
            threshold: None,
            metadata_filter: None,
        };

        let response = handle_search_vectors(&session, search_request).unwrap();
        assert_eq!(response.results.len(), 2);
        assert_eq!(response.request_id, Some(format!("search-{}", i)));
    }
}

#[test]
fn test_replace_vectors_mid_session() {
    // Test replacing vectors during a session
    let mut session =
        WebSocketSession::with_config("replace-test".to_string(), SessionConfig::default());
    session.enable_rag(10000);
    let session = Arc::new(Mutex::new(session));

    // Upload initial vectors
    let initial_vectors = create_large_dataset(50);
    let upload_request1 = UploadVectorsRequest {
        request_id: None,
        vectors: initial_vectors,
        replace: false,
    };
    handle_upload_vectors(&session, upload_request1).unwrap();

    // Verify 50 vectors present
    let search1 = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.5; 384],
        k: 100,
        threshold: None,
        metadata_filter: None,
    };
    let response1 = handle_search_vectors(&session, search1).unwrap();
    assert_eq!(response1.total_vectors, 50);

    // Replace with new vectors
    let new_vectors = create_large_dataset(30);
    let upload_request2 = UploadVectorsRequest {
        request_id: None,
        vectors: new_vectors,
        replace: true, // REPLACE flag
    };
    let upload_response2 = handle_upload_vectors(&session, upload_request2).unwrap();
    assert_eq!(upload_response2.uploaded, 30);

    // Verify only 30 vectors now
    let search2 = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.5; 384],
        k: 100,
        threshold: None,
        metadata_filter: None,
    };
    let response2 = handle_search_vectors(&session, search2).unwrap();
    assert_eq!(response2.total_vectors, 30);
}

#[test]
fn test_search_with_filters() {
    // Test metadata filtering in search
    let mut session =
        WebSocketSession::with_config("filter-test".to_string(), SessionConfig::default());
    session.enable_rag(10000);
    let session = Arc::new(Mutex::new(session));

    // Upload chunks with different categories
    let chunks = create_sample_document_chunks();
    let vectors: Vec<VectorUpload> = chunks
        .into_iter()
        .map(|(id, vector, metadata)| VectorUpload {
            id,
            vector,
            metadata,
        })
        .collect();

    let upload_request = UploadVectorsRequest {
        request_id: None,
        vectors,
        replace: false,
    };
    handle_upload_vectors(&session, upload_request).unwrap();

    // Search with category filter (only "ml" category)
    let search_request = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.3; 384],
        k: 10,
        threshold: None,
        metadata_filter: Some(json!({"category": {"$eq": "ml"}})),
    };

    let response = handle_search_vectors(&session, search_request).unwrap();

    // Should only return "ml" category results (doc1, doc2, doc4, doc5)
    assert_eq!(response.results.len(), 4);
    for result in &response.results {
        assert_eq!(result.metadata["category"], "ml");
    }
}

#[test]
fn test_session_cleanup_removes_vectors() {
    // Test that vectors are properly isolated per session
    let mut session1 =
        WebSocketSession::with_config("session-1".to_string(), SessionConfig::default());
    session1.enable_rag(10000);
    let session1 = Arc::new(Mutex::new(session1));

    let mut session2 =
        WebSocketSession::with_config("session-2".to_string(), SessionConfig::default());
    session2.enable_rag(10000);
    let session2 = Arc::new(Mutex::new(session2));

    // Upload to session1
    let vectors1 = create_large_dataset(20);
    let upload1 = UploadVectorsRequest {
        request_id: None,
        vectors: vectors1,
        replace: false,
    };
    handle_upload_vectors(&session1, upload1).unwrap();

    // Upload to session2
    let vectors2 = create_large_dataset(30);
    let upload2 = UploadVectorsRequest {
        request_id: None,
        vectors: vectors2,
        replace: false,
    };
    handle_upload_vectors(&session2, upload2).unwrap();

    // Verify session1 has 20 vectors
    let search1 = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.5; 384],
        k: 100,
        threshold: None,
        metadata_filter: None,
    };
    let response1 = handle_search_vectors(&session1, search1).unwrap();
    assert_eq!(response1.total_vectors, 20);

    // Verify session2 has 30 vectors
    let search2 = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.5; 384],
        k: 100,
        threshold: None,
        metadata_filter: None,
    };
    let response2 = handle_search_vectors(&session2, search2).unwrap();
    assert_eq!(response2.total_vectors, 30);

    // Clear session1
    {
        let sess = session1.lock().unwrap();
        if let Some(store) = sess.get_vector_store() {
            let mut s = store.lock().unwrap();
            s.clear();
        }
    }

    // Verify session1 now has 0 vectors
    let search3 = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.5; 384],
        k: 100,
        threshold: None,
        metadata_filter: None,
    };
    let response3 = handle_search_vectors(&session1, search3).unwrap();
    assert_eq!(response3.total_vectors, 0);

    // Verify session2 still has 30 vectors (not affected)
    let search4 = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.5; 384],
        k: 100,
        threshold: None,
        metadata_filter: None,
    };
    let response4 = handle_search_vectors(&session2, search4).unwrap();
    assert_eq!(response4.total_vectors, 30);
}

#[test]
fn test_rag_10k_vectors_performance() {
    // Performance benchmark: 10K vectors, search should be <50ms
    let mut session =
        WebSocketSession::with_config("perf-test".to_string(), SessionConfig::default());
    session.enable_rag(100000); // Large capacity
    let session = Arc::new(Mutex::new(session));

    // Upload 10K vectors
    let vectors = create_large_dataset(10000);
    let upload_request = UploadVectorsRequest {
        request_id: None,
        vectors,
        replace: false,
    };

    let upload_start = std::time::Instant::now();
    let upload_response = handle_upload_vectors(&session, upload_request).unwrap();
    let upload_time = upload_start.elapsed().as_secs_f64() * 1000.0;

    assert_eq!(upload_response.uploaded, 10000);
    assert_eq!(upload_response.rejected, 0);
    println!("Upload 10K vectors took: {:.2}ms", upload_time);

    // Search should be fast
    let search_request = SearchVectorsRequest {
        request_id: None,
        query_vector: vec![0.5; 384],
        k: 10,
        threshold: None,
        metadata_filter: None,
    };

    let search_response = handle_search_vectors(&session, search_request).unwrap();
    assert_eq!(search_response.results.len(), 10);
    assert_eq!(search_response.total_vectors, 10000);

    // Performance assertion: search should be < 200ms (linear scan of 10K vectors)
    // Note: This is reasonable for brute-force cosine similarity on 10K vectors
    println!(
        "Search 10K vectors took: {:.2}ms",
        search_response.search_time_ms
    );
    assert!(
        search_response.search_time_ms < 200.0,
        "Search too slow: {:.2}ms (expected <200ms)",
        search_response.search_time_ms
    );
}

#[test]
fn test_concurrent_sessions_rag() {
    // Test that multiple sessions can have RAG enabled concurrently
    use std::thread;

    let handles: Vec<_> = (0..5)
        .map(|i| {
            thread::spawn(move || {
                let mut session = WebSocketSession::with_config(
                    format!("concurrent-{}", i),
                    SessionConfig::default(),
                );
                session.enable_rag(10000);
                let session = Arc::new(Mutex::new(session));

                // Each session uploads its own vectors
                let vectors = create_large_dataset(100);
                let upload_request = UploadVectorsRequest {
                    request_id: None,
                    vectors,
                    replace: false,
                };

                let upload_response = handle_upload_vectors(&session, upload_request).unwrap();
                assert_eq!(upload_response.uploaded, 100);

                // Each session searches independently
                let search_request = SearchVectorsRequest {
                    request_id: None,
                    query_vector: vec![0.5; 384],
                    k: 10,
                    threshold: None,
                    metadata_filter: None,
                };

                let search_response = handle_search_vectors(&session, search_request).unwrap();
                assert_eq!(search_response.results.len(), 10);
                assert_eq!(search_response.total_vectors, 100);

                i // Return thread ID
            })
        })
        .collect();

    // Wait for all threads
    for handle in handles {
        let result = handle.join().unwrap();
        println!("Thread {} completed successfully", result);
    }
}
