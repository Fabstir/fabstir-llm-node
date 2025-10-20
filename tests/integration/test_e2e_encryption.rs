// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! End-to-End Encryption Integration Tests (Phase 8.2)
//!
//! These tests verify the complete encryption flow from client to node,
//! simulating real SDK client behavior.

use anyhow::{anyhow, Result};
use fabstir_llm_node::crypto::{
    decrypt_session_init, decrypt_with_aead, encrypt_with_aead, recover_client_address,
    EncryptedSessionPayload, SessionInitData,
};
use hkdf::Hkdf;
use k256::ecdh::EphemeralSecret;
use k256::ecdsa::{signature::Signer, Signature, SigningKey};
use k256::elliptic_curve::rand_core::OsRng;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::{PublicKey, SecretKey};
use sha2::{Digest, Sha256};
use tiny_keccak::{Hasher, Keccak};

/// Helper to simulate client encrypting session init
fn simulate_client_session_init(
    node_public_key: &PublicKey,
    client_signing_key: &SigningKey,
) -> Result<(EncryptedSessionPayload, SessionInitData)> {
    // 1. Generate ephemeral keypair for ECDH
    let client_ephemeral = EphemeralSecret::random(&mut OsRng);
    let client_eph_pub = PublicKey::from(&client_ephemeral);
    let client_eph_pub_bytes = client_eph_pub.to_encoded_point(true); // compressed

    // 2. Perform ECDH with node's public key
    let shared_secret = client_ephemeral.diffie_hellman(node_public_key);

    // 3. Derive encryption key using HKDF-SHA256
    // This mirrors what the SDK does and what the node expects
    // Extract the shared secret bytes explicitly as a byte slice
    let shared_secret_bytes: &[u8] = shared_secret.raw_secret_bytes();

    // Apply HKDF to derive a 32-byte encryption key
    // This matches the node's derive_shared_key function (line 77 in ecdh.rs)
    let hkdf = Hkdf::<Sha256>::new(None, shared_secret_bytes);
    let mut shared_key = [0u8; 32];
    hkdf.expand(&[], &mut shared_key)
        .expect("HKDF expand should not fail with valid length");

    // 4. Create session data (client-side payload structure)
    // Note: The client sends job_id, model_name, session_key (as hex), and price_per_token
    // The client_address is NOT sent - it's recovered from the signature on the node side
    let session_key_secret = SecretKey::random(&mut OsRng);
    let session_key_bytes_generic = session_key_secret.to_bytes();
    // Convert GenericArray to [u8; 32]
    let session_key_bytes: [u8; 32] = {
        let mut key = [0u8; 32];
        key.copy_from_slice(session_key_bytes_generic.as_slice());
        key
    };

    let client_payload = serde_json::json!({
        "jobId": "42",
        "modelName": "llama-3",
        "sessionKey": hex::encode(&session_key_bytes),
        "pricePerToken": 1000
    });

    let plaintext = serde_json::to_string(&client_payload)?;

    // Store original session data for comparison (what node should decrypt to)
    let session_data = SessionInitData {
        job_id: "42".to_string(),
        model_name: "llama-3".to_string(),
        session_key: session_key_bytes,
        price_per_token: 1000,
        client_address: String::new(), // Will be filled by decrypt_session_init
    };

    // 5. Generate random nonce (24 bytes)
    let nonce = rand::random::<[u8; 24]>();

    // 6. Create AAD (optional)
    let aad = b"session_init";

    // 7. Encrypt with XChaCha20-Poly1305
    // Note: encrypt_with_aead signature is (plaintext, nonce, aad, key)
    let ciphertext = encrypt_with_aead(plaintext.as_bytes(), &nonce, aad, &shared_key)?;

    // 8. Sign the ciphertext (sign the raw data, not the hash)
    // The signing function hashes internally, but recovery expects pre-hashed message
    let ciphertext_hash = Sha256::digest(&ciphertext);
    let signature: Signature = client_signing_key.sign(&ciphertext); // Sign ciphertext, NOT hash

    // Convert signature to bytes and create 65-byte compact signature
    let signature_bytes = signature.to_bytes();
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&signature_bytes[..]);

    // Calculate expected client address for verification
    let verifying_key = client_signing_key.verifying_key();
    let client_public_key = PublicKey::from(verifying_key);
    let expected_address = {
        let pub_bytes = client_public_key.to_encoded_point(false);
        let pub_uncompressed = &pub_bytes.as_bytes()[1..]; // Skip 0x04 prefix

        let mut keccak = Keccak::v256();
        keccak.update(pub_uncompressed);
        let mut hash_out = [0u8; 32];
        keccak.finalize(&mut hash_out);
        format!("0x{}", hex::encode(&hash_out[12..]))
    };

    // Try recovery IDs 0 and 1 to find the correct one
    let mut found = false;
    for recovery_id in 0u8..2u8 {
        sig_bytes[64] = recovery_id;
        if let Ok(recovered_addr) = recover_client_address(&sig_bytes, ciphertext_hash.as_slice()) {
            if recovered_addr == expected_address {
                found = true;
                break;
            }
        }
    }

    if !found {
        return Err(anyhow!("Failed to find valid recovery ID for signature"));
    }

    // 9. Build encrypted payload
    let payload = EncryptedSessionPayload {
        eph_pub: client_eph_pub_bytes.as_bytes().to_vec(),
        ciphertext: ciphertext.clone(),
        nonce: nonce.to_vec(),
        signature: sig_bytes.to_vec(),
        aad: aad.to_vec(),
    };

    Ok((payload, session_data))
}

/// Helper to simulate client encrypting a message with session key
fn simulate_client_encrypt_message(
    session_key: &[u8],
    prompt: &str,
    message_index: usize,
) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>)> {
    // Generate random nonce
    let nonce = rand::random::<[u8; 24]>();

    // Create AAD with message index
    let aad = format!("message_{}", message_index);

    // Encrypt prompt
    // Note: encrypt_with_aead signature is (plaintext, nonce, aad, key)
    let ciphertext = encrypt_with_aead(prompt.as_bytes(), &nonce, aad.as_bytes(), session_key)?;

    Ok((ciphertext, nonce.to_vec(), aad.into_bytes()))
}

#[test]
fn test_encrypted_session_flow() {
    // Simulate complete session initialization flow

    // 1. Setup: Generate node keypair
    let node_secret = SecretKey::random(&mut OsRng);
    let node_public = node_secret.public_key();
    let node_priv_bytes = node_secret.to_bytes();

    // 2. Setup: Generate client signing keypair
    let client_signing_key = SigningKey::random(&mut OsRng);

    // 3. Client: Create encrypted session init
    let (encrypted_payload, original_session_data) =
        simulate_client_session_init(&node_public, &client_signing_key)
            .expect("Client should encrypt session init");

    // 4. Node: Decrypt session init
    let decrypted =
        decrypt_session_init(&encrypted_payload, &node_priv_bytes).expect("Node should decrypt");

    // 5. Verify: Session data matches
    assert_eq!(decrypted.job_id, original_session_data.job_id);
    assert_eq!(decrypted.model_name, original_session_data.model_name);
    assert_eq!(decrypted.session_key, original_session_data.session_key);
    assert_eq!(
        decrypted.price_per_token,
        original_session_data.price_per_token
    );

    // 6. Verify: Client address recovered
    assert!(decrypted.client_address.starts_with("0x"));
    assert_eq!(decrypted.client_address.len(), 42); // 0x + 40 hex chars
}

#[test]
fn test_encrypted_message_flow() {
    // Test encrypted message exchange after session init

    // 1. Setup: Session key (from session init)
    let session_key_hex = hex::encode(SecretKey::random(&mut OsRng).to_bytes());
    let session_key = hex::decode(&session_key_hex).unwrap();

    // 2. Client: Encrypt prompt
    let prompt = "What is the capital of France?";
    let (ciphertext, nonce, aad) = simulate_client_encrypt_message(&session_key, prompt, 0)
        .expect("Client should encrypt message");

    // 3. Node: Decrypt prompt
    let decrypted = decrypt_with_aead(&ciphertext, &nonce, &aad, &session_key)
        .expect("Node should decrypt message");

    // 4. Verify: Prompt matches
    let decrypted_prompt = String::from_utf8(decrypted).expect("Should be valid UTF-8");
    assert_eq!(decrypted_prompt, prompt);
}

#[test]
fn test_encrypted_response_flow() {
    // Test encrypted response from node to client

    // Setup: Session key
    let session_key = SecretKey::random(&mut OsRng).to_bytes();

    // Node: Encrypt response
    let response = "The capital of France is Paris.";
    let nonce = rand::random::<[u8; 24]>();
    let aad = b"response_0";

    let ciphertext = encrypt_with_aead(response.as_bytes(), &nonce, aad, &session_key)
        .expect("Node should encrypt response");

    // Client: Decrypt response
    let decrypted =
        decrypt_with_aead(&ciphertext, &nonce, aad, &session_key).expect("Client should decrypt");

    let decrypted_response = String::from_utf8(decrypted).expect("Should be valid UTF-8");
    assert_eq!(decrypted_response, response);
}

#[test]
fn test_streaming_encrypted_chunks() {
    // Test streaming with multiple encrypted chunks

    let session_key = SecretKey::random(&mut OsRng).to_bytes();

    let chunks = vec![
        "The ",
        "capital ",
        "of ",
        "France ",
        "is ",
        "Paris.",
    ];

    let mut encrypted_chunks = Vec::new();

    // Node: Encrypt each chunk
    for (index, chunk) in chunks.iter().enumerate() {
        let nonce = rand::random::<[u8; 24]>();
        let aad = format!("chunk_{}", index);

        let ciphertext = encrypt_with_aead(chunk.as_bytes(), &nonce, aad.as_bytes(), &session_key)
            .expect("Should encrypt chunk");

        encrypted_chunks.push((ciphertext, nonce, aad));
    }

    // Client: Decrypt all chunks
    let mut decrypted_chunks = Vec::new();
    for (index, (ciphertext, nonce, aad)) in encrypted_chunks.iter().enumerate() {
        let decrypted = decrypt_with_aead(ciphertext, nonce, aad.as_bytes(), &session_key)
            .expect("Should decrypt chunk");

        let chunk_text = String::from_utf8(decrypted).expect("Should be valid UTF-8");
        decrypted_chunks.push(chunk_text);

        // Verify AAD contains correct index
        assert!(aad.contains(&index.to_string()));
    }

    // Verify: Full response matches
    let full_response: String = decrypted_chunks.concat();
    assert_eq!(full_response, chunks.concat());
}

#[test]
fn test_concurrent_encrypted_sessions() {
    // Test multiple sessions with different keys

    let node_secret = SecretKey::random(&mut OsRng);
    let node_public = node_secret.public_key();
    let node_priv_bytes = node_secret.to_bytes();

    // Create 3 concurrent sessions
    let mut sessions = Vec::new();

    for i in 0..3 {
        let client_signing_key = SigningKey::random(&mut OsRng);
        let (encrypted_payload, _original_data) =
            simulate_client_session_init(&node_public, &client_signing_key)
                .expect("Should create session");

        let decrypted = decrypt_session_init(&encrypted_payload, &node_priv_bytes)
            .expect("Should decrypt session");

        sessions.push((decrypted.session_key.clone(), i));
    }

    // Verify: All session keys are unique
    let unique_keys: std::collections::HashSet<_> =
        sessions.iter().map(|(key, _)| key.clone()).collect();
    assert_eq!(unique_keys.len(), 3);

    // Verify: Each session can encrypt/decrypt independently
    for (session_key, session_id) in sessions {
        let prompt = format!("Prompt for session {}", session_id);

        let (ciphertext, nonce, aad) =
            simulate_client_encrypt_message(&session_key, &prompt, 0).expect("Should encrypt");

        let decrypted = decrypt_with_aead(&ciphertext, &nonce, &aad, &session_key)
            .expect("Should decrypt");

        let decrypted_prompt = String::from_utf8(decrypted).expect("Should be UTF-8");
        assert_eq!(decrypted_prompt, prompt);
    }
}

#[test]
fn test_session_key_isolation() {
    // Verify that session keys are isolated (wrong key fails)

    let session_key_1 = SecretKey::random(&mut OsRng).to_bytes();
    let session_key_2 = SecretKey::random(&mut OsRng).to_bytes();

    // Encrypt with session 1 key
    let prompt = "Secret message for session 1";
    let nonce = rand::random::<[u8; 24]>();
    let aad = b"message_0";

    let ciphertext = encrypt_with_aead(prompt.as_bytes(), &nonce, aad, &session_key_1)
        .expect("Should encrypt with key 1");

    // Try to decrypt with session 2 key (should fail)
    let result = decrypt_with_aead(&ciphertext, &nonce, aad, &session_key_2);
    assert!(
        result.is_err(),
        "Decryption with wrong session key should fail"
    );

    // Decrypt with correct key (should succeed)
    let decrypted = decrypt_with_aead(&ciphertext, &nonce, aad, &session_key_1)
        .expect("Decryption with correct key should succeed");

    let decrypted_prompt = String::from_utf8(decrypted).expect("Should be UTF-8");
    assert_eq!(decrypted_prompt, prompt);
}

#[test]
fn test_nonce_uniqueness_per_chunk() {
    // Verify that each chunk uses a unique nonce

    let session_key = SecretKey::random(&mut OsRng).to_bytes();

    let mut nonces = Vec::new();

    // Encrypt 10 chunks
    for i in 0..10 {
        let chunk = format!("Chunk {}", i);
        let nonce = rand::random::<[u8; 24]>();
        let aad = format!("chunk_{}", i);

        let _ciphertext = encrypt_with_aead(chunk.as_bytes(), &nonce, aad.as_bytes(), &session_key)
            .expect("Should encrypt");

        nonces.push(nonce);
    }

    // Verify: All nonces are unique
    let unique_nonces: std::collections::HashSet<_> = nonces.iter().collect();
    assert_eq!(unique_nonces.len(), 10, "All nonces should be unique");
}

#[test]
fn test_aad_validation() {
    // Verify that AAD is properly validated (tampered AAD fails)

    let session_key = SecretKey::random(&mut OsRng).to_bytes();
    let nonce = rand::random::<[u8; 24]>();
    let aad = b"message_0";

    let prompt = "Test message";
    let ciphertext = encrypt_with_aead(prompt.as_bytes(), &nonce, aad, &session_key)
        .expect("Should encrypt");

    // Try to decrypt with tampered AAD (should fail)
    let tampered_aad = b"message_1"; // Wrong AAD
    let result = decrypt_with_aead(&ciphertext, &nonce, tampered_aad, &session_key);
    assert!(result.is_err(), "Tampered AAD should fail verification");

    // Decrypt with correct AAD (should succeed)
    let decrypted = decrypt_with_aead(&ciphertext, &nonce, aad, &session_key)
        .expect("Correct AAD should succeed");

    let decrypted_prompt = String::from_utf8(decrypted).expect("Should be UTF-8");
    assert_eq!(decrypted_prompt, prompt);
}

#[test]
fn test_client_signature_recovery() {
    // Verify client address recovery from signature

    let node_secret = SecretKey::random(&mut OsRng);
    let node_public = node_secret.public_key();
    let node_priv_bytes = node_secret.to_bytes();

    let client_signing_key = SigningKey::random(&mut OsRng);

    // Get expected client address
    let verifying_key = client_signing_key.verifying_key();
    let client_public_key = PublicKey::from(verifying_key);
    let pub_bytes = client_public_key.to_encoded_point(false);
    let pub_uncompressed = &pub_bytes.as_bytes()[1..]; // Skip 0x04 prefix

    let mut hasher = Keccak::v256();
    hasher.update(pub_uncompressed);
    let mut hash_bytes = [0u8; 32];
    hasher.finalize(&mut hash_bytes);
    let expected_address = format!("0x{}", hex::encode(&hash_bytes[12..]));

    // Client encrypts and signs
    let (encrypted_payload, _) = simulate_client_session_init(&node_public, &client_signing_key)
        .expect("Should create session");

    // Node decrypts and recovers address
    let decrypted = decrypt_session_init(&encrypted_payload, &node_priv_bytes)
        .expect("Should decrypt");

    // Verify: Recovered address matches
    assert_eq!(
        decrypted.client_address, expected_address,
        "Recovered client address should match"
    );
}

#[test]
fn test_replay_attack_prevention() {
    // Verify that AAD with message index prevents replay attacks

    let session_key = SecretKey::random(&mut OsRng).to_bytes();

    // Client sends message 0
    let (ciphertext_0, nonce_0, aad_0) =
        simulate_client_encrypt_message(&session_key, "Message 0", 0).expect("Should encrypt");

    // Client sends message 1
    let (ciphertext_1, nonce_1, aad_1) =
        simulate_client_encrypt_message(&session_key, "Message 1", 1).expect("Should encrypt");

    // Node decrypts message 0
    let decrypted_0 = decrypt_with_aead(&ciphertext_0, &nonce_0, &aad_0, &session_key)
        .expect("Should decrypt message 0");
    assert_eq!(
        String::from_utf8(decrypted_0).unwrap(),
        "Message 0"
    );

    // Attacker tries to replay message 0 as message 1 (should fail AAD check)
    // This is prevented because AAD contains message index
    let result = decrypt_with_aead(&ciphertext_0, &nonce_0, &aad_1, &session_key);
    assert!(
        result.is_err(),
        "Replay with different AAD should fail"
    );

    // Node decrypts message 1 normally (should succeed)
    let decrypted_1 = decrypt_with_aead(&ciphertext_1, &nonce_1, &aad_1, &session_key)
        .expect("Should decrypt message 1");
    assert_eq!(
        String::from_utf8(decrypted_1).unwrap(),
        "Message 1"
    );
}

#[test]
fn test_session_lifecycle_complete() {
    // Test complete session lifecycle: init → messages → cleanup

    // 1. Session Init
    let node_secret = SecretKey::random(&mut OsRng);
    let node_public = node_secret.public_key();
    let node_priv_bytes = node_secret.to_bytes();

    let client_signing_key = SigningKey::random(&mut OsRng);

    let (encrypted_payload, _) = simulate_client_session_init(&node_public, &client_signing_key)
        .expect("Should init session");

    let session_data = decrypt_session_init(&encrypted_payload, &node_priv_bytes)
        .expect("Should decrypt session init");

    let session_key = &session_data.session_key;

    // 2. Exchange multiple messages
    let messages = vec![
        "Hello, node!",
        "What is 2+2?",
        "Thank you!",
    ];

    let mut responses = Vec::new();

    for (index, msg) in messages.iter().enumerate() {
        // Client: Encrypt message
        let (ciphertext, nonce, aad) =
            simulate_client_encrypt_message(session_key, msg, index).expect("Should encrypt");

        // Node: Decrypt message
        let decrypted = decrypt_with_aead(&ciphertext, &nonce, &aad, session_key)
            .expect("Should decrypt");

        let prompt = String::from_utf8(decrypted).expect("Should be UTF-8");
        assert_eq!(&prompt, msg);

        // Node: Encrypt response
        let response = format!("Response to: {}", msg);
        let response_nonce = rand::random::<[u8; 24]>();
        let response_aad = format!("response_{}", index);

        let response_ciphertext = encrypt_with_aead(
            response.as_bytes(),
            &response_nonce,
            response_aad.as_bytes(),
            session_key,
        )
        .expect("Should encrypt response");

        // Client: Decrypt response
        let decrypted_response = decrypt_with_aead(
            &response_ciphertext,
            &response_nonce,
            response_aad.as_bytes(),
            session_key,
        )
        .expect("Should decrypt response");

        let response_text = String::from_utf8(decrypted_response).expect("Should be UTF-8");
        responses.push(response_text);
    }

    // 3. Verify all responses received
    assert_eq!(responses.len(), 3);
    for (index, response) in responses.iter().enumerate() {
        assert!(response.contains(&messages[index]));
    }

    // 4. Session cleanup (in real implementation, clear session_key from memory)
    // This is simulated by dropping the session_key variable
    drop(session_key);
}

#[test]
fn test_tampered_ciphertext_rejected() {
    // Verify that tampered ciphertext fails authentication

    let session_key = SecretKey::random(&mut OsRng).to_bytes();
    let nonce = rand::random::<[u8; 24]>();
    let aad = b"message_0";

    let prompt = "Original message";
    let mut ciphertext = encrypt_with_aead(prompt.as_bytes(), &nonce, aad, &session_key)
        .expect("Should encrypt");

    // Tamper with ciphertext
    if let Some(byte) = ciphertext.get_mut(0) {
        *byte = byte.wrapping_add(1);
    }

    // Try to decrypt tampered ciphertext (should fail)
    let result = decrypt_with_aead(&ciphertext, &nonce, aad, &session_key);
    assert!(
        result.is_err(),
        "Tampered ciphertext should fail authentication"
    );
}

#[test]
fn test_empty_message_handling() {
    // Test that empty messages can be encrypted and decrypted

    let session_key = SecretKey::random(&mut OsRng).to_bytes();
    let nonce = rand::random::<[u8; 24]>();
    let aad = b"empty_message";

    let empty_message = "";
    let ciphertext = encrypt_with_aead(empty_message.as_bytes(), &nonce, aad, &session_key)
        .expect("Should encrypt empty message");

    let decrypted = decrypt_with_aead(&ciphertext, &nonce, aad, &session_key)
        .expect("Should decrypt empty message");

    let decrypted_message = String::from_utf8(decrypted).expect("Should be UTF-8");
    assert_eq!(decrypted_message, empty_message);
}

#[test]
fn test_large_message_encryption() {
    // Test encryption/decryption of large messages

    let session_key = SecretKey::random(&mut OsRng).to_bytes();
    let nonce = rand::random::<[u8; 24]>();
    let aad = b"large_message";

    // Create a large message (10KB)
    let large_message = "A".repeat(10 * 1024);

    let ciphertext = encrypt_with_aead(large_message.as_bytes(), &nonce, aad, &session_key)
        .expect("Should encrypt large message");

    let decrypted = decrypt_with_aead(&ciphertext, &nonce, aad, &session_key)
        .expect("Should decrypt large message");

    let decrypted_message = String::from_utf8(decrypted).expect("Should be UTF-8");
    assert_eq!(decrypted_message, large_message);
    assert_eq!(decrypted_message.len(), 10 * 1024);
}
