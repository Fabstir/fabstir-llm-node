use fabstir_llm_node::api::websocket::{
    handlers::session_resume::SessionResumeHandler,
    messages::{ConversationMessage, WebSocketMessage},
};
use std::sync::Arc;

#[tokio::test]
async fn test_session_resume_rebuilds_cache() {
    let handler = SessionResumeHandler::new();
    
    let context = vec![
        ConversationMessage {
            role: "user".to_string(),
            content: "What is AI?".to_string(),
            timestamp: Some(1),
            tokens: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "AI is artificial intelligence".to_string(),
            timestamp: Some(2),
            tokens: Some(10),
        },
        ConversationMessage {
            role: "user".to_string(),
            content: "Tell me more".to_string(),
            timestamp: Some(3),
            tokens: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "Machine learning is a subset...".to_string(),
            timestamp: Some(4),
            tokens: Some(20),
        },
    ];

    let result = handler
        .handle_session_resume("session-456", 12345, context.clone(), 4)
        .await
        .unwrap();

    assert_eq!(result.session_id, "session-456");
    assert_eq!(result.job_id, 12345);
    assert_eq!(result.message_count, 4);
    assert_eq!(result.total_tokens, 30);
    assert_eq!(result.last_message_index, 4);
}

#[tokio::test]
async fn test_session_resume_with_partial_history() {
    let handler = SessionResumeHandler::new();
    
    // Simulating recovery after crash - only have first 2 messages
    let partial_context = vec![
        ConversationMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
            timestamp: Some(1),
            tokens: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "Hi there!".to_string(),
            timestamp: Some(2),
            tokens: Some(3),
        },
    ];

    let result = handler
        .handle_session_resume("crashed-session", 999, partial_context, 2)
        .await
        .unwrap();

    assert_eq!(result.message_count, 2);
    assert_eq!(result.last_message_index, 2);
    assert!(result.resumed_successfully);
}

#[tokio::test]
async fn test_session_resume_validates_message_index() {
    let handler = SessionResumeHandler::new();
    
    let context = vec![
        ConversationMessage {
            role: "user".to_string(),
            content: "Test".to_string(),
            timestamp: None,
            tokens: None,
        },
    ];

    // Last message index should match context length (1 message, but we pass 5)
    let result = handler
        .handle_session_resume("session-123", 100, context.clone(), 5)
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("index mismatch"));
}

#[tokio::test]
async fn test_session_resume_handles_large_context() {
    let handler = SessionResumeHandler::new();
    
    // Create large conversation history
    let mut context = vec![];
    let mut total_tokens = 0;
    
    for i in 0..50 {
        context.push(ConversationMessage {
            role: "user".to_string(),
            content: format!("Question {}", i),
            timestamp: Some(i * 2),
            tokens: None,
        });
        
        let tokens = (i + 1) * 2;
        total_tokens += tokens;
        
        context.push(ConversationMessage {
            role: "assistant".to_string(),
            content: format!("Answer {}", i),
            timestamp: Some(i * 2 + 1),
            tokens: Some(tokens as u32),
        });
    }

    let result = handler
        .handle_session_resume("large-session", 500, context.clone(), 100)
        .await
        .unwrap();

    assert_eq!(result.message_count, 100);
    assert_eq!(result.total_tokens, total_tokens as u32);
}

#[tokio::test]
async fn test_session_resume_clears_old_session() {
    let handler = SessionResumeHandler::new();
    
    // First resume
    let context1 = vec![
        ConversationMessage {
            role: "user".to_string(),
            content: "Old question".to_string(),
            timestamp: None,
            tokens: None,
        },
    ];
    
    handler
        .handle_session_resume("session-789", 100, context1, 1)
        .await
        .unwrap();

    // Second resume with different context
    let context2 = vec![
        ConversationMessage {
            role: "user".to_string(),
            content: "New question".to_string(),
            timestamp: None,
            tokens: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "New answer".to_string(),
            timestamp: None,
            tokens: Some(5),
        },
    ];
    
    let result = handler
        .handle_session_resume("session-789", 101, context2.clone(), 2)
        .await
        .unwrap();
    
    // Verify cache has new context
    let cache = handler.get_cache("session-789").await.unwrap();
    let messages = cache.get_messages().await;
    
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "New question");
    assert_eq!(result.job_id, 101);
}

#[tokio::test]
async fn test_session_resume_with_system_message() {
    let handler = SessionResumeHandler::new();
    
    let context = vec![
        ConversationMessage {
            role: "system".to_string(),
            content: "You are a helpful AI assistant".to_string(),
            timestamp: Some(0),
            tokens: None,
        },
        ConversationMessage {
            role: "user".to_string(),
            content: "What can you do?".to_string(),
            timestamp: Some(1),
            tokens: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "I can help with various tasks".to_string(),
            timestamp: Some(2),
            tokens: Some(8),
        },
    ];

    let result = handler
        .handle_session_resume("system-session", 200, context, 3)
        .await
        .unwrap();

    assert_eq!(result.message_count, 3);
    
    // Verify system message is preserved
    let cache = handler.get_cache("system-session").await.unwrap();
    let messages = cache.get_messages().await;
    assert_eq!(messages[0].role, "system");
}

#[tokio::test]
async fn test_concurrent_session_resumes() {
    let handler = Arc::new(SessionResumeHandler::new());
    
    let mut handles = vec![];
    
    // Create 10 concurrent session resumes
    for i in 0..10 {
        let h = handler.clone();
        let handle = tokio::spawn(async move {
            let context = vec![
                ConversationMessage {
                    role: "user".to_string(),
                    content: format!("Question {}", i),
                    timestamp: None,
                    tokens: None,
                },
                ConversationMessage {
                    role: "assistant".to_string(),
                    content: format!("Answer {}", i),
                    timestamp: None,
                    tokens: Some(i as u32 + 1),
                },
            ];
            
            h.handle_session_resume(
                &format!("concurrent-{}", i),
                i as u64 + 1,
                context,
                2,
            )
            .await
        });
        handles.push(handle);
    }
    
    // All should succeed
    for handle in handles {
        let result = handle.await.unwrap().unwrap();
        assert!(result.session_id.starts_with("concurrent-"));
        assert!(result.resumed_successfully);
    }
}