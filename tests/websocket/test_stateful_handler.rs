use fabstir_llm_node::api::websocket::{
    handler::{WebSocketHandler, HandlerConfig},
    message_types::{WebSocketMessage, MessageType, InferenceMessage, SessionControl},
    session_store::{SessionStore, SessionStoreConfig},
    session::SessionConfig,
};
use fabstir_llm_node::job_processor::Message;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_handler_creation() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig::default();
    
    let handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    assert_eq!(handler.active_sessions(), 0);
}

#[tokio::test]
async fn test_session_initialization_on_connect() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig::default();
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    let session_id = handler.on_connect().await.unwrap();
    
    assert!(!session_id.is_empty());
    assert_eq!(handler.active_sessions(), 1);
    assert!(handler.has_session(&session_id).await);
}

#[tokio::test]
async fn test_handle_inference_message_with_session() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig::default();
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    // Create session
    let session_id = handler.on_connect().await.unwrap();
    
    // Create inference message
    let message = WebSocketMessage {
        msg_type: MessageType::Inference,
        session_id: Some(session_id.clone()),
        payload: json!({
            "prompt": "Hello, how are you?",
            "max_tokens": 100
        }),
    };
    
    // Handle message
    let response = handler.handle_message(message).await.unwrap();
    
    // Check response
    assert_eq!(response.msg_type, MessageType::InferenceResponse);
    assert_eq!(response.session_id, Some(session_id));
}

#[tokio::test]
async fn test_automatic_context_building() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig::default();
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    let session_id = handler.on_connect().await.unwrap();
    
    // Send first message
    let msg1 = WebSocketMessage {
        msg_type: MessageType::Inference,
        session_id: Some(session_id.clone()),
        payload: json!({
            "prompt": "What is the capital of France?",
            "max_tokens": 50
        }),
    };
    
    handler.handle_message(msg1).await.unwrap();
    
    // Simulate adding response to session
    handler.add_message_to_session(&session_id, Message {
        role: "assistant".to_string(),
        content: "The capital of France is Paris.".to_string(),
        timestamp: None,
    }).await.unwrap();
    
    // Send second message - should have context
    let _msg2 = WebSocketMessage {
        msg_type: MessageType::Inference,
        session_id: Some(session_id.clone()),
        payload: json!({
            "prompt": "What is its population?",
            "max_tokens": 50
        }),
    };
    
    let context = handler.build_context_for_session(&session_id, "What is its population?").await.unwrap();
    
    // Context should include previous messages
    assert!(context.contains("What is the capital of France?"));
    assert!(context.contains("The capital of France is Paris."));
    assert!(context.contains("What is its population?"));
}

#[tokio::test]
async fn test_session_cleanup_on_disconnect() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig::default();
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    let session_id = handler.on_connect().await.unwrap();
    assert_eq!(handler.active_sessions(), 1);
    
    handler.on_disconnect(&session_id).await.unwrap();
    assert_eq!(handler.active_sessions(), 0);
    assert!(!handler.has_session(&session_id).await);
}

#[tokio::test]
async fn test_handle_session_control_messages() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig::default();
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    let session_id = handler.on_connect().await.unwrap();
    
    // Clear session message
    let clear_msg = WebSocketMessage {
        msg_type: MessageType::SessionControl,
        session_id: Some(session_id.clone()),
        payload: json!({
            "action": "clear"
        }),
    };
    
    let response = handler.handle_message(clear_msg).await.unwrap();
    assert_eq!(response.msg_type, MessageType::SessionControlAck);
    
    // Session should be cleared but still exist
    assert!(handler.has_session(&session_id).await);
    let messages = handler.get_session_messages(&session_id).await.unwrap();
    assert_eq!(messages.len(), 0);
}

#[tokio::test]
async fn test_concurrent_session_handling() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig::default();
    let handler = Arc::new(RwLock::new(WebSocketHandler::new(Arc::new(RwLock::new(store)), config)));
    
    let mut handles = vec![];
    
    // Create multiple sessions concurrently
    for _ in 0..10 {
        let handler_clone = handler.clone();
        let handle = tokio::spawn(async move {
            let mut handler = handler_clone.write().await;
            handler.on_connect().await.unwrap()
        });
        handles.push(handle);
    }
    
    let mut session_ids = vec![];
    for handle in handles {
        session_ids.push(handle.await.unwrap());
    }
    
    // All session IDs should be unique
    let unique_count = session_ids.iter().collect::<std::collections::HashSet<_>>().len();
    assert_eq!(unique_count, 10);
    
    let handler_read = handler.read().await;
    assert_eq!(handler_read.active_sessions(), 10);
}

#[tokio::test]
async fn test_message_without_session_id() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig::default();
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    // Message without session_id should create a new session
    let message = WebSocketMessage {
        msg_type: MessageType::Inference,
        session_id: None,
        payload: json!({
            "prompt": "Hello",
            "max_tokens": 50
        }),
    };
    
    let response = handler.handle_message(message).await.unwrap();
    
    // Should have created a session
    assert!(response.session_id.is_some());
    assert_eq!(handler.active_sessions(), 1);
}

#[tokio::test]
async fn test_invalid_session_id() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig::default();
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    let message = WebSocketMessage {
        msg_type: MessageType::Inference,
        session_id: Some("invalid-session-id".to_string()),
        payload: json!({
            "prompt": "Hello",
            "max_tokens": 50
        }),
    };
    
    let result = handler.handle_message(message).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("SESSION_NOT_FOUND"));
}

#[tokio::test]
async fn test_session_timeout_handling() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig {
        session_timeout_seconds: 1,
        ..Default::default()
    };
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    let session_id = handler.on_connect().await.unwrap();
    
    // Wait for timeout
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    
    // Try to use expired session
    let message = WebSocketMessage {
        msg_type: MessageType::Inference,
        session_id: Some(session_id.clone()),
        payload: json!({
            "prompt": "Hello",
            "max_tokens": 50
        }),
    };
    
    let result = handler.handle_message(message).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("expired"));
}

#[tokio::test]
async fn test_handler_metrics() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig::default();
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    // Create sessions and send messages
    let session1 = handler.on_connect().await.unwrap();
    let session2 = handler.on_connect().await.unwrap();
    
    for session_id in &[session1, session2] {
        let msg = WebSocketMessage {
            msg_type: MessageType::Inference,
            session_id: Some(session_id.clone()),
            payload: json!({
                "prompt": "Test message",
                "max_tokens": 50
            }),
        };
        handler.handle_message(msg).await.unwrap();
    }
    
    let metrics = handler.get_metrics().await;
    
    assert_eq!(metrics.active_sessions, 2);
    assert_eq!(metrics.total_messages_processed, 2);
    assert!(metrics.total_memory_bytes > 0);
}

#[tokio::test]
async fn test_error_handling_in_message_processing() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig::default();
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    let session_id = handler.on_connect().await.unwrap();
    
    // Invalid message payload
    let invalid_msg = WebSocketMessage {
        msg_type: MessageType::Inference,
        session_id: Some(session_id),
        payload: json!({
            // Missing required "prompt" field
            "max_tokens": 50
        }),
    };
    
    let result = handler.handle_message(invalid_msg).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_ping_pong_handling() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig::default();
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    let session_id = handler.on_connect().await.unwrap();
    
    let ping_msg = WebSocketMessage {
        msg_type: MessageType::Ping,
        session_id: Some(session_id),
        payload: json!({}),
    };
    
    let response = handler.handle_message(ping_msg).await.unwrap();
    assert_eq!(response.msg_type, MessageType::Pong);
}