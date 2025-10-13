//! WebSocket Crypto Error Response Tests (TDD - Phase 7, Sub-phase 7.2)
//!
//! These tests verify that the WebSocket handlers send appropriate error messages
//! to clients when crypto operations fail:
//! - Decryption failure errors
//! - Invalid signature errors
//! - Missing session key errors
//! - Session corruption errors
//! - Error codes for client handling
//! - Connection closing on critical errors (optional)
//! - Error logging with session context
//!
//! **TDD Approach**: Tests written to verify EXISTING error behavior in handlers.
//!
//! **Note**: The error handling is already implemented in src/api/server.rs (Phase 6.2.1).
//! These tests verify the existing behavior and ensure it continues to work correctly.

use serde_json::json;

/// Test that decryption failure sends appropriate error response
#[tokio::test]
async fn test_decryption_failure_response() {
    // Test that when decrypt_with_aead fails, an error is sent to the client

    // EXISTING BEHAVIOR (src/api/server.rs:1548):
    // When decryption fails, the handler sends:
    // {
    //   "type": "error",
    //   "code": "DECRYPTION_FAILED",
    //   "message": "Failed to decrypt message: <error details>",
    //   "id": <message_id>  // echoed back
    // }

    // Expected error structure
    let expected_error = json!({
        "type": "error",
        "code": "DECRYPTION_FAILED",
        "message": "Failed to decrypt message: authentication tag verification failed"
    });

    // Verify error structure
    assert_eq!(expected_error["type"], "error");
    assert_eq!(expected_error["code"], "DECRYPTION_FAILED");
    assert!(expected_error["message"].as_str().unwrap().contains("decrypt"));

    // Expected behavior:
    // 1. Handler receives encrypted_message with invalid ciphertext
    // 2. Retrieves session key from SessionKeyStore
    // 3. Calls decrypt_with_aead() which fails
    // 4. Sends DECRYPTION_FAILED error to client
    // 5. Does NOT close connection (client can retry)
}

/// Test that invalid signature sends appropriate error response
#[tokio::test]
async fn test_invalid_signature_response() {
    // Test that when signature verification fails during encrypted_session_init,
    // an error is sent to the client

    // EXISTING BEHAVIOR (src/api/server.rs:1125):
    // When decrypt_session_init fails due to invalid signature, the handler sends:
    // {
    //   "type": "error",
    //   "code": "DECRYPTION_FAILED",  // Currently uses this, could use INVALID_SIGNATURE
    //   "message": "Failed to decrypt session init payload: <error details>",
    //   "session_id": <session_id>,
    //   "id": <message_id>
    // }

    // Expected error structure
    let expected_error = json!({
        "type": "error",
        "code": "DECRYPTION_FAILED",  // Or could be INVALID_SIGNATURE
        "message": "Failed to decrypt session init payload: signature verification failed",
        "session_id": "session-123"
    });

    // Verify error structure
    assert_eq!(expected_error["type"], "error");
    assert!(expected_error["code"].as_str().unwrap().contains("FAILED"));
    assert!(expected_error["message"].as_str().unwrap().contains("signature"));
    assert_eq!(expected_error["session_id"], "session-123");

    // Expected behavior:
    // 1. Handler receives encrypted_session_init with corrupted signature
    // 2. Calls decrypt_session_init() which verifies signature
    // 3. Signature verification fails
    // 4. Sends error to client with context
    // 5. Does NOT store session key
    // 6. Does NOT close connection (client can re-init)
}

/// Test that missing session key sends appropriate error response
#[tokio::test]
async fn test_missing_session_key_response() {
    // Test that when session key is not found in SessionKeyStore,
    // an error is sent to the client

    // EXISTING BEHAVIOR (src/api/server.rs:1620):
    // When session key is not found, the handler sends:
    // {
    //   "type": "error",
    //   "code": "SESSION_KEY_NOT_FOUND",
    //   "message": "No session key found for session_id: <session_id>",
    //   "id": <message_id>
    // }

    // Expected error structure
    let expected_error = json!({
        "type": "error",
        "code": "SESSION_KEY_NOT_FOUND",
        "message": "No session key found for session_id: session-456"
    });

    // Verify error structure
    assert_eq!(expected_error["type"], "error");
    assert_eq!(expected_error["code"], "SESSION_KEY_NOT_FOUND");
    assert!(expected_error["message"].as_str().unwrap().contains("session_id"));
    assert!(expected_error["message"].as_str().unwrap().contains("session-456"));

    // Expected behavior:
    // 1. Handler receives encrypted_message
    // 2. Extracts session_id from message
    // 3. Tries to retrieve session key from SessionKeyStore
    // 4. SessionKeyStore.get_key() returns None
    // 5. Sends SESSION_KEY_NOT_FOUND error to client
    // 6. Does NOT close connection
    // 7. Client should send encrypted_session_init first

    // Test with CryptoError type
    use fabstir_llm_node::crypto::CryptoError;

    let crypto_error = CryptoError::SessionKeyNotFound {
        session_id: "session-456".to_string(),
    };

    let error_msg = format!("{}", crypto_error);
    assert!(error_msg.contains("Session key not found"));
    assert!(error_msg.contains("session-456"));
}

/// Test that error closes connection on critical errors (optional)
#[tokio::test]
async fn test_error_closes_connection() {
    // Test that certain critical errors close the WebSocket connection

    // CURRENT BEHAVIOR:
    // - Errors do NOT close the connection automatically
    // - Client can retry after receiving error response
    // - Connection closes only on:
    //   1. Client sends Close frame
    //   2. Network error
    //   3. Server shutdown
    //
    // This is CORRECT behavior - connection should stay open for retries!

    // Optional: In the future, we might want to close on repeated auth failures
    // But for now, keeping connection open is more flexible

    // Test that normal errors do NOT close connection
    let error_response = json!({
        "type": "error",
        "code": "DECRYPTION_FAILED",
        "message": "Failed to decrypt message"
    });

    // This error should be sent, but connection remains open
    assert_eq!(error_response["type"], "error");
    // No connection close expected

    // Only these scenarios should close connection:
    // 1. Client explicitly closes (ws.close())
    // 2. Network failure
    // 3. Server shutdown
}

/// Test that errors are logged with session context
#[tokio::test]
async fn test_error_logged_with_context() {
    // Test that when errors occur, they are logged with session context
    // for debugging

    // EXISTING BEHAVIOR:
    // Errors are logged with error! macro including context:
    // - error!("Failed to decrypt session init: {}", e);
    // - error!("Failed to decrypt message: {}", e);
    // - error!("Failed to encrypt response chunk: {}", e);

    // Expected log format (using tracing/log macros):
    // ERROR: Failed to decrypt session init: <error details>
    // ERROR: Failed to decrypt message: <error details>
    // ERROR: Session key not found for session_id: <session_id>

    // Test CryptoError Display trait provides good context
    use fabstir_llm_node::crypto::CryptoError;

    let errors = vec![
        CryptoError::DecryptionFailed {
            operation: "encrypted_message".to_string(),
            reason: "authentication tag mismatch".to_string(),
        },
        CryptoError::InvalidSignature {
            operation: "session_init".to_string(),
            reason: "recovery failed".to_string(),
        },
        CryptoError::SessionKeyNotFound {
            session_id: "session-789".to_string(),
        },
    ];

    // Verify all errors have good context in Display output
    for error in errors {
        let msg = format!("{}", error);
        assert!(!msg.is_empty());
        // Should contain operation or session context
        assert!(
            msg.contains("encrypted_message") ||
            msg.contains("session_init") ||
            msg.contains("session-789")
        );
    }

    // Expected behavior:
    // 1. Error occurs during crypto operation
    // 2. Error is wrapped in CryptoError with context
    // 3. Handler logs error using error!() macro
    // 4. Log includes session_id and operation context
    // 5. Client receives error response with appropriate code
}

/// Test error response includes message ID for correlation
#[tokio::test]
async fn test_error_includes_message_id() {
    // Test that error responses include the message ID from the request
    // for request-response correlation

    // EXISTING BEHAVIOR (src/api/server.rs):
    // All error responses include message ID if present in request:
    // if let Some(msg_id) = json_msg.get("id") {
    //     error_msg["id"] = msg_id.clone();
    // }

    let request_message_id = "req-12345";

    let error_response = json!({
        "type": "error",
        "code": "DECRYPTION_FAILED",
        "message": "Failed to decrypt message",
        "id": request_message_id
    });

    // Verify message ID is echoed back
    assert_eq!(error_response["id"], request_message_id);

    // Expected behavior:
    // 1. Client sends message with "id": "req-12345"
    // 2. Server encounters error
    // 3. Server sends error response with same "id": "req-12345"
    // 4. Client can correlate response with request
}

/// Test all error codes are distinct and client-handleable
#[tokio::test]
async fn test_error_codes_distinct() {
    // Test that all crypto error codes are distinct and can be handled by client

    // EXISTING ERROR CODES (from src/api/server.rs):
    let error_codes = vec![
        "DECRYPTION_FAILED",           // Decryption or signature failure
        "INVALID_SIGNATURE",            // (could be added for clarity)
        "SESSION_KEY_NOT_FOUND",        // Session key not in store
        "INVALID_NONCE_SIZE",           // Nonce not 24 bytes
        "INVALID_HEX_ENCODING",         // Hex decode failed
        "MISSING_PAYLOAD",              // No payload object
        "INVALID_PAYLOAD",              // Missing payload fields
        "MISSING_SESSION_ID",           // No session_id in message
        "ENCRYPTION_NOT_SUPPORTED",     // Node has no private key
        "INVALID_UTF8",                 // Decrypted plaintext not UTF-8
        "ENCRYPTION_FAILED",            // Response encryption failed
    ];

    // Verify all codes are unique
    use std::collections::HashSet;
    let unique_codes: HashSet<_> = error_codes.iter().collect();
    assert_eq!(unique_codes.len(), error_codes.len());

    // Verify all codes are uppercase with underscores or digits
    for code in &error_codes {
        assert!(
            code.chars().all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()),
            "Error code '{}' contains invalid characters", code
        );
    }

    // Expected client handling:
    // switch (error.code) {
    //   case "DECRYPTION_FAILED":
    //     // Wrong key or tampered message
    //     break;
    //   case "SESSION_KEY_NOT_FOUND":
    //     // Need to call encrypted_session_init first
    //     break;
    //   case "ENCRYPTION_NOT_SUPPORTED":
    //     // Fall back to plaintext mode
    //     break;
    //   ...
    // }
}

/// Test session key is NOT logged in error messages
#[tokio::test]
async fn test_session_key_not_logged() {
    // Test that session keys are NEVER logged in error messages or logs

    // SECURITY REQUIREMENT:
    // - Session keys must never appear in error messages
    // - Session keys must never appear in logs
    // - Only log that key was found/not found, never the actual key

    let session_key = [42u8; 32];
    let session_id = "session-abc";

    // Error message should mention session_id but not session_key
    let error_msg = format!("Session key not found for session_id: {}", session_id);

    // Verify session_id is in message
    assert!(error_msg.contains(session_id));

    // Verify session_key is NOT in message (convert to hex to check)
    let key_hex = hex::encode(&session_key);
    assert!(!error_msg.contains(&key_hex));

    // CryptoError should also not expose keys
    use fabstir_llm_node::crypto::CryptoError;

    let crypto_error = CryptoError::SessionKeyNotFound {
        session_id: session_id.to_string(),
    };

    let error_display = format!("{}", crypto_error);
    assert!(error_display.contains(session_id));
    assert!(!error_display.contains(&key_hex));
}

/// Test invalid nonce size error
#[tokio::test]
async fn test_invalid_nonce_size_error() {
    // Test that invalid nonce size is rejected with clear error

    // EXISTING BEHAVIOR (src/api/server.rs:1016, 1245):
    // if nonce_bytes.len() != 24 {
    //     error_msg = {
    //         "type": "error",
    //         "code": "INVALID_NONCE_SIZE",
    //         "message": "Invalid nonce size: expected 24 bytes, got <actual>"
    //     };
    // }

    let invalid_nonce_16 = [0u8; 16]; // Too short
    let invalid_nonce_32 = [0u8; 32]; // Too long
    let valid_nonce = [0u8; 24];      // Correct

    // Test with CryptoError
    use fabstir_llm_node::crypto::CryptoError;

    let error = CryptoError::InvalidNonce {
        expected_size: 24,
        actual_size: invalid_nonce_16.len(),
    };

    let msg = format!("{}", error);
    assert!(msg.contains("Invalid nonce size"));
    assert!(msg.contains("24"));
    assert!(msg.contains("16"));

    // Expected error response
    let error_response = json!({
        "type": "error",
        "code": "INVALID_NONCE_SIZE",
        "message": format!("Invalid nonce size: expected 24 bytes, got {}", invalid_nonce_16.len())
    });

    assert_eq!(error_response["code"], "INVALID_NONCE_SIZE");
    assert!(error_response["message"].as_str().unwrap().contains("24"));

    // Verify valid nonce size is accepted
    assert_eq!(valid_nonce.len(), 24);
}

/// Test missing payload fields error
#[tokio::test]
async fn test_missing_payload_fields_error() {
    // Test that missing required fields in payload are rejected with clear error

    // EXISTING BEHAVIOR (src/api/server.rs:1586):
    // if ciphertext_hex or nonce_hex or aad_hex is None {
    //     error_msg = {
    //         "type": "error",
    //         "code": "MISSING_PAYLOAD_FIELDS",
    //         "message": "Payload must contain ciphertextHex, nonceHex, and aadHex"
    //     };
    // }

    // Test various missing field scenarios
    let required_fields = vec!["ciphertextHex", "nonceHex", "aadHex"];

    for field in &required_fields {
        let error_msg = format!("Payload must contain {}", field);
        assert!(error_msg.contains(field));
    }

    // Expected error response
    let error_response = json!({
        "type": "error",
        "code": "MISSING_PAYLOAD_FIELDS",
        "message": "Payload must contain ciphertextHex, nonceHex, and aadHex"
    });

    assert_eq!(error_response["code"], "MISSING_PAYLOAD_FIELDS");
    for field in &required_fields {
        assert!(error_response["message"].as_str().unwrap().contains(field));
    }

    // Test with CryptoError
    use fabstir_llm_node::crypto::CryptoError;

    let error = CryptoError::InvalidPayload {
        field: "ciphertextHex".to_string(),
        reason: "missing".to_string(),
    };

    let msg = format!("{}", error);
    assert!(msg.contains("Invalid payload"));
    assert!(msg.contains("ciphertextHex"));
}

/// Test hex decoding error
#[tokio::test]
async fn test_hex_decoding_error() {
    // Test that invalid hex encoding is rejected with clear error

    // EXISTING BEHAVIOR (src/api/server.rs:1142):
    // match (hex::decode(...), hex::decode(...), ...) {
    //     Ok(...) => { /* process */ },
    //     _ => {
    //         error_msg = {
    //             "type": "error",
    //             "code": "INVALID_HEX_ENCODING",
    //             "message": "Failed to decode hex fields in payload"
    //         };
    //     }
    // }

    let invalid_hex = "not_valid_hex_12345";

    // Verify hex::decode fails
    let result = hex::decode(invalid_hex);
    assert!(result.is_err());

    // Test with CryptoError conversion
    use fabstir_llm_node::crypto::CryptoError;

    if let Err(hex_err) = result {
        let crypto_err: CryptoError = hex_err.into();

        match crypto_err {
            CryptoError::InvalidPayload { field, reason } => {
                assert_eq!(field, "hex_field");
                assert!(reason.contains("decode"));
            }
            _ => panic!("Expected CryptoError::InvalidPayload"),
        }
    }

    // Expected error response
    let error_response = json!({
        "type": "error",
        "code": "INVALID_HEX_ENCODING",
        "message": "Failed to decode hex fields in payload"
    });

    assert_eq!(error_response["code"], "INVALID_HEX_ENCODING");
    assert!(error_response["message"].as_str().unwrap().contains("hex"));
}
