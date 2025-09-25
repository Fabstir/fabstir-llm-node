use fabstir_llm_node::api::websocket::session::{SessionConfig, SessionMetrics, WebSocketSession};
use fabstir_llm_node::job_processor::Message;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[test]
fn test_session_creation() {
    let session_id = Uuid::new_v4().to_string();
    let config = SessionConfig::default();

    let session = WebSocketSession::with_config(session_id.clone(), config);

    assert_eq!(session.id(), &session_id);
    assert_eq!(session.message_count(), 0);
    assert!(session.conversation_history().is_empty());
    assert!(session.created_at().elapsed() < Duration::from_secs(1));
}

#[test]
fn test_add_message_to_session() {
    let session_id = Uuid::new_v4().to_string();
    let config = SessionConfig::default();
    let mut session = WebSocketSession::with_config(session_id, config);

    let message = Message {
        role: "user".to_string(),
        content: "Hello, how are you?".to_string(),
        timestamp: Some(1234567890),
    };

    session.add_message(message.clone());

    assert_eq!(session.message_count(), 1);
    assert_eq!(session.conversation_history().len(), 1);
    assert_eq!(session.conversation_history()[0].content, message.content);
}

#[test]
fn test_session_memory_limit() {
    let session_id = Uuid::new_v4().to_string();
    let config = SessionConfig {
        max_memory_bytes: 1024, // 1KB limit
        ..Default::default()
    };
    let mut session = WebSocketSession::with_config(session_id, config);

    // Add messages until we exceed memory limit
    let large_message = Message {
        role: "user".to_string(),
        content: "a".repeat(400), // 400 bytes content + overhead
        timestamp: None,
    };

    assert!(session.add_message(large_message.clone()).is_ok());
    assert!(session.add_message(large_message.clone()).is_ok());

    // Third message should fail due to memory limit
    let result = session.add_message(large_message);
    assert!(result.is_err());
    assert_eq!(session.message_count(), 2);
}

#[test]
fn test_session_context_window() {
    let session_id = Uuid::new_v4().to_string();
    let config = SessionConfig {
        context_window_size: 3,
        ..Default::default()
    };
    let mut session = WebSocketSession::with_config(session_id, config);

    // Add 5 messages
    for i in 0..5 {
        let message = Message {
            role: "user".to_string(),
            content: format!("Message {}", i),
            timestamp: None,
        };
        session.add_message(message).unwrap();
    }

    // Should have all 5 messages in history
    assert_eq!(session.message_count(), 5);

    // But context should only return last 3
    let context = session.get_context_messages();
    assert_eq!(context.len(), 3);
    assert_eq!(context[0].content, "Message 2");
    assert_eq!(context[1].content, "Message 3");
    assert_eq!(context[2].content, "Message 4");
}

#[test]
fn test_session_clear() {
    let session_id = Uuid::new_v4().to_string();
    let config = SessionConfig::default();
    let mut session = WebSocketSession::with_config(session_id, config);

    // Add some messages
    for i in 0..3 {
        let message = Message {
            role: "user".to_string(),
            content: format!("Message {}", i),
            timestamp: None,
        };
        session.add_message(message).unwrap();
    }

    assert_eq!(session.message_count(), 3);

    // Clear the session
    session.clear();

    assert_eq!(session.message_count(), 0);
    assert!(session.conversation_history().is_empty());
}

#[test]
fn test_session_last_activity() {
    let session_id = Uuid::new_v4().to_string();
    let config = SessionConfig::default();
    let mut session = WebSocketSession::with_config(session_id, config);

    let initial_activity = session.last_activity();

    // Wait a bit
    std::thread::sleep(Duration::from_millis(100));

    // Add a message
    let message = Message {
        role: "user".to_string(),
        content: "Test message".to_string(),
        timestamp: None,
    };
    session.add_message(message).unwrap();

    let new_activity = session.last_activity();
    assert!(new_activity > initial_activity);
}

#[test]
fn test_session_metrics() {
    let session_id = Uuid::new_v4().to_string();
    let config = SessionConfig::default();
    let mut session = WebSocketSession::with_config(session_id, config);

    // Add messages with different roles
    session
        .add_message(Message {
            role: "user".to_string(),
            content: "Hello".to_string(),
            timestamp: None,
        })
        .unwrap();

    session
        .add_message(Message {
            role: "assistant".to_string(),
            content: "Hi there! How can I help you today?".to_string(),
            timestamp: None,
        })
        .unwrap();

    session
        .add_message(Message {
            role: "user".to_string(),
            content: "What's the weather?".to_string(),
            timestamp: None,
        })
        .unwrap();

    let metrics = session.metrics();

    assert_eq!(metrics.total_messages, 3);
    assert_eq!(metrics.user_messages, 2);
    assert_eq!(metrics.assistant_messages, 1);
    assert!(metrics.total_tokens > 0);
    assert!(metrics.memory_bytes > 0);
}

#[test]
fn test_session_is_expired() {
    let session_id = Uuid::new_v4().to_string();
    let config = SessionConfig {
        timeout_seconds: 1, // 1 second timeout
        ..Default::default()
    };
    let session = WebSocketSession::with_config(session_id, config);

    // Initially not expired
    assert!(!session.is_expired());

    // Wait for timeout
    std::thread::sleep(Duration::from_secs(2));

    // Now should be expired
    assert!(session.is_expired());
}

#[test]
fn test_session_token_counting() {
    let session_id = Uuid::new_v4().to_string();
    let config = SessionConfig::default();
    let mut session = WebSocketSession::with_config(session_id, config);

    let message = Message {
        role: "user".to_string(),
        content: "This is a test message with several words.".to_string(),
        timestamp: None,
    };

    session.add_message(message).unwrap();

    let token_count = session.total_tokens();
    // Rough estimate: ~1 token per 4 characters
    assert!(token_count > 0);
    assert!(token_count < 20); // Should be reasonable for this message
}

#[test]
fn test_session_memory_calculation() {
    let session_id = Uuid::new_v4().to_string();
    let config = SessionConfig::default();
    let mut session = WebSocketSession::with_config(session_id, config);

    let initial_memory = session.memory_used();

    let message = Message {
        role: "user".to_string(),
        content: "x".repeat(1000), // 1000 bytes of content
        timestamp: None,
    };

    session.add_message(message).unwrap();

    let new_memory = session.memory_used();
    assert!(new_memory > initial_memory);
    assert!(new_memory >= 1000); // At least the message content size
}

#[test]
fn test_session_id_generation() {
    let session_id = WebSocketSession::generate_id();

    // Should be a valid UUID v4
    assert!(Uuid::parse_str(&session_id).is_ok());

    // Should be unique
    let another_id = WebSocketSession::generate_id();
    assert_ne!(session_id, another_id);
}

#[test]
fn test_session_config_defaults() {
    let config = SessionConfig::default();

    assert_eq!(config.max_memory_bytes, 10 * 1024 * 1024); // 10MB
    assert_eq!(config.context_window_size, 20);
    assert_eq!(config.timeout_seconds, 1800); // 30 minutes
    assert!(config.enable_compression);
    assert!(!config.enable_persistence);
}

#[test]
fn test_session_with_system_message() {
    let session_id = Uuid::new_v4().to_string();
    let config = SessionConfig::default();
    let mut session = WebSocketSession::with_config(session_id, config);

    let system_message = Message {
        role: "system".to_string(),
        content: "You are a helpful assistant.".to_string(),
        timestamp: None,
    };

    session.add_message(system_message).unwrap();

    assert_eq!(session.message_count(), 1);
    assert_eq!(session.conversation_history()[0].role, "system");
}
