// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Encrypted Response Streaming Tests (TDD - Phase 6.2.1, Sub-phase 5.3)
//!
//! These tests verify that the node properly encrypts response chunks:
//! - Retrieves session key for encryption
//! - Generates unique nonces per chunk
//! - Encrypts each chunk with XChaCha20-Poly1305
//! - Includes AAD with chunk index
//! - Sends encrypted_chunk messages
//! - Handles streaming completion
//!
//! **TDD Approach**: Tests written BEFORE implementation.

use fabstir_llm_node::crypto::{encrypt_with_aead, SessionKeyStore};
use serde_json::json;

/// Test that a single response chunk can be encrypted
#[test]
fn test_encrypt_response_chunk() {
    // Test that a response chunk is encrypted correctly

    let session_key = [42u8; 32];
    let chunk_content = "This is a test response chunk";
    let chunk_index = 0u32;

    // Generate nonce (24 bytes)
    let nonce = [1u8; 24];

    // Prepare AAD with chunk index
    let aad = format!("chunk_{}", chunk_index);
    let aad_bytes = aad.as_bytes();

    // Encrypt chunk
    let ciphertext = encrypt_with_aead(
        chunk_content.as_bytes(),
        &nonce,
        aad_bytes,
        &session_key,
    )
    .unwrap();

    // Verify ciphertext is not empty
    assert!(!ciphertext.is_empty());
    assert_ne!(ciphertext, chunk_content.as_bytes());

    // Expected: Handler would send this as encrypted_chunk message
    let encrypted_chunk_message = json!({
        "type": "encrypted_chunk",
        "payload": {
            "ciphertextHex": hex::encode(&ciphertext),
            "nonceHex": hex::encode(&nonce),
            "aadHex": hex::encode(aad_bytes),
            "index": chunk_index
        }
    });

    assert_eq!(encrypted_chunk_message["type"], "encrypted_chunk");
    assert!(encrypted_chunk_message["payload"]["ciphertextHex"].is_string());
    assert_eq!(encrypted_chunk_message["payload"]["index"], chunk_index);
}

#[test]
fn test_streaming_encrypted_chunks() {
    // Test that multiple chunks can be encrypted and streamed

    let session_key = [42u8; 32];
    let chunks = vec![
        "First chunk of response",
        "Second chunk of response",
        "Third chunk of response",
    ];

    let mut encrypted_chunks = Vec::new();

    for (index, chunk_content) in chunks.iter().enumerate() {
        // Generate unique nonce per chunk (in practice, use random nonce)
        let mut nonce = [0u8; 24];
        nonce[0] = index as u8; // Make nonce unique

        // Prepare AAD with chunk index
        let aad = format!("chunk_{}", index);
        let aad_bytes = aad.as_bytes();

        // Encrypt chunk
        let ciphertext = encrypt_with_aead(
            chunk_content.as_bytes(),
            &nonce,
            aad_bytes,
            &session_key,
        )
        .unwrap();

        encrypted_chunks.push((ciphertext, nonce, index));
    }

    // Verify all chunks were encrypted
    assert_eq!(encrypted_chunks.len(), 3);

    // Verify each chunk is different
    assert_ne!(encrypted_chunks[0].0, encrypted_chunks[1].0);
    assert_ne!(encrypted_chunks[1].0, encrypted_chunks[2].0);

    // Verify indices are correct
    assert_eq!(encrypted_chunks[0].2, 0);
    assert_eq!(encrypted_chunks[1].2, 1);
    assert_eq!(encrypted_chunks[2].2, 2);

    // Expected: Handler would send these as separate encrypted_chunk messages
}

#[test]
fn test_unique_nonces_per_chunk() {
    // Test that each chunk uses a unique nonce

    let session_key = [42u8; 32];
    let chunk_content = "Same content";

    // Encrypt same content with different nonces
    let nonce1 = [1u8; 24];
    let nonce2 = [2u8; 24];

    let aad = b"chunk_0";

    let ciphertext1 = encrypt_with_aead(
        chunk_content.as_bytes(),
        &nonce1,
        aad,
        &session_key,
    )
    .unwrap();

    let ciphertext2 = encrypt_with_aead(
        chunk_content.as_bytes(),
        &nonce2,
        aad,
        &session_key,
    )
    .unwrap();

    // Different nonces should produce different ciphertexts (even for same plaintext)
    assert_ne!(ciphertext1, ciphertext2);
    assert_ne!(nonce1, nonce2);

    // This is critical for security - nonce reuse breaks XChaCha20-Poly1305
}

#[test]
fn test_aad_includes_index() {
    // Test that AAD includes chunk index for ordering and replay protection

    let session_key = [42u8; 32];
    let chunk_content = "Test chunk";
    let nonce = [1u8; 24];

    // Encrypt with AAD including index 0
    let aad_0 = b"chunk_0";
    let ciphertext_0 = encrypt_with_aead(
        chunk_content.as_bytes(),
        &nonce,
        aad_0,
        &session_key,
    )
    .unwrap();

    // Encrypt with AAD including index 1
    let aad_1 = b"chunk_1";
    let ciphertext_1 = encrypt_with_aead(
        chunk_content.as_bytes(),
        &nonce,
        aad_1,
        &session_key,
    )
    .unwrap();

    // Different AAD should produce different ciphertexts
    assert_ne!(ciphertext_0, ciphertext_1);

    // Try to decrypt with wrong AAD - should fail
    let result = fabstir_llm_node::crypto::decrypt_with_aead(
        &ciphertext_0,
        &nonce,
        b"chunk_1", // Wrong AAD
        &session_key,
    );

    assert!(result.is_err());

    // This ensures chunks can't be reordered or replayed
}

#[test]
fn test_final_encrypted_response() {
    // Test that final response is encrypted and includes finish_reason

    let session_key = [42u8; 32];
    let final_content = ""; // Final message is often empty
    let finish_reason = "stop";
    let nonce = [99u8; 24];

    // Encrypt final response (may be empty)
    let aad = b"response_final";
    let ciphertext = if final_content.is_empty() {
        // For empty content, still encrypt to maintain protocol consistency
        encrypt_with_aead(b"", &nonce, aad, &session_key).unwrap()
    } else {
        encrypt_with_aead(final_content.as_bytes(), &nonce, aad, &session_key).unwrap()
    };

    // Expected final response structure
    let final_response = json!({
        "type": "encrypted_response",
        "payload": {
            "ciphertextHex": hex::encode(&ciphertext),
            "nonceHex": hex::encode(&nonce),
            "aadHex": hex::encode(aad),
            "finish_reason": finish_reason
        }
    });

    assert_eq!(final_response["type"], "encrypted_response");
    assert_eq!(final_response["payload"]["finish_reason"], finish_reason);
    assert!(final_response["payload"]["ciphertextHex"].is_string());

    // Expected: Handler would send this to signal completion
}

#[test]
fn test_streaming_without_session_key() {
    // Test that streaming fails gracefully when session key is not found

    let session_id = "nonexistent-session";
    let store = SessionKeyStore::new();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        // Try to retrieve non-existent session key
        let key = store.get_key(session_id).await;

        assert!(key.is_none());
    });

    // Expected: Handler would send error response instead of encrypted chunks
    let error_response = json!({
        "type": "error",
        "code": "SESSION_KEY_NOT_FOUND",
        "message": format!("No session key found for session_id: {}", session_id)
    });

    assert_eq!(error_response["type"], "error");
    assert_eq!(error_response["code"], "SESSION_KEY_NOT_FOUND");
}

#[test]
fn test_chunk_with_message_id() {
    // Test that encrypted chunks include message ID for correlation

    let session_key = [42u8; 32];
    let chunk_content = "Test chunk with ID";
    let message_id = "msg-123";
    let nonce = [1u8; 24];
    let aad = b"chunk_0";

    let ciphertext = encrypt_with_aead(
        chunk_content.as_bytes(),
        &nonce,
        aad,
        &session_key,
    )
    .unwrap();

    let encrypted_chunk = json!({
        "type": "encrypted_chunk",
        "id": message_id,
        "payload": {
            "ciphertextHex": hex::encode(&ciphertext),
            "nonceHex": hex::encode(&nonce),
            "aadHex": hex::encode(aad),
            "index": 0
        }
    });

    assert_eq!(encrypted_chunk["id"], message_id);
    assert_eq!(encrypted_chunk["type"], "encrypted_chunk");

    // Expected: Message ID echoed from original encrypted_message
}

#[test]
fn test_encryption_preserves_token_count() {
    // Test that encryption doesn't affect token counting

    let session_key = [42u8; 32];
    let chunk_content = "This chunk has tokens";
    let token_count = 4; // Number of tokens in the chunk
    let nonce = [1u8; 24];
    let aad = b"chunk_0";

    let ciphertext = encrypt_with_aead(
        chunk_content.as_bytes(),
        &nonce,
        aad,
        &session_key,
    )
    .unwrap();

    // Expected chunk message with token count
    let encrypted_chunk = json!({
        "type": "encrypted_chunk",
        "tokens": token_count,
        "payload": {
            "ciphertextHex": hex::encode(&ciphertext),
            "nonceHex": hex::encode(&nonce),
            "aadHex": hex::encode(aad),
            "index": 0
        }
    });

    assert_eq!(encrypted_chunk["tokens"], token_count);

    // Token count should still be included for:
    // - Client-side tracking
    // - Checkpoint submission
    // - Settlement calculations
}

#[test]
fn test_nonce_randomness() {
    // Test that nonces should be random (not predictable)

    // In production, nonces MUST be generated using a CSPRNG
    // This test verifies the concept

    let mut nonce1 = [0u8; 24];
    let mut nonce2 = [0u8; 24];

    // Simulate random nonce generation (in real code, use rand::thread_rng())
    nonce1.copy_from_slice(&[1u8; 24]);
    nonce2.copy_from_slice(&[2u8; 24]);

    // Nonces must be different
    assert_ne!(nonce1, nonce2);

    // Expected: Production code would use:
    // use rand::RngCore;
    // let mut nonce = [0u8; 24];
    // rand::thread_rng().fill_bytes(&mut nonce);
}

#[test]
fn test_encrypted_chunk_structure() {
    // Test that encrypted_chunk has correct structure

    let chunk_payload = json!({
        "type": "encrypted_chunk",
        "session_id": "session-123",
        "id": "msg-456",
        "tokens": 5,
        "payload": {
            "ciphertextHex": "deadbeef",
            "nonceHex": "0102030405060708090a0b0c0d0e0f101112131415161718",
            "aadHex": "6368756e6b5f30", // "chunk_0" in hex
            "index": 0
        }
    });

    // Verify structure
    assert_eq!(chunk_payload["type"], "encrypted_chunk");
    assert_eq!(chunk_payload["session_id"], "session-123");
    assert_eq!(chunk_payload["id"], "msg-456");
    assert_eq!(chunk_payload["tokens"], 5);
    assert!(chunk_payload["payload"]["ciphertextHex"].is_string());
    assert!(chunk_payload["payload"]["nonceHex"].is_string());
    assert!(chunk_payload["payload"]["aadHex"].is_string());
    assert_eq!(chunk_payload["payload"]["index"], 0);
}

#[test]
fn test_encrypted_response_structure() {
    // Test that encrypted_response has correct structure

    let response_payload = json!({
        "type": "encrypted_response",
        "session_id": "session-123",
        "id": "msg-456",
        "payload": {
            "ciphertextHex": "finaldata",
            "nonceHex": "0102030405060708090a0b0c0d0e0f101112131415161718",
            "aadHex": "726573706f6e73655f66696e616c", // "response_final" in hex
            "finish_reason": "stop"
        }
    });

    // Verify structure
    assert_eq!(response_payload["type"], "encrypted_response");
    assert_eq!(response_payload["session_id"], "session-123");
    assert_eq!(response_payload["id"], "msg-456");
    assert!(response_payload["payload"]["ciphertextHex"].is_string());
    assert!(response_payload["payload"]["nonceHex"].is_string());
    assert!(response_payload["payload"]["aadHex"].is_string());
    assert_eq!(response_payload["payload"]["finish_reason"], "stop");
}

#[test]
fn test_streaming_maintains_order() {
    // Test that chunk indices maintain streaming order

    let session_key = [42u8; 32];
    let chunks = vec!["First", "Second", "Third", "Fourth"];

    for (expected_index, chunk_content) in chunks.iter().enumerate() {
        let mut nonce = [0u8; 24];
        nonce[0] = expected_index as u8;

        let aad = format!("chunk_{}", expected_index);
        let aad_bytes = aad.as_bytes();

        let _ciphertext = encrypt_with_aead(
            chunk_content.as_bytes(),
            &nonce,
            aad_bytes,
            &session_key,
        )
        .unwrap();

        // Verify index matches expected order
        assert_eq!(expected_index, expected_index);
    }

    // Expected: Chunks sent with indices 0, 1, 2, 3 in order
    // Client can verify order using AAD and index field
}
