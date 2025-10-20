// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::api::websocket::{
    handlers::session_init::SessionInitHandler,
    memory_cache::ConversationCache,
    messages::{ConversationMessage, WebSocketMessage},
};
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_session_init_creates_memory_cache() {
    let handler = SessionInitHandler::new();

    let context = vec![
        ConversationMessage {
            role: "user".to_string(),
            content: "What is AI?".to_string(),
            timestamp: Some(1234567890),
            tokens: None,
            proof: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "AI is artificial intelligence".to_string(),
            timestamp: Some(1234567891),
            tokens: Some(10),
            proof: None,
        },
    ];

    let result = handler
        .handle_session_init("session-123", 12345, context.clone())
        .await
        .unwrap();

    assert_eq!(result.session_id, "session-123");
    assert_eq!(result.job_id, 12345);
    assert_eq!(result.message_count, 2);
    assert_eq!(result.total_tokens, 10);
}

#[tokio::test]
async fn test_session_init_with_empty_context() {
    let handler = SessionInitHandler::new();

    let result = handler
        .handle_session_init("new-session", 999, vec![])
        .await
        .unwrap();

    assert_eq!(result.session_id, "new-session");
    assert_eq!(result.job_id, 999);
    assert_eq!(result.message_count, 0);
    assert_eq!(result.total_tokens, 0);
}

#[tokio::test]
async fn test_session_init_validates_job_id() {
    let handler = SessionInitHandler::new();

    // Job ID 0 should be invalid
    let result = handler.handle_session_init("session-123", 0, vec![]).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Invalid job_id"));
}

#[tokio::test]
async fn test_session_init_replaces_existing_session() {
    let handler = SessionInitHandler::new();

    // First init
    let context1 = vec![ConversationMessage {
        role: "user".to_string(),
        content: "First question".to_string(),
        timestamp: None,
        tokens: None,
        proof: None,
    }];

    let result1 = handler
        .handle_session_init("session-123", 100, context1)
        .await
        .unwrap();
    assert_eq!(result1.message_count, 1);

    // Second init with same session_id should replace
    let context2 = vec![
        ConversationMessage {
            role: "user".to_string(),
            content: "New question".to_string(),
            timestamp: None,
            tokens: None,
            proof: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "New answer".to_string(),
            timestamp: None,
            tokens: Some(5),
            proof: None,
        },
    ];

    let result2 = handler
        .handle_session_init("session-123", 101, context2)
        .await
        .unwrap();

    assert_eq!(result2.message_count, 2);
    assert_eq!(result2.job_id, 101); // New job_id
}

#[tokio::test]
async fn test_session_init_counts_tokens_correctly() {
    let handler = SessionInitHandler::new();

    let context = vec![
        ConversationMessage {
            role: "user".to_string(),
            content: "Question 1".to_string(),
            timestamp: None,
            tokens: Some(2), // User tokens should be counted
            proof: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "Answer 1".to_string(),
            timestamp: None,
            tokens: Some(10),
            proof: None,
        },
        ConversationMessage {
            role: "user".to_string(),
            content: "Question 2".to_string(),
            timestamp: None,
            tokens: Some(3),
            proof: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "Answer 2".to_string(),
            timestamp: None,
            tokens: Some(15),
            proof: None,
        },
    ];

    let result = handler
        .handle_session_init("session-123", 200, context)
        .await
        .unwrap();

    assert_eq!(result.total_tokens, 30); // 2 + 10 + 3 + 15
}

#[tokio::test]
async fn test_session_init_preserves_message_order() {
    let handler = SessionInitHandler::new();

    let context = vec![
        ConversationMessage {
            role: "system".to_string(),
            content: "You are a helpful assistant".to_string(),
            timestamp: Some(1),
            tokens: None,
            proof: None,
        },
        ConversationMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
            timestamp: Some(2),
            tokens: None,
            proof: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "Hi there!".to_string(),
            timestamp: Some(3),
            tokens: Some(3),
            proof: None,
        },
    ];

    let result = handler
        .handle_session_init("session-123", 300, context.clone())
        .await
        .unwrap();

    // Verify cache has messages in correct order
    let cache = handler.get_cache("session-123").await.unwrap();
    let messages = cache.get_messages().await;

    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].role, "system");
    assert_eq!(messages[1].role, "user");
    assert_eq!(messages[2].role, "assistant");
}

#[tokio::test]
async fn test_concurrent_session_inits() {
    let handler = Arc::new(SessionInitHandler::new());

    let mut handles = vec![];

    // Create 10 concurrent sessions
    for i in 0..10 {
        let h = handler.clone();
        let handle = tokio::spawn(async move {
            let context = vec![ConversationMessage {
                role: "user".to_string(),
                content: format!("Question {}", i),
                timestamp: None,
                tokens: None,
                proof: None,
            }];

            h.handle_session_init(&format!("session-{}", i), i as u64 + 1, context)
                .await
        });
        handles.push(handle);
    }

    // All should succeed
    for handle in handles {
        let result = handle.await.unwrap().unwrap();
        assert!(result.session_id.starts_with("session-"));
    }
}
