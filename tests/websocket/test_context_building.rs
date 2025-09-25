use fabstir_llm_node::api::websocket::{
    context_manager::{ContextConfig, ContextManager},
    session::{SessionConfig, WebSocketSession},
};
use fabstir_llm_node::job_processor::Message;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_context_manager_creation() {
    let config = ContextConfig::default();
    let manager = ContextManager::new(config);

    assert_eq!(manager.max_tokens(), 2048);
    assert_eq!(manager.window_size(), 20);
}

#[tokio::test]
async fn test_build_context_from_session() {
    let config = ContextConfig::default();
    let manager = ContextManager::new(config);

    let session = create_test_session_with_messages();
    let current_prompt = "What about its population?";

    let context = manager
        .build_context(&session, current_prompt)
        .await
        .unwrap();

    assert!(context.contains("What is the capital of France?"));
    assert!(context.contains("The capital of France is Paris"));
    assert!(context.contains(current_prompt));
}

#[tokio::test]
async fn test_sliding_window_context() {
    let config = ContextConfig {
        window_size: 3,
        ..Default::default()
    };
    let manager = ContextManager::new(config);

    let mut session = create_large_session();
    let context = manager
        .build_context(&session, "New question")
        .await
        .unwrap();

    // Should only contain last 3 messages + current prompt
    assert!(!context.contains("Message 0"));
    assert!(!context.contains("Message 1"));
    assert!(context.contains("Message 7")); // Last 3: 7, 8, 9
    assert!(context.contains("Message 8"));
    assert!(context.contains("Message 9"));
    assert!(context.contains("New question"));
}

#[tokio::test]
async fn test_token_counting() {
    let config = ContextConfig::default();
    let manager = ContextManager::new(config);

    let messages = vec![
        Message {
            role: "user".to_string(),
            content: "Hello world".to_string(), // ~2-3 tokens
            timestamp: None,
        },
        Message {
            role: "assistant".to_string(),
            content: "Hi there! How can I help you today?".to_string(), // ~8-10 tokens
            timestamp: None,
        },
    ];

    let token_count = manager.count_tokens(&messages).await;
    assert!(token_count > 0);
    assert!(token_count < 20); // Should be reasonable for these short messages
}

#[tokio::test]
async fn test_context_with_token_limit() {
    let config = ContextConfig {
        max_tokens: 100,
        window_size: 20,
        ..Default::default()
    };
    let manager = ContextManager::new(config);

    let session = create_large_session();
    let context = manager.build_context(&session, "Question").await.unwrap();

    let token_count = manager.estimate_tokens(&context);
    assert!(token_count <= 100);
}

#[tokio::test]
async fn test_context_validation() {
    let config = ContextConfig::default();
    let manager = ContextManager::new(config);

    // Test with valid context
    let valid_messages = vec![Message {
        role: "user".to_string(),
        content: "Normal message".to_string(),
        timestamp: None,
    }];

    assert!(manager.validate_context(&valid_messages).await.is_ok());

    // Test with invalid role
    let invalid_messages = vec![Message {
        role: "invalid_role".to_string(),
        content: "Message".to_string(),
        timestamp: None,
    }];

    let result = manager.validate_context(&invalid_messages).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_context_sanitization() {
    let config = ContextConfig::default();
    let manager = ContextManager::new(config);

    let messages = vec![
        Message {
            role: "user".to_string(),
            content: "Hello\x00\x01\x02world".to_string(), // Contains control characters
            timestamp: None,
        },
        Message {
            role: "assistant".to_string(),
            content: "Response with\nnewlines\tand\ttabs".to_string(),
            timestamp: None,
        },
    ];

    let sanitized = manager.sanitize_messages(messages).await;

    // Control characters should be removed
    assert!(!sanitized[0].content.contains('\x00'));
    assert!(!sanitized[0].content.contains('\x01'));
    assert!(!sanitized[0].content.contains('\x02'));

    // Newlines and tabs should be preserved
    assert!(sanitized[1].content.contains('\n'));
    assert!(sanitized[1].content.contains('\t'));
}

#[tokio::test]
async fn test_format_context_for_llm() {
    let config = ContextConfig::default();
    let manager = ContextManager::new(config);

    let messages = vec![
        Message {
            role: "system".to_string(),
            content: "You are a helpful assistant.".to_string(),
            timestamp: None,
        },
        Message {
            role: "user".to_string(),
            content: "What is 2+2?".to_string(),
            timestamp: None,
        },
        Message {
            role: "assistant".to_string(),
            content: "2+2 equals 4.".to_string(),
            timestamp: None,
        },
    ];

    let formatted = manager
        .format_for_llm(&messages, "What is 3+3?")
        .await
        .unwrap();

    assert!(formatted.contains("system: You are a helpful assistant."));
    assert!(formatted.contains("user: What is 2+2?"));
    assert!(formatted.contains("assistant: 2+2 equals 4."));
    assert!(formatted.contains("user: What is 3+3?"));
    assert!(formatted.ends_with("assistant:"));
}

#[tokio::test]
async fn test_context_with_system_prompt() {
    let config = ContextConfig {
        include_system_prompt: true,
        default_system_prompt: Some("You are a coding assistant.".to_string()),
        ..Default::default()
    };
    let manager = ContextManager::new(config);

    let session = WebSocketSession::new("test-id".to_string());

    let context = manager
        .build_context(&session, "Write hello world")
        .await
        .unwrap();

    assert!(context.contains("You are a coding assistant"));
    assert!(context.contains("Write hello world"));
}

#[tokio::test]
async fn test_context_cache() {
    let config = ContextConfig {
        enable_cache: true,
        cache_ttl_seconds: 60,
        ..Default::default()
    };
    let manager = Arc::new(RwLock::new(ContextManager::new(config)));

    let session = create_test_session_with_messages();
    let prompt = "Test prompt";

    // First call - should build context
    let context1 = {
        let mgr = manager.read().await;
        mgr.build_context(&session, prompt).await.unwrap()
    };

    // Second call - should use cache
    let context2 = {
        let mgr = manager.read().await;
        mgr.build_context(&session, prompt).await.unwrap()
    };

    assert_eq!(context1, context2);

    // Check cache hit metrics
    let mgr = manager.read().await;
    assert!(mgr.cache_hits() > 0);
}

#[tokio::test]
async fn test_parallel_context_building() {
    let config = ContextConfig::default();
    let manager = Arc::new(ContextManager::new(config));

    let mut handles = vec![];

    for i in 0..10 {
        let manager_clone = manager.clone();
        let handle = tokio::spawn(async move {
            let session = create_test_session_with_messages();
            let prompt = format!("Question {}", i);
            manager_clone.build_context(&session, &prompt).await
        });
        handles.push(handle);
    }

    let mut results = vec![];
    for handle in handles {
        results.push(handle.await.unwrap());
    }

    // All should succeed
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 10);
}

#[tokio::test]
async fn test_empty_session_context() {
    let config = ContextConfig::default();
    let manager = ContextManager::new(config);

    let session = WebSocketSession::new("empty-session".to_string());

    let context = manager
        .build_context(&session, "First message")
        .await
        .unwrap();

    assert!(context.contains("First message"));
    assert!(context.contains("user:"));
    assert!(context.contains("assistant:"));
}

// Helper functions
fn create_test_session_with_messages() -> WebSocketSession {
    let mut session = WebSocketSession::new("test-session".to_string());

    session
        .add_message(Message {
            role: "user".to_string(),
            content: "What is the capital of France?".to_string(),
            timestamp: None,
        })
        .unwrap();

    session
        .add_message(Message {
            role: "assistant".to_string(),
            content: "The capital of France is Paris.".to_string(),
            timestamp: None,
        })
        .unwrap();

    session
}

fn create_large_session() -> WebSocketSession {
    let mut session = WebSocketSession::new("large-session".to_string());

    for i in 0..10 {
        session
            .add_message(Message {
                role: if i % 2 == 0 { "user" } else { "assistant" }.to_string(),
                content: format!("Message {}", i),
                timestamp: None,
            })
            .unwrap();
    }

    session
}
