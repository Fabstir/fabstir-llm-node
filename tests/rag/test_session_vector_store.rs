// TDD Tests for SessionVectorStore - Written FIRST before implementation
// Sub-phase 1.1: Create SessionVectorStore Struct

use fabstir_llm_node::rag::session_vector_store::{SessionVectorStore, VectorEntry};
use serde_json::json;
use std::time::Instant;

#[test]
fn test_new_creates_empty_store() {
    let store = SessionVectorStore::new("session-123".to_string(), 1000);

    assert_eq!(store.session_id(), "session-123");
    assert_eq!(store.count(), 0);
    assert_eq!(store.max_vectors(), 1000);
}

#[test]
fn test_add_single_vector() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);
    let vector = vec![0.1; 384]; // 384-dimensional vector
    let metadata = json!({"title": "Test Document"});

    let result = store.add("doc1".to_string(), vector.clone(), metadata.clone());

    assert!(result.is_ok());
    assert_eq!(store.count(), 1);

    let entry = store.get("doc1");
    assert!(entry.is_some());
    let entry = entry.unwrap();
    assert_eq!(entry.vector, vector);
    assert_eq!(entry.metadata, metadata);
}

#[test]
fn test_add_validates_dimensions() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);

    // Test with wrong dimensions (should be 384)
    let vector_wrong = vec![0.1; 256]; // Wrong: 256 dimensions
    let metadata = json!({"title": "Test"});

    let result = store.add("doc1".to_string(), vector_wrong, metadata.clone());

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("384"));
    assert_eq!(store.count(), 0); // Should not be added

    // Test with correct dimensions
    let vector_correct = vec![0.1; 384]; // Correct: 384 dimensions
    let result = store.add("doc2".to_string(), vector_correct, metadata);

    assert!(result.is_ok());
    assert_eq!(store.count(), 1);
}

#[test]
fn test_add_with_metadata() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);
    let vector = vec![0.5; 384];
    let metadata = json!({
        "title": "Machine Learning Basics",
        "author": "John Doe",
        "page": 42,
        "tags": ["ml", "ai", "tutorial"]
    });

    store
        .add("doc1".to_string(), vector, metadata.clone())
        .unwrap();

    let entry = store.get("doc1").unwrap();
    assert_eq!(entry.metadata["title"], "Machine Learning Basics");
    assert_eq!(entry.metadata["page"], 42);
    assert_eq!(entry.metadata["tags"][0], "ml");
}

#[test]
fn test_add_duplicate_id_replaces() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);

    // Add first vector
    let vector1 = vec![0.1; 384];
    let metadata1 = json!({"version": 1});
    store.add("doc1".to_string(), vector1, metadata1).unwrap();
    assert_eq!(store.count(), 1);

    // Add second vector with same ID (should replace)
    let vector2 = vec![0.9; 384];
    let metadata2 = json!({"version": 2});
    store
        .add("doc1".to_string(), vector2.clone(), metadata2.clone())
        .unwrap();

    assert_eq!(store.count(), 1); // Still 1, not 2
    let entry = store.get("doc1").unwrap();
    assert_eq!(entry.vector, vector2);
    assert_eq!(entry.metadata["version"], 2);
}

#[test]
fn test_get_existing_vector() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);
    let vector = vec![0.3; 384];
    let metadata = json!({"test": true});

    store
        .add("doc1".to_string(), vector.clone(), metadata.clone())
        .unwrap();

    let entry = store.get("doc1");
    assert!(entry.is_some());

    let entry = entry.unwrap();
    assert_eq!(entry.vector.len(), 384);
    assert_eq!(entry.metadata, metadata);
    assert!(entry.created_at <= Instant::now());
}

#[test]
fn test_get_nonexistent_returns_none() {
    let store = SessionVectorStore::new("session-123".to_string(), 1000);

    let entry = store.get("nonexistent");
    assert!(entry.is_none());
}

#[test]
fn test_delete_existing_vector() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);
    let vector = vec![0.2; 384];
    let metadata = json!({"title": "Test"});

    store.add("doc1".to_string(), vector, metadata).unwrap();
    assert_eq!(store.count(), 1);

    let deleted = store.delete("doc1");
    assert!(deleted);
    assert_eq!(store.count(), 0);
    assert!(store.get("doc1").is_none());
}

#[test]
fn test_delete_nonexistent_returns_false() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);

    let deleted = store.delete("nonexistent");
    assert!(!deleted);
    assert_eq!(store.count(), 0);
}

#[test]
fn test_count_accurate() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);
    assert_eq!(store.count(), 0);

    // Add 5 vectors
    for i in 0..5 {
        let vector = vec![0.1 * i as f32; 384];
        let metadata = json!({"id": i});
        store.add(format!("doc{}", i), vector, metadata).unwrap();
    }

    assert_eq!(store.count(), 5);

    // Delete 2
    store.delete("doc1");
    store.delete("doc3");

    assert_eq!(store.count(), 3);
}

#[test]
fn test_clear_removes_all() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 1000);

    // Add multiple vectors
    for i in 0..10 {
        let vector = vec![0.1; 384];
        let metadata = json!({"index": i});
        store.add(format!("doc{}", i), vector, metadata).unwrap();
    }

    assert_eq!(store.count(), 10);

    store.clear();

    assert_eq!(store.count(), 0);
    assert!(store.get("doc0").is_none());
    assert!(store.get("doc9").is_none());
}

#[test]
fn test_max_vectors_enforced() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 5); // Max 5 vectors

    // Add 5 vectors (should succeed)
    for i in 0..5 {
        let vector = vec![0.1; 384];
        let metadata = json!({"index": i});
        let result = store.add(format!("doc{}", i), vector, metadata);
        assert!(result.is_ok());
    }

    assert_eq!(store.count(), 5);

    // Try to add 6th vector (should fail)
    let vector = vec![0.1; 384];
    let metadata = json!({"index": 6});
    let result = store.add("doc6".to_string(), vector, metadata);

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("max"));
    assert_eq!(store.count(), 5); // Still 5, not 6
}

#[test]
fn test_multiple_sessions_isolated() {
    let mut store1 = SessionVectorStore::new("session-1".to_string(), 1000);
    let mut store2 = SessionVectorStore::new("session-2".to_string(), 1000);

    let vector = vec![0.1; 384];
    let metadata1 = json!({"session": 1});
    let metadata2 = json!({"session": 2});

    store1
        .add("doc1".to_string(), vector.clone(), metadata1.clone())
        .unwrap();
    store2
        .add("doc1".to_string(), vector, metadata2.clone())
        .unwrap();

    // Both have "doc1" but they're isolated
    assert_eq!(store1.count(), 1);
    assert_eq!(store2.count(), 1);

    assert_eq!(store1.get("doc1").unwrap().metadata, metadata1);
    assert_eq!(store2.get("doc1").unwrap().metadata, metadata2);
}

#[test]
fn test_concurrent_add_safe() {
    use std::sync::{Arc, Mutex};
    use std::thread;

    let store = Arc::new(Mutex::new(SessionVectorStore::new(
        "session-123".to_string(),
        1000,
    )));
    let mut handles = vec![];

    // Spawn 10 threads, each adding 10 vectors
    for i in 0..10 {
        let store_clone = Arc::clone(&store);
        let handle = thread::spawn(move || {
            for j in 0..10 {
                let vector = vec![0.1; 384];
                let metadata = json!({"thread": i, "index": j});
                let id = format!("doc-{}-{}", i, j);
                let mut s = store_clone.lock().unwrap();
                let _ = s.add(id, vector, metadata);
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    let store = store.lock().unwrap();
    assert_eq!(store.count(), 100); // All 100 vectors added
}

#[test]
fn test_memory_usage_reasonable() {
    let mut store = SessionVectorStore::new("session-123".to_string(), 10000);

    // Add 1000 vectors (should use ~1.5MB: 1000 * 384 * 4 bytes)
    for i in 0..1000 {
        let vector = vec![0.1; 384];
        let metadata = json!({"index": i, "title": format!("Document {}", i)});
        store.add(format!("doc{}", i), vector, metadata).unwrap();
    }

    assert_eq!(store.count(), 1000);

    // Memory should be reasonable (this is a smoke test, not exact measurement)
    // Each vector: 384 floats * 4 bytes = 1536 bytes
    // 1000 vectors = ~1.5MB + overhead
    // Just verify we can create and use the store without OOM
}

// ============================================================================
// Issue 1: NaN/Infinity Validation Tests (Critical Fix)
// ============================================================================

#[test]
fn test_add_rejects_nan_values() {
    let mut store = SessionVectorStore::new("test-session".to_string(), 1000);

    // Create vector with NaN value
    let mut vector = vec![0.5; 384];
    vector[100] = f32::NAN;

    let result = store.add("doc1".to_string(), vector, json!({}));

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("NaN") || err_msg.contains("invalid"));
    assert_eq!(store.count(), 0); // Should not be added
}

#[test]
fn test_add_rejects_infinity_values() {
    let mut store = SessionVectorStore::new("test-session".to_string(), 1000);

    // Create vector with Infinity value
    let mut vector = vec![0.5; 384];
    vector[200] = f32::INFINITY;

    let result = store.add("doc1".to_string(), vector, json!({}));

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Infinity") || err_msg.contains("invalid"));
    assert_eq!(store.count(), 0); // Should not be added
}

#[test]
fn test_add_rejects_negative_infinity() {
    let mut store = SessionVectorStore::new("test-session".to_string(), 1000);

    // Create vector with -Infinity value
    let mut vector = vec![0.5; 384];
    vector[150] = f32::NEG_INFINITY;

    let result = store.add("doc1".to_string(), vector, json!({}));

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Infinity") || err_msg.contains("invalid"));
    assert_eq!(store.count(), 0); // Should not be added
}

#[test]
fn test_search_zero_magnitude_vector() {
    let mut store = SessionVectorStore::new("test-session".to_string(), 1000);

    // Add normal vectors
    store
        .add("doc1".to_string(), vec![0.5; 384], json!({}))
        .unwrap();
    store
        .add("doc2".to_string(), vec![1.0; 384], json!({}))
        .unwrap();

    // Add zero vector (all zeros) - should succeed
    let zero_vector = vec![0.0; 384];
    let result = store.add("doc-zero".to_string(), zero_vector.clone(), json!({}));
    assert!(result.is_ok()); // Zero is valid, just edge case

    // Search with zero vector - should return results (scores will be 0.0 or NaN handled)
    let search_result = store.search(zero_vector, 5, None);
    assert!(search_result.is_ok());
}

// ============================================================================
// Issue 2: Metadata Size Limit Tests (Critical Fix)
// ============================================================================

#[test]
fn test_add_rejects_oversized_metadata() {
    let mut store = SessionVectorStore::new("test-session".to_string(), 1000);

    // Create metadata larger than 10KB limit
    let large_string = "x".repeat(11 * 1024); // 11KB string
    let oversized_metadata = json!({
        "content": large_string,
        "title": "Test"
    });

    let result = store.add("doc1".to_string(), vec![0.5; 384], oversized_metadata);

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Metadata too large")
            || err_msg.contains("metadata")
            || err_msg.contains("size")
    );
    assert_eq!(store.count(), 0); // Should not be added
}

#[test]
fn test_add_accepts_reasonable_metadata() {
    let mut store = SessionVectorStore::new("test-session".to_string(), 1000);

    // Create metadata under 10KB limit (about 9KB)
    let reasonable_string = "x".repeat(9 * 1024);
    let metadata = json!({
        "content": reasonable_string,
        "title": "Test"
    });

    let result = store.add("doc1".to_string(), vec![0.5; 384], metadata);

    assert!(result.is_ok());
    assert_eq!(store.count(), 1);
}
