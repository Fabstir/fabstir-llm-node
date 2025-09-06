use serde::{Deserialize, Serialize};
use std::fmt;

/// Conversation message structure aligned with TypeScript SDK
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u32>,
}

/// Error codes for WebSocket messages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    SessionNotFound,
    InvalidJobId,
    InvalidMessageIndex,
    EmptyPrompt,
    InferenceError,
    TokenLimitExceeded,
    RateLimitExceeded,
    AuthenticationFailed,
    InternalError,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorCode::SessionNotFound => write!(f, "SESSION_NOT_FOUND"),
            ErrorCode::InvalidJobId => write!(f, "INVALID_JOB_ID"),
            ErrorCode::InvalidMessageIndex => write!(f, "INVALID_MESSAGE_INDEX"),
            ErrorCode::EmptyPrompt => write!(f, "EMPTY_PROMPT"),
            ErrorCode::InferenceError => write!(f, "INFERENCE_ERROR"),
            ErrorCode::TokenLimitExceeded => write!(f, "TOKEN_LIMIT_EXCEEDED"),
            ErrorCode::RateLimitExceeded => write!(f, "RATE_LIMIT_EXCEEDED"),
            ErrorCode::AuthenticationFailed => write!(f, "AUTHENTICATION_FAILED"),
            ErrorCode::InternalError => write!(f, "INTERNAL_ERROR"),
        }
    }
}

/// WebSocket message types aligned with SDK protocol
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WebSocketMessage {
    /// Initialize a new session with optional context
    SessionInit {
        session_id: String,
        job_id: u64,
        conversation_context: Vec<ConversationMessage>,
    },
    
    /// Resume an existing session with full context
    SessionResume {
        session_id: String,
        job_id: u64,
        conversation_context: Vec<ConversationMessage>,
        last_message_index: u32,
    },
    
    /// Send a new prompt (minimal data during active session)
    Prompt {
        session_id: String,
        content: String,
        message_index: u32,
    },
    
    /// Response from the LLM
    Response {
        session_id: String,
        content: String,
        tokens_used: u32,
        message_index: u32,
    },
    
    /// Error message
    Error {
        session_id: String,
        error: String,
        code: ErrorCode,
    },
    
    /// End the session
    SessionEnd {
        session_id: String,
    },
}

impl WebSocketMessage {
    /// Get the session ID from any message type
    pub fn session_id(&self) -> &str {
        match self {
            WebSocketMessage::SessionInit { session_id, .. } => session_id,
            WebSocketMessage::SessionResume { session_id, .. } => session_id,
            WebSocketMessage::Prompt { session_id, .. } => session_id,
            WebSocketMessage::Response { session_id, .. } => session_id,
            WebSocketMessage::Error { session_id, .. } => session_id,
            WebSocketMessage::SessionEnd { session_id } => session_id,
        }
    }
    
    /// Get the message type as a string
    pub fn message_type(&self) -> &str {
        match self {
            WebSocketMessage::SessionInit { .. } => "session_init",
            WebSocketMessage::SessionResume { .. } => "session_resume",
            WebSocketMessage::Prompt { .. } => "prompt",
            WebSocketMessage::Response { .. } => "response",
            WebSocketMessage::Error { .. } => "error",
            WebSocketMessage::SessionEnd { .. } => "session_end",
        }
    }
}

/// Response for session initialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInitResponse {
    pub session_id: String,
    pub job_id: u64,
    pub message_count: usize,
    pub total_tokens: u32,
}

/// Response for session resume
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResumeResponse {
    pub session_id: String,
    pub job_id: u64,
    pub message_count: usize,
    pub total_tokens: u32,
    pub last_message_index: u32,
    pub resumed_successfully: bool,
}

/// Response for prompt handling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptResponse {
    pub session_id: String,
    pub message_index: u32,
    pub added_to_cache: bool,
}

/// Token for streaming response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamToken {
    pub content: String,
    pub is_final: bool,
    pub total_tokens: u32,
    pub message_index: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_message_serialization() {
        let msg = WebSocketMessage::SessionInit {
            session_id: "test".to_string(),
            job_id: 123,
            conversation_context: vec![],
        };
        
        let json = serde_json::to_value(&msg).unwrap();
        assert_eq!(json["type"], "session_init");
    }
    
    #[test]
    fn test_message_deserialization() {
        let json = json!({
            "type": "prompt",
            "session_id": "test",
            "content": "Hello",
            "message_index": 1
        });
        
        let msg: WebSocketMessage = serde_json::from_value(json).unwrap();
        assert_eq!(msg.message_type(), "prompt");
    }
    
    #[test]
    fn test_error_code_serialization() {
        let code = ErrorCode::SessionNotFound;
        let json = serde_json::to_value(&code).unwrap();
        assert_eq!(json, "SESSION_NOT_FOUND");
    }
}