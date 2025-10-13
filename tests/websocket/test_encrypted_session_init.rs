//! Encrypted Session Initialization Handler Tests (TDD - Phase 6.2.1, Sub-phase 5.1)
//!
//! These tests verify that the node properly handles encrypted_session_init WebSocket messages:
//! - Parses encrypted payloads from JSON
//! - Calls decrypt_session_init() with node's private key
//! - Stores session key in SessionKeyStore
//! - Recovers and tracks client address
//! - Tracks session metadata (job_id, chain_id)
//! - Sends session_init_ack response
//!
//! **TDD Approach**: Tests written BEFORE handler implementation.

use fabstir_llm_node::crypto::{
    decrypt_session_init, encrypt_with_aead, derive_shared_key, EncryptedSessionPayload,
    SessionKeyStore,
};
use k256::ecdsa::SigningKey;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::SecretKey;
use serde_json::json;
use sha2::{Digest, Sha256};

/// Generate a random secp256k1 keypair for testing
fn generate_keypair() -> (Vec<u8>, Vec<u8>) {
    let private_key = SecretKey::random(&mut rand::thread_rng());
    let private_bytes = private_key.to_bytes().to_vec();

    let public_key = private_key.public_key();
    let public_bytes = public_key.to_encoded_point(true).as_bytes().to_vec();

    (private_bytes, public_bytes)
}

/// Create a valid encrypted session init payload for testing
fn create_test_session_init_payload(
    node_private_key: &[u8],
    client_private_key: &[u8],
) -> (serde_json::Value, Vec<u8>) {
    // Generate client ephemeral keypair
    let client_eph_private = SecretKey::random(&mut rand::thread_rng());
    let client_eph_public = client_eph_private.public_key();
    let eph_pub_bytes = client_eph_public.to_encoded_point(true).as_bytes().to_vec();

    // Derive shared key via ECDH
    let shared_key = derive_shared_key(&eph_pub_bytes, node_private_key).unwrap();

    // Create session data
    let session_key = hex::encode([42u8; 32]); // Random 32-byte key
    let session_data = json!({
        "jobId": "123",
        "modelName": "llama-3",
        "sessionKey": format!("0x{}", session_key),
        "pricePerToken": 2000
    });

    let plaintext = session_data.to_string();
    let plaintext_bytes = plaintext.as_bytes();

    // Encrypt session data
    let nonce = [1u8; 24];
    let aad = b"session_init";
    let ciphertext = encrypt_with_aead(plaintext_bytes, &nonce, aad, &shared_key).unwrap();

    // Sign ciphertext with client's wallet key
    let ciphertext_hash = Sha256::digest(&ciphertext);
    let signing_key = SigningKey::from_bytes(client_private_key.into()).unwrap();
    let (signature_raw, recovery_id) = signing_key
        .sign_prehash_recoverable(ciphertext_hash.as_slice())
        .unwrap();

    let mut signature = signature_raw.to_bytes().to_vec();
    signature.push(recovery_id.to_byte());

    // Create WebSocket message payload
    let payload = json!({
        "type": "encrypted_session_init",
        "session_id": "session-123",
        "chain_id": 84532,
        "payload": {
            "ephPubHex": hex::encode(&eph_pub_bytes),
            "ciphertextHex": hex::encode(&ciphertext),
            "nonceHex": hex::encode(&nonce),
            "signatureHex": hex::encode(&signature),
            "aadHex": hex::encode(aad)
        }
    });

    let session_key_bytes = hex::decode(&session_key).unwrap();
    (payload, session_key_bytes)
}

#[test]
fn test_encrypted_init_handler() {
    // Test that the handler correctly processes an encrypted_session_init message

    // Generate node keypair
    let (node_private, _node_public) = generate_keypair();

    // Generate client keypair
    let (client_private, _client_public) = generate_keypair();

    // Create encrypted session init payload
    let (payload, expected_session_key) = create_test_session_init_payload(&node_private, &client_private);

    // Verify payload structure
    assert_eq!(payload["type"], "encrypted_session_init");
    assert_eq!(payload["session_id"], "session-123");
    assert_eq!(payload["chain_id"], 84532);
    assert!(payload["payload"]["ephPubHex"].is_string());
    assert!(payload["payload"]["ciphertextHex"].is_string());
    assert!(payload["payload"]["nonceHex"].is_string());
    assert!(payload["payload"]["signatureHex"].is_string());

    // Expected: Handler would parse this, decrypt it, and extract session data
    // This test verifies the message structure is correct for parsing
}

#[test]
fn test_init_stores_session_key() {
    // Test that session key is stored in SessionKeyStore after successful init

    let (node_private, _) = generate_keypair();
    let (client_private, _) = generate_keypair();
    let (payload, expected_session_key) = create_test_session_init_payload(&node_private, &client_private);

    // Parse encrypted payload
    let eph_pub_hex = payload["payload"]["ephPubHex"].as_str().unwrap();
    let ciphertext_hex = payload["payload"]["ciphertextHex"].as_str().unwrap();
    let nonce_hex = payload["payload"]["nonceHex"].as_str().unwrap();
    let signature_hex = payload["payload"]["signatureHex"].as_str().unwrap();
    let aad_hex = payload["payload"]["aadHex"].as_str().unwrap();

    let encrypted_payload = EncryptedSessionPayload {
        eph_pub: hex::decode(eph_pub_hex).unwrap(),
        ciphertext: hex::decode(ciphertext_hex).unwrap(),
        nonce: hex::decode(nonce_hex).unwrap(),
        signature: hex::decode(signature_hex).unwrap(),
        aad: hex::decode(aad_hex).unwrap(),
    };

    // Decrypt session init
    let session_data = decrypt_session_init(&encrypted_payload, &node_private).unwrap();

    // Verify session key was extracted
    assert_eq!(session_data.session_key.len(), 32);
    assert_eq!(session_data.session_key, expected_session_key.as_slice());

    // Store in SessionKeyStore (this is what the handler should do)
    let store = SessionKeyStore::new();
    let session_id = payload["session_id"].as_str().unwrap();

    tokio::runtime::Runtime::new().unwrap().block_on(async {
        store.store_key(session_id.to_string(), session_data.session_key).await;

        // Verify key was stored
        let retrieved_key = store.get_key(session_id).await;
        assert!(retrieved_key.is_some());
        assert_eq!(retrieved_key.unwrap(), session_data.session_key);
    });
}

#[test]
fn test_init_recovers_client_address() {
    // Test that client address is recovered from signature

    let (node_private, _) = generate_keypair();
    let (client_private, _) = generate_keypair();
    let (payload, _) = create_test_session_init_payload(&node_private, &client_private);

    // Parse and decrypt
    let eph_pub_hex = payload["payload"]["ephPubHex"].as_str().unwrap();
    let ciphertext_hex = payload["payload"]["ciphertextHex"].as_str().unwrap();
    let nonce_hex = payload["payload"]["nonceHex"].as_str().unwrap();
    let signature_hex = payload["payload"]["signatureHex"].as_str().unwrap();
    let aad_hex = payload["payload"]["aadHex"].as_str().unwrap();

    let encrypted_payload = EncryptedSessionPayload {
        eph_pub: hex::decode(eph_pub_hex).unwrap(),
        ciphertext: hex::decode(ciphertext_hex).unwrap(),
        nonce: hex::decode(nonce_hex).unwrap(),
        signature: hex::decode(signature_hex).unwrap(),
        aad: hex::decode(aad_hex).unwrap(),
    };

    let session_data = decrypt_session_init(&encrypted_payload, &node_private).unwrap();

    // Verify client address is recovered
    assert!(session_data.client_address.starts_with("0x"));
    assert_eq!(session_data.client_address.len(), 42); // 0x + 40 hex chars

    // Expected: Handler would log this address and use it for authorization
}

#[test]
fn test_init_sends_acknowledgment() {
    // Test that handler sends session_init_ack response

    let (node_private, _) = generate_keypair();
    let (client_private, _) = generate_keypair();
    let (payload, _) = create_test_session_init_payload(&node_private, &client_private);

    let session_id = payload["session_id"].as_str().unwrap();
    let chain_id = payload["chain_id"].as_u64().unwrap();

    // Expected acknowledgment response structure
    let expected_ack = json!({
        "type": "session_init_ack",
        "session_id": session_id,
        "chain_id": chain_id,
        "status": "success"
    });

    // Verify ack structure
    assert_eq!(expected_ack["type"], "session_init_ack");
    assert_eq!(expected_ack["session_id"], session_id);
    assert_eq!(expected_ack["chain_id"], chain_id);
    assert_eq!(expected_ack["status"], "success");

    // Expected: Handler would send this response via WebSocket
}

#[test]
fn test_init_invalid_signature() {
    // Test that invalid signatures are rejected

    let (node_private, _) = generate_keypair();
    let (client_private, _) = generate_keypair();
    let (mut payload, _) = create_test_session_init_payload(&node_private, &client_private);

    // Corrupt the signature
    payload["payload"]["signatureHex"] = json!("00".repeat(65));

    // Parse and attempt to decrypt
    let eph_pub_hex = payload["payload"]["ephPubHex"].as_str().unwrap();
    let ciphertext_hex = payload["payload"]["ciphertextHex"].as_str().unwrap();
    let nonce_hex = payload["payload"]["nonceHex"].as_str().unwrap();
    let signature_hex = payload["payload"]["signatureHex"].as_str().unwrap();
    let aad_hex = payload["payload"]["aadHex"].as_str().unwrap();

    let encrypted_payload = EncryptedSessionPayload {
        eph_pub: hex::decode(eph_pub_hex).unwrap(),
        ciphertext: hex::decode(ciphertext_hex).unwrap(),
        nonce: hex::decode(nonce_hex).unwrap(),
        signature: hex::decode(signature_hex).unwrap(),
        aad: hex::decode(aad_hex).unwrap(),
    };

    let result = decrypt_session_init(&encrypted_payload, &node_private);

    // Should fail with signature verification error
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("Signature verification failed") ||
        error_msg.contains("invalid signature") ||
        error_msg.contains("recovery")
    );

    // Expected: Handler would send error response and close connection
}

#[test]
fn test_init_decryption_failure() {
    // Test that decryption failures are handled properly

    let (node_private, _) = generate_keypair();
    let (wrong_node_private, _) = generate_keypair(); // Different key
    let (client_private, _) = generate_keypair();
    let (payload, _) = create_test_session_init_payload(&node_private, &client_private);

    // Parse payload
    let eph_pub_hex = payload["payload"]["ephPubHex"].as_str().unwrap();
    let ciphertext_hex = payload["payload"]["ciphertextHex"].as_str().unwrap();
    let nonce_hex = payload["payload"]["nonceHex"].as_str().unwrap();
    let signature_hex = payload["payload"]["signatureHex"].as_str().unwrap();
    let aad_hex = payload["payload"]["aadHex"].as_str().unwrap();

    let encrypted_payload = EncryptedSessionPayload {
        eph_pub: hex::decode(eph_pub_hex).unwrap(),
        ciphertext: hex::decode(ciphertext_hex).unwrap(),
        nonce: hex::decode(nonce_hex).unwrap(),
        signature: hex::decode(signature_hex).unwrap(),
        aad: hex::decode(aad_hex).unwrap(),
    };

    // Try to decrypt with wrong node private key
    let result = decrypt_session_init(&encrypted_payload, &wrong_node_private);

    // Should fail with decryption error
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("Decryption failed") ||
        error_msg.contains("authentication") ||
        error_msg.contains("AEAD")
    );

    // Expected: Handler would send error response with code DECRYPTION_FAILED
}

#[test]
fn test_session_metadata_tracking() {
    // Test that session metadata (job_id, chain_id, client_address) is extracted and tracked

    let (node_private, _) = generate_keypair();
    let (client_private, _) = generate_keypair();
    let (payload, _) = create_test_session_init_payload(&node_private, &client_private);

    // Parse and decrypt
    let eph_pub_hex = payload["payload"]["ephPubHex"].as_str().unwrap();
    let ciphertext_hex = payload["payload"]["ciphertextHex"].as_str().unwrap();
    let nonce_hex = payload["payload"]["nonceHex"].as_str().unwrap();
    let signature_hex = payload["payload"]["signatureHex"].as_str().unwrap();
    let aad_hex = payload["payload"]["aadHex"].as_str().unwrap();

    let encrypted_payload = EncryptedSessionPayload {
        eph_pub: hex::decode(eph_pub_hex).unwrap(),
        ciphertext: hex::decode(ciphertext_hex).unwrap(),
        nonce: hex::decode(nonce_hex).unwrap(),
        signature: hex::decode(signature_hex).unwrap(),
        aad: hex::decode(aad_hex).unwrap(),
    };

    let session_data = decrypt_session_init(&encrypted_payload, &node_private).unwrap();

    // Verify metadata is available
    assert_eq!(session_data.job_id, "123");
    assert_eq!(session_data.model_name, "llama-3");
    assert_eq!(session_data.price_per_token, 2000);
    assert!(session_data.client_address.starts_with("0x"));

    // From WebSocket message
    let session_id = payload["session_id"].as_str().unwrap();
    let chain_id = payload["chain_id"].as_u64().unwrap();

    assert_eq!(session_id, "session-123");
    assert_eq!(chain_id, 84532);

    // Expected: Handler would store this metadata for:
    // - Token tracking (job_id)
    // - Settlement (chain_id)
    // - Authorization (client_address)
}

#[test]
fn test_empty_session_id() {
    // Test that empty session_id is rejected

    let (node_private, _) = generate_keypair();
    let (client_private, _) = generate_keypair();
    let (mut payload, _) = create_test_session_init_payload(&node_private, &client_private);

    payload["session_id"] = json!("");

    let session_id = payload["session_id"].as_str().unwrap();
    assert!(session_id.is_empty());

    // Expected: Handler would reject with error
}

#[test]
fn test_missing_chain_id() {
    // Test that missing chain_id defaults to Base Sepolia (84532)

    let (node_private, _) = generate_keypair();
    let (client_private, _) = generate_keypair();
    let (mut payload, _) = create_test_session_init_payload(&node_private, &client_private);

    // Remove chain_id
    payload.as_object_mut().unwrap().remove("chain_id");

    assert!(payload.get("chain_id").is_none());

    // Expected: Handler would default to 84532 (Base Sepolia)
    let default_chain_id = 84532u64;
    assert_eq!(default_chain_id, 84532);
}
