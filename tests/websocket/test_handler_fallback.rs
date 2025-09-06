use fabstir_llm_node::api::websocket::{
    handler::{WebSocketHandler, HandlerConfig},
    message_types::{WebSocketMessage, MessageType},
    session_store::{SessionStore, SessionStoreConfig},
};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_stateless_message_handling() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig {
        enable_stateless_fallback: true,
        ..Default::default()
    };
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    // Send stateless message (with explicit flag)
    let message = WebSocketMessage {
        msg_type: MessageType::StatelessInference,
        session_id: None,
        payload: json!({
            "prompt": "What is 2+2?",
            "max_tokens": 50,
            "conversation_context": []
        }),
    };
    
    let response = handler.handle_message(message).await.unwrap();
    
    // Should not create a session
    assert_eq!(handler.active_sessions(), 0);
    assert_eq!(response.msg_type, MessageType::InferenceResponse);
}

#[tokio::test]
async fn test_message_type_discrimination() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig::default();
    let handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    // Test different message types
    let types = vec![
        MessageType::Init,
        MessageType::Inference,
        MessageType::StatelessInference,
        MessageType::SessionControl,
        MessageType::Ping,
    ];
    
    for msg_type in types {
        assert!(handler.can_handle_message_type(&msg_type));
    }
    
    // Unknown type handling
    let unknown = MessageType::Unknown;
    assert!(!handler.can_handle_message_type(&unknown));
}

#[tokio::test]
async fn test_auto_session_creation_when_disabled() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig {
        auto_create_session: false,
        ..Default::default()
    };
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    // Message without session ID and auto-create disabled
    let message = WebSocketMessage {
        msg_type: MessageType::Inference,
        session_id: None,
        payload: json!({
            "prompt": "Hello",
            "max_tokens": 50
        }),
    };
    
    let result = handler.handle_message(message).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("session"));
}

#[tokio::test]
async fn test_graceful_error_response() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig::default();
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    // Malformed message
    let bad_message = WebSocketMessage {
        msg_type: MessageType::Inference,
        session_id: Some("non-existent".to_string()),
        payload: json!(null),
    };
    
    let result = handler.handle_message(bad_message).await;
    assert!(result.is_err());
    
    // Error should be informative
    let error = result.unwrap_err();
    assert!(!error.to_string().is_empty());
}

#[tokio::test]
async fn test_max_sessions_limit_fallback() {
    let store_config = SessionStoreConfig {
        max_sessions: 2,
        ..Default::default()
    };
    let store = SessionStore::new(store_config);
    let config = HandlerConfig {
        enable_stateless_fallback: true,
        ..Default::default()
    };
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    // Create max sessions
    let _s1 = handler.on_connect().await.unwrap();
    let _s2 = handler.on_connect().await.unwrap();
    
    // Third connection should fallback to stateless
    let result = handler.on_connect_with_fallback().await;
    assert!(result.is_ok());
    
    let mode = result.unwrap();
    assert_eq!(mode.mode, "stateless");
    assert!(mode.reason.as_ref().unwrap().contains("limit"));
}

#[tokio::test]
async fn test_session_recovery_attempt() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig::default();
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    // Create and disconnect session
    let session_id = handler.on_connect().await.unwrap();
    handler.on_disconnect(&session_id).await.unwrap();
    
    // Try to reconnect with same session ID
    let reconnect_msg = WebSocketMessage {
        msg_type: MessageType::Init,
        session_id: Some(session_id.clone()),
        payload: json!({
            "action": "resume"
        }),
    };
    
    let result = handler.handle_message(reconnect_msg).await;
    // Should fail since we don't support persistence yet
    assert!(result.is_err());
}

#[tokio::test]
async fn test_error_codes_and_messages() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig::default();
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    // Test various error conditions
    let test_cases = vec![
        (
            WebSocketMessage {
                msg_type: MessageType::Inference,
                session_id: Some("invalid".to_string()),
                payload: json!({"prompt": "test"}),
            },
            "SESSION_NOT_FOUND",
        ),
        (
            WebSocketMessage {
                msg_type: MessageType::Unknown,
                session_id: None,
                payload: json!({}),
            },
            "UNKNOWN_MESSAGE_TYPE",
        ),
    ];
    
    for (message, expected_code) in test_cases {
        let result = handler.handle_message(message).await;
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(error.to_string().contains(expected_code));
    }
}

#[tokio::test]
async fn test_mode_switching_not_allowed() {
    let store = SessionStore::new(SessionStoreConfig::default());
    let config = HandlerConfig::default();
    let mut handler = WebSocketHandler::new(Arc::new(RwLock::new(store)), config);
    
    // Start with stateful session
    let session_id = handler.on_connect().await.unwrap();
    
    // Try to switch to stateless mid-session
    let stateless_msg = WebSocketMessage {
        msg_type: MessageType::StatelessInference,
        session_id: Some(session_id),
        payload: json!({
            "prompt": "test",
            "max_tokens": 50
        }),
    };
    
    let result = handler.handle_message(stateless_msg).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("mode"));
}