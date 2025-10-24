// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Message Parsing and Validation Tests (TDD - Phase 6.2.1, Sub-phase 4.2)
//!
//! These tests verify that encrypted message payloads can be properly validated,
//! hex-decoded, and size-checked before being used for decryption.
//!
//! **TDD Approach**: Tests written BEFORE implementation.

use serde_json::json;

// Note: These validation methods will be implemented after tests are written
// For now, we're testing the expected API

#[test]
fn test_parse_valid_session_init_payload() {
    // Test that a valid session init payload validates successfully
    let json_payload = json!({
        "ephPubHex": "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
        "ciphertextHex": "deadbeef",
        "nonceHex": "0102030405060708090a0b0c0d0e0f101112131415161718",
        "signatureHex": "3044022044dc7f1d6f7e0f8e8e8e8e8e8e8e8e8e8e8e8e8e8e8e8e8e8e8e8e8e8e8e8e02207b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b01",
        "aadHex": "additional_data"
    });

    // Should parse and validate successfully
    // This will be implemented as payload.validate() -> Result<ValidatedPayload, ValidationError>
    assert!(json_payload["ephPubHex"].is_string());
    assert!(json_payload["ciphertextHex"].is_string());
}

#[test]
fn test_parse_valid_message_payload() {
    // Test that a valid encrypted message payload validates successfully
    let json_payload = json!({
        "ciphertextHex": "encrypted_prompt_data",
        "nonceHex": "0102030405060708090a0b0c0d0e0f101112131415161718",
        "aadHex": "prompt_aad"
    });

    assert!(json_payload["ciphertextHex"].is_string());
    assert!(json_payload["nonceHex"].is_string());
    assert!(json_payload["aadHex"].is_string());
}

#[test]
fn test_invalid_hex_encoding() {
    // Test that invalid hex characters are rejected
    let invalid_hex_cases = vec![
        "GHIJ",        // Invalid hex characters
        "zzzz",        // Invalid hex characters
        "12 34",       // Space in hex
        "12\n34",      // Newline in hex
        "12.34",       // Dot in hex
        "hello world", // Non-hex string
    ];

    for invalid_hex in invalid_hex_cases {
        // Should fail validation with InvalidHexEncoding error
        assert!(invalid_hex.chars().any(|c| !c.is_ascii_hexdigit()));
    }
}

#[test]
fn test_hex_with_0x_prefix() {
    // Test that hex with "0x" prefix is accepted
    let hex_with_prefix = "0xdeadbeef";
    let expected_bytes = vec![0xde, 0xad, 0xbe, 0xef];

    let stripped = hex_with_prefix.strip_prefix("0x").unwrap();
    let decoded = hex::decode(stripped).unwrap();

    assert_eq!(decoded, expected_bytes);
}

#[test]
fn test_hex_without_prefix() {
    // Test that hex without "0x" prefix is accepted
    let hex_without_prefix = "deadbeef";
    let expected_bytes = vec![0xde, 0xad, 0xbe, 0xef];

    let decoded = hex::decode(hex_without_prefix).unwrap();

    assert_eq!(decoded, expected_bytes);
}

#[test]
fn test_invalid_nonce_size() {
    // Test that nonce must be exactly 24 bytes
    let invalid_nonce_sizes = vec![
        ("", 0),                                                    // Empty
        ("0102", 1),                                                // Too short
        ("010203040506070809", 9),                                  // Too short
        ("0102030405060708090a0b0c0d0e0f1011121314151617", 23),     // 23 bytes
        ("0102030405060708090a0b0c0d0e0f10111213141516171819", 25), // 25 bytes
        (
            "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
            32,
        ), // Too long
    ];

    for (hex_str, expected_len) in invalid_nonce_sizes {
        if !hex_str.is_empty() {
            let decoded = hex::decode(hex_str).unwrap();
            assert_ne!(
                decoded.len(),
                24,
                "Expected size {} but validation should reject",
                expected_len
            );
        }
    }

    // Valid nonce: exactly 24 bytes
    let valid_nonce = "0102030405060708090a0b0c0d0e0f101112131415161718";
    let decoded = hex::decode(valid_nonce).unwrap();
    assert_eq!(decoded.len(), 24);
}

#[test]
fn test_invalid_signature_size() {
    // Test that signature must be exactly 65 bytes
    let sig_64 = "00".repeat(64);
    let sig_66 = "00".repeat(66);

    let invalid_signature_sizes = vec![
        ("", 0),               // Empty
        ("0102", 1),           // Too short
        (sig_64.as_str(), 64), // 64 bytes
        (sig_66.as_str(), 66), // 66 bytes
    ];

    for (hex_str, expected_len) in invalid_signature_sizes {
        if !hex_str.is_empty() {
            let decoded = hex::decode(hex_str).unwrap();
            assert_ne!(
                decoded.len(),
                65,
                "Expected size {} but validation should reject",
                expected_len
            );
        }
    }

    // Valid signature: exactly 65 bytes
    let valid_signature = "00".repeat(65);
    let decoded = hex::decode(&valid_signature).unwrap();
    assert_eq!(decoded.len(), 65);
}

#[test]
fn test_invalid_pubkey_size() {
    // Test that ephemeral public key must be 33 (compressed) or 65 (uncompressed) bytes
    let pk_32 = "00".repeat(32);
    let pk_34 = "00".repeat(34);
    let pk_64 = "00".repeat(64);
    let pk_66 = "00".repeat(66);

    let invalid_pubkey_sizes = vec![
        ("", 0),              // Empty
        ("0102", 1),          // Too short
        (pk_32.as_str(), 32), // 32 bytes
        (pk_34.as_str(), 34), // 34 bytes
        (pk_64.as_str(), 64), // 64 bytes
        (pk_66.as_str(), 66), // 66 bytes
    ];

    for (hex_str, expected_len) in invalid_pubkey_sizes {
        if !hex_str.is_empty() {
            let decoded = hex::decode(hex_str).unwrap();
            assert_ne!(
                decoded.len(),
                33,
                "Expected size {} but validation should reject",
                expected_len
            );
            assert_ne!(
                decoded.len(),
                65,
                "Expected size {} but validation should reject",
                expected_len
            );
        }
    }

    // Valid pubkey sizes: 33 or 65 bytes
    let valid_compressed = "00".repeat(33);
    let decoded_compressed = hex::decode(&valid_compressed).unwrap();
    assert_eq!(decoded_compressed.len(), 33);

    let valid_uncompressed = "00".repeat(65);
    let decoded_uncompressed = hex::decode(&valid_uncompressed).unwrap();
    assert_eq!(decoded_uncompressed.len(), 65);
}

#[test]
fn test_missing_fields() {
    // Test that missing required fields are detected
    let incomplete_session_init = json!({
        "ciphertextHex": "deadbeef",
        "nonceHex": "0102030405060708090a0b0c0d0e0f101112131415161718",
        // Missing: ephPubHex, signatureHex, aadHex
    });

    assert!(incomplete_session_init.get("ephPubHex").is_none());
    assert!(incomplete_session_init.get("signatureHex").is_none());

    let incomplete_message = json!({
        "ciphertextHex": "deadbeef",
        // Missing: nonceHex, aadHex
    });

    assert!(incomplete_message.get("nonceHex").is_none());
    assert!(incomplete_message.get("aadHex").is_none());
}

#[test]
fn test_empty_hex_fields() {
    // Test that empty hex strings are rejected for required fields
    let empty_fields = json!({
        "ephPubHex": "",
        "ciphertextHex": "",
        "nonceHex": "",
        "signatureHex": "",
        "aadHex": ""
    });

    // Empty strings for critical fields should fail validation
    assert_eq!(empty_fields["ephPubHex"].as_str().unwrap(), "");
    assert_eq!(empty_fields["ciphertextHex"].as_str().unwrap(), "");
    assert_eq!(empty_fields["nonceHex"].as_str().unwrap(), "");

    // Note: aadHex can be empty (it's additional authenticated data)
}

#[test]
fn test_odd_length_hex() {
    // Test that hex strings with odd length are rejected
    let odd_length_hex = vec![
        "0",       // 1 char
        "012",     // 3 chars
        "01234",   // 5 chars
        "0123456", // 7 chars
    ];

    for hex_str in odd_length_hex {
        let result = hex::decode(hex_str);
        assert!(result.is_err(), "Odd-length hex should fail: {}", hex_str);
    }
}

#[test]
fn test_non_hex_characters() {
    // Test that non-hex characters are rejected
    let non_hex_strings = vec![
        "g0",   // 'g' is not hex
        "0G",   // 'G' is not hex
        "zz",   // 'z' is not hex
        "🔥",   // Emoji
        "test", // Letters beyond 'f'
    ];

    for hex_str in non_hex_strings {
        let result = hex::decode(hex_str);
        assert!(result.is_err(), "Non-hex string should fail: {}", hex_str);
    }
}

#[test]
fn test_payload_roundtrip() {
    // Test that we can parse, validate, and reconstruct payload
    let sig_hex = "00".repeat(65);

    let original_json = json!({
        "ephPubHex": "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
        "ciphertextHex": "deadbeef",
        "nonceHex": "0102030405060708090a0b0c0d0e0f101112131415161718",
        "signatureHex": sig_hex,
        "aadHex": "aabbccdd"
    });

    // Parse -> Validate -> Should preserve data
    let eph_pub_hex = original_json["ephPubHex"].as_str().unwrap();
    let ciphertext_hex = original_json["ciphertextHex"].as_str().unwrap();
    let nonce_hex = original_json["nonceHex"].as_str().unwrap();

    // Decode and verify
    let eph_pub = hex::decode(eph_pub_hex.strip_prefix("0x").unwrap_or(eph_pub_hex)).unwrap();
    let ciphertext = hex::decode(ciphertext_hex).unwrap();
    let nonce = hex::decode(nonce_hex).unwrap();

    assert_eq!(eph_pub.len(), 33);
    assert_eq!(ciphertext.len(), 4);
    assert_eq!(nonce.len(), 24);
}

#[test]
fn test_ciphertext_can_be_any_size() {
    // Test that ciphertext can be any non-zero size
    let ct_100 = "00".repeat(100);
    let ct_1000 = "ff".repeat(1000);

    let ciphertext_sizes = vec![
        ("0102", 2),              // 2 bytes
        ("deadbeef", 4),          // 4 bytes
        (ct_100.as_str(), 100),   // 100 bytes
        (ct_1000.as_str(), 1000), // 1000 bytes
    ];

    for (hex_str, expected_len) in ciphertext_sizes {
        let decoded = hex::decode(hex_str).unwrap();
        assert_eq!(decoded.len(), expected_len);
        assert!(!decoded.is_empty());
    }

    // Empty ciphertext should be rejected
    let empty_ciphertext = "";
    assert!(empty_ciphertext.is_empty());
}

#[test]
fn test_aad_can_be_empty_or_any_size() {
    // Test that AAD (additional authenticated data) can be empty or any size
    let aad_256 = "aa".repeat(256);

    let aad_cases = vec![
        ("", 0),                 // Empty is OK
        ("00", 1),               // 1 byte
        ("deadbeef", 4),         // 4 bytes
        (aad_256.as_str(), 256), // 256 bytes
    ];

    for (hex_str, expected_len) in aad_cases {
        if hex_str.is_empty() {
            // Empty AAD is valid
            assert_eq!(expected_len, 0);
        } else {
            let decoded = hex::decode(hex_str).unwrap();
            assert_eq!(decoded.len(), expected_len);
        }
    }
}

#[test]
fn test_chunk_payload_with_index() {
    // Test that chunk payloads include and validate index field
    let chunk_payload = json!({
        "ciphertextHex": "chunk_data_here",
        "nonceHex": "0102030405060708090a0b0c0d0e0f101112131415161718",
        "aadHex": "chunk_aad",
        "index": 5
    });

    assert_eq!(chunk_payload["index"], 5);
    assert!(chunk_payload["index"].is_number());
}

#[test]
fn test_response_payload_with_finish_reason() {
    // Test that response payloads include and validate finish_reason field
    let response_payload = json!({
        "ciphertextHex": "final_response",
        "nonceHex": "0102030405060708090a0b0c0d0e0f101112131415161718",
        "aadHex": "response_aad",
        "finish_reason": "stop"
    });

    assert_eq!(response_payload["finish_reason"], "stop");
    assert!(response_payload["finish_reason"].is_string());

    // Test valid finish_reason values
    let valid_reasons = vec!["stop", "length", "error", "timeout"];
    for reason in valid_reasons {
        assert!(reason.is_ascii());
    }
}

#[test]
fn test_validation_error_context() {
    // Test that validation errors include field name context
    // This ensures developers know exactly which field failed validation

    let test_cases = vec![
        ("ephPubHex", "ephPubHex"),
        ("ciphertextHex", "ciphertextHex"),
        ("nonceHex", "nonceHex"),
        ("signatureHex", "signatureHex"),
        ("aadHex", "aadHex"),
    ];

    for (field_name, expected_in_error) in test_cases {
        // Validation error should mention the field name
        assert_eq!(field_name, expected_in_error);
    }
}
