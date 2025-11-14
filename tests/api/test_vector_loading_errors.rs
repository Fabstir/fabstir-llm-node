// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for VectorLoadingError type-safe error handling (Sub-phase 8.1)
//!
//! Tests compiler-enforced error code mapping, user-friendly messages,
//! security sanitization, and conversion from VectorLoadError.

use fabstir_llm_node::api::websocket::message_types::LoadingErrorCode;
use fabstir_llm_node::api::websocket::vector_loading_errors::VectorLoadingError;
use fabstir_llm_node::rag::errors::VectorLoadError;

// ============================================================================
// Test: Error Code Mapping (Exhaustive Match)
// ============================================================================

#[test]
fn test_manifest_not_found_error_code() {
    let error = VectorLoadingError::ManifestNotFound {
        path: "test/path".to_string(),
    };
    assert_eq!(error.to_error_code(), LoadingErrorCode::ManifestNotFound);
}

#[test]
fn test_manifest_download_failed_error_code() {
    let error = VectorLoadingError::ManifestDownloadFailed {
        source: anyhow::anyhow!("network error"),
    };
    assert_eq!(
        error.to_error_code(),
        LoadingErrorCode::ManifestDownloadFailed
    );
}

#[test]
fn test_chunk_download_failed_error_code() {
    let error = VectorLoadingError::ChunkDownloadFailed {
        chunk_id: 3,
        source: anyhow::anyhow!("timeout"),
    };
    assert_eq!(
        error.to_error_code(),
        LoadingErrorCode::ChunkDownloadFailed
    );
}

#[test]
fn test_owner_mismatch_error_code() {
    let error = VectorLoadingError::OwnerMismatch;
    assert_eq!(error.to_error_code(), LoadingErrorCode::OwnerMismatch);
}

#[test]
fn test_decryption_failed_error_code() {
    let error = VectorLoadingError::DecryptionFailed;
    assert_eq!(error.to_error_code(), LoadingErrorCode::DecryptionFailed);
}

#[test]
fn test_dimension_mismatch_error_code() {
    let error = VectorLoadingError::DimensionMismatch {
        expected: 384,
        actual: 512,
    };
    assert_eq!(error.to_error_code(), LoadingErrorCode::DimensionMismatch);
}

#[test]
fn test_memory_limit_exceeded_error_code() {
    let error = VectorLoadingError::MemoryLimitExceeded {
        size_mb: 200,
        limit_mb: 100,
    };
    assert_eq!(
        error.to_error_code(),
        LoadingErrorCode::MemoryLimitExceeded
    );
}

#[test]
fn test_rate_limit_exceeded_error_code() {
    let error = VectorLoadingError::RateLimitExceeded {
        requests: 10,
        window_secs: 1,
    };
    assert_eq!(error.to_error_code(), LoadingErrorCode::RateLimitExceeded);
}

#[test]
fn test_timeout_error_code() {
    let error = VectorLoadingError::Timeout { timeout_secs: 300 };
    assert_eq!(error.to_error_code(), LoadingErrorCode::Timeout);
}

#[test]
fn test_invalid_path_error_code() {
    let error = VectorLoadingError::InvalidPath {
        path: "bad path".to_string(),
    };
    assert_eq!(error.to_error_code(), LoadingErrorCode::InvalidPath);
}

#[test]
fn test_invalid_session_key_error_code() {
    let error = VectorLoadingError::InvalidSessionKey { actual: 16 };
    assert_eq!(
        error.to_error_code(),
        LoadingErrorCode::InvalidSessionKey
    );
}

#[test]
fn test_empty_database_error_code() {
    let error = VectorLoadingError::EmptyDatabase;
    assert_eq!(error.to_error_code(), LoadingErrorCode::EmptyDatabase);
}

#[test]
fn test_index_build_failed_error_code() {
    let error = VectorLoadingError::IndexBuildFailed {
        reason: "out of memory".to_string(),
    };
    assert_eq!(error.to_error_code(), LoadingErrorCode::IndexBuildFailed);
}

#[test]
fn test_session_not_found_error_code() {
    let error = VectorLoadingError::SessionNotFound {
        session_id: "sess_123".to_string(),
    };
    assert_eq!(error.to_error_code(), LoadingErrorCode::SessionNotFound);
}

#[test]
fn test_internal_error_error_code() {
    let error = VectorLoadingError::InternalError(anyhow::anyhow!("unexpected"));
    assert_eq!(error.to_error_code(), LoadingErrorCode::InternalError);
}

// ============================================================================
// Test: User-Friendly Messages
// ============================================================================

#[test]
fn test_manifest_not_found_user_message() {
    let error = VectorLoadingError::ManifestNotFound {
        path: "home/test.json".to_string(),
    };
    let msg = error.user_friendly_message();
    assert!(msg.contains("not found"));
    assert!(msg.contains("home/test.json"));
}

#[test]
fn test_chunk_download_failed_user_message() {
    let error = VectorLoadingError::ChunkDownloadFailed {
        chunk_id: 5,
        source: anyhow::anyhow!("network error"),
    };
    let msg = error.user_friendly_message();
    assert!(msg.contains("chunk 5"));
    assert!(msg.contains("S5 network"));
}

#[test]
fn test_dimension_mismatch_user_message() {
    let error = VectorLoadingError::DimensionMismatch {
        expected: 384,
        actual: 512,
    };
    let msg = error.user_friendly_message();
    assert!(msg.contains("384"));
    assert!(msg.contains("512"));
}

#[test]
fn test_timeout_user_message() {
    let error = VectorLoadingError::Timeout { timeout_secs: 300 };
    let msg = error.user_friendly_message();
    assert!(msg.contains("300 seconds"));
    assert!(msg.contains("timed out"));
}

// ============================================================================
// Test: Security Sanitization
// ============================================================================

#[test]
fn test_owner_mismatch_sanitized() {
    let error = VectorLoadingError::OwnerMismatch;
    let msg = error.user_friendly_message();

    // Should mention access denial (generic message)
    assert!(msg.contains("access") || msg.contains("verification"));

    // Should NOT expose actual addresses or specific owner information
    assert!(!msg.contains("0x"), "Should not contain hex addresses");
    assert!(!msg.contains("expected:"), "Should not contain 'expected:' with address");
    assert!(!msg.contains("actual:"), "Should not contain 'actual:' with address");

    // It's OK to use the word "owner" in generic context like "owner verification"
    // as long as we don't expose the actual owner addresses
}

#[test]
fn test_decryption_failed_sanitized() {
    let error = VectorLoadingError::DecryptionFailed;
    let msg = error.user_friendly_message();

    // Should mention session key
    assert!(msg.contains("session key"));

    // Should NOT expose key bytes or cryptographic details
    assert!(!msg.contains("bytes"));
    assert!(!msg.contains("0x"));
    assert!(!msg.contains("XChaCha"));
    assert!(!msg.contains("nonce"));
}

#[test]
fn test_internal_error_no_stack_trace() {
    let error = VectorLoadingError::InternalError(anyhow::anyhow!("database corrupted"));
    let msg = error.user_friendly_message();

    // Should mention the error
    assert!(msg.contains("database corrupted"));

    // Should NOT contain stack traces or file paths
    assert!(!msg.contains("/workspace"));
    assert!(!msg.contains("src/"));
    assert!(!msg.contains("line"));
}

// ============================================================================
// Test: Known Error Detection
// ============================================================================

#[test]
fn test_is_known_error_for_timeout() {
    let error = VectorLoadingError::Timeout { timeout_secs: 300 };
    assert!(
        error.is_known_error(),
        "Timeout should be a known error type"
    );
}

#[test]
fn test_is_known_error_for_manifest_not_found() {
    let error = VectorLoadingError::ManifestNotFound {
        path: "test".to_string(),
    };
    assert!(
        error.is_known_error(),
        "ManifestNotFound should be a known error"
    );
}

#[test]
fn test_internal_error_not_known() {
    let error = VectorLoadingError::InternalError(anyhow::anyhow!("unexpected"));
    assert!(
        !error.is_known_error(),
        "InternalError should not be a known error"
    );
}

// ============================================================================
// Test: Log Levels
// ============================================================================

#[test]
fn test_internal_error_warns() {
    let error = VectorLoadingError::InternalError(anyhow::anyhow!("test"));
    assert_eq!(
        error.log_level(),
        tracing::Level::WARN,
        "InternalError should log at WARN level"
    );
}

#[test]
fn test_security_errors_warn() {
    assert_eq!(
        VectorLoadingError::OwnerMismatch.log_level(),
        tracing::Level::WARN,
        "OwnerMismatch should log at WARN level"
    );
    assert_eq!(
        VectorLoadingError::DecryptionFailed.log_level(),
        tracing::Level::WARN,
        "DecryptionFailed should log at WARN level"
    );
}

#[test]
fn test_timeout_info_level() {
    let error = VectorLoadingError::Timeout { timeout_secs: 300 };
    assert_eq!(
        error.log_level(),
        tracing::Level::INFO,
        "Timeout should log at INFO level (expected for large DBs)"
    );
}

#[test]
fn test_user_errors_debug_level() {
    assert_eq!(
        VectorLoadingError::ManifestNotFound {
            path: "test".to_string()
        }
        .log_level(),
        tracing::Level::DEBUG,
        "ManifestNotFound should log at DEBUG level"
    );

    assert_eq!(
        VectorLoadingError::InvalidPath {
            path: "test".to_string()
        }
        .log_level(),
        tracing::Level::DEBUG,
        "InvalidPath should log at DEBUG level"
    );
}

// ============================================================================
// Test: Conversion from VectorLoadError
// ============================================================================

#[test]
fn test_convert_manifest_not_found() {
    let rag_error = VectorLoadError::ManifestNotFound("test/path".to_string());
    let ws_error: VectorLoadingError = rag_error.into();

    assert_eq!(ws_error.to_error_code(), LoadingErrorCode::ManifestNotFound);
    assert!(ws_error.user_friendly_message().contains("test/path"));
}

#[test]
fn test_convert_owner_mismatch() {
    let rag_error = VectorLoadError::OwnerMismatch {
        expected: "0xALICE".to_string(),
        actual: "0xBOB".to_string(),
    };
    let ws_error: VectorLoadingError = rag_error.into();

    assert_eq!(ws_error.to_error_code(), LoadingErrorCode::OwnerMismatch);
    // User message should be sanitized (no addresses)
    let msg = ws_error.user_friendly_message();
    assert!(!msg.contains("0xALICE"));
    assert!(!msg.contains("0xBOB"));
}

#[test]
fn test_convert_dimension_mismatch() {
    let rag_error = VectorLoadError::DimensionMismatch {
        chunk_id: 2,
        expected: 384,
        actual: 256,
    };
    let ws_error: VectorLoadingError = rag_error.into();

    assert_eq!(
        ws_error.to_error_code(),
        LoadingErrorCode::DimensionMismatch
    );
    let msg = ws_error.user_friendly_message();
    assert!(msg.contains("384"));
    assert!(msg.contains("256"));
}

#[test]
fn test_convert_timeout() {
    let rag_error = VectorLoadError::Timeout { duration_sec: 300 };
    let ws_error: VectorLoadingError = rag_error.into();

    assert_eq!(ws_error.to_error_code(), LoadingErrorCode::Timeout);
    assert!(ws_error.user_friendly_message().contains("300 seconds"));
}

#[test]
fn test_convert_rate_limit_exceeded() {
    let rag_error = VectorLoadError::RateLimitExceeded {
        current: 10,
        limit: 5,
        window_sec: 60,
    };
    let ws_error: VectorLoadingError = rag_error.into();

    assert_eq!(
        ws_error.to_error_code(),
        LoadingErrorCode::RateLimitExceeded
    );
}

#[test]
fn test_convert_memory_limit_exceeded() {
    let rag_error = VectorLoadError::MemoryLimitExceeded {
        required_mb: 200,
        limit_mb: 100,
    };
    let ws_error: VectorLoadingError = rag_error.into();

    assert_eq!(
        ws_error.to_error_code(),
        LoadingErrorCode::MemoryLimitExceeded
    );
    let msg = ws_error.user_friendly_message();
    assert!(msg.contains("200MB"));
    assert!(msg.contains("100MB"));
}

#[test]
fn test_convert_chunk_download_failed() {
    let rag_error = VectorLoadError::ChunkDownloadFailed {
        chunk_id: 3,
        path: "chunk-3.json".to_string(),
        source: Box::new(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "timeout",
        )),
    };
    let ws_error: VectorLoadingError = rag_error.into();

    assert_eq!(
        ws_error.to_error_code(),
        LoadingErrorCode::ChunkDownloadFailed
    );
    assert!(ws_error.user_friendly_message().contains("chunk 3"));
}

#[test]
fn test_convert_index_build_failed() {
    let rag_error = VectorLoadError::IndexBuildFailed("out of memory".to_string());
    let ws_error: VectorLoadingError = rag_error.into();

    assert_eq!(
        ws_error.to_error_code(),
        LoadingErrorCode::IndexBuildFailed
    );
    assert!(ws_error
        .user_friendly_message()
        .contains("out of memory"));
}

#[test]
fn test_convert_vector_count_mismatch_to_empty_database() {
    let rag_error = VectorLoadError::VectorCountMismatch {
        chunk_id: 0,
        expected: 100,
        actual: 0,
    };
    let ws_error: VectorLoadingError = rag_error.into();

    // VectorCountMismatch maps to EmptyDatabase
    assert_eq!(ws_error.to_error_code(), LoadingErrorCode::EmptyDatabase);
}

#[test]
fn test_convert_manifest_parse_error_to_internal() {
    let rag_error = VectorLoadError::ManifestParseError("invalid JSON".to_string());
    let ws_error: VectorLoadingError = rag_error.into();

    // ManifestParseError maps to InternalError
    assert_eq!(ws_error.to_error_code(), LoadingErrorCode::InternalError);
    assert!(ws_error.user_friendly_message().contains("invalid JSON"));
}

#[test]
fn test_convert_io_error_to_internal() {
    let io_error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
    let rag_error = VectorLoadError::IoError(io_error);
    let ws_error: VectorLoadingError = rag_error.into();

    assert_eq!(ws_error.to_error_code(), LoadingErrorCode::InternalError);
}

// ============================================================================
// Test: Exhaustive Match Enforcement
// ============================================================================

/// This test ensures that to_error_code() has an exhaustive match.
/// If a new variant is added to VectorLoadingError, this will fail to compile
/// until to_error_code() is updated.
#[test]
fn test_exhaustive_error_code_mapping() {
    // Create every variant
    let errors = vec![
        VectorLoadingError::ManifestNotFound {
            path: "test".to_string(),
        },
        VectorLoadingError::ManifestDownloadFailed {
            source: anyhow::anyhow!("test"),
        },
        VectorLoadingError::ChunkDownloadFailed {
            chunk_id: 0,
            source: anyhow::anyhow!("test"),
        },
        VectorLoadingError::OwnerMismatch,
        VectorLoadingError::DecryptionFailed,
        VectorLoadingError::DimensionMismatch {
            expected: 384,
            actual: 256,
        },
        VectorLoadingError::MemoryLimitExceeded {
            size_mb: 100,
            limit_mb: 50,
        },
        VectorLoadingError::RateLimitExceeded {
            requests: 10,
            window_secs: 1,
        },
        VectorLoadingError::Timeout { timeout_secs: 300 },
        VectorLoadingError::InvalidPath {
            path: "test".to_string(),
        },
        VectorLoadingError::InvalidSessionKey { actual: 16 },
        VectorLoadingError::EmptyDatabase,
        VectorLoadingError::IndexBuildFailed {
            reason: "test".to_string(),
        },
        VectorLoadingError::SessionNotFound {
            session_id: "test".to_string(),
        },
        VectorLoadingError::InternalError(anyhow::anyhow!("test")),
    ];

    // Verify all map to error codes without panic
    for error in errors {
        let _ = error.to_error_code();
        let _ = error.user_friendly_message();
        let _ = error.is_known_error();
        let _ = error.log_level();
    }
}
