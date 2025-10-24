// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
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

    // Encrypted message types (Phase 6.2.1)
    EncryptedSessionInit,
    EncryptedMessage,
    EncryptedChunk,
    EncryptedResponse,

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

// ============================================================================
// Encrypted Message Payloads (Phase 6.2.1)
// ============================================================================

/// Encrypted payload for session initialization (includes ephemeral public key and signature)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInitEncryptedPayload {
    #[serde(rename = "ephPubHex")]
    pub eph_pub_hex: String,

    #[serde(rename = "ciphertextHex")]
    pub ciphertext_hex: String,

    #[serde(rename = "nonceHex")]
    pub nonce_hex: String,

    #[serde(rename = "signatureHex")]
    pub signature_hex: String,

    #[serde(rename = "aadHex")]
    pub aad_hex: String,
}

/// Encrypted payload for regular messages (no ephemeral key or signature)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEncryptedPayload {
    #[serde(rename = "ciphertextHex")]
    pub ciphertext_hex: String,

    #[serde(rename = "nonceHex")]
    pub nonce_hex: String,

    #[serde(rename = "aadHex")]
    pub aad_hex: String,
}

/// Encrypted payload for streaming response chunks (includes chunk index)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkEncryptedPayload {
    #[serde(rename = "ciphertextHex")]
    pub ciphertext_hex: String,

    #[serde(rename = "nonceHex")]
    pub nonce_hex: String,

    #[serde(rename = "aadHex")]
    pub aad_hex: String,

    pub index: u32,
}

/// Encrypted payload for final response (includes finish_reason)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseEncryptedPayload {
    #[serde(rename = "ciphertextHex")]
    pub ciphertext_hex: String,

    #[serde(rename = "nonceHex")]
    pub nonce_hex: String,

    #[serde(rename = "aadHex")]
    pub aad_hex: String,

    pub finish_reason: String,
}

// ============================================================================
// Validation Error Types (Phase 6.2.1, Sub-phase 4.2)
// ============================================================================

/// Validation error for encrypted message payloads
#[derive(Debug, Clone)]
pub enum ValidationError {
    /// Invalid hex encoding in a field
    InvalidHexEncoding { field: String, message: String },
    /// Field has invalid size
    InvalidFieldSize {
        field: String,
        expected: String,
        actual: usize,
    },
    /// Required field is missing
    MissingField { field: String },
    /// Field is empty when it shouldn't be
    EmptyField { field: String },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::InvalidHexEncoding { field, message } => {
                write!(f, "Invalid hex encoding in field '{}': {}", field, message)
            }
            ValidationError::InvalidFieldSize {
                field,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "Invalid size for field '{}': expected {}, got {} bytes",
                    field, expected, actual
                )
            }
            ValidationError::MissingField { field } => {
                write!(f, "Missing required field: '{}'", field)
            }
            ValidationError::EmptyField { field } => {
                write!(f, "Field '{}' cannot be empty", field)
            }
        }
    }
}

impl std::error::Error for ValidationError {}

// ============================================================================
// Validation Helper Functions (Phase 6.2.1, Sub-phase 4.2)
// ============================================================================

/// Decode hex-encoded field, supporting both "0x"-prefixed and non-prefixed hex
fn decode_hex_field(hex_str: &str, field_name: &str) -> Result<Vec<u8>, ValidationError> {
    if hex_str.is_empty() {
        return Err(ValidationError::EmptyField {
            field: field_name.to_string(),
        });
    }

    // Strip "0x" prefix if present
    let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);

    // Decode hex
    hex::decode(hex_str).map_err(|e| ValidationError::InvalidHexEncoding {
        field: field_name.to_string(),
        message: e.to_string(),
    })
}

/// Decode hex field but allow empty (for optional fields like AAD)
fn decode_hex_field_optional(hex_str: &str, field_name: &str) -> Result<Vec<u8>, ValidationError> {
    if hex_str.is_empty() {
        return Ok(Vec::new());
    }

    let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);

    hex::decode(hex_str).map_err(|e| ValidationError::InvalidHexEncoding {
        field: field_name.to_string(),
        message: e.to_string(),
    })
}

/// Validate that a field has exactly the expected size
fn validate_exact_size(
    data: &[u8],
    expected: usize,
    field_name: &str,
) -> Result<(), ValidationError> {
    if data.len() != expected {
        return Err(ValidationError::InvalidFieldSize {
            field: field_name.to_string(),
            expected: format!("{} bytes", expected),
            actual: data.len(),
        });
    }
    Ok(())
}

/// Validate that a field has one of multiple valid sizes
fn validate_size_options(
    data: &[u8],
    options: &[usize],
    field_name: &str,
) -> Result<(), ValidationError> {
    if !options.contains(&data.len()) {
        let expected_str = options
            .iter()
            .map(|s| format!("{}", s))
            .collect::<Vec<_>>()
            .join(" or ");
        return Err(ValidationError::InvalidFieldSize {
            field: field_name.to_string(),
            expected: format!("{} bytes", expected_str),
            actual: data.len(),
        });
    }
    Ok(())
}

// ============================================================================
// Validated Payload Structs (Phase 6.2.1, Sub-phase 4.2)
// ============================================================================

/// Validated session init payload with decoded bytes
#[derive(Debug, Clone)]
pub struct ValidatedSessionInitPayload {
    pub eph_pub: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub nonce: [u8; 24],
    pub signature: [u8; 65],
    pub aad: Vec<u8>,
}

/// Validated message payload with decoded bytes
#[derive(Debug, Clone)]
pub struct ValidatedMessagePayload {
    pub ciphertext: Vec<u8>,
    pub nonce: [u8; 24],
    pub aad: Vec<u8>,
}

/// Validated chunk payload with decoded bytes
#[derive(Debug, Clone)]
pub struct ValidatedChunkPayload {
    pub ciphertext: Vec<u8>,
    pub nonce: [u8; 24],
    pub aad: Vec<u8>,
    pub index: u32,
}

/// Validated response payload with decoded bytes
#[derive(Debug, Clone)]
pub struct ValidatedResponsePayload {
    pub ciphertext: Vec<u8>,
    pub nonce: [u8; 24],
    pub aad: Vec<u8>,
    pub finish_reason: String,
}

// Helper functions for parsing encrypted messages
impl SessionInitEncryptedPayload {
    pub fn from_payload(payload: &Value) -> Result<Self, String> {
        serde_json::from_value(payload.clone())
            .map_err(|e| format!("Failed to parse session init encrypted payload: {}", e))
    }

    /// Validate and decode the encrypted session init payload
    pub fn validate(&self) -> Result<ValidatedSessionInitPayload, ValidationError> {
        // Decode hex fields
        let eph_pub = decode_hex_field(&self.eph_pub_hex, "ephPubHex")?;
        let ciphertext = decode_hex_field(&self.ciphertext_hex, "ciphertextHex")?;
        let nonce_bytes = decode_hex_field(&self.nonce_hex, "nonceHex")?;
        let signature_bytes = decode_hex_field(&self.signature_hex, "signatureHex")?;
        let aad = decode_hex_field_optional(&self.aad_hex, "aadHex")?;

        // Validate sizes
        validate_size_options(&eph_pub, &[33, 65], "ephPubHex")?; // Compressed or uncompressed
        validate_exact_size(&nonce_bytes, 24, "nonceHex")?;
        validate_exact_size(&signature_bytes, 65, "signatureHex")?;

        // Convert to fixed-size arrays
        let mut nonce = [0u8; 24];
        nonce.copy_from_slice(&nonce_bytes);

        let mut signature = [0u8; 65];
        signature.copy_from_slice(&signature_bytes);

        Ok(ValidatedSessionInitPayload {
            eph_pub,
            ciphertext,
            nonce,
            signature,
            aad,
        })
    }
}

impl MessageEncryptedPayload {
    pub fn from_payload(payload: &Value) -> Result<Self, String> {
        serde_json::from_value(payload.clone())
            .map_err(|e| format!("Failed to parse message encrypted payload: {}", e))
    }

    /// Validate and decode the encrypted message payload
    pub fn validate(&self) -> Result<ValidatedMessagePayload, ValidationError> {
        // Decode hex fields
        let ciphertext = decode_hex_field(&self.ciphertext_hex, "ciphertextHex")?;
        let nonce_bytes = decode_hex_field(&self.nonce_hex, "nonceHex")?;
        let aad = decode_hex_field_optional(&self.aad_hex, "aadHex")?;

        // Validate sizes
        validate_exact_size(&nonce_bytes, 24, "nonceHex")?;

        // Convert to fixed-size array
        let mut nonce = [0u8; 24];
        nonce.copy_from_slice(&nonce_bytes);

        Ok(ValidatedMessagePayload {
            ciphertext,
            nonce,
            aad,
        })
    }
}

impl ChunkEncryptedPayload {
    pub fn from_payload(payload: &Value) -> Result<Self, String> {
        serde_json::from_value(payload.clone())
            .map_err(|e| format!("Failed to parse chunk encrypted payload: {}", e))
    }

    /// Validate and decode the encrypted chunk payload
    pub fn validate(&self) -> Result<ValidatedChunkPayload, ValidationError> {
        // Decode hex fields
        let ciphertext = decode_hex_field(&self.ciphertext_hex, "ciphertextHex")?;
        let nonce_bytes = decode_hex_field(&self.nonce_hex, "nonceHex")?;
        let aad = decode_hex_field_optional(&self.aad_hex, "aadHex")?;

        // Validate sizes
        validate_exact_size(&nonce_bytes, 24, "nonceHex")?;

        // Convert to fixed-size array
        let mut nonce = [0u8; 24];
        nonce.copy_from_slice(&nonce_bytes);

        Ok(ValidatedChunkPayload {
            ciphertext,
            nonce,
            aad,
            index: self.index,
        })
    }
}

impl ResponseEncryptedPayload {
    pub fn from_payload(payload: &Value) -> Result<Self, String> {
        serde_json::from_value(payload.clone())
            .map_err(|e| format!("Failed to parse response encrypted payload: {}", e))
    }

    /// Validate and decode the encrypted response payload
    pub fn validate(&self) -> Result<ValidatedResponsePayload, ValidationError> {
        // Decode hex fields
        let ciphertext = decode_hex_field(&self.ciphertext_hex, "ciphertextHex")?;
        let nonce_bytes = decode_hex_field(&self.nonce_hex, "nonceHex")?;
        let aad = decode_hex_field_optional(&self.aad_hex, "aadHex")?;

        // Validate sizes
        validate_exact_size(&nonce_bytes, 24, "nonceHex")?;

        // Validate finish_reason is not empty
        if self.finish_reason.is_empty() {
            return Err(ValidationError::EmptyField {
                field: "finish_reason".to_string(),
            });
        }

        // Convert to fixed-size array
        let mut nonce = [0u8; 24];
        nonce.copy_from_slice(&nonce_bytes);

        Ok(ValidatedResponsePayload {
            ciphertext,
            nonce,
            aad,
            finish_reason: self.finish_reason.clone(),
        })
    }
}
