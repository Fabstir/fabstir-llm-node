// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//
// Tests for session re-init preservation (v8.15.5)
// Bug: second encrypted_session_init wipes uploaded vectors
use fabstir_llm_node::api::websocket::session::SessionConfig;
use fabstir_llm_node::api::websocket::session_store::{SessionStore, SessionStoreConfig};
use fabstir_llm_node::job_processor::Message;
use serde_json::json;

fn make_store(max_sessions: usize) -> SessionStore {
    SessionStore::new(SessionStoreConfig {
        max_sessions,
        ..Default::default()
    })
}

#[tokio::test]
async fn test_ensure_session_creates_when_not_exists() {
    let mut store = make_store(10);
    let result = store
        .ensure_session_exists_with_chain("s1".to_string(), SessionConfig::default(), 84532)
        .await;
    assert_eq!(result.unwrap(), true, "should return true for new session");
    assert!(store.get_session("s1").await.is_some());
}

#[tokio::test]
async fn test_ensure_session_noop_when_exists() {
    let mut store = make_store(10);
    // Create first
    store
        .create_session_with_chain("s1".to_string(), SessionConfig::default(), 84532)
        .await
        .unwrap();
    // Ensure again
    let result = store
        .ensure_session_exists_with_chain("s1".to_string(), SessionConfig::default(), 84532)
        .await;
    assert_eq!(result.unwrap(), false, "should return false for existing");
    assert_eq!(store.async_session_count().await, 1);
}

#[tokio::test]
async fn test_ensure_session_preserves_vectors_on_reinit() {
    let mut store = make_store(10);
    // Step 1: create session
    store
        .create_session_with_chain("s1".to_string(), SessionConfig::default(), 84532)
        .await
        .unwrap();
    // Step 2: enable RAG + upload vectors
    let session = store
        .get_or_create_rag_session("s1".to_string(), 100_000)
        .await
        .unwrap();
    let vs = session
        .get_vector_store()
        .expect("vector_store should exist");
    {
        let mut locked = vs.lock().unwrap();
        for i in 0..5 {
            locked
                .add(format!("v{}", i), vec![0.1_f32; 384], json!({"i": i}))
                .unwrap();
        }
    }
    // Step 3: re-init (the bug scenario)
    let result = store
        .ensure_session_exists_with_chain("s1".to_string(), SessionConfig::default(), 84532)
        .await;
    assert_eq!(result.unwrap(), false);
    // Step 4: verify vectors survived
    let session = store.get_session("s1").await.unwrap();
    let vs = session
        .get_vector_store()
        .expect("vector_store must survive re-init");
    assert_eq!(vs.lock().unwrap().count(), 5, "all 5 vectors must survive");
}

#[tokio::test]
async fn test_ensure_session_preserves_conversation_history() {
    let mut store = make_store(10);
    store
        .create_session_with_chain("s1".to_string(), SessionConfig::default(), 84532)
        .await
        .unwrap();
    // Add messages via update_session
    store
        .update_session(
            "s1",
            Message {
                role: "user".to_string(),
                content: "hello".to_string(),
                timestamp: None,
            },
        )
        .await
        .unwrap();
    store
        .update_session(
            "s1",
            Message {
                role: "assistant".to_string(),
                content: "hi".to_string(),
                timestamp: None,
            },
        )
        .await
        .unwrap();
    // Re-init
    store
        .ensure_session_exists_with_chain("s1".to_string(), SessionConfig::default(), 84532)
        .await
        .unwrap();
    // Verify history survived
    let session = store.get_session("s1").await.unwrap();
    assert_eq!(session.conversation_history().len(), 2);
}

#[tokio::test]
async fn test_ensure_session_respects_max_sessions() {
    let mut store = make_store(2);
    store
        .create_session_with_chain("s1".to_string(), SessionConfig::default(), 84532)
        .await
        .unwrap();
    store
        .create_session_with_chain("s2".to_string(), SessionConfig::default(), 84532)
        .await
        .unwrap();
    // 3rd session should fail
    let result = store
        .ensure_session_exists_with_chain("s3".to_string(), SessionConfig::default(), 84532)
        .await;
    assert!(result.is_err(), "should reject when at capacity");
}

#[tokio::test]
async fn test_ensure_existing_does_not_count_against_max() {
    let mut store = make_store(2);
    store
        .create_session_with_chain("s1".to_string(), SessionConfig::default(), 84532)
        .await
        .unwrap();
    store
        .create_session_with_chain("s2".to_string(), SessionConfig::default(), 84532)
        .await
        .unwrap();
    // Re-init existing should succeed even at capacity
    let result = store
        .ensure_session_exists_with_chain("s1".to_string(), SessionConfig::default(), 84532)
        .await;
    assert_eq!(
        result.unwrap(),
        false,
        "existing session at capacity must succeed"
    );
}

#[tokio::test]
async fn test_create_session_still_replaces() {
    let mut store = make_store(10);
    store
        .create_session_with_chain("s1".to_string(), SessionConfig::default(), 84532)
        .await
        .unwrap();
    // Add a message
    store
        .update_session(
            "s1",
            Message {
                role: "user".to_string(),
                content: "hello".to_string(),
                timestamp: None,
            },
        )
        .await
        .unwrap();
    // create_session_with_chain again (old behaviour: unconditional replace)
    store
        .create_session_with_chain("s1".to_string(), SessionConfig::default(), 84532)
        .await
        .unwrap();
    // History should be gone (replaced)
    let session = store.get_session("s1").await.unwrap();
    assert_eq!(
        session.conversation_history().len(),
        0,
        "create_session_with_chain must still replace unconditionally"
    );
}
