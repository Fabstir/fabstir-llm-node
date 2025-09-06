use fabstir_llm_node::api::websocket::{
    handlers::{response::ResponseHandler, session_init::SessionInitHandler},
    messages::{ConversationMessage, WebSocketMessage},
};
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_response_streaming_basic() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let response_handler = ResponseHandler::new(session_handler.clone());
    
    // Initialize session
    session_handler
        .handle_session_init("stream-session", 100, vec![])
        .await
        .unwrap();
    
    // Create response stream
    let mut stream = response_handler
        .create_response_stream("stream-session", "Tell me about AI", 1)
        .await
        .unwrap();
    
    // Collect streamed tokens
    let mut tokens = vec![];
    while let Some(token) = stream.next().await {
        match token {
            Ok(t) => tokens.push(t),
            Err(e) => panic!("Stream error: {}", e),
        }
    }
    
    assert!(!tokens.is_empty());
    assert!(tokens.iter().any(|t| t.is_final));
}

#[tokio::test]
async fn test_response_streaming_with_tokens() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let response_handler = ResponseHandler::new(session_handler.clone());
    
    session_handler
        .handle_session_init("token-session", 200, vec![])
        .await
        .unwrap();
    
    // Add the user prompt to cache first
    let cache = session_handler.get_cache("token-session").await.unwrap();
    cache.add_message(ConversationMessage {
        role: "user".to_string(),
        content: "Short prompt".to_string(),
        timestamp: None,
        tokens: None,
    }).await;
    
    let mut stream = response_handler
        .create_response_stream("token-session", "Short prompt", 1)
        .await
        .unwrap();
    
    let mut total_tokens = 0;
    let mut content_parts = vec![];
    
    while let Some(token) = stream.next().await {
        if let Ok(t) = token {
            content_parts.push(t.content.clone());
            if t.is_final {
                total_tokens = t.total_tokens;
            }
        }
    }
    
    assert!(total_tokens > 0);
    assert!(!content_parts.is_empty());
    
    // Verify response was added to cache
    let cache = session_handler.get_cache("token-session").await.unwrap();
    let messages = cache.get_messages().await;
    assert!(messages.len() >= 2); // User prompt + assistant response
}

#[tokio::test]
async fn test_response_streaming_session_not_found() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let response_handler = ResponseHandler::new(session_handler);
    
    let result = response_handler
        .create_response_stream("non-existent", "Test", 1)
        .await;
    
    assert!(result.is_err());
    if let Err(e) = result {
        assert!(e.to_string().contains("Session not found"));
    }
}

#[tokio::test]
async fn test_response_streaming_with_context() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let response_handler = ResponseHandler::new(session_handler.clone());
    
    // Initialize with context
    let context = vec![
        ConversationMessage {
            role: "user".to_string(),
            content: "What is 2+2?".to_string(),
            timestamp: None,
            tokens: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "2+2 equals 4".to_string(),
            timestamp: None,
            tokens: Some(5),
        },
    ];
    
    session_handler
        .handle_session_init("context-stream", 300, context)
        .await
        .unwrap();
    
    // Stream response with context
    let mut stream = response_handler
        .create_response_stream("context-stream", "What was my first question?", 3)
        .await
        .unwrap();
    
    let mut has_content = false;
    while let Some(token) = stream.next().await {
        if let Ok(t) = token {
            if !t.content.is_empty() {
                has_content = true;
            }
        }
    }
    
    assert!(has_content);
}

#[tokio::test]
async fn test_response_streaming_cancellation() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let response_handler = ResponseHandler::new(session_handler.clone());
    
    session_handler
        .handle_session_init("cancel-session", 400, vec![])
        .await
        .unwrap();
    
    let mut stream = response_handler
        .create_response_stream("cancel-session", "Long response please", 1)
        .await
        .unwrap();
    
    // Read only first token then drop stream
    if let Some(Ok(_)) = stream.next().await {
        // Dropping stream should cancel generation
        drop(stream);
    }
    
    // Verify session still exists and is usable
    let cache = session_handler.get_cache("cancel-session").await;
    assert!(cache.is_ok());
}

#[tokio::test]
async fn test_response_streaming_message_index() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let response_handler = ResponseHandler::new(session_handler.clone());
    
    session_handler
        .handle_session_init("index-stream", 500, vec![])
        .await
        .unwrap();
    
    // Stream with specific message index
    let mut stream = response_handler
        .create_response_stream("index-stream", "Test prompt", 1)
        .await
        .unwrap();
    
    let mut final_index = 0;
    while let Some(token) = stream.next().await {
        if let Ok(t) = token {
            if t.is_final {
                final_index = t.message_index;
            }
        }
    }
    
    assert_eq!(final_index, 2); // Response should be index 2 after prompt at index 1
}

#[tokio::test]
async fn test_concurrent_response_streams() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let response_handler = Arc::new(ResponseHandler::new(session_handler.clone()));
    
    // Initialize multiple sessions
    for i in 0..3 {
        session_handler
            .handle_session_init(&format!("concurrent-{}", i), i as u64 + 1, vec![])
            .await
            .unwrap();
    }
    
    let mut handles = vec![];
    
    // Create concurrent streams
    for i in 0..3 {
        let rh = response_handler.clone();
        let handle = tokio::spawn(async move {
            let mut stream = rh
                .create_response_stream(
                    &format!("concurrent-{}", i),
                    &format!("Prompt {}", i),
                    1,
                )
                .await
                .unwrap();
            
            let mut count = 0;
            while let Some(token) = stream.next().await {
                if token.is_ok() {
                    count += 1;
                }
            }
            count
        });
        handles.push(handle);
    }
    
    // All streams should complete
    for handle in handles {
        let count = handle.await.unwrap();
        assert!(count > 0);
    }
}

#[tokio::test]
async fn test_response_streaming_error_handling() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let response_handler = ResponseHandler::new(session_handler.clone());
    
    session_handler
        .handle_session_init("error-session", 600, vec![])
        .await
        .unwrap();
    
    // Simulate error conditions
    let mut stream = response_handler
        .create_response_stream("error-session", "", 1) // Empty prompt might cause error
        .await
        .unwrap();
    
    let mut has_error = false;
    while let Some(token) = stream.next().await {
        if token.is_err() {
            has_error = true;
            break;
        }
    }
    
    // Should handle errors gracefully
    assert!(has_error || stream.next().await.is_none());
}

#[tokio::test]
async fn test_response_streaming_adds_to_cache() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let response_handler = ResponseHandler::new(session_handler.clone());
    
    session_handler
        .handle_session_init("cache-stream", 700, vec![])
        .await
        .unwrap();
    
    // Add user prompt first
    let cache = session_handler.get_cache("cache-stream").await.unwrap();
    cache.add_message(ConversationMessage {
        role: "user".to_string(),
        content: "Hello AI".to_string(),
        timestamp: Some(1),
        tokens: None,
    }).await;
    
    // Stream response
    let mut stream = response_handler
        .create_response_stream("cache-stream", "Hello AI", 1)
        .await
        .unwrap();
    
    let mut full_response = String::new();
    while let Some(token) = stream.next().await {
        if let Ok(t) = token {
            full_response.push_str(&t.content);
        }
    }
    
    // Verify response was added to cache
    let messages = cache.get_messages().await;
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1].role, "assistant");
    assert!(!messages[1].content.is_empty());
}