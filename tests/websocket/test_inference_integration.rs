use fabstir_llm_node::api::websocket::{
    handlers::{
        inference::{InferenceHandler, StreamConfig},
        session_init::SessionInitHandler,
    },
    messages::{ConversationMessage, StreamToken, WebSocketMessage},
};
use std::sync::Arc;
use futures::StreamExt;

#[tokio::test]
async fn test_inference_handler_basic_generation() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());
    
    // Initialize session
    session_handler
        .handle_session_init("inf-session", 100, vec![])
        .await
        .unwrap();
    
    // Generate response
    let response = inference_handler
        .generate_response("inf-session", "What is AI?", 1)
        .await
        .unwrap();
    
    assert_eq!(response.role, "assistant");
    assert!(!response.content.is_empty());
    assert!(response.tokens.is_some());
    assert!(response.tokens.unwrap() > 0);
}

#[tokio::test]
async fn test_inference_handler_with_context() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());
    
    // Initialize with context
    let context = vec![
        ConversationMessage {
            role: "user".to_string(),
            content: "My name is Alice".to_string(),
            timestamp: Some(1),
            tokens: None,
            proof: None,
        },
        ConversationMessage {
            role: "assistant".to_string(),
            content: "Nice to meet you, Alice!".to_string(),
            timestamp: Some(2),
            tokens: Some(6),
            proof: None,
        },
    ];
    
    session_handler
        .handle_session_init("context-inf", 200, context)
        .await
        .unwrap();
    
    // Add new prompt to cache
    let cache = session_handler.get_cache("context-inf").await.unwrap();
    cache.add_message(ConversationMessage {
        role: "user".to_string(),
        content: "What's my name?".to_string(),
        timestamp: Some(3),
        tokens: None,
        proof: None,
    }).await;
    
    // Generate response with context awareness
    let response = inference_handler
        .generate_response("context-inf", "What's my name?", 3)
        .await
        .unwrap();
    
    // Response should acknowledge the name from context
    assert!(response.content.to_lowercase().contains("alice") || 
            response.content.contains("name"));
}

#[tokio::test]
async fn test_inference_handler_streaming() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());
    
    session_handler
        .handle_session_init("stream-inf", 300, vec![])
        .await
        .unwrap();
    
    // Add prompt to cache first
    let cache = session_handler.get_cache("stream-inf").await.unwrap();
    cache.add_message(ConversationMessage {
        role: "user".to_string(),
        content: "Tell me a story".to_string(),
        timestamp: Some(1),
        tokens: None,
        proof: None,
    }).await;
    
    // Create streaming response
    let config = StreamConfig {
        max_tokens: 100,
        temperature: 0.7,
        stream: true,
    };
    
    let mut stream = inference_handler
        .create_streaming_response("stream-inf", "Tell me a story", 1, config)
        .await
        .unwrap();
    
    let mut tokens_received = 0;
    let mut has_final = false;
    let mut total_content = String::new();
    
    while let Some(result) = stream.next().await {
        if let Ok(token) = result {
            tokens_received += 1;
            total_content.push_str(&token.content);
            
            if token.is_final {
                has_final = true;
                assert!(token.total_tokens > 0);
            }
        }
    }
    
    assert!(tokens_received > 1); // Should stream multiple tokens
    assert!(has_final);
    assert!(!total_content.is_empty());
    
    // Verify response was added to cache
    let messages = cache.get_messages().await;
    assert_eq!(messages.len(), 2); // User prompt + assistant response
    assert_eq!(messages[1].role, "assistant");
}

#[tokio::test]
async fn test_inference_handler_max_tokens_limit() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());
    
    session_handler
        .handle_session_init("token-limit", 400, vec![])
        .await
        .unwrap();
    
    let config = StreamConfig {
        max_tokens: 10, // Very low limit
        temperature: 0.5,
        stream: false,
    };
    
    let response = inference_handler
        .generate_response_with_config("token-limit", "Write a long essay", 1, config)
        .await
        .unwrap();
    
    // Response should be limited by max_tokens
    assert!(response.tokens.unwrap() <= 10);
}

#[tokio::test]
async fn test_inference_handler_temperature_variation() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());
    
    session_handler
        .handle_session_init("temp-test", 500, vec![])
        .await
        .unwrap();
    
    // Low temperature (more deterministic)
    let config_low = StreamConfig {
        max_tokens: 50,
        temperature: 0.1,
        stream: false,
    };
    
    let response1 = inference_handler
        .generate_response_with_config("temp-test", "What is 2+2?", 1, config_low)
        .await
        .unwrap();
    
    // High temperature (more creative)
    let config_high = StreamConfig {
        max_tokens: 50,
        temperature: 1.5,
        stream: false,
    };
    
    let response2 = inference_handler
        .generate_response_with_config("temp-test", "What is 2+2?", 3, config_high)
        .await
        .unwrap();
    
    // Both should generate something
    assert!(!response1.content.is_empty());
    assert!(!response2.content.is_empty());
}

#[tokio::test]
async fn test_inference_handler_session_not_found() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler);
    
    let result = inference_handler
        .generate_response("non-existent", "Hello", 1)
        .await;
    
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Session not found"));
}

#[tokio::test]
async fn test_inference_handler_with_system_prompt() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());
    
    // Initialize with system message
    let context = vec![
        ConversationMessage {
            role: "system".to_string(),
            content: "You are a helpful math tutor.".to_string(),
            timestamp: Some(0),
            tokens: None,
            proof: None,
        },
    ];
    
    session_handler
        .handle_session_init("system-inf", 600, context)
        .await
        .unwrap();
    
    let cache = session_handler.get_cache("system-inf").await.unwrap();
    cache.add_message(ConversationMessage {
        role: "user".to_string(),
        content: "What is calculus?".to_string(),
        timestamp: Some(1),
        tokens: None,
        proof: None,
    }).await;
    
    let response = inference_handler
        .generate_response("system-inf", "What is calculus?", 1)
        .await
        .unwrap();
    
    // Response should be educational/tutorial in nature
    assert!(!response.content.is_empty());
    assert!(response.role == "assistant");
}

#[tokio::test]
async fn test_concurrent_inference_requests() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = Arc::new(InferenceHandler::new(session_handler.clone()));
    
    // Initialize multiple sessions
    for i in 0..3 {
        session_handler
            .handle_session_init(&format!("concurrent-{}", i), 700 + i, vec![])
            .await
            .unwrap();
    }
    
    let mut handles = vec![];
    
    // Launch concurrent inference requests
    for i in 0..3 {
        let ih = inference_handler.clone();
        let sh = session_handler.clone();
        let handle = tokio::spawn(async move {
            // Add prompt to cache
            let cache = sh.get_cache(&format!("concurrent-{}", i)).await.unwrap();
            cache.add_message(ConversationMessage {
                role: "user".to_string(),
                content: format!("Question {}", i),
                timestamp: Some(1),
                tokens: None,
                proof: None,
            }).await;
            
            ih.generate_response(
                &format!("concurrent-{}", i),
                &format!("Question {}", i),
                1,
            )
            .await
        });
        handles.push(handle);
    }
    
    // All should succeed
    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(!response.content.is_empty());
    }
}

#[tokio::test]
async fn test_inference_handler_error_recovery() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());
    
    session_handler
        .handle_session_init("error-inf", 800, vec![])
        .await
        .unwrap();
    
    // Try with invalid/problematic input
    let result = inference_handler
        .generate_response("error-inf", "", 1)
        .await;
    
    // Should handle empty prompt gracefully
    assert!(result.is_err() || result.unwrap().content.is_empty());
}

#[tokio::test]
async fn test_inference_handler_streaming_cancellation() {
    let session_handler = Arc::new(SessionInitHandler::new());
    let inference_handler = InferenceHandler::new(session_handler.clone());
    
    session_handler
        .handle_session_init("cancel-inf", 900, vec![])
        .await
        .unwrap();
    
    let cache = session_handler.get_cache("cancel-inf").await.unwrap();
    cache.add_message(ConversationMessage {
        role: "user".to_string(),
        content: "Long story please".to_string(),
        timestamp: Some(1),
        tokens: None,
        proof: None,
    }).await;
    
    let config = StreamConfig {
        max_tokens: 1000,
        temperature: 0.7,
        stream: true,
    };
    
    let mut stream = inference_handler
        .create_streaming_response("cancel-inf", "Long story please", 1, config)
        .await
        .unwrap();
    
    // Read only first few tokens then drop
    let mut count = 0;
    while let Some(result) = stream.next().await {
        if result.is_ok() {
            count += 1;
            if count >= 3 {
                drop(stream);
                break;
            }
        }
    }
    
    // Session should still be valid
    let cache_after = session_handler.get_cache("cancel-inf").await;
    assert!(cache_after.is_ok());
}