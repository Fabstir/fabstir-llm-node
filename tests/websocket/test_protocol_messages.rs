use fabstir_llm_node::api::websocket::{
    protocol::{MessageType, ProtocolError, ProtocolMessage, SessionCommand},
    protocol_handlers::{HandlerRegistry, MessageHandler},
};
use serde_json::{json, Value};
use uuid::Uuid;

#[test]
fn test_protocol_message_serialization() {
    let msg = ProtocolMessage {
        msg_type: MessageType::SessionControl,
        command: Some(SessionCommand::Init),
        session_id: Some("test-session".to_string()),
        metadata: Some(json!({
            "test": "value"
        })),
        payload: None,
    };

    let serialized = serde_json::to_string(&msg).unwrap();
    let deserialized: ProtocolMessage = serde_json::from_str(&serialized).unwrap();

    assert_eq!(deserialized.msg_type, MessageType::SessionControl);
    assert_eq!(deserialized.command, Some(SessionCommand::Init));
    assert_eq!(deserialized.session_id, Some("test-session".to_string()));
}

#[test]
fn test_message_type_variants() {
    let types = vec![
        MessageType::SessionControl,
        MessageType::Heartbeat,
        MessageType::HeartbeatAck,
        MessageType::Metadata,
        MessageType::MetadataAck,
        MessageType::StateSync,
        MessageType::StateSyncAck,
        MessageType::Capabilities,
        MessageType::CapabilitiesAck,
        MessageType::Version,
        MessageType::VersionAck,
        MessageType::Error,
        MessageType::Data,
    ];

    for msg_type in types {
        let msg = ProtocolMessage {
            msg_type: msg_type.clone(),
            command: None,
            session_id: None,
            metadata: None,
            payload: None,
        };

        let json = serde_json::to_value(&msg).unwrap();
        assert!(json["msg_type"].is_string());
    }
}

#[test]
fn test_session_command_variants() {
    let commands = vec![
        SessionCommand::Init,
        SessionCommand::InitAck,
        SessionCommand::Resume,
        SessionCommand::ResumeAck,
        SessionCommand::Clear,
        SessionCommand::ClearAck,
        SessionCommand::Handoff,
        SessionCommand::HandoffReady,
        SessionCommand::Terminate,
        SessionCommand::TerminateAck,
    ];

    for command in commands {
        let msg = ProtocolMessage {
            msg_type: MessageType::SessionControl,
            command: Some(command.clone()),
            session_id: None,
            metadata: None,
            payload: None,
        };

        let json = serde_json::to_value(&msg).unwrap();
        assert!(json["command"].is_string());
    }
}

#[tokio::test]
async fn test_handler_registry() {
    let mut registry = HandlerRegistry::new();

    // Register a custom handler
    registry.register(MessageType::Heartbeat, |msg| {
        Box::pin(async move {
            Ok(ProtocolMessage {
                msg_type: MessageType::HeartbeatAck,
                command: None,
                session_id: msg.session_id,
                metadata: Some(json!({ "handled": true })),
                payload: None,
            })
        })
    });

    let msg = ProtocolMessage {
        msg_type: MessageType::Heartbeat,
        command: None,
        session_id: Some("test".to_string()),
        metadata: None,
        payload: None,
    };

    let response = registry.handle(msg).await.unwrap();
    assert_eq!(response.msg_type, MessageType::HeartbeatAck);
    assert_eq!(response.metadata.unwrap()["handled"], true);
}

#[tokio::test]
async fn test_error_message_creation() {
    let error = ProtocolError::SessionNotFound("test-session".to_string());
    let error_msg = error.to_protocol_message();

    assert_eq!(error_msg.msg_type, MessageType::Error);
    assert!(error_msg.metadata.is_some());

    let metadata = error_msg.metadata.unwrap();
    assert!(metadata["error_code"].is_string());
    assert!(metadata["error_message"].is_string());
}

#[test]
fn test_message_validation() {
    // Valid message
    let valid_msg = ProtocolMessage {
        msg_type: MessageType::SessionControl,
        command: Some(SessionCommand::Init),
        session_id: None,
        metadata: None,
        payload: None,
    };
    assert!(valid_msg.validate().is_ok());

    // Invalid: SessionControl without command
    let invalid_msg = ProtocolMessage {
        msg_type: MessageType::SessionControl,
        command: None,
        session_id: None,
        metadata: None,
        payload: None,
    };
    assert!(invalid_msg.validate().is_err());

    // Invalid: Resume without session_id
    let invalid_resume = ProtocolMessage {
        msg_type: MessageType::SessionControl,
        command: Some(SessionCommand::Resume),
        session_id: None,
        metadata: None,
        payload: None,
    };
    assert!(invalid_resume.validate().is_err());
}

#[test]
fn test_message_builder() {
    let msg = ProtocolMessage::builder()
        .msg_type(MessageType::Heartbeat)
        .session_id("test-session".to_string())
        .metadata(json!({ "timestamp": 123456 }))
        .build();

    assert_eq!(msg.msg_type, MessageType::Heartbeat);
    assert_eq!(msg.session_id, Some("test-session".to_string()));
    assert!(msg.metadata.is_some());
}

#[tokio::test]
async fn test_handler_chain() {
    let mut handler = MessageHandler::new();

    // Add pre-processing middleware
    handler.add_middleware(|msg| {
        Box::pin(async move {
            let mut msg = msg;
            if let Some(ref mut metadata) = msg.metadata {
                metadata["preprocessed"] = json!(true);
            } else {
                msg.metadata = Some(json!({ "preprocessed": true }));
            }
            Ok(msg)
        })
    });

    let input = ProtocolMessage {
        msg_type: MessageType::Data,
        command: None,
        session_id: None,
        metadata: None,
        payload: Some(json!({ "data": "test" })),
    };

    let processed = handler.process(input).await.unwrap();
    assert_eq!(processed.metadata.unwrap()["preprocessed"], true);
}

#[test]
fn test_protocol_error_types() {
    let errors = vec![
        ProtocolError::SessionNotFound("session".to_string()),
        ProtocolError::InvalidMessage("bad message".to_string()),
        ProtocolError::UnsupportedVersion(1, 2),
        ProtocolError::CapabilityMismatch(vec!["cap1".to_string()]),
        ProtocolError::HandoffFailed("reason".to_string()),
        ProtocolError::Timeout(5000),
    ];

    for error in errors {
        let msg = error.to_protocol_message();
        assert_eq!(msg.msg_type, MessageType::Error);
        assert!(msg.metadata.is_some());
    }
}

#[tokio::test]
async fn test_batch_message_handling() {
    let handler = MessageHandler::new();

    let messages = vec![
        ProtocolMessage {
            msg_type: MessageType::Heartbeat,
            command: None,
            session_id: Some("session1".to_string()),
            metadata: None,
            payload: None,
        },
        ProtocolMessage {
            msg_type: MessageType::Heartbeat,
            command: None,
            session_id: Some("session2".to_string()),
            metadata: None,
            payload: None,
        },
    ];

    let responses = handler.process_batch(messages).await.unwrap();
    assert_eq!(responses.len(), 2);

    for response in responses {
        assert_eq!(response.msg_type, MessageType::HeartbeatAck);
    }
}
