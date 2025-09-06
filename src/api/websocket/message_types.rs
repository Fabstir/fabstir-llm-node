use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    // Session management
    Init,
    SessionControl,
    SessionControlAck,
    
    // Inference
    Inference,
    StatelessInference,
    InferenceResponse,
    
    // Connection management
    Ping,
    Pong,
    Close,
    
    // Error and unknown
    Error,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketMessage {
    #[serde(rename = "type")]
    pub msg_type: MessageType,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceMessage {
    pub prompt: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_context: Option<Vec<ContextMessage>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionControl {
    Clear,
    Resume,
    Status,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionControlMessage {
    pub action: SessionControl,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorMessage {
    pub code: String,
    pub message: String,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionMode {
    pub mode: String, // "stateful" or "stateless"
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl WebSocketMessage {
    pub fn new(msg_type: MessageType, payload: Value) -> Self {
        Self {
            msg_type,
            session_id: None,
            payload,
        }
    }

    pub fn with_session(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }

    pub fn error(code: &str, message: &str) -> Self {
        Self {
            msg_type: MessageType::Error,
            session_id: None,
            payload: serde_json::json!({
                "code": code,
                "message": message
            }),
        }
    }

    pub fn inference_response(session_id: Option<String>, content: String) -> Self {
        Self {
            msg_type: MessageType::InferenceResponse,
            session_id,
            payload: serde_json::json!({
                "content": content,
                "finish_reason": "stop"
            }),
        }
    }
}

impl Default for MessageType {
    fn default() -> Self {
        MessageType::Unknown
    }
}

// Helper functions for parsing messages
impl InferenceMessage {
    pub fn from_payload(payload: &Value) -> Result<Self, String> {
        serde_json::from_value(payload.clone())
            .map_err(|e| format!("Failed to parse inference message: {}", e))
    }
}

impl SessionControlMessage {
    pub fn from_payload(payload: &Value) -> Result<Self, String> {
        serde_json::from_value(payload.clone())
            .map_err(|e| format!("Failed to parse session control message: {}", e))
    }
}