use serde::{Deserialize, Serialize};
use std::fmt;
use std::collections::HashSet;

/// Proof data for verifiable inference
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProofData {
    pub hash: String,
    pub proof_type: String,
    pub model_hash: String,
    pub input_hash: String,
    pub output_hash: String,
    pub timestamp: u64,
}

/// Conversation message structure aligned with TypeScript SDK
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<ProofData>,
}

/// Error codes for WebSocket messages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    InvalidRequest,
    SessionNotFound,
    InvalidJobId,
    InvalidMessageIndex,
    EmptyPrompt,
    ModelNotLoaded,
    InferenceError,
    TokenLimitExceeded,
    RateLimitExceeded,
    AuthenticationFailed,
    InternalError,
    Timeout,
    UnsupportedChain,
    ChainMismatch,
    JobNotFoundOnChain,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorCode::InvalidRequest => write!(f, "INVALID_REQUEST"),
            ErrorCode::SessionNotFound => write!(f, "SESSION_NOT_FOUND"),
            ErrorCode::InvalidJobId => write!(f, "INVALID_JOB_ID"),
            ErrorCode::InvalidMessageIndex => write!(f, "INVALID_MESSAGE_INDEX"),
            ErrorCode::EmptyPrompt => write!(f, "EMPTY_PROMPT"),
            ErrorCode::ModelNotLoaded => write!(f, "MODEL_NOT_LOADED"),
            ErrorCode::InferenceError => write!(f, "INFERENCE_ERROR"),
            ErrorCode::TokenLimitExceeded => write!(f, "TOKEN_LIMIT_EXCEEDED"),
            ErrorCode::RateLimitExceeded => write!(f, "RATE_LIMIT_EXCEEDED"),
            ErrorCode::AuthenticationFailed => write!(f, "AUTHENTICATION_FAILED"),
            ErrorCode::InternalError => write!(f, "INTERNAL_ERROR"),
            ErrorCode::Timeout => write!(f, "TIMEOUT"),
            ErrorCode::UnsupportedChain => write!(f, "UNSUPPORTED_CHAIN"),
            ErrorCode::ChainMismatch => write!(f, "CHAIN_MISMATCH"),
            ErrorCode::JobNotFoundOnChain => write!(f, "JOB_NOT_FOUND_ON_CHAIN"),
        }
    }
}

/// Chain information for multi-chain support
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChainInfo {
    pub chain_id: u64,
    pub chain_name: String,
    pub native_token: String,
    pub rpc_url: String,
}

/// Session initialization message with chain support
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionInitMessage {
    pub job_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_id: Option<u64>,
    pub user_address: String,
    pub host_address: String,
    pub model_id: String,
    pub timestamp: u64,
}

impl SessionInitMessage {
    /// Convert from legacy format without chain_id
    pub fn from_legacy(legacy: LegacySessionInitMessage) -> Self {
        Self {
            job_id: legacy.job_id,
            chain_id: Some(84532), // Default to Base Sepolia
            user_address: legacy.user_address,
            host_address: legacy.host_address,
            model_id: legacy.model_id,
            timestamp: legacy.timestamp,
        }
    }
}

/// Legacy session init message for backwards compatibility
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LegacySessionInitMessage {
    pub job_id: u64,
    pub user_address: String,
    pub host_address: String,
    pub model_id: String,
    pub timestamp: u64,
}

/// WebSocket message types aligned with SDK protocol
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WebSocketMessage {
    /// Initialize a new session with optional context
    SessionInit {
        session_id: String,
        job_id: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        chain_id: Option<u64>,
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

/// Session response with chain information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionResponse {
    pub session_id: u64,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_info: Option<ChainInfo>,
    pub tokens_used: u64,
    pub timestamp: u64,
}

/// Response for session initialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInitResponse {
    pub session_id: String,
    pub job_id: u64,
    pub message_count: usize,
    pub total_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_info: Option<ChainInfo>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_info: Option<ChainInfo>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof: Option<ProofData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_info: Option<ChainInfo>,
}

/// WebSocket error type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketError {
    pub code: ErrorCode,
    pub message: String,
    pub session_id: Option<String>,
}

/// Message validator for chain validation
pub struct MessageValidator {
    supported_chains: HashSet<u64>,
}

impl MessageValidator {
    pub fn new() -> Self {
        let mut supported_chains = HashSet::new();
        supported_chains.insert(84532); // Base Sepolia
        supported_chains.insert(5611);  // opBNB Testnet
        Self { supported_chains }
    }

    pub fn validate_chain(&self, msg: &SessionInitMessage) -> Result<(), String> {
        if let Some(chain_id) = msg.chain_id {
            if !self.supported_chains.contains(&chain_id) {
                return Err(format!("Unsupported chain ID: {}", chain_id));
            }
        }
        Ok(())
    }

    pub fn is_chain_supported(&self, chain_id: u64) -> bool {
        self.supported_chains.contains(&chain_id)
    }
}

impl Default for MessageValidator {
    fn default() -> Self {
        Self::new()
    }
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
            chain_id: Some(84532),
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

    #[test]
    fn test_chain_aware_session_init() {
        // Test SessionInitMessage with chain_id
        let msg = SessionInitMessage {
            job_id: 123,
            chain_id: Some(84532), // Base Sepolia
            user_address: "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb7".to_string(),
            host_address: "0x5aAeb6053f3E94C9b9A09f33669435E7Ef1BeAed".to_string(),
            model_id: "llama-7b".to_string(),
            timestamp: 1640000000,
        };

        assert_eq!(msg.chain_id, Some(84532));

        // Test serialization includes chain_id
        let json_str = serde_json::to_string(&msg).unwrap();
        let json_val: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(json_val["chain_id"], 84532);
    }

    #[test]
    fn test_chain_info_structure() {
        let chain_info = ChainInfo {
            chain_id: 5611,
            chain_name: "opBNB Testnet".to_string(),
            native_token: "BNB".to_string(),
            rpc_url: "https://opbnb-testnet-rpc.bnbchain.org".to_string(),
        };

        assert_eq!(chain_info.chain_id, 5611);
        assert_eq!(chain_info.native_token, "BNB");
    }

    #[test]
    fn test_message_validator() {
        let validator = MessageValidator::new();

        // Test supported chains
        assert!(validator.is_chain_supported(84532)); // Base Sepolia
        assert!(validator.is_chain_supported(5611));  // opBNB Testnet

        // Test unsupported chains
        assert!(!validator.is_chain_supported(1));    // Ethereum mainnet
        assert!(!validator.is_chain_supported(99999)); // Invalid chain
    }

    #[test]
    fn test_legacy_message_conversion() {
        let legacy = LegacySessionInitMessage {
            job_id: 456,
            user_address: "0xabc".to_string(),
            host_address: "0xdef".to_string(),
            model_id: "gpt-4".to_string(),
            timestamp: 1640000000,
        };

        let converted = SessionInitMessage::from_legacy(legacy);
        assert_eq!(converted.chain_id, Some(84532)); // Defaults to Base Sepolia
        assert_eq!(converted.job_id, 456);
    }

    #[test]
    fn test_session_response_with_chain() {
        let chain_info = ChainInfo {
            chain_id: 84532,
            chain_name: "Base Sepolia".to_string(),
            native_token: "ETH".to_string(),
            rpc_url: "https://sepolia.base.org".to_string(),
        };

        let response = SessionResponse {
            session_id: 123,
            status: "active".to_string(),
            chain_info: Some(chain_info),
            tokens_used: 450,
            timestamp: 1640000000,
        };

        assert!(response.chain_info.is_some());
        let chain = response.chain_info.unwrap();
        assert_eq!(chain.chain_id, 84532);
        assert_eq!(chain.native_token, "ETH");
    }
}