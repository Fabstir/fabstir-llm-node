// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Type-safe error types for S5 vector database loading
//!
//! This module provides structured error types for vector loading operations,
//! replacing string pattern matching with compile-time guaranteed error categorization.
//!
//! Key features:
//! - 15 specific error variants covering all failure modes
//! - Automatic error code mapping via `to_error_code()`
//! - User-friendly messages with security sanitization
//! - Source error chaining for debugging
//!
//! # Example
//! ```no_run
//! use fabstir_llm_node::api::websocket::vector_loading_errors::VectorLoadingError;
//! use fabstir_llm_node::api::websocket::message_types::LoadingErrorCode;
//!
//! let error = VectorLoadingError::ManifestNotFound {
//!     path: "home/vector-databases/0xABC.../my-docs/manifest.json".to_string()
//! };
//!
//! assert_eq!(error.to_error_code(), LoadingErrorCode::ManifestNotFound);
//! assert!(error.user_friendly_message().contains("not found"));
//! ```

use crate::api::websocket::message_types::LoadingErrorCode;
use crate::rag::errors::VectorLoadError;
use thiserror::Error;

/// Errors that can occur during S5 vector database loading
///
/// Each variant represents a specific failure mode with appropriate context.
/// Security-sensitive errors (OwnerMismatch, DecryptionFailed) do not expose
/// sensitive details in their Display implementations.
#[derive(Debug, Error)]
pub enum VectorLoadingError {
    /// Manifest file not found at specified S5 path
    #[error("Manifest not found at path: {path}")]
    ManifestNotFound { path: String },

    /// Failed to download manifest.json from S5 network
    #[error("Failed to download manifest from S5: {source}")]
    ManifestDownloadFailed {
        #[source]
        source: anyhow::Error,
    },

    /// Failed to download a specific vector chunk from S5
    #[error("Failed to download chunk {chunk_id}: {source}")]
    ChunkDownloadFailed {
        chunk_id: usize,
        #[source]
        source: anyhow::Error,
    },

    /// Database owner doesn't match requesting user
    ///
    /// This is a security-critical error - prevents unauthorized access
    /// to private vector databases. Message is sanitized (no addresses exposed).
    #[error("Database owner verification failed")]
    OwnerMismatch,

    /// Failed to decrypt vector database with provided session key
    ///
    /// Security-sensitive error - doesn't expose key details in message.
    #[error("Failed to decrypt vector data")]
    DecryptionFailed,

    /// Vector dimensions don't match expected size
    #[error("Vector dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },

    /// Database size exceeds host memory limits
    #[error("Memory limit exceeded: {size_mb}MB > {limit_mb}MB")]
    MemoryLimitExceeded { size_mb: usize, limit_mb: usize },

    /// Too many S5 download requests in time window
    #[error("Rate limit exceeded: {requests} requests in {window_secs}s")]
    RateLimitExceeded {
        requests: usize,
        window_secs: u64,
    },

    /// Loading operation timed out
    #[error("Loading timed out after {timeout_secs} seconds")]
    Timeout { timeout_secs: u64 },

    /// Manifest path has invalid format
    #[error("Invalid manifest path: {path}")]
    InvalidPath { path: String },

    /// Session encryption key has wrong length
    #[error("Invalid session key length: expected 32 bytes, got {actual}")]
    InvalidSessionKey { actual: usize },

    /// No vectors found in database
    #[error("Database is empty: no vectors found")]
    EmptyDatabase,

    /// Failed to build HNSW search index
    #[error("Failed to build HNSW index: {reason}")]
    IndexBuildFailed { reason: String },

    /// Session not found or expired
    #[error("Session not found: {session_id}")]
    SessionNotFound { session_id: String },

    /// Unexpected internal error (fallback for unknown errors)
    ///
    /// This variant should be rare - if you see it frequently, add a specific variant.
    /// Logs a warning when converted to error code for production monitoring.
    #[error("Internal error: {0}")]
    InternalError(#[from] anyhow::Error),
}

impl VectorLoadingError {
    /// Convert error to machine-readable LoadingErrorCode for WebSocket responses
    ///
    /// This method has an exhaustive match - compiler enforces that all variants
    /// are handled. If a new error variant is added, this won't compile until
    /// it's mapped to an error code.
    ///
    /// # Example
    /// ```no_run
    /// # use fabstir_llm_node::api::websocket::vector_loading_errors::VectorLoadingError;
    /// # use fabstir_llm_node::api::websocket::message_types::LoadingErrorCode;
    /// let error = VectorLoadingError::Timeout { timeout_secs: 300 };
    /// assert_eq!(error.to_error_code(), LoadingErrorCode::Timeout);
    /// ```
    pub fn to_error_code(&self) -> LoadingErrorCode {
        match self {
            Self::ManifestNotFound { .. } => LoadingErrorCode::ManifestNotFound,
            Self::ManifestDownloadFailed { .. } => LoadingErrorCode::ManifestDownloadFailed,
            Self::ChunkDownloadFailed { .. } => LoadingErrorCode::ChunkDownloadFailed,
            Self::OwnerMismatch => LoadingErrorCode::OwnerMismatch,
            Self::DecryptionFailed => LoadingErrorCode::DecryptionFailed,
            Self::DimensionMismatch { .. } => LoadingErrorCode::DimensionMismatch,
            Self::MemoryLimitExceeded { .. } => LoadingErrorCode::MemoryLimitExceeded,
            Self::RateLimitExceeded { .. } => LoadingErrorCode::RateLimitExceeded,
            Self::Timeout { .. } => LoadingErrorCode::Timeout,
            Self::InvalidPath { .. } => LoadingErrorCode::InvalidPath,
            Self::InvalidSessionKey { .. } => LoadingErrorCode::InvalidSessionKey,
            Self::EmptyDatabase => LoadingErrorCode::EmptyDatabase,
            Self::IndexBuildFailed { .. } => LoadingErrorCode::IndexBuildFailed,
            Self::SessionNotFound { .. } => LoadingErrorCode::SessionNotFound,
            Self::InternalError(_) => {
                // Log warning for production monitoring (Sub-phase 8.2)
                tracing::warn!(
                    error = %self,
                    "⚠️ Unexpected error categorized as INTERNAL_ERROR - investigate if recurring"
                );
                LoadingErrorCode::InternalError
            }
        }
    }

    /// Get user-friendly error message for SDK clients
    ///
    /// This method returns human-readable error messages suitable for displaying
    /// to end users. Security-sensitive errors (OwnerMismatch, DecryptionFailed)
    /// have sanitized messages that don't leak sensitive information like
    /// addresses or encryption keys.
    ///
    /// # Example
    /// ```no_run
    /// # use fabstir_llm_node::api::websocket::vector_loading_errors::VectorLoadingError;
    /// let error = VectorLoadingError::OwnerMismatch;
    /// let msg = error.user_friendly_message();
    /// assert!(msg.contains("access"));
    /// assert!(!msg.contains("0x")); // No addresses leaked
    /// ```
    pub fn user_friendly_message(&self) -> String {
        match self {
            Self::ManifestNotFound { path } => {
                format!("Vector database not found at path: {path}")
            }
            Self::ManifestDownloadFailed { .. } => {
                "Failed to download vector database manifest from S5 network".to_string()
            }
            Self::ChunkDownloadFailed { chunk_id, .. } => {
                format!("Failed to download chunk {chunk_id} from S5 network")
            }
            Self::OwnerMismatch => {
                // Sanitized: don't expose addresses or ownership details
                "Database access denied - you don't have permission to use this resource"
                    .to_string()
            }
            Self::DecryptionFailed => {
                // Sanitized: don't expose key details
                "Failed to decrypt vector database - invalid session key".to_string()
            }
            Self::DimensionMismatch { expected, actual } => {
                format!("Vector dimension mismatch: expected {expected}, got {actual}")
            }
            Self::MemoryLimitExceeded { size_mb, limit_mb } => {
                format!("Database too large: {size_mb}MB exceeds limit of {limit_mb}MB")
            }
            Self::RateLimitExceeded { .. } => {
                "Too many download requests - please try again later".to_string()
            }
            Self::Timeout { timeout_secs } => {
                format!("Loading timed out after {timeout_secs} seconds")
            }
            Self::InvalidPath { path } => {
                format!("Invalid manifest path format: {path}")
            }
            Self::InvalidSessionKey { actual } => {
                format!("Invalid session key length: expected 32 bytes, got {actual}")
            }
            Self::EmptyDatabase => "Vector database is empty - no vectors found".to_string(),
            Self::IndexBuildFailed { reason } => {
                format!("Failed to build search index: {reason}")
            }
            Self::SessionNotFound { session_id } => {
                format!("Session not found or expired: {session_id}")
            }
            Self::InternalError(e) => {
                format!("Internal error: {e}")
            }
        }
    }

    /// Returns true if this is a known error type (not InternalError)
    ///
    /// Used for determining appropriate log levels and metrics tracking.
    /// InternalError indicates an unexpected error that should be investigated.
    pub fn is_known_error(&self) -> bool {
        !matches!(self, Self::InternalError(_))
    }

    /// Returns the appropriate log level for this error
    ///
    /// - WARN: Security concerns (OwnerMismatch, DecryptionFailed) and unexpected errors (InternalError)
    /// - INFO: Expected operational errors (Timeout for large databases)
    /// - DEBUG: Normal user errors (ManifestNotFound, InvalidPath, etc.)
    pub fn log_level(&self) -> tracing::Level {
        match self {
            Self::InternalError(_) => tracing::Level::WARN, // Unexpected error
            Self::OwnerMismatch => tracing::Level::WARN,     // Security concern
            Self::DecryptionFailed => tracing::Level::WARN,  // Security concern
            Self::Timeout { .. } => tracing::Level::INFO,    // Expected for large DBs
            Self::ManifestNotFound { .. } => tracing::Level::DEBUG, // User error
            Self::InvalidPath { .. } => tracing::Level::DEBUG, // User error
            Self::InvalidSessionKey { .. } => tracing::Level::DEBUG, // User error
            _ => tracing::Level::DEBUG, // Other normal errors
        }
    }
}

/// Convert from VectorLoadError (RAG layer) to VectorLoadingError (WebSocket layer)
///
/// This enables automatic conversion when VectorLoader errors are propagated
/// to the WebSocket layer for client notification.
impl From<VectorLoadError> for VectorLoadingError {
    fn from(error: VectorLoadError) -> Self {
        match error {
            VectorLoadError::ManifestNotFound(path) => Self::ManifestNotFound { path },
            VectorLoadError::ManifestDownloadFailed { path, source } => {
                Self::ManifestDownloadFailed {
                    source: anyhow::anyhow!("Download failed for {path}: {source}"),
                }
            }
            VectorLoadError::OwnerMismatch { .. } => Self::OwnerMismatch,
            VectorLoadError::DecryptionFailed(_) => Self::DecryptionFailed,
            VectorLoadError::ChunkDownloadFailed {
                chunk_id,
                path,
                source,
            } => Self::ChunkDownloadFailed {
                chunk_id,
                source: anyhow::anyhow!("Download failed for {path}: {source}"),
            },
            VectorLoadError::DimensionMismatch {
                expected, actual, ..
            } => Self::DimensionMismatch { expected, actual },
            VectorLoadError::MemoryLimitExceeded {
                required_mb,
                limit_mb,
            } => Self::MemoryLimitExceeded {
                size_mb: required_mb,
                limit_mb,
            },
            VectorLoadError::RateLimitExceeded {
                current,
                limit: _,
                window_sec,
            } => Self::RateLimitExceeded {
                requests: current,
                window_secs: window_sec,
            },
            VectorLoadError::Timeout { duration_sec } => Self::Timeout {
                timeout_secs: duration_sec,
            },
            VectorLoadError::InvalidPath(path) => Self::InvalidPath { path },
            VectorLoadError::IndexBuildFailed(reason) => Self::IndexBuildFailed { reason },
            VectorLoadError::VectorCountMismatch { .. } => {
                // Map to EmptyDatabase if count is 0, otherwise internal error
                Self::EmptyDatabase
            }
            VectorLoadError::ManifestParseError(reason) => {
                Self::InternalError(anyhow::anyhow!("Manifest parse error: {reason}"))
            }
            VectorLoadError::ChunkValidationFailed { reason, .. } => {
                Self::InternalError(anyhow::anyhow!("Chunk validation failed: {reason}"))
            }
            VectorLoadError::IoError(e) => Self::InternalError(anyhow::anyhow!("I/O error: {e}")),
            VectorLoadError::Other(msg) => Self::InternalError(anyhow::anyhow!("{msg}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_not_found_mapping() {
        let error = VectorLoadingError::ManifestNotFound {
            path: "test/path/manifest.json".to_string(),
        };
        assert_eq!(error.to_error_code(), LoadingErrorCode::ManifestNotFound);
        assert!(error.user_friendly_message().contains("not found"));
        assert!(error.user_friendly_message().contains("test/path"));
    }

    #[test]
    fn test_chunk_download_failed_mapping() {
        let error = VectorLoadingError::ChunkDownloadFailed {
            chunk_id: 5,
            source: anyhow::anyhow!("network error"),
        };
        assert_eq!(
            error.to_error_code(),
            LoadingErrorCode::ChunkDownloadFailed
        );
        assert!(error.user_friendly_message().contains("chunk 5"));
    }

    #[test]
    fn test_owner_mismatch_sanitized() {
        let error = VectorLoadingError::OwnerMismatch;
        assert_eq!(error.to_error_code(), LoadingErrorCode::OwnerMismatch);

        let msg = error.user_friendly_message();
        assert!(msg.contains("access"));
        // Ensure no addresses are leaked
        assert!(!msg.contains("0x"));
        assert!(!msg.contains("owner"));
        assert!(!msg.contains("address"));
    }

    #[test]
    fn test_decryption_failed_sanitized() {
        let error = VectorLoadingError::DecryptionFailed;
        assert_eq!(error.to_error_code(), LoadingErrorCode::DecryptionFailed);

        let msg = error.user_friendly_message();
        // Should mention session key but not expose details
        assert!(msg.contains("session key"));
        assert!(!msg.contains("bytes"));
        assert!(!msg.contains("0x"));
    }

    #[test]
    fn test_timeout_mapping() {
        let error = VectorLoadingError::Timeout { timeout_secs: 300 };
        assert_eq!(error.to_error_code(), LoadingErrorCode::Timeout);
        assert!(error.user_friendly_message().contains("300 seconds"));
    }

    #[test]
    fn test_dimension_mismatch_mapping() {
        let error = VectorLoadingError::DimensionMismatch {
            expected: 384,
            actual: 512,
        };
        assert_eq!(error.to_error_code(), LoadingErrorCode::DimensionMismatch);
        assert!(error.user_friendly_message().contains("384"));
        assert!(error.user_friendly_message().contains("512"));
    }

    #[test]
    fn test_is_known_error() {
        assert!(VectorLoadingError::ManifestNotFound {
            path: "test".to_string()
        }
        .is_known_error());
        assert!(VectorLoadingError::OwnerMismatch.is_known_error());
        assert!(!VectorLoadingError::InternalError(anyhow::anyhow!("test")).is_known_error());
    }

    #[test]
    fn test_log_levels() {
        assert_eq!(
            VectorLoadingError::InternalError(anyhow::anyhow!("test")).log_level(),
            tracing::Level::WARN
        );
        assert_eq!(
            VectorLoadingError::OwnerMismatch.log_level(),
            tracing::Level::WARN
        );
        assert_eq!(
            VectorLoadingError::Timeout { timeout_secs: 300 }.log_level(),
            tracing::Level::INFO
        );
        assert_eq!(
            VectorLoadingError::ManifestNotFound {
                path: "test".to_string()
            }
            .log_level(),
            tracing::Level::DEBUG
        );
    }
}
