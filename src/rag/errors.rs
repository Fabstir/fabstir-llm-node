// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Error types for S5 vector database loading
//!
//! Comprehensive error handling for vector loading operations including:
//! - S5 storage errors (download failures, not found)
//! - Decryption errors (invalid key, corrupt data)
//! - Validation errors (owner mismatch, dimension mismatch)
//! - Security errors (rate limiting, memory limits, timeouts)

use thiserror::Error;

/// Errors that can occur during vector database loading from S5
#[derive(Error, Debug)]
pub enum VectorLoadError {
    /// Manifest file not found at specified path
    #[error("Manifest not found at path: {0}")]
    ManifestNotFound(String),

    /// Failed to download manifest from S5
    #[error("Failed to download manifest from {path}: {source}")]
    ManifestDownloadFailed {
        path: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Failed to parse manifest JSON
    #[error("Failed to parse manifest: {0}")]
    ManifestParseError(String),

    /// Owner verification failed - user does not own this database
    #[error("Owner mismatch: expected {expected}, but manifest owner is {actual}")]
    OwnerMismatch { expected: String, actual: String },

    /// Decryption failed with provided session key
    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),

    /// Failed to download chunk from S5
    #[error("Failed to download chunk {chunk_id} from {path}: {source}")]
    ChunkDownloadFailed {
        chunk_id: usize,
        path: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Chunk validation failed
    #[error("Chunk {chunk_id} validation failed: {reason}")]
    ChunkValidationFailed { chunk_id: usize, reason: String },

    /// Vector dimensions don't match manifest specification
    #[error("Dimension mismatch in chunk {chunk_id}: expected {expected}D, got {actual}D vectors")]
    DimensionMismatch {
        chunk_id: usize,
        expected: usize,
        actual: usize,
    },

    /// Vector count in chunk doesn't match manifest
    #[error(
        "Vector count mismatch in chunk {chunk_id}: expected {expected}, got {actual} vectors"
    )]
    VectorCountMismatch {
        chunk_id: usize,
        expected: usize,
        actual: usize,
    },

    /// Failed to build HNSW index from loaded vectors
    #[error("Failed to build index: {0}")]
    IndexBuildFailed(String),

    /// Rate limit exceeded for S5 downloads
    #[error("Rate limit exceeded: {current} downloads in {window_sec}s (limit: {limit})")]
    RateLimitExceeded {
        current: usize,
        limit: usize,
        window_sec: u64,
    },

    /// Memory limit exceeded for loaded vectors
    #[error("Memory limit exceeded: dataset requires {required_mb}MB, limit is {limit_mb}MB")]
    MemoryLimitExceeded { required_mb: usize, limit_mb: usize },

    /// Loading operation timed out
    #[error("Loading timeout: operation exceeded {duration_sec}s limit")]
    Timeout { duration_sec: u64 },

    /// Invalid manifest path format
    #[error("Invalid manifest path: {0}")]
    InvalidPath(String),

    /// Generic I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Generic error for unexpected failures
    #[error("Unexpected error: {0}")]
    Other(String),
}

// Implement conversion from anyhow::Error for backward compatibility
impl From<anyhow::Error> for VectorLoadError {
    fn from(err: anyhow::Error) -> Self {
        VectorLoadError::Other(err.to_string())
    }
}

// Implement Display for user-friendly error messages
impl VectorLoadError {
    /// Get user-friendly error message for API responses
    pub fn user_message(&self) -> String {
        match self {
            VectorLoadError::ManifestNotFound(path) => {
                format!("Vector database not found at: {}", path)
            }
            VectorLoadError::OwnerMismatch { expected, .. } => {
                format!("Access denied: you ({}) do not own this database", expected)
            }
            VectorLoadError::DecryptionFailed(_) => {
                "Failed to decrypt database - invalid session key".to_string()
            }
            VectorLoadError::DimensionMismatch {
                expected, actual, ..
            } => {
                format!(
                    "Database integrity error: expected {}D vectors, found {}D",
                    expected, actual
                )
            }
            VectorLoadError::MemoryLimitExceeded {
                required_mb,
                limit_mb,
            } => {
                format!(
                    "Database too large: requires {}MB, limit is {}MB",
                    required_mb, limit_mb
                )
            }
            VectorLoadError::Timeout { duration_sec } => {
                format!("Loading timed out after {}s", duration_sec)
            }
            VectorLoadError::RateLimitExceeded {
                limit, window_sec, ..
            } => {
                format!("Too many requests: limit is {} per {}s", limit, window_sec)
            }
            _ => self.to_string(),
        }
    }

    /// Get error code for logging and metrics
    pub fn error_code(&self) -> &'static str {
        match self {
            VectorLoadError::ManifestNotFound(_) => "MANIFEST_NOT_FOUND",
            VectorLoadError::ManifestDownloadFailed { .. } => "MANIFEST_DOWNLOAD_FAILED",
            VectorLoadError::ManifestParseError(_) => "MANIFEST_PARSE_ERROR",
            VectorLoadError::OwnerMismatch { .. } => "OWNER_MISMATCH",
            VectorLoadError::DecryptionFailed(_) => "DECRYPTION_FAILED",
            VectorLoadError::ChunkDownloadFailed { .. } => "CHUNK_DOWNLOAD_FAILED",
            VectorLoadError::ChunkValidationFailed { .. } => "CHUNK_VALIDATION_FAILED",
            VectorLoadError::DimensionMismatch { .. } => "DIMENSION_MISMATCH",
            VectorLoadError::VectorCountMismatch { .. } => "VECTOR_COUNT_MISMATCH",
            VectorLoadError::IndexBuildFailed(_) => "INDEX_BUILD_FAILED",
            VectorLoadError::RateLimitExceeded { .. } => "RATE_LIMIT_EXCEEDED",
            VectorLoadError::MemoryLimitExceeded { .. } => "MEMORY_LIMIT_EXCEEDED",
            VectorLoadError::Timeout { .. } => "TIMEOUT",
            VectorLoadError::InvalidPath(_) => "INVALID_PATH",
            VectorLoadError::IoError(_) => "IO_ERROR",
            VectorLoadError::Other(_) => "OTHER",
        }
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            VectorLoadError::ManifestDownloadFailed { .. }
                | VectorLoadError::ChunkDownloadFailed { .. }
                | VectorLoadError::Timeout { .. }
        )
    }

    /// Check if this error is a security violation
    pub fn is_security_error(&self) -> bool {
        matches!(
            self,
            VectorLoadError::OwnerMismatch { .. }
                | VectorLoadError::DecryptionFailed(_)
                | VectorLoadError::RateLimitExceeded { .. }
                | VectorLoadError::MemoryLimitExceeded { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes_unique() {
        // Ensure all error codes are unique
        let codes = vec![
            VectorLoadError::ManifestNotFound("test".to_string()).error_code(),
            VectorLoadError::OwnerMismatch {
                expected: "a".to_string(),
                actual: "b".to_string(),
            }
            .error_code(),
            VectorLoadError::DecryptionFailed("test".to_string()).error_code(),
            VectorLoadError::DimensionMismatch {
                chunk_id: 0,
                expected: 384,
                actual: 256,
            }
            .error_code(),
            VectorLoadError::VectorCountMismatch {
                chunk_id: 0,
                expected: 100,
                actual: 50,
            }
            .error_code(),
            VectorLoadError::RateLimitExceeded {
                current: 10,
                limit: 5,
                window_sec: 1,
            }
            .error_code(),
            VectorLoadError::MemoryLimitExceeded {
                required_mb: 200,
                limit_mb: 100,
            }
            .error_code(),
            VectorLoadError::Timeout { duration_sec: 30 }.error_code(),
        ];

        // Check for duplicates
        for (i, code1) in codes.iter().enumerate() {
            for (j, code2) in codes.iter().enumerate() {
                if i != j {
                    assert_ne!(code1, code2, "Duplicate error codes found: {}", code1);
                }
            }
        }
    }

    #[test]
    fn test_user_messages() {
        let err = VectorLoadError::OwnerMismatch {
            expected: "0xALICE".to_string(),
            actual: "0xBOB".to_string(),
        };
        let msg = err.user_message();
        assert!(
            msg.contains("Access denied"),
            "User message should be friendly"
        );
        assert!(msg.contains("0xALICE"), "Should include user address");
    }

    #[test]
    fn test_retryable_errors() {
        assert!(
            VectorLoadError::Timeout { duration_sec: 30 }.is_retryable(),
            "Timeout should be retryable"
        );
        assert!(
            !VectorLoadError::OwnerMismatch {
                expected: "a".to_string(),
                actual: "b".to_string()
            }
            .is_retryable(),
            "Owner mismatch should not be retryable"
        );
    }

    #[test]
    fn test_security_errors() {
        assert!(
            VectorLoadError::OwnerMismatch {
                expected: "a".to_string(),
                actual: "b".to_string()
            }
            .is_security_error(),
            "Owner mismatch is a security error"
        );
        assert!(
            VectorLoadError::RateLimitExceeded {
                current: 10,
                limit: 5,
                window_sec: 1
            }
            .is_security_error(),
            "Rate limit is a security error"
        );
        assert!(
            !VectorLoadError::ManifestNotFound("test".to_string()).is_security_error(),
            "Not found is not a security error"
        );
    }
}
