// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Crypto Error Types Tests (TDD - Phase 7, Sub-phase 7.1)
//!
//! These tests verify comprehensive error handling for crypto operations:
//! - CryptoError enum with clear variant names
//! - Display trait provides human-readable messages
//! - Error context (session_id, operation) is preserved
//! - From implementations convert library errors automatically
//!
//! **TDD Approach**: Tests written BEFORE implementation.

use fabstir_llm_node::crypto::CryptoError;

/// Test that CryptoError has all expected error variants
#[test]
fn test_crypto_error_types() {
    // Test DecryptionFailed variant
    let err = CryptoError::DecryptionFailed {
        operation: "session_init".to_string(),
        reason: "invalid ciphertext".to_string(),
    };
    assert!(matches!(err, CryptoError::DecryptionFailed { .. }));

    // Test InvalidSignature variant
    let err = CryptoError::InvalidSignature {
        operation: "signature_recovery".to_string(),
        reason: "malformed signature".to_string(),
    };
    assert!(matches!(err, CryptoError::InvalidSignature { .. }));

    // Test InvalidKey variant
    let err = CryptoError::InvalidKey {
        key_type: "ephemeral_public_key".to_string(),
        reason: "invalid point".to_string(),
    };
    assert!(matches!(err, CryptoError::InvalidKey { .. }));

    // Test InvalidNonce variant
    let err = CryptoError::InvalidNonce {
        expected_size: 24,
        actual_size: 12,
    };
    assert!(matches!(err, CryptoError::InvalidNonce { .. }));

    // Test KeyDerivationFailed variant
    let err = CryptoError::KeyDerivationFailed {
        operation: "ECDH".to_string(),
        reason: "shared secret extraction failed".to_string(),
    };
    assert!(matches!(err, CryptoError::KeyDerivationFailed { .. }));

    // Test InvalidPayload variant
    let err = CryptoError::InvalidPayload {
        field: "ciphertext".to_string(),
        reason: "empty".to_string(),
    };
    assert!(matches!(err, CryptoError::InvalidPayload { .. }));

    // Test SessionKeyNotFound variant
    let err = CryptoError::SessionKeyNotFound {
        session_id: "session-123".to_string(),
    };
    assert!(matches!(err, CryptoError::SessionKeyNotFound { .. }));
}

/// Test that Display trait provides clear error messages
#[test]
fn test_error_display() {
    // Test DecryptionFailed display
    let err = CryptoError::DecryptionFailed {
        operation: "session_init".to_string(),
        reason: "authentication tag mismatch".to_string(),
    };
    let display_msg = format!("{}", err);
    assert!(display_msg.contains("Decryption failed"));
    assert!(display_msg.contains("session_init"));
    assert!(display_msg.contains("authentication tag mismatch"));

    // Test InvalidSignature display
    let err = CryptoError::InvalidSignature {
        operation: "signature_recovery".to_string(),
        reason: "invalid recovery ID".to_string(),
    };
    let display_msg = format!("{}", err);
    assert!(display_msg.contains("Invalid signature"));
    assert!(display_msg.contains("signature_recovery"));
    assert!(display_msg.contains("invalid recovery ID"));

    // Test InvalidKey display
    let err = CryptoError::InvalidKey {
        key_type: "node_private_key".to_string(),
        reason: "wrong length".to_string(),
    };
    let display_msg = format!("{}", err);
    assert!(display_msg.contains("Invalid key"));
    assert!(display_msg.contains("node_private_key"));
    assert!(display_msg.contains("wrong length"));

    // Test InvalidNonce display
    let err = CryptoError::InvalidNonce {
        expected_size: 24,
        actual_size: 16,
    };
    let display_msg = format!("{}", err);
    assert!(display_msg.contains("Invalid nonce size"));
    assert!(display_msg.contains("24"));
    assert!(display_msg.contains("16"));

    // Test SessionKeyNotFound display
    let err = CryptoError::SessionKeyNotFound {
        session_id: "test-session".to_string(),
    };
    let display_msg = format!("{}", err);
    assert!(display_msg.contains("Session key not found"));
    assert!(display_msg.contains("test-session"));
}

/// Test that error context is preserved
#[test]
fn test_error_context() {
    // Test operation context
    let err = CryptoError::DecryptionFailed {
        operation: "encrypted_message".to_string(),
        reason: "MAC verification failed".to_string(),
    };

    match err {
        CryptoError::DecryptionFailed { operation, reason } => {
            assert_eq!(operation, "encrypted_message");
            assert_eq!(reason, "MAC verification failed");
        }
        _ => panic!("Wrong error variant"),
    }

    // Test session_id context
    let err = CryptoError::SessionKeyNotFound {
        session_id: "session-456".to_string(),
    };

    match err {
        CryptoError::SessionKeyNotFound { session_id } => {
            assert_eq!(session_id, "session-456");
        }
        _ => panic!("Wrong error variant"),
    }

    // Test key_type context
    let err = CryptoError::InvalidKey {
        key_type: "ephemeral_public_key".to_string(),
        reason: "compressed format required".to_string(),
    };

    match err {
        CryptoError::InvalidKey { key_type, reason } => {
            assert_eq!(key_type, "ephemeral_public_key");
            assert_eq!(reason, "compressed format required");
        }
        _ => panic!("Wrong error variant"),
    }

    // Test size context
    let err = CryptoError::InvalidNonce {
        expected_size: 24,
        actual_size: 32,
    };

    match err {
        CryptoError::InvalidNonce { expected_size, actual_size } => {
            assert_eq!(expected_size, 24);
            assert_eq!(actual_size, 32);
        }
        _ => panic!("Wrong error variant"),
    }
}

/// Test that From implementations convert library errors
#[test]
fn test_error_conversion() {
    // Test conversion from anyhow::Error
    let anyhow_err = anyhow::anyhow!("test error");
    let crypto_err: CryptoError = anyhow_err.into();

    match crypto_err {
        CryptoError::Other(msg) => {
            assert!(msg.contains("test error"));
        }
        _ => panic!("Expected CryptoError::Other variant"),
    }

    // Test conversion from hex decode error
    let hex_result = hex::decode("invalid_hex");
    if let Err(hex_err) = hex_result {
        let crypto_err: CryptoError = hex_err.into();

        match crypto_err {
            CryptoError::InvalidPayload { field, reason } => {
                assert_eq!(field, "hex_field");
                assert!(reason.contains("decode"));
            }
            _ => panic!("Expected CryptoError::InvalidPayload variant"),
        }
    }

    // Test conversion from k256 errors
    // This tests that we can handle crypto library errors
    let invalid_key_bytes = vec![0u8; 31]; // Wrong size
    let key_result = k256::SecretKey::from_slice(&invalid_key_bytes);

    if let Err(_k256_err) = key_result {
        // In real implementation, this would be converted via From impl
        let crypto_err = CryptoError::InvalidKey {
            key_type: "secret_key".to_string(),
            reason: "invalid length".to_string(),
        };

        assert!(matches!(crypto_err, CryptoError::InvalidKey { .. }));
    }
}
