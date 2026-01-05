// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use anyhow::{anyhow, Result};
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

    // Vector loading progress (Sub-phase 3.3)
    VectorLoadingProgress,

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

// ============================================================================
// RAG (Retrieval-Augmented Generation) Message Types - Phase 2
// ============================================================================

/// Maximum number of vectors allowed per upload batch
/// Prevents memory exhaustion and ensures reasonable message sizes
pub const MAX_UPLOAD_BATCH_SIZE: usize = 1000;

/// Request to upload vectors to session storage
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadVectorsRequest {
    /// Optional request ID for tracking (client-generated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,

    /// Vectors to upload
    pub vectors: Vec<VectorUpload>,

    /// If true, clear existing vectors before uploading
    /// If false, append to existing vectors
    pub replace: bool,
}

/// Single vector to upload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VectorUpload {
    /// Unique identifier for this vector
    pub id: String,

    /// 384-dimensional embedding vector
    pub vector: Vec<f32>,

    /// JSON metadata associated with this vector
    pub metadata: Value,
}

/// Response to vector upload request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadVectorsResponse {
    /// Message type for client routing
    #[serde(rename = "type")]
    pub msg_type: String,

    /// Request ID (if provided in request)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,

    /// Status of the upload: "success" if all uploaded, "partial" if some rejected
    pub status: String,

    /// Number of vectors successfully uploaded
    pub uploaded: usize,

    /// Number of vectors rejected (validation errors)
    pub rejected: usize,

    /// Error messages for rejected vectors
    pub errors: Vec<String>,
}

impl UploadVectorsRequest {
    /// Validate the upload request
    ///
    /// Checks:
    /// - Batch size <= MAX_UPLOAD_BATCH_SIZE
    /// - All vectors have 384 dimensions
    ///
    /// Returns Ok(()) if valid, Err with details if invalid
    pub fn validate(&self) -> Result<()> {
        // Check batch size
        if self.vectors.len() > MAX_UPLOAD_BATCH_SIZE {
            return Err(anyhow!(
                "Upload batch size too large: {} vectors (max: {})",
                self.vectors.len(),
                MAX_UPLOAD_BATCH_SIZE
            ));
        }

        // Check vector dimensions
        for (idx, upload) in self.vectors.iter().enumerate() {
            if upload.vector.len() != 384 {
                return Err(anyhow!(
                    "Vector {} (id: {}): Invalid dimensions: expected 384, got {}",
                    idx,
                    upload.id,
                    upload.vector.len()
                ));
            }
        }

        Ok(())
    }
}

// ============================================================================
// Vector Search Messages (Sub-phase 2.2)
// ============================================================================

/// Maximum number of results to return from search
pub const MAX_SEARCH_K: usize = 100;

/// Request to search vectors in session storage
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchVectorsRequest {
    /// Optional request ID for async correlation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,

    /// Query vector to search for (384-dimensional)
    pub query_vector: Vec<f32>,

    /// Number of top results to return (max: MAX_SEARCH_K)
    pub k: usize,

    /// Optional minimum similarity score threshold (0.0-1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f32>,

    /// Optional metadata filter (JSON query object)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_filter: Option<Value>,
}

/// Response containing search results
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchVectorsResponse {
    /// Message type for client routing
    #[serde(rename = "type")]
    pub msg_type: String,

    /// Optional request ID matching the request
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,

    /// Search results ordered by descending similarity score
    pub results: Vec<VectorSearchResult>,

    /// Total number of vectors in storage
    pub total_vectors: usize,

    /// Search execution time in milliseconds
    pub search_time_ms: f64,
}

/// Single vector search result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VectorSearchResult {
    /// Vector ID
    pub id: String,

    /// Cosine similarity score (0.0-1.0, higher is better)
    pub score: f32,

    /// Vector metadata
    pub metadata: Value,
}

impl SearchVectorsRequest {
    /// Validates the search request
    pub fn validate(&self) -> Result<()> {
        // Validate k limit
        if self.k > MAX_SEARCH_K {
            return Err(anyhow!(
                "Search k too large: {} (max: {})",
                self.k,
                MAX_SEARCH_K
            ));
        }

        // Validate query vector dimensions
        if self.query_vector.len() != 384 {
            return Err(anyhow!(
                "Invalid query vector dimensions: expected 384, got {}",
                self.query_vector.len()
            ));
        }

        Ok(())
    }
}

// ============================================================================
// S5 Vector Database Loading Messages (Sub-phase 1.1)
// ============================================================================

/// Information about an S5-stored vector database to load for RAG
/// Enables hosts to load pre-existing vector databases from S5 storage
/// instead of requiring clients to upload vectors via WebSocket for every session
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VectorDatabaseInfo {
    /// Path to manifest.json in S5 storage
    /// Format: "home/vector-databases/{userAddress}/{databaseName}/manifest.json"
    pub manifest_path: String,

    /// Owner's Ethereum address (must match manifest.owner for security)
    pub user_address: String,
}

// ============================================================================
// Vector Loading Progress Messages (Sub-phase 7.1)
// ============================================================================

/// Error codes for loading failures (Sub-phase 7.3)
/// Enables SDKs to categorize and handle errors appropriately
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LoadingErrorCode {
    /// S5 manifest path does not exist
    ManifestNotFound,
    /// Network error downloading manifest
    ManifestDownloadFailed,
    /// Network error downloading chunk
    ChunkDownloadFailed,
    /// manifest.owner != user_address (security)
    OwnerMismatch,
    /// Invalid session key or corrupted data
    DecryptionFailed,
    /// Vector dimensions don't match manifest
    DimensionMismatch,
    /// Database too large for configured limit
    MemoryLimitExceeded,
    /// Too many downloads in time window
    RateLimitExceeded,
    /// Loading exceeded 5-minute limit
    Timeout,
    /// manifest_path format invalid
    InvalidPath,
    /// Invalid session key length
    InvalidSessionKey,
    /// No vectors in database
    EmptyDatabase,
    /// Index building failed
    IndexBuildFailed,
    /// Session not found
    SessionNotFound,
    /// Unknown/internal error
    InternalError,
}

/// Real-time progress updates for S5 vector database loading
/// Sent to SDK clients during async loading operations to provide feedback
#[derive(Debug, Clone, PartialEq)]
pub enum LoadingProgressMessage {
    /// Manifest downloaded and parsed successfully
    ManifestDownloaded,

    /// Chunk downloaded and decrypted
    ChunkDownloaded {
        /// Current chunk index (0-based)
        chunk_id: usize,
        /// Total number of chunks
        total: usize,
    },

    /// Building HNSW index from loaded vectors
    IndexBuilding,

    /// Loading completed successfully
    LoadingComplete {
        /// Total number of vectors loaded
        vector_count: usize,
        /// Total loading time in milliseconds
        duration_ms: u64,
    },

    /// Loading failed with error
    LoadingError {
        /// Machine-readable error code for SDK categorization
        error_code: LoadingErrorCode,
        /// User-friendly error message
        error: String,
    },
}

impl LoadingProgressMessage {
    /// Get user-friendly message for this progress event
    pub fn message(&self) -> String {
        match self {
            LoadingProgressMessage::ManifestDownloaded => {
                "Manifest downloaded, loading chunks...".to_string()
            }
            LoadingProgressMessage::ChunkDownloaded { chunk_id, total } => {
                let percent = ((chunk_id + 1) as f64 / *total as f64 * 100.0) as u32;
                format!(
                    "Downloading chunks... {}% ({}/{})",
                    percent,
                    chunk_id + 1,
                    total
                )
            }
            LoadingProgressMessage::IndexBuilding => {
                "Building search index...".to_string()
            }
            LoadingProgressMessage::LoadingComplete { vector_count, duration_ms } => {
                let duration_secs = *duration_ms as f64 / 1000.0;
                format!(
                    "Vector database ready ({} vectors, loaded in {:.2}s)",
                    vector_count, duration_secs
                )
            }
            LoadingProgressMessage::LoadingError { error, .. } => {
                format!("Loading failed: {}", error)
            }
        }
    }
}

/// Custom serialization to include message and computed fields
impl Serialize for LoadingProgressMessage {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(None)?;

        match self {
            LoadingProgressMessage::ManifestDownloaded => {
                map.serialize_entry("event", "manifest_downloaded")?;
                map.serialize_entry("message", &self.message())?;
            }
            LoadingProgressMessage::ChunkDownloaded { chunk_id, total } => {
                let percent = ((chunk_id + 1) as f64 / *total as f64 * 100.0) as u32;
                map.serialize_entry("event", "chunk_downloaded")?;
                map.serialize_entry("chunk_id", chunk_id)?;
                map.serialize_entry("total", total)?;
                map.serialize_entry("percent", &percent)?;
                map.serialize_entry("message", &self.message())?;
            }
            LoadingProgressMessage::IndexBuilding => {
                map.serialize_entry("event", "index_building")?;
                map.serialize_entry("message", &self.message())?;
            }
            LoadingProgressMessage::LoadingComplete { vector_count, duration_ms } => {
                map.serialize_entry("event", "loading_complete")?;
                map.serialize_entry("vector_count", vector_count)?;
                map.serialize_entry("duration_ms", duration_ms)?;
                map.serialize_entry("message", &self.message())?;
            }
            LoadingProgressMessage::LoadingError { error_code, error } => {
                map.serialize_entry("event", "loading_error")?;
                map.serialize_entry("error_code", error_code)?;
                map.serialize_entry("error", error)?;
                map.serialize_entry("message", &self.message())?;
            }
        }

        map.end()
    }
}

/// Custom deserialization for LoadingProgressMessage
impl<'de> Deserialize<'de> for LoadingProgressMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        struct LoadingProgressVisitor;

        impl<'de> Visitor<'de> for LoadingProgressVisitor {
            type Value = LoadingProgressMessage;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a loading progress message")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut event: Option<String> = None;
                let mut chunk_id: Option<usize> = None;
                let mut total: Option<usize> = None;
                let mut vector_count: Option<usize> = None;
                let mut duration_ms: Option<u64> = None;
                let mut error_code: Option<LoadingErrorCode> = None;
                let mut error: Option<String> = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "event" => event = Some(map.next_value()?),
                        "chunk_id" => chunk_id = Some(map.next_value()?),
                        "total" => total = Some(map.next_value()?),
                        "vector_count" => vector_count = Some(map.next_value()?),
                        "duration_ms" => duration_ms = Some(map.next_value()?),
                        "error_code" => error_code = Some(map.next_value()?),
                        "error" => error = Some(map.next_value()?),
                        // Ignore unknown fields (percent, message, etc.)
                        _ => {
                            let _: serde_json::Value = map.next_value()?;
                        }
                    }
                }

                let event = event.ok_or_else(|| de::Error::missing_field("event"))?;

                match event.as_str() {
                    "manifest_downloaded" => Ok(LoadingProgressMessage::ManifestDownloaded),
                    "chunk_downloaded" => {
                        let chunk_id = chunk_id.ok_or_else(|| de::Error::missing_field("chunk_id"))?;
                        let total = total.ok_or_else(|| de::Error::missing_field("total"))?;
                        Ok(LoadingProgressMessage::ChunkDownloaded { chunk_id, total })
                    }
                    "index_building" => Ok(LoadingProgressMessage::IndexBuilding),
                    "loading_complete" => {
                        let vector_count = vector_count.ok_or_else(|| de::Error::missing_field("vector_count"))?;
                        let duration_ms = duration_ms.ok_or_else(|| de::Error::missing_field("duration_ms"))?;
                        Ok(LoadingProgressMessage::LoadingComplete { vector_count, duration_ms })
                    }
                    "loading_error" => {
                        let error_code = error_code.ok_or_else(|| de::Error::missing_field("error_code"))?;
                        let error = error.ok_or_else(|| de::Error::missing_field("error"))?;
                        Ok(LoadingProgressMessage::LoadingError { error_code, error })
                    }
                    _ => Err(de::Error::unknown_variant(&event, &[
                        "manifest_downloaded",
                        "chunk_downloaded",
                        "index_building",
                        "loading_complete",
                        "loading_error",
                    ])),
                }
            }
        }

        deserializer.deserialize_map(LoadingProgressVisitor)
    }
}

impl VectorDatabaseInfo {
    /// Validates the vector database info
    ///
    /// Checks:
    /// - manifest_path ends with "manifest.json"
    /// - user_address is a valid Ethereum address (0x + 40 hex chars)
    ///
    /// Returns Ok(()) if valid, Err with details if invalid
    pub fn validate(&self) -> Result<()> {
        // Validate manifest_path format
        if self.manifest_path.is_empty() {
            return Err(anyhow!("manifest_path cannot be empty"));
        }

        if !self.manifest_path.ends_with("manifest.json") {
            return Err(anyhow!(
                "manifest_path must end with 'manifest.json', got: {}",
                self.manifest_path
            ));
        }

        // Validate user_address format
        if self.user_address.is_empty() {
            return Err(anyhow!("user_address cannot be empty"));
        }

        if !self.user_address.starts_with("0x") {
            return Err(anyhow!(
                "user_address must start with '0x', got: {}",
                self.user_address
            ));
        }

        // Check length: 0x + 40 hex characters = 42 total
        if self.user_address.len() != 42 {
            return Err(anyhow!(
                "user_address must be 42 characters (0x + 40 hex), got {} characters",
                self.user_address.len()
            ));
        }

        // Validate hex characters (after 0x prefix)
        let hex_part = &self.user_address[2..];
        if !hex_part.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(anyhow!(
                "user_address contains invalid hex characters: {}",
                self.user_address
            ));
        }

        Ok(())
    }
}
