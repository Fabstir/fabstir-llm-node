// TDD Tests for SessionVectorStore - Vector Search (Sub-phase 1.2)
// Written FIRST before implementation

use fabstir_llm_node::rag::session_vector_store::{SearchResult, SessionVectorStore};
use serde_json::json;

#[test]
fn test_search_empty_store_returns_empty() {
    let store = SessionVectorStore::new("session-123".to_string(), 1000);
    let query = vec![0.1; 384];

    let results = store.search(query, 5, None);

    assert!(results.is_ok());
    let results = results.unwrap();
    assert_eq!(results.len(), 0);
}

#[test]
fn test_search_single_vector() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);
    let vector = vec![0.5; 384];
    store.add("doc1".to_string(), vector.clone(), json!({"title": "Test"})).unwrap();

    let query = vec![0.5; 384]; // Same as stored vector
    let results = store.search(query, 5, None).unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "doc1");
    assert!(results[0].score > 0.99); // Should be ~1.0 (identical)
    assert_eq!(results[0].metadata["title"], "Test");
}

#[test]
fn test_search_returns_top_k() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);

    // Add 10 vectors with varying similarity
    for i in 0..10 {
        let mut vector = vec![0.0; 384];
        vector[0] = i as f32 * 0.1; // Different first component
        store.add(format!("doc{}", i), vector, json!({"index": i})).unwrap();
    }

    let query = vec![0.5; 384]; // Query in middle
    let results = store.search(query, 3, None).unwrap();

    assert_eq!(results.len(), 3); // Only top 3
}

#[test]
fn test_search_sorted_by_score_descending() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);

    // Add 5 vectors with known similarities
    for i in 0..5 {
        let mut vector = vec![0.1; 384];
        vector[0] = i as f32 * 0.2; // 0.0, 0.2, 0.4, 0.6, 0.8
        store.add(format!("doc{}", i), vector, json!({"index": i})).unwrap();
    }

    let query = vec![0.8; 384]; // Should match doc4 best
    let results = store.search(query, 5, None).unwrap();

    // Verify descending order
    for i in 0..results.len() - 1 {
        assert!(results[i].score >= results[i + 1].score);
    }
}

#[test]
fn test_search_validates_query_dimensions() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);
    store.add("doc1".to_string(), vec![0.1; 384], json!({})).unwrap();

    let query_wrong = vec![0.1; 256]; // Wrong: 256 dimensions
    let result = store.search(query_wrong, 5, None);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("384"));
}

#[test]
fn test_search_with_threshold() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);

    // Add vectors with different similarities
    let high_sim = vec![1.0; 384]; // Will have high similarity with query
    let low_sim = vec![-1.0; 384]; // Will have low similarity with query

    store.add("high".to_string(), high_sim, json!({})).unwrap();
    store.add("low".to_string(), low_sim, json!({})).unwrap();

    let query = vec![1.0; 384];
    let results = store.search(query, 10, Some(0.9)).unwrap(); // Threshold 0.9

    // Only high similarity vector should pass
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "high");
}

#[test]
fn test_search_threshold_filters_results() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);

    // Add 5 vectors
    for i in 0..5 {
        let mut vector = vec![0.1; 384];
        vector[0] = i as f32;
        store.add(format!("doc{}", i), vector, json!({})).unwrap();
    }

    let query = vec![2.0; 384]; // Query close to doc2

    // Without threshold: get all 5
    let results_all = store.search(query.clone(), 10, None).unwrap();
    assert_eq!(results_all.len(), 5);

    // With high threshold: get fewer
    let results_filtered = store.search(query, 10, Some(0.95)).unwrap();
    assert!(results_filtered.len() < 5);
}

#[test]
fn test_search_exact_match_highest_score() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);

    // Create vectors with different patterns
    let mut vec1 = vec![0.0; 384];
    vec1[0] = 1.0; // Only first component

    let mut vec2 = vec![0.0; 384];
    vec2[1] = 1.0; // Only second component

    let mut vec3 = vec![0.0; 384];
    vec3[2] = 1.0; // Only third component

    store.add("doc1".to_string(), vec1, json!({})).unwrap();
    store.add("doc2".to_string(), vec2.clone(), json!({})).unwrap();
    store.add("doc3".to_string(), vec3, json!({})).unwrap();

    let query = vec2; // Exact match with doc2
    let results = store.search(query, 5, None).unwrap();

    // First result should be doc2 with score ~1.0
    assert_eq!(results[0].id, "doc2");
    assert!(results[0].score > 0.99);
}

#[test]
fn test_search_orthogonal_vectors_low_score() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);

    // Create orthogonal vectors
    let mut vec1 = vec![0.0; 384];
    vec1[0] = 1.0; // Only first component

    let mut vec2 = vec![0.0; 384];
    vec2[1] = 1.0; // Only second component (orthogonal)

    store.add("doc1".to_string(), vec1.clone(), json!({})).unwrap();
    store.add("doc2".to_string(), vec2, json!({})).unwrap();

    let query = vec1; // Same as doc1
    let results = store.search(query, 5, None).unwrap();

    // doc1 should have high score, doc2 should have low score (~0.0)
    assert_eq!(results[0].id, "doc1");
    assert!(results[0].score > 0.9);

    if results.len() > 1 {
        assert_eq!(results[1].id, "doc2");
        assert!(results[1].score < 0.1); // Orthogonal = low similarity
    }
}

#[test]
fn test_search_k_larger_than_store() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);

    // Add only 3 vectors
    for i in 0..3 {
        let vector = vec![i as f32; 384];
        store.add(format!("doc{}", i), vector, json!({})).unwrap();
    }

    let query = vec![0.5; 384];
    let results = store.search(query, 100, None).unwrap(); // Ask for 100, only 3 exist

    assert_eq!(results.len(), 3); // Should return all 3, not error
}

#[test]
fn test_search_with_metadata_filter_eq() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);

    store.add("doc1".to_string(), vec![0.1; 384], json!({"category": "science"})).unwrap();
    store.add("doc2".to_string(), vec![0.2; 384], json!({"category": "math"})).unwrap();
    store.add("doc3".to_string(), vec![0.3; 384], json!({"category": "science"})).unwrap();

    let query = vec![0.15; 384];
    let filter = json!({"category": {"$eq": "science"}});
    let results = store.search_with_filter(query, 10, filter).unwrap();

    // Only doc1 and doc3 should match
    assert_eq!(results.len(), 2);
    assert!(results.iter().any(|r| r.id == "doc1"));
    assert!(results.iter().any(|r| r.id == "doc3"));
    assert!(results.iter().all(|r| r.id != "doc2"));
}

#[test]
fn test_search_with_metadata_filter_in() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);

    store.add("doc1".to_string(), vec![0.1; 384], json!({"category": "science"})).unwrap();
    store.add("doc2".to_string(), vec![0.2; 384], json!({"category": "math"})).unwrap();
    store.add("doc3".to_string(), vec![0.3; 384], json!({"category": "history"})).unwrap();

    let query = vec![0.15; 384];
    let filter = json!({"category": {"$in": ["science", "math"]}});
    let results = store.search_with_filter(query, 10, filter).unwrap();

    // Only doc1 and doc2 should match
    assert_eq!(results.len(), 2);
    assert!(results.iter().any(|r| r.id == "doc1"));
    assert!(results.iter().any(|r| r.id == "doc2"));
    assert!(results.iter().all(|r| r.id != "doc3"));
}

// ============================================================================
// Issue 3: Search Edge Case Tests (Critical Fix)
// ============================================================================

#[test]
fn test_search_k_zero_returns_empty() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);

    // Add some vectors
    store.add("doc1".to_string(), vec![0.1; 384], json!({})).unwrap();
    store.add("doc2".to_string(), vec![0.2; 384], json!({})).unwrap();

    let query = vec![0.15; 384];
    let results = store.search(query, 0, None).unwrap();

    // k=0 should return empty array
    assert_eq!(results.len(), 0);
}

#[test]
fn test_search_negative_threshold() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);

    // Add vectors
    store.add("doc1".to_string(), vec![0.1; 384], json!({})).unwrap();
    store.add("doc2".to_string(), vec![0.2; 384], json!({})).unwrap();

    let query = vec![0.15; 384];
    let results = store.search(query, 5, Some(-0.5)).unwrap();

    // Negative threshold should still work (all scores >= -0.5)
    // This tests graceful handling rather than rejection
    assert!(results.len() >= 0); // Should not panic
}

#[test]
fn test_search_threshold_above_one() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);

    // Add vectors
    store.add("doc1".to_string(), vec![0.1; 384], json!({})).unwrap();
    store.add("doc2".to_string(), vec![0.2; 384], json!({})).unwrap();

    let query = vec![0.15; 384];
    let results = store.search(query, 5, Some(1.5)).unwrap();

    // Threshold >1.0 should return empty (cosine similarity max is 1.0)
    assert_eq!(results.len(), 0);
}

#[test]
fn test_add_after_delete_at_capacity() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 5); // Max 5 vectors

    // Fill to capacity
    for i in 0..5 {
        store.add(format!("doc{}", i), vec![i as f32; 384], json!({})).unwrap();
    }
    assert_eq!(store.count(), 5);

    // Try to add 6th (should fail)
    let result = store.add("doc5".to_string(), vec![5.0; 384], json!({}));
    assert!(result.is_err());
    assert_eq!(store.count(), 5);

    // Delete one
    assert!(store.delete("doc2"));
    assert_eq!(store.count(), 4);

    // Now should be able to add new one
    let result = store.add("doc-new".to_string(), vec![10.0; 384], json!({}));
    assert!(result.is_ok());
    assert_eq!(store.count(), 5);
}
