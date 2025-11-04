// TDD Tests for RAG Session Integration (Sub-phase 1.3)
// Written FIRST before implementation

use fabstir_llm_node::api::websocket::session::{SessionConfig, WebSocketSession};
use fabstir_llm_node::rag::session_vector_store::SessionVectorStore;
use serde_json::json;
use std::sync::{Arc, Mutex};

#[test]
fn test_session_creates_vector_store() {
    let mut session = WebSocketSession::new("test-session");
    session.enable_rag(1000);

    let store = session.get_vector_store();
    assert!(store.is_some());

    let store = store.unwrap();
    let store_locked = store.lock().unwrap();
    assert_eq!(store_locked.session_id(), "test-session");
    assert_eq!(store_locked.max_vectors(), 1000);
    assert_eq!(store_locked.count(), 0);
}

#[test]
fn test_session_rag_disabled_no_store() {
    let session = WebSocketSession::new("test-session");
    // Don't call enable_rag()

    let store = session.get_vector_store();
    assert!(store.is_none());
}

#[test]
fn test_session_vector_store_isolated() {
    let mut session1 = WebSocketSession::new("session-1");
    let mut session2 = WebSocketSession::new("session-2");

    session1.enable_rag(1000);
    session2.enable_rag(1000);

    let store1 = session1.get_vector_store().unwrap();
    let store2 = session2.get_vector_store().unwrap();

    // Add vector to store1
    let mut store1_locked = store1.lock().unwrap();
    store1_locked.add("doc1".to_string(), vec![0.1; 384], json!({})).unwrap();
    drop(store1_locked);

    // Store2 should be empty
    let store2_locked = store2.lock().unwrap();
    assert_eq!(store2_locked.count(), 0);
    assert!(store2_locked.get("doc1").is_none());
}

#[test]
fn test_session_cleanup_clears_vectors() {
    let mut session = WebSocketSession::new("test-session");
    session.enable_rag(1000);

    let store = session.get_vector_store().unwrap();

    // Add some vectors
    {
        let mut store_locked = store.lock().unwrap();
        for i in 0..5 {
            store_locked.add(format!("doc{}", i), vec![0.1; 384], json!({})).unwrap();
        }
        assert_eq!(store_locked.count(), 5);
    }

    // Clear session (simulates disconnect)
    session.clear();

    // Vectors should be cleared
    let store_locked = store.lock().unwrap();
    assert_eq!(store_locked.count(), 0);
}

#[test]
fn test_concurrent_sessions_independent_stores() {
    use std::thread;

    let mut session1 = WebSocketSession::new("session-1");
    let mut session2 = WebSocketSession::new("session-2");

    session1.enable_rag(1000);
    session2.enable_rag(1000);

    let store1 = session1.get_vector_store().unwrap();
    let store2 = session2.get_vector_store().unwrap();

    // Spawn threads to add vectors concurrently
    let store1_clone = Arc::clone(&store1);
    let handle1 = thread::spawn(move || {
        for i in 0..10 {
            let mut store = store1_clone.lock().unwrap();
            let _ = store.add(format!("doc1-{}", i), vec![0.1; 384], json!({"session": 1}));
        }
    });

    let store2_clone = Arc::clone(&store2);
    let handle2 = thread::spawn(move || {
        for i in 0..10 {
            let mut store = store2_clone.lock().unwrap();
            let _ = store.add(format!("doc2-{}", i), vec![0.2; 384], json!({"session": 2}));
        }
    });

    handle1.join().unwrap();
    handle2.join().unwrap();

    // Each store should have 10 vectors
    let store1_locked = store1.lock().unwrap();
    let store2_locked = store2.lock().unwrap();

    assert_eq!(store1_locked.count(), 10);
    assert_eq!(store2_locked.count(), 10);

    // Verify isolation
    assert!(store1_locked.get("doc1-0").is_some());
    assert!(store1_locked.get("doc2-0").is_none());

    assert!(store2_locked.get("doc2-0").is_some());
    assert!(store2_locked.get("doc1-0").is_none());
}

#[test]
fn test_max_vectors_configurable() {
    let mut session_small = WebSocketSession::new("session-small");
    let mut session_large = WebSocketSession::new("session-large");

    session_small.enable_rag(5);
    session_large.enable_rag(10000);

    let store_small = session_small.get_vector_store().unwrap();
    let store_large = session_large.get_vector_store().unwrap();

    let store_small_locked = store_small.lock().unwrap();
    let store_large_locked = store_large.lock().unwrap();

    assert_eq!(store_small_locked.max_vectors(), 5);
    assert_eq!(store_large_locked.max_vectors(), 10000);
}

#[test]
fn test_session_disconnect_frees_memory() {
    let mut session = WebSocketSession::new("test-session");
    session.enable_rag(1000);

    let store = session.get_vector_store().unwrap();

    // Add 100 vectors
    {
        let mut store_locked = store.lock().unwrap();
        for i in 0..100 {
            store_locked.add(format!("doc{}", i), vec![0.1; 384], json!({})).unwrap();
        }
        assert_eq!(store_locked.count(), 100);
    }

    // Simulate disconnect by clearing session
    session.clear();

    // Memory should be freed
    let store_locked = store.lock().unwrap();
    assert_eq!(store_locked.count(), 0);

    // Can add new vectors after clear
    drop(store_locked);
    let mut store_locked = store.lock().unwrap();
    store_locked.add("new".to_string(), vec![0.5; 384], json!({})).unwrap();
    assert_eq!(store_locked.count(), 1);
}

#[test]
fn test_session_vector_store_thread_safe() {
    use std::thread;

    let mut session = WebSocketSession::new("test-session");
    session.enable_rag(1000);

    let store = session.get_vector_store().unwrap();

    let mut handles = vec![];

    // Spawn 5 threads, each adding 20 vectors
    for thread_id in 0..5 {
        let store_clone = Arc::clone(&store);
        let handle = thread::spawn(move || {
            for i in 0..20 {
                let mut store = store_clone.lock().unwrap();
                let id = format!("thread{}-doc{}", thread_id, i);
                let _ = store.add(id, vec![thread_id as f32; 384], json!({"thread": thread_id}));
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Should have 100 vectors total (5 threads * 20 vectors)
    let store_locked = store.lock().unwrap();
    assert_eq!(store_locked.count(), 100);
}

#[test]
fn test_multiple_sessions_no_memory_leak() {
    // Create and destroy 10 sessions
    for i in 0..10 {
        let mut session = WebSocketSession::new(format!("session-{}", i));
        session.enable_rag(1000);

        let store = session.get_vector_store().unwrap();

        // Add vectors
        {
            let mut store_locked = store.lock().unwrap();
            for j in 0..50 {
                store_locked.add(format!("doc{}", j), vec![0.1; 384], json!({})).unwrap();
            }
            assert_eq!(store_locked.count(), 50);
        }

        // Clear session (simulates disconnect)
        session.clear();

        // Verify cleared
        let store_locked = store.lock().unwrap();
        assert_eq!(store_locked.count(), 0);
    }

    // This test passes if no memory leak (would crash/hang otherwise)
}

#[test]
fn test_rag_enable_disable_toggle() {
    let mut session = WebSocketSession::new("test-session");

    // Start without RAG
    assert!(session.get_vector_store().is_none());

    // Enable RAG
    session.enable_rag(1000);
    assert!(session.get_vector_store().is_some());

    let store = session.get_vector_store().unwrap();

    // Add vectors
    {
        let mut store_locked = store.lock().unwrap();
        store_locked.add("doc1".to_string(), vec![0.1; 384], json!({})).unwrap();
        assert_eq!(store_locked.count(), 1);
    }

    // Disable RAG (clear removes it)
    session.clear();

    // Can still access store (Arc kept alive)
    let store_locked = store.lock().unwrap();
    assert_eq!(store_locked.count(), 0);
}
