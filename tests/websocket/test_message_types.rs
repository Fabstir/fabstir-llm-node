use serde::{Deserialize, Serialize};
use serde_json::json;

// Test message structures aligned with SDK protocol
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Message {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamp: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tokens: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabstir_llm_node::api::websocket::messages::{
        ConversationMessage, ErrorCode, WebSocketMessage,
    };

    #[test]
    fn test_session_init_message_structure() {
        let context = vec![
            ConversationMessage {
                role: "user".to_string(),
                content: "What is AI?".to_string(),
                timestamp: Some(1234567890),
                tokens: None,
                proof: None,
            },
            ConversationMessage {
                role: "assistant".to_string(),
                content: "AI is...".to_string(),
                timestamp: Some(1234567891),
                tokens: Some(45),
                proof: None,
            },
        ];

        let msg = WebSocketMessage::SessionInit {
            session_id: "user-generated-uuid".to_string(),
            job_id: 12345,
            chain_id: Some(84532), // Base Sepolia
            conversation_context: context.clone(),
        };

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "session_init");
        assert_eq!(json["session_id"], "user-generated-uuid");
        assert_eq!(json["job_id"], 12345);
        assert_eq!(json["conversation_context"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_session_resume_message_structure() {
        let context = vec![ConversationMessage {
            role: "user".to_string(),
            content: "Previous question".to_string(),
            timestamp: None,
            tokens: None,
            proof: None,
        }];

        let msg = WebSocketMessage::SessionResume {
            session_id: "same-uuid".to_string(),
            job_id: 12345,
            conversation_context: context,
            last_message_index: 8,
        };

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "session_resume");
        assert_eq!(json["last_message_index"], 8);
    }

    #[test]
    fn test_prompt_message_structure() {
        let msg = WebSocketMessage::Prompt {
            session_id: "user-generated-uuid".to_string(),
            content: "Tell me more about machine learning".to_string(),
            message_index: 5,
        };

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "prompt");
        assert_eq!(json["content"], "Tell me more about machine learning");
        assert_eq!(json["message_index"], 5);
    }

    #[test]
    fn test_response_message_structure() {
        let msg = WebSocketMessage::Response {
            session_id: "user-generated-uuid".to_string(),
            content: "Machine learning is a subset of AI...".to_string(),
            tokens_used: 45,
            message_index: 6,
        };

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "response");
        assert_eq!(json["tokens_used"], 45);
        assert_eq!(json["message_index"], 6);
    }

    #[test]
    fn test_error_message_structure() {
        let msg = WebSocketMessage::Error {
            session_id: "user-generated-uuid".to_string(),
            error: "Invalid session".to_string(),
            code: ErrorCode::SessionNotFound,
        };

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "error");
        assert_eq!(json["error"], "Invalid session");
        assert_eq!(json["code"], "SESSION_NOT_FOUND");
    }

    #[test]
    fn test_session_end_message_structure() {
        let msg = WebSocketMessage::SessionEnd {
            session_id: "user-generated-uuid".to_string(),
        };

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "session_end");
        assert_eq!(json["session_id"], "user-generated-uuid");
    }

    #[test]
    fn test_message_deserialization_from_sdk() {
        // Test deserializing a message from TypeScript SDK
        let sdk_json = json!({
            "type": "session_init",
            "session_id": "test-123",
            "job_id": 999,
            "conversation_context": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi there", "tokens": 5}
            ]
        });

        let msg: WebSocketMessage = serde_json::from_value(sdk_json).unwrap();
        match msg {
            WebSocketMessage::SessionInit {
                session_id,
                job_id,
                chain_id,
                conversation_context,
            } => {
                assert_eq!(session_id, "test-123");
                assert_eq!(job_id, 999);
                assert_eq!(conversation_context.len(), 2);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_empty_conversation_context() {
        let msg = WebSocketMessage::SessionInit {
            session_id: "new-session".to_string(),
            job_id: 1,
            chain_id: None, // No specific chain
            conversation_context: vec![],
        };

        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["conversation_context"].as_array().unwrap().len(), 0);
    }
}
