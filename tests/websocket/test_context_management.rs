// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use fabstir_llm_node::api::websocket::{
    handlers::{inference::InferenceHandler, session_init::SessionInitHandler},
    memory_cache::MAX_CONTEXT_TOKENS,
    messages::ConversationMessage,
};
use std::sync::Arc;

#[tokio::test]
async fn test_context_window_trimming() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());

    // Create a large conversation history
    let mut context = vec![];
    for i in 0..100 {
        context.push(ConversationMessage {
            role: if i % 2 == 0 { "user" } else { "assistant" }.to_string(),
            content: format!("This is message number {} with some content", i),
            timestamp: Some(i as u64),
            tokens: Some(50), // Each message ~50 tokens = 5000 total (exceeds 4096 limit)
            proof: None,
        });
    }

    // Initialize with large context (5000 tokens total, should trim to 4096)
    session_handler
        .handle_session_init("trim-test", 1000, context)
        .await
        .unwrap();

    // Cache should have trimmed to fit within token limit
    let cache = session_handler.get_cache("trim-test").await.unwrap();
    assert!(cache.is_within_token_limit().await);

    // Should keep most recent messages
    let messages = cache.get_messages().await;
    assert!(messages.len() < 100); // Should have trimmed some messages

    // Most recent message should still be there
    let last_msg = messages.last().unwrap();
    assert!(last_msg.content.contains("99"));
}

#[tokio::test]
async fn test_context_window_with_system_prompt() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());

    // System prompt should be preserved during trimming
    let mut context = vec![ConversationMessage {
        role: "system".to_string(),
        content: "You are a helpful assistant. Always be concise.".to_string(),
        timestamp: Some(0),
        tokens: Some(10),
        proof: None,
    }];

    // Add many messages
    for i in 1..50 {
        context.push(ConversationMessage {
            role: if i % 2 == 1 { "user" } else { "assistant" }.to_string(),
            content: format!("Message {}: {}", i, "x".repeat(100)),
            timestamp: Some(i as u64),
            tokens: Some(50),
            proof: None,
        });
    }

    session_handler
        .handle_session_init("system-trim", 1100, context)
        .await
        .unwrap();

    let cache = session_handler.get_cache("system-trim").await.unwrap();
    let messages = cache.get_messages().await;

    // System message should always be first
    assert_eq!(messages[0].role, "system");
    assert!(messages[0].content.contains("helpful assistant"));
}

#[tokio::test]
async fn test_context_preparation_for_llm() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());

    let context = vec![
        ConversationMessage {
            role: "user".to_string(),
            content: "What's the weather?".to_string(),
            timestamp: Some(1),
            tokens: None,
            proof: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "I don't have weather data.".to_string(),
            timestamp: Some(2),
            tokens: Some(6),
            proof: None,
        },
        ConversationMessage {
            role: "user".to_string(),
            content: "Tell me a joke instead".to_string(),
            timestamp: Some(3),
            tokens: None,
            proof: None,
        },
    ];

    session_handler
        .handle_session_init("prep-test", 1200, context)
        .await
        .unwrap();

    // Get prepared context for LLM
    let prepared = inference_handler
        .prepare_context_for_llm("prep-test")
        .await
        .unwrap();

    // Should have all messages in correct format
    assert_eq!(prepared.len(), 3);
    assert_eq!(prepared[0].role, "user");
    assert_eq!(prepared[1].role, "assistant");
    assert_eq!(prepared[2].role, "user");
}

#[tokio::test]
async fn test_sliding_window_context() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());

    // Initialize empty session
    session_handler
        .handle_session_init("sliding-window", 1300, vec![])
        .await
        .unwrap();

    let cache = session_handler.get_cache("sliding-window").await.unwrap();

    // Add messages one by one and verify sliding window behavior
    for i in 0..50 {
        // Add user message
        cache
            .add_message(ConversationMessage {
                role: "user".to_string(),
                content: format!("Question {}", i),
                timestamp: Some(i * 2),
                tokens: Some(100), // Large tokens to trigger trimming
                proof: None,
            })
            .await;

        // Add assistant response
        cache
            .add_message(ConversationMessage {
                role: "assistant".to_string(),
                content: format!("Answer {}", i),
                timestamp: Some(i * 2 + 1),
                tokens: Some(100),
                proof: None,
            })
            .await;

        // Verify we stay within token limit
        assert!(cache.is_within_token_limit().await);
    }

    let final_messages = cache.get_messages().await;
    // Should have trimmed old messages
    assert!(final_messages.len() < 100);

    // Most recent messages should be preserved
    let last_msg = final_messages.last().unwrap();
    assert!(last_msg.content.contains("49"));
}

#[tokio::test]
async fn test_context_token_counting() {
    let session_handler = Arc::new(SessionInitHandler::new());

    let context = vec![
        ConversationMessage {
            role: "user".to_string(),
            content: "Short".to_string(),
            timestamp: Some(1),
            tokens: Some(1),
            proof: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "Medium length response here".to_string(),
            timestamp: Some(2),
            tokens: Some(5),
            proof: None,
        },
        ConversationMessage {
            role: "user".to_string(),
            content: "This is a much longer message with many words".to_string(),
            timestamp: Some(3),
            tokens: Some(10),
            proof: None,
        },
    ];

    session_handler
        .handle_session_init("token-count", 1400, context)
        .await
        .unwrap();

    let cache = session_handler.get_cache("token-count").await.unwrap();
    let total = cache.get_total_tokens().await;

    assert_eq!(total, 16); // 1 + 5 + 10
}

#[tokio::test]
async fn test_context_with_no_token_info() {
    let session_handler = Arc::new(SessionInitHandler::new());

    // Messages without token counts (need estimation)
    let context = vec![
        ConversationMessage {
            role: "user".to_string(),
            content: "Hello world".to_string(),
            timestamp: Some(1),
            tokens: None, // No token count
            proof: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "Hi there, how can I help you today?".to_string(),
            timestamp: Some(2),
            tokens: None, // No token count
            proof: None,
        },
    ];

    session_handler
        .handle_session_init("no-tokens", 1500, context)
        .await
        .unwrap();

    let cache = session_handler.get_cache("no-tokens").await.unwrap();
    let total = cache.get_total_tokens().await;

    // Should have estimated tokens (roughly 4 chars per token)
    assert!(total > 0);
    assert!(total < 100); // Reasonable estimate for short messages
}

#[tokio::test]
async fn test_context_message_ordering() {
    let session_handler = Arc::new(SessionInitHandler::new());

    // Messages might arrive out of order
    let context = vec![
        ConversationMessage {
            role: "user".to_string(),
            content: "Third message".to_string(),
            timestamp: Some(3),
            tokens: None,
            proof: None,
        },
        ConversationMessage {
            role: "user".to_string(),
            content: "First message".to_string(),
            timestamp: Some(1),
            tokens: None,
            proof: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "Second message".to_string(),
            timestamp: Some(2),
            tokens: Some(3),
            proof: None,
        },
    ];

    session_handler
        .handle_session_init("ordering", 1600, context)
        .await
        .unwrap();

    let cache = session_handler.get_cache("ordering").await.unwrap();
    let messages = cache.get_messages_sorted().await;

    // Should be sorted by timestamp
    assert_eq!(messages[0].timestamp.unwrap(), 1);
    assert_eq!(messages[1].timestamp.unwrap(), 2);
    assert_eq!(messages[2].timestamp.unwrap(), 3);
}

#[tokio::test]
async fn test_max_context_boundary() {
    let session_handler = Arc::new(SessionInitHandler::new());

    // Create context exactly at MAX_CONTEXT_TOKENS
    let mut context = vec![];
    let tokens_per_msg = 100;
    let num_messages = MAX_CONTEXT_TOKENS / tokens_per_msg;

    for i in 0..num_messages {
        context.push(ConversationMessage {
            role: if i % 2 == 0 { "user" } else { "assistant" }.to_string(),
            content: format!("Message {}", i),
            timestamp: Some(i as u64),
            tokens: Some(tokens_per_msg as u32),
            proof: None,
        });
    }

    session_handler
        .handle_session_init("max-boundary", 1700, context)
        .await
        .unwrap();

    let cache = session_handler.get_cache("max-boundary").await.unwrap();

    // Should accept messages at boundary
    assert!(cache.is_within_token_limit().await);

    // Adding one more should trigger trimming
    cache
        .add_message(ConversationMessage {
            role: "user".to_string(),
            content: "One more message".to_string(),
            timestamp: Some(num_messages as u64),
            tokens: Some(100),
            proof: None,
        })
        .await;

    // Should still be within limit after trimming
    assert!(cache.is_within_token_limit().await);

    let messages = cache.get_messages().await;
    // Should have trimmed oldest messages
    assert!(messages.len() <= num_messages);
}

#[tokio::test]
async fn test_context_continuity_across_sessions() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());

    // First session
    let context1 = vec![
        ConversationMessage {
            role: "user".to_string(),
            content: "My favorite color is blue".to_string(),
            timestamp: Some(1),
            tokens: None,
            proof: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "Blue is a nice color!".to_string(),
            timestamp: Some(2),
            tokens: Some(5),
            proof: None,
        },
    ];

    session_handler
        .handle_session_init("session-1", 1800, context1.clone())
        .await
        .unwrap();

    // Simulate session end
    session_handler.end_session("session-1").await.unwrap();

    // Resume in new session with full context
    session_handler
        .handle_session_init("session-2", 1801, context1)
        .await
        .unwrap();

    // Add new prompt referencing old context
    let cache = session_handler.get_cache("session-2").await.unwrap();
    cache
        .add_message(ConversationMessage {
            role: "user".to_string(),
            content: "What was my favorite color?".to_string(),
            timestamp: Some(3),
            tokens: None,
            proof: None,
        })
        .await;

    // Context should contain the color reference
    let messages = cache.get_messages().await;
    assert!(messages.iter().any(|m| m.content.contains("blue")));
}
