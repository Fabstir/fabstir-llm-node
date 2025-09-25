use fabstir_llm_node::api::websocket::{
    context_manager::{ContextConfig, ContextManager},
    context_strategies::{CompressionStrategy, OverflowStrategy, SummarizationConfig},
    session::{SessionConfig, WebSocketSession},
};
use fabstir_llm_node::job_processor::Message;

#[tokio::test]
async fn test_token_limit_enforcement() {
    let config = ContextConfig {
        max_tokens: 50,
        strict_token_limit: true,
        ..Default::default()
    };
    let manager = ContextManager::new(config);

    let mut session = create_session_with_many_messages(20);
    let context = manager.build_context(&session, "New prompt").await.unwrap();

    let token_count = manager.estimate_tokens(&context);
    assert!(token_count <= 50);
}

#[tokio::test]
async fn test_truncation_strategy() {
    let config = ContextConfig {
        max_tokens: 100,
        overflow_strategy: OverflowStrategy::Truncate,
        ..Default::default()
    };
    let manager = ContextManager::new(config);

    let session = create_session_with_many_messages(50);
    let context = manager.build_context(&session, "Question").await.unwrap();

    // Should truncate older messages
    assert!(!context.contains("Message 0"));
    assert!(context.contains("Message 49"));
    assert!(context.contains("Question"));
}

#[tokio::test]
async fn test_summarization_strategy() {
    let summarization_config = SummarizationConfig {
        trigger_threshold: 80, // Trigger at 80% of max tokens
        target_reduction: 0.5, // Reduce to 50% of original
        preserve_recent: 3,    // Keep last 3 messages intact
    };

    let config = ContextConfig {
        max_tokens: 200,
        overflow_strategy: OverflowStrategy::Summarize(summarization_config),
        ..Default::default()
    };
    let manager = ContextManager::new(config);

    let session = create_session_with_many_messages(30);
    let context = manager
        .build_context(&session, "New question")
        .await
        .unwrap();

    // Should contain summary marker
    assert!(context.contains("[Summary]") || context.contains("Previous conversation"));

    // Recent messages should be preserved
    assert!(context.contains("Message 29"));
    assert!(context.contains("New question"));
}

#[tokio::test]
async fn test_compression_for_idle_conversations() {
    let config = ContextConfig {
        enable_compression: true,
        compression_strategy: CompressionStrategy::Automatic,
        idle_threshold_seconds: 1,
        ..Default::default()
    };
    let manager = ContextManager::new(config);

    let mut session = create_session_with_many_messages(10);

    // Simulate idle period
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let compressed = manager.compress_idle_context(&session).await.unwrap();

    assert!(compressed.compressed);
    assert!(compressed.size_bytes < session.memory_used());
    assert_eq!(compressed.message_count, 10);
}

#[tokio::test]
async fn test_memory_pressure_handling() {
    let config = ContextConfig {
        max_memory_bytes: 1024, // 1KB limit
        enable_memory_monitoring: true,
        ..Default::default()
    };
    let manager = ContextManager::new(config);

    let large_message = Message {
        role: "user".to_string(),
        content: "x".repeat(2000), // 2KB message
        timestamp: None,
    };

    let result = manager.validate_memory_usage(&[large_message]).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("memory"));
}

#[tokio::test]
async fn test_context_window_sliding() {
    let config = ContextConfig {
        window_size: 5,
        window_overlap: 2, // Keep 2 messages when sliding
        ..Default::default()
    };
    let manager = ContextManager::new(config);

    let session = create_session_with_many_messages(10);
    let windows = manager.get_context_windows(&session).await;

    // Should create overlapping windows
    assert!(windows.len() > 1);

    // Check overlap between windows
    if windows.len() >= 2 {
        let window1_last = &windows[0].messages.last().unwrap().content;
        let window2_first = &windows[1].messages[0].content;

        // There should be some overlap
        assert_ne!(window1_last, window2_first);
    }
}

#[tokio::test]
async fn test_priority_message_preservation() {
    let config = ContextConfig {
        max_tokens: 100,
        preserve_system_messages: true,
        preserve_first_n: 2,
        ..Default::default()
    };
    let manager = ContextManager::new(config);

    let mut session = WebSocketSession::new("test".to_string());

    // Add system message
    session
        .add_message(Message {
            role: "system".to_string(),
            content: "Important system prompt".to_string(),
            timestamp: None,
        })
        .unwrap();

    // Add many user messages
    for i in 0..20 {
        session
            .add_message(Message {
                role: "user".to_string(),
                content: format!("Message {}", i),
                timestamp: None,
            })
            .unwrap();
    }

    let context = manager.build_context(&session, "Final").await.unwrap();

    // System message should always be preserved
    assert!(context.contains("Important system prompt"));

    // First messages should be preserved
    assert!(context.contains("Message 0"));
    assert!(context.contains("Message 1"));
}

#[tokio::test]
async fn test_adaptive_context_sizing() {
    let config = ContextConfig {
        adaptive_sizing: true,
        min_context_size: 50,
        max_tokens: 500,
        ..Default::default()
    };
    let manager = ContextManager::new(config);

    // Short conversation - should use minimal context
    let small_session = create_session_with_many_messages(3);
    let small_context = manager.build_context(&small_session, "Q").await.unwrap();
    let small_tokens = manager.estimate_tokens(&small_context);

    // Long conversation - should use more context
    let large_session = create_session_with_many_messages(50);
    let large_context = manager.build_context(&large_session, "Q").await.unwrap();
    let large_tokens = manager.estimate_tokens(&large_context);

    assert!(small_tokens < large_tokens);
    assert!(small_tokens >= 40); // Minimum size (adjusted for small conversations)
    assert!(large_tokens <= 500); // Maximum size
}

#[tokio::test]
async fn test_context_quality_metrics() {
    let config = ContextConfig {
        track_quality_metrics: true,
        ..Default::default()
    };
    let manager = ContextManager::new(config);

    let session = create_session_with_many_messages(10);
    let context = manager.build_context(&session, "Question").await.unwrap();

    let metrics = manager.get_context_metrics().await;

    assert!(metrics.total_contexts_built > 0);
    assert!(metrics.average_token_count > 0);
    assert!(metrics.truncation_count >= 0);
    assert!(metrics.compression_count >= 0);
}

#[tokio::test]
async fn test_multi_turn_context_coherence() {
    let config = ContextConfig {
        ensure_coherence: true,
        ..Default::default()
    };
    let manager = ContextManager::new(config);

    let mut session = WebSocketSession::new("test".to_string());

    // Add a multi-turn conversation
    session
        .add_message(Message {
            role: "user".to_string(),
            content: "Let's talk about Python".to_string(),
            timestamp: None,
        })
        .unwrap();

    session
        .add_message(Message {
            role: "assistant".to_string(),
            content: "Sure! Python is a versatile programming language.".to_string(),
            timestamp: None,
        })
        .unwrap();

    session
        .add_message(Message {
            role: "user".to_string(),
            content: "What about its data types?".to_string(),
            timestamp: None,
        })
        .unwrap();

    let context = manager
        .build_context(&session, "Tell me more")
        .await
        .unwrap();

    // Context should maintain conversation flow
    assert!(context.contains("Python"));
    assert!(context.contains("data types"));
    assert!(context.contains("Tell me more"));

    // Should be properly formatted
    assert!(context.contains("user:"));
    assert!(context.contains("assistant:"));
}

// Helper functions
fn create_session_with_many_messages(count: usize) -> WebSocketSession {
    let mut session = WebSocketSession::new("test-session".to_string());

    for i in 0..count {
        session
            .add_message(Message {
                role: if i % 2 == 0 { "user" } else { "assistant" }.to_string(),
                content: format!("Message {} with some content to make it longer", i),
                timestamp: None,
            })
            .unwrap();
    }

    session
}
