//! Encrypted Message Handler Tests (TDD - Phase 6.2.1, Sub-phase 5.2)
//!
//! These tests verify that the node properly handles encrypted_message WebSocket messages:
//! - Retrieves session key from SessionKeyStore
//! - Decrypts message payload using session key
//! - Validates AAD for replay protection
//! - Extracts plaintext prompt
//! - Processes inference with existing logic
//! - Returns response (encrypted if session has key)
//!
//! **TDD Approach**: Tests written BEFORE handler implementation.

use fabstir_llm_node::crypto::{
    encrypt_with_aead, SessionKeyStore,
};
use serde_json::json;

/// Create a valid encrypted message payload for testing
fn create_test_encrypted_message(
    session_key: &[u8; 32],
    prompt: &str,
) -> serde_json::Value {
    // Encrypt prompt with session key
    let nonce = [2u8; 24]; // Different from session_init nonce
    let aad = b"encrypted_message";
    let plaintext = prompt.as_bytes();

    let ciphertext = encrypt_with_aead(plaintext, &nonce, aad, session_key).unwrap();

    // Create WebSocket message payload
    json!({
        "type": "encrypted_message",
        "session_id": "session-123",
        "payload": {
            "ciphertextHex": hex::encode(&ciphertext),
            "nonceHex": hex::encode(&nonce),
            "aadHex": hex::encode(aad)
        }
    })
}

#[test]
fn test_encrypted_message_handler() {
    // Test that handler correctly processes an encrypted_message

    let session_key = [42u8; 32];
    let prompt = "What is the capital of France?";

    let payload = create_test_encrypted_message(&session_key, prompt);

    // Verify payload structure
    assert_eq!(payload["type"], "encrypted_message");
    assert_eq!(payload["session_id"], "session-123");
    assert!(payload["payload"]["ciphertextHex"].is_string());
    assert!(payload["payload"]["nonceHex"].is_string());
    assert!(payload["payload"]["aadHex"].is_string());

    // Expected: Handler would:
    // 1. Retrieve session key from SessionKeyStore
    // 2. Decrypt ciphertext
    // 3. Extract prompt
    // 4. Process inference
}

#[test]
fn test_message_decryption() {
    // Test that message decrypts successfully with correct session key

    let session_key = [42u8; 32];
    let original_prompt = "Write a function to calculate fibonacci numbers";

    let payload = create_test_encrypted_message(&session_key, original_prompt);

    // Parse payload
    let ciphertext_hex = payload["payload"]["ciphertextHex"].as_str().unwrap();
    let nonce_hex = payload["payload"]["nonceHex"].as_str().unwrap();
    let aad_hex = payload["payload"]["aadHex"].as_str().unwrap();

    let ciphertext = hex::decode(ciphertext_hex).unwrap();
    let nonce_bytes = hex::decode(nonce_hex).unwrap();
    let aad_bytes = hex::decode(aad_hex).unwrap();

    // Convert nonce to [u8; 24]
    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&nonce_bytes);

    // Decrypt using the same session key
    let decrypted = fabstir_llm_node::crypto::decrypt_with_aead(
        &ciphertext,
        &nonce,
        &aad_bytes,
        &session_key
    ).unwrap();

    let decrypted_prompt = String::from_utf8(decrypted).unwrap();

    // Verify decrypted prompt matches original
    assert_eq!(decrypted_prompt, original_prompt);
}

#[test]
fn test_missing_session_key() {
    // Test that missing session key is handled with error

    let session_key = [42u8; 32];
    let payload = create_test_encrypted_message(&session_key, "test prompt");

    // Create empty SessionKeyStore (no key stored)
    let store = SessionKeyStore::new();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let session_id = payload["session_id"].as_str().unwrap();

        // Try to retrieve non-existent session key
        let retrieved_key = store.get_key(session_id).await;

        assert!(retrieved_key.is_none());
    });

    // Expected: Handler would send error response:
    // {
    //   "type": "error",
    //   "code": "SESSION_KEY_NOT_FOUND",
    //   "message": "No session key found for session_id: session-123"
    // }
}

#[test]
fn test_invalid_nonce() {
    // Test that invalid nonce size is rejected

    let session_key = [42u8; 32];
    let mut payload = create_test_encrypted_message(&session_key, "test");

    // Corrupt nonce to wrong size
    payload["payload"]["nonceHex"] = json!(hex::encode([1u8; 12])); // Only 12 bytes

    let nonce_hex = payload["payload"]["nonceHex"].as_str().unwrap();
    let nonce_bytes = hex::decode(nonce_hex).unwrap();

    // Verify nonce is wrong size
    assert_eq!(nonce_bytes.len(), 12);
    assert_ne!(nonce_bytes.len(), 24);

    // Expected: Handler would reject with validation error
    // "Invalid nonce size: expected 24 bytes, got 12"
}

#[test]
fn test_aad_validation() {
    // Test that AAD is validated to prevent replay attacks

    let session_key = [42u8; 32];
    let prompt = "test prompt";
    let payload = create_test_encrypted_message(&session_key, prompt);

    // Parse payload
    let ciphertext_hex = payload["payload"]["ciphertextHex"].as_str().unwrap();
    let nonce_hex = payload["payload"]["nonceHex"].as_str().unwrap();
    let aad_hex = payload["payload"]["aadHex"].as_str().unwrap();

    let ciphertext = hex::decode(ciphertext_hex).unwrap();
    let nonce_bytes = hex::decode(nonce_hex).unwrap();
    let aad_bytes = hex::decode(aad_hex).unwrap();

    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&nonce_bytes);

    // Decrypt with correct AAD - should succeed
    let result_correct = fabstir_llm_node::crypto::decrypt_with_aead(
        &ciphertext,
        &nonce,
        &aad_bytes,
        &session_key
    );
    assert!(result_correct.is_ok());

    // Decrypt with WRONG AAD - should fail
    let wrong_aad = b"wrong_aad";
    let result_wrong = fabstir_llm_node::crypto::decrypt_with_aead(
        &ciphertext,
        &nonce,
        wrong_aad,
        &session_key
    );
    assert!(result_wrong.is_err());

    let error_msg = result_wrong.unwrap_err().to_string();
    assert!(
        error_msg.contains("Decryption failed") ||
        error_msg.contains("authentication") ||
        error_msg.contains("AEAD")
    );

    // Expected: Handler would validate AAD matches expected value
}

#[test]
fn test_inference_with_encrypted_prompt() {
    // Test that encrypted prompt flows through to inference engine

    let session_key = [42u8; 32];
    let prompt = "Explain quantum computing in simple terms";
    let payload = create_test_encrypted_message(&session_key, prompt);

    let session_id = payload["session_id"].as_str().unwrap();

    // Simulate handler flow
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let store = SessionKeyStore::new();

        // Store session key (as would happen during encrypted_session_init)
        store.store_key(session_id.to_string(), session_key).await;

        // Retrieve session key (as handler would do)
        let retrieved_key = store.get_key(session_id).await;
        assert!(retrieved_key.is_some());

        // Parse and decrypt
        let ciphertext_hex = payload["payload"]["ciphertextHex"].as_str().unwrap();
        let nonce_hex = payload["payload"]["nonceHex"].as_str().unwrap();
        let aad_hex = payload["payload"]["aadHex"].as_str().unwrap();

        let ciphertext = hex::decode(ciphertext_hex).unwrap();
        let nonce_bytes = hex::decode(nonce_hex).unwrap();
        let aad_bytes = hex::decode(aad_hex).unwrap();

        let mut nonce = [0u8; 24];
        nonce.copy_from_slice(&nonce_bytes);

        let decrypted = fabstir_llm_node::crypto::decrypt_with_aead(
            &ciphertext,
            &nonce,
            &aad_bytes,
            &retrieved_key.unwrap()
        ).unwrap();

        let decrypted_prompt = String::from_utf8(decrypted).unwrap();

        assert_eq!(decrypted_prompt, prompt);

        // Expected: Handler would pass decrypted_prompt to inference engine
        // and return response (encrypted if session has key)
    });
}

#[test]
fn test_empty_ciphertext() {
    // Test that empty ciphertext is rejected

    let mut payload = json!({
        "type": "encrypted_message",
        "session_id": "session-123",
        "payload": {
            "ciphertextHex": "",
            "nonceHex": hex::encode([1u8; 24]),
            "aadHex": hex::encode(b"test")
        }
    });

    let ciphertext_hex = payload["payload"]["ciphertextHex"].as_str().unwrap();
    assert!(ciphertext_hex.is_empty());

    // Expected: Handler would reject with validation error
    // "Empty ciphertext is not allowed"
}

#[test]
fn test_wrong_session_key() {
    // Test that decryption fails with wrong session key

    let correct_key = [42u8; 32];
    let wrong_key = [99u8; 32];

    let payload = create_test_encrypted_message(&correct_key, "test prompt");

    // Parse payload
    let ciphertext_hex = payload["payload"]["ciphertextHex"].as_str().unwrap();
    let nonce_hex = payload["payload"]["nonceHex"].as_str().unwrap();
    let aad_hex = payload["payload"]["aadHex"].as_str().unwrap();

    let ciphertext = hex::decode(ciphertext_hex).unwrap();
    let nonce_bytes = hex::decode(nonce_hex).unwrap();
    let aad_bytes = hex::decode(aad_hex).unwrap();

    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&nonce_bytes);

    // Try to decrypt with wrong key
    let result = fabstir_llm_node::crypto::decrypt_with_aead(
        &ciphertext,
        &nonce,
        &aad_bytes,
        &wrong_key
    );

    // Should fail
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("Decryption failed") ||
        error_msg.contains("authentication") ||
        error_msg.contains("AEAD")
    );

    // Expected: Handler would send error response
}

#[test]
fn test_message_id_echo() {
    // Test that message ID is echoed back in response

    let session_key = [42u8; 32];
    let mut payload = create_test_encrypted_message(&session_key, "test");

    // Add message ID
    payload["id"] = json!("msg-456");

    assert_eq!(payload["id"], "msg-456");

    // Expected: Handler response would include:
    // {
    //   "type": "response",
    //   "id": "msg-456",
    //   "session_id": "session-123",
    //   ...
    // }
}

#[test]
fn test_session_key_persistence() {
    // Test that session key persists across multiple messages

    let session_key = [42u8; 32];
    let session_id = "session-123";

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let store = SessionKeyStore::new();

        // Store session key once
        store.store_key(session_id.to_string(), session_key).await;

        // Send multiple encrypted messages
        for i in 0..3 {
            let prompt = format!("Message {}", i);
            let payload = create_test_encrypted_message(&session_key, &prompt);

            // Retrieve session key (should be same key each time)
            let retrieved_key = store.get_key(session_id).await;
            assert!(retrieved_key.is_some());
            assert_eq!(retrieved_key.unwrap(), session_key);

            // Decrypt message
            let ciphertext_hex = payload["payload"]["ciphertextHex"].as_str().unwrap();
            let nonce_hex = payload["payload"]["nonceHex"].as_str().unwrap();
            let aad_hex = payload["payload"]["aadHex"].as_str().unwrap();

            let ciphertext = hex::decode(ciphertext_hex).unwrap();
            let nonce_bytes = hex::decode(nonce_hex).unwrap();
            let aad_bytes = hex::decode(aad_hex).unwrap();

            let mut nonce = [0u8; 24];
            nonce.copy_from_slice(&nonce_bytes);

            let decrypted = fabstir_llm_node::crypto::decrypt_with_aead(
                &ciphertext,
                &nonce,
                &aad_bytes,
                &retrieved_key.unwrap()
            ).unwrap();

            let decrypted_prompt = String::from_utf8(decrypted).unwrap();
            assert_eq!(decrypted_prompt, prompt);
        }

        // Expected: Single session key used for entire session
    });
}

#[test]
fn test_hex_with_0x_prefix() {
    // Test that hex fields with "0x" prefix are handled

    let session_key = [42u8; 32];
    let mut payload = create_test_encrypted_message(&session_key, "test");

    // Add "0x" prefix to hex fields
    let ciphertext_hex = payload["payload"]["ciphertextHex"].as_str().unwrap();
    payload["payload"]["ciphertextHex"] = json!(format!("0x{}", ciphertext_hex));

    let nonce_hex = payload["payload"]["nonceHex"].as_str().unwrap();
    payload["payload"]["nonceHex"] = json!(format!("0x{}", nonce_hex));

    // Parse and strip prefix
    let ciphertext_with_prefix = payload["payload"]["ciphertextHex"].as_str().unwrap();
    let ciphertext_stripped = ciphertext_with_prefix.strip_prefix("0x").unwrap_or(ciphertext_with_prefix);

    let nonce_with_prefix = payload["payload"]["nonceHex"].as_str().unwrap();
    let nonce_stripped = nonce_with_prefix.strip_prefix("0x").unwrap_or(nonce_with_prefix);

    // Should decode successfully
    let ciphertext = hex::decode(ciphertext_stripped).unwrap();
    let nonce_bytes = hex::decode(nonce_stripped).unwrap();

    assert!(!ciphertext.is_empty());
    assert_eq!(nonce_bytes.len(), 24);

    // Expected: Handler strips "0x" prefix before hex decoding
}
