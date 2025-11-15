// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Phase 4.5: Production Error Handling Tests (Phase 8 Integration)
// Test LoadingError WebSocket message delivery with all 15 error codes

use fabstir_llm_node::api::websocket::message_types::{LoadingErrorCode, LoadingProgressMessage};
use fabstir_llm_node::api::websocket::vector_loading_errors::VectorLoadingError;

#[tokio::test]
async fn test_all_15_error_codes_mapping() {
    println!("\n🧪 Phase 4.5.2: All 15 LoadingErrorCode Variants");
    println!("=================================================\n");

    // Test that all VectorLoadingError variants map to correct LoadingErrorCode
    let test_cases = vec![
        (
            VectorLoadingError::ManifestNotFound {
                path: "test/path/manifest.json".to_string(),
            },
            LoadingErrorCode::ManifestNotFound,
            "MANIFEST_NOT_FOUND",
        ),
        (
            VectorLoadingError::ManifestDownloadFailed {
                source: anyhow::anyhow!("network timeout"),
            },
            LoadingErrorCode::ManifestDownloadFailed,
            "MANIFEST_DOWNLOAD_FAILED",
        ),
        (
            VectorLoadingError::ChunkDownloadFailed {
                chunk_id: 5,
                source: anyhow::anyhow!("S5 network error"),
            },
            LoadingErrorCode::ChunkDownloadFailed,
            "CHUNK_DOWNLOAD_FAILED",
        ),
        (
            VectorLoadingError::OwnerMismatch,
            LoadingErrorCode::OwnerMismatch,
            "OWNER_MISMATCH",
        ),
        (
            VectorLoadingError::DecryptionFailed,
            LoadingErrorCode::DecryptionFailed,
            "DECRYPTION_FAILED",
        ),
        (
            VectorLoadingError::DimensionMismatch {
                expected: 384,
                actual: 128,
            },
            LoadingErrorCode::DimensionMismatch,
            "DIMENSION_MISMATCH",
        ),
        (
            VectorLoadingError::MemoryLimitExceeded {
                size_mb: 2000,
                limit_mb: 500,
            },
            LoadingErrorCode::MemoryLimitExceeded,
            "MEMORY_LIMIT_EXCEEDED",
        ),
        (
            VectorLoadingError::RateLimitExceeded {
                requests: 100,
                window_secs: 60,
            },
            LoadingErrorCode::RateLimitExceeded,
            "RATE_LIMIT_EXCEEDED",
        ),
        (
            VectorLoadingError::Timeout { timeout_secs: 300 },
            LoadingErrorCode::Timeout,
            "TIMEOUT",
        ),
        (
            VectorLoadingError::InvalidPath {
                path: "../../../etc/passwd".to_string(),
            },
            LoadingErrorCode::InvalidPath,
            "INVALID_PATH",
        ),
        (
            VectorLoadingError::InvalidSessionKey { actual: 16 },
            LoadingErrorCode::InvalidSessionKey,
            "INVALID_SESSION_KEY",
        ),
        (
            VectorLoadingError::EmptyDatabase,
            LoadingErrorCode::EmptyDatabase,
            "EMPTY_DATABASE",
        ),
        (
            VectorLoadingError::IndexBuildFailed {
                reason: "HNSW construction failed".to_string(),
            },
            LoadingErrorCode::IndexBuildFailed,
            "INDEX_BUILD_FAILED",
        ),
        (
            VectorLoadingError::SessionNotFound {
                session_id: "session123".to_string(),
            },
            LoadingErrorCode::SessionNotFound,
            "SESSION_NOT_FOUND",
        ),
        (
            VectorLoadingError::InternalError(anyhow::anyhow!("unexpected error")),
            LoadingErrorCode::InternalError,
            "INTERNAL_ERROR",
        ),
    ];

    println!("Testing {} error code mappings:\n", test_cases.len());

    for (i, (ws_error, expected_code, expected_str)) in test_cases.iter().enumerate() {
        // Get error code
        let error_code = ws_error.to_error_code();

        // Verify mapping
        assert_eq!(
            error_code, *expected_code,
            "Test case {}: {:?} should map to {:?}",
            i + 1,
            ws_error,
            expected_code
        );

        // Verify serialization
        let json = serde_json::to_value(&error_code).unwrap();
        assert_eq!(json, *expected_str);

        // Verify user-friendly message exists
        let message = ws_error.user_friendly_message();
        assert!(!message.is_empty(), "Test case {}: Should have user-friendly message", i + 1);
        assert!(
            !message.contains("Error("),
            "Test case {}: Should not contain debug strings",
            i + 1
        );

        println!("  ✅ {}: {} → {}", i + 1, expected_str, &message[..60.min(message.len())]);
    }

    println!("\n🎉 All 15 error codes mapped correctly!\n");
}

#[tokio::test]
async fn test_security_sanitization() {
    println!("\n🧪 Phase 4.5.3: Security-Sensitive Error Sanitization");
    println!("=====================================================\n");

    // Test OwnerMismatch doesn't leak addresses
    println!("🔐 Test 1: OwnerMismatch sanitization...");
    let owner_error = VectorLoadingError::OwnerMismatch;

    let message = owner_error.user_friendly_message();

    println!("   Message: {}", message);

    // Should NOT contain addresses (sanitized at WebSocket layer)
    assert!(
        !message.contains("0x"),
        "Should not expose any addresses"
    );

    // Should contain generic security message
    assert!(
        message.to_lowercase().contains("access") ||
            message.to_lowercase().contains("verification"),
        "Should mention access/verification"
    );
    println!("   ✅ No address leakage\n");

    // Test DecryptionFailed doesn't leak key details
    println!("🔐 Test 2: DecryptionFailed sanitization...");
    let decrypt_error = VectorLoadingError::DecryptionFailed;

    let message = decrypt_error.user_friendly_message();

    println!("   Message: {}", message);

    // Should NOT contain key details
    assert!(!message.contains("bytes"), "Should not show byte details");
    assert!(!message.contains("0x"), "Should not expose hex values");

    // Should mention session key generically
    assert!(
        message.to_lowercase().contains("session key"),
        "Should mention session key"
    );
    println!("   ✅ No key leakage\n");

    println!("🎉 Security sanitization tests PASSED\n");
}

#[tokio::test]
async fn test_error_message_quality() {
    println!("\n🧪 Phase 4.5: Error Message Quality");
    println!("====================================\n");

    let test_cases = vec![
        (
            VectorLoadingError::ManifestNotFound {
                path: "home/vectors/db1/manifest.json".to_string(),
            },
            vec!["manifest", "not found"],
        ),
        (
            VectorLoadingError::DimensionMismatch {
                expected: 384,
                actual: 128,
            },
            vec!["384", "128", "dimension"],
        ),
        (
            VectorLoadingError::MemoryLimitExceeded {
                size_mb: 2000,
                limit_mb: 500,
            },
            vec!["2000", "500"],
        ),
        (
            VectorLoadingError::Timeout { timeout_secs: 300 },
            vec!["timed", "300"],
        ),
    ];

    println!("Testing message quality for {} error types:\n", test_cases.len());

    for (i, (ws_error, required_terms)) in test_cases.iter().enumerate() {
        let message = ws_error.user_friendly_message().to_lowercase();

        println!("  Test {}: {:?}", i + 1, ws_error.to_error_code());
        println!("    Message: {}", message);

        for term in required_terms {
            assert!(
                message.contains(&term.to_lowercase()),
                "Message should contain '{}': {}",
                term,
                message
            );
        }

        // Should be user-friendly (no Rust error syntax)
        assert!(!message.contains("error("), "Should not have Error() wrapper");
        assert!(!message.contains("anyhow"), "Should not mention anyhow");
        assert!(!message.contains("source:"), "Should not show source field");

        println!("    ✅ All required terms present\n");
    }

    println!("🎉 Error message quality tests PASSED\n");
}

#[tokio::test]
async fn test_error_context_preservation() {
    println!("\n🧪 Phase 4.5.7: Error Context Preservation");
    println!("============================================\n");

    // Test ChunkDownloadFailed preserves chunk ID
    let ws_error = VectorLoadingError::ChunkDownloadFailed {
        chunk_id: 42,
        source: anyhow::anyhow!("Network timeout after 30s"),
    };
    let message = ws_error.user_friendly_message();

    println!("🔍 ChunkDownloadFailed error:");
    println!("   Error code: {:?}", ws_error.to_error_code());
    println!("   Message: {}\n", message);

    // Should preserve chunk ID context
    assert!(message.contains("42"), "Should mention chunk ID");
    assert!(message.contains("chunk"), "Should mention chunk");

    // Test DimensionMismatch preserves both dimensions
    let ws_error = VectorLoadingError::DimensionMismatch {
        expected: 1536,
        actual: 768,
    };
    let message = ws_error.user_friendly_message();

    println!("🔍 DimensionMismatch error:");
    println!("   Error code: {:?}", ws_error.to_error_code());
    println!("   Message: {}\n", message);

    assert!(message.contains("1536"), "Should mention expected dimension");
    assert!(message.contains("768"), "Should mention actual dimension");

    // Test MemoryLimitExceeded preserves size details
    let ws_error = VectorLoadingError::MemoryLimitExceeded {
        size_mb: 2000,
        limit_mb: 500,
    };
    let message = ws_error.user_friendly_message();

    println!("🔍 MemoryLimitExceeded error:");
    println!("   Error code: {:?}", ws_error.to_error_code());
    println!("   Message: {}\n", message);

    assert!(message.contains("2000"), "Should mention required size");
    assert!(message.contains("500"), "Should mention limit");

    println!("✅ Error context preserved in all variants\n");
    println!("🎉 Context preservation test PASSED\n");
}

#[tokio::test]
async fn test_loading_progress_message_serialization() {
    println!("\n🧪 Phase 4.5: LoadingProgressMessage Serialization");
    println!("==================================================\n");

    // Test LoadingError message serialization
    let error_msg = LoadingProgressMessage::LoadingError {
        error_code: LoadingErrorCode::ManifestNotFound,
        error: "Manifest file not found at specified path".to_string(),
    };

    let json = serde_json::to_value(&error_msg).unwrap();
    println!("📄 Serialized LoadingError:");
    println!("{}\n", serde_json::to_string_pretty(&json).unwrap());

    // Verify structure (custom serialization uses "event", not "type")
    assert_eq!(json["event"], "loading_error");
    assert_eq!(json["error_code"], "MANIFEST_NOT_FOUND");
    assert_eq!(json["error"], "Manifest file not found at specified path");

    println!("✅ LoadingError serialization correct\n");

    // Test all error codes serialize correctly
    let all_codes = vec![
        LoadingErrorCode::ManifestNotFound,
        LoadingErrorCode::ManifestDownloadFailed,
        LoadingErrorCode::OwnerMismatch,
        LoadingErrorCode::DecryptionFailed,
        LoadingErrorCode::ChunkDownloadFailed,
        LoadingErrorCode::DimensionMismatch,
        LoadingErrorCode::MemoryLimitExceeded,
        LoadingErrorCode::RateLimitExceeded,
        LoadingErrorCode::InvalidPath,
        LoadingErrorCode::EmptyDatabase,
        LoadingErrorCode::Timeout,
        LoadingErrorCode::SessionNotFound,
        LoadingErrorCode::InvalidSessionKey,
        LoadingErrorCode::IndexBuildFailed,
        LoadingErrorCode::InternalError,
    ];

    println!("Testing serialization of all {} error codes:\n", all_codes.len());

    for (i, code) in all_codes.iter().enumerate() {
        let json = serde_json::to_value(code).unwrap();
        assert!(json.is_string(), "Error code should serialize as string");
        let code_str = json.as_str().unwrap();
        assert!(code_str.chars().all(|c| c.is_uppercase() || c == '_'),
                "Error code should be UPPER_SNAKE_CASE: {}", code_str);

        println!("  {}. {:?} → \"{}\" ✅", i + 1, code, code_str);
    }

    println!("\n🎉 Serialization tests PASSED\n");
}
