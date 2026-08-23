// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! TDD Tests for Session Init Decryption
//!
//! These tests define the expected behavior of the decrypt_session_init()
//! function BEFORE implementation. Following strict TDD methodology.

use fabstir_llm_node::crypto::{
    decrypt_session_init, derive_shared_key, encrypt_with_aead, recover_client_address,
    EncryptedSessionPayload, SessionInitData,
};
use k256::{
    ecdsa::{signature::Signer, SigningKey},
    PublicKey, SecretKey,
};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

#[test]
fn test_decrypt_session_init_valid() {
    // Generate node keypair
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();
    let node_public = node_secret.public_key();

    // Generate client ephemeral keypair
    // Note: Using SecretKey instead of EphemeralSecret to simulate both client and node sides
    let client_secret = SecretKey::random(&mut OsRng);
    let client_eph_pub = client_secret.public_key();
    let client_eph_pub_bytes = client_eph_pub.to_sec1_bytes();

    // Client performs ECDH to derive shared key
    let shared_key = derive_shared_key(
        node_public.to_sec1_bytes().as_ref(),
        &client_secret.to_bytes(),
    )
    .unwrap();

    // Create session data
    let session_data = serde_json::json!({
        "jobId": "test-job-123",
        "modelName": "llama-3",
        "sessionKey": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        "pricePerToken": 2000
    });
    let plaintext = session_data.to_string();

    // Encrypt session data
    let nonce = [1u8; 24];
    let aad = b"session-init";
    let ciphertext = encrypt_with_aead(plaintext.as_bytes(), &nonce, aad, &shared_key).unwrap();

    // Client signs the ciphertext
    let client_signing_key = SigningKey::random(&mut OsRng);
    let ciphertext_hash = Sha256::digest(&ciphertext);
    let signature: k256::ecdsa::Signature = client_signing_key.sign(&ciphertext);
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&signature.to_bytes());
    sig_bytes[64] = 0; // recovery ID

    // Create encrypted payload
    let payload = EncryptedSessionPayload {
        eph_pub: client_eph_pub_bytes.to_vec(),
        ciphertext: ciphertext.clone(),
        nonce: nonce.to_vec(),
        signature: sig_bytes.to_vec(),
        aad: aad.to_vec(),
    };

    // Node decrypts and verifies
    let result = decrypt_session_init(&payload, &node_priv_bytes);
    assert!(result.is_ok(), "Session init decryption should succeed");

    let session_init = result.unwrap();
    assert_eq!(session_init.job_id, "test-job-123");
    assert_eq!(session_init.model_name, "llama-3");
    assert_eq!(session_init.price_per_token, 2000);
    assert_eq!(session_init.session_key.len(), 32);
    assert!(session_init.client_address.starts_with("0x"));
    assert_eq!(session_init.client_address.len(), 42);
}

#[test]
fn test_session_init_round_trip() {
    // Generate keys
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();
    let node_public = node_secret.public_key();

    let client_secret = SecretKey::random(&mut OsRng);
    let client_eph_pub = client_secret.public_key();
    let client_eph_pub_bytes = client_eph_pub.to_sec1_bytes();

    // Derive shared key
    let shared_key = derive_shared_key(
        node_public.to_sec1_bytes().as_ref(),
        &client_secret.to_bytes(),
    )
    .unwrap();

    // Create and encrypt session data
    let original_session_key = [42u8; 32];
    let session_key_hex = format!("0x{}", hex::encode(original_session_key));

    let session_data = serde_json::json!({
        "jobId": "round-trip-test",
        "modelName": "test-model",
        "sessionKey": session_key_hex,
        "pricePerToken": 1500
    });

    let plaintext = session_data.to_string();
    let nonce = [99u8; 24];
    let aad = b"test-aad";
    let ciphertext = encrypt_with_aead(plaintext.as_bytes(), &nonce, aad, &shared_key).unwrap();

    // Sign
    let client_signing_key = SigningKey::random(&mut OsRng);
    let ciphertext_hash = Sha256::digest(&ciphertext);
    let signature: k256::ecdsa::Signature = client_signing_key.sign(&ciphertext);
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&signature.to_bytes());
    sig_bytes[64] = 0;

    let payload = EncryptedSessionPayload {
        eph_pub: client_eph_pub_bytes.to_vec(),
        ciphertext,
        nonce: nonce.to_vec(),
        signature: sig_bytes.to_vec(),
        aad: aad.to_vec(),
    };

    // Decrypt and verify
    let result = decrypt_session_init(&payload, &node_priv_bytes).unwrap();
    assert_eq!(result.session_key, original_session_key);
    assert_eq!(result.job_id, "round-trip-test");
    assert_eq!(result.model_name, "test-model");
    assert_eq!(result.price_per_token, 1500);
}

#[test]
fn test_signature_verification() {
    // This test verifies that the correct client address is recovered
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();
    let node_public = node_secret.public_key();

    let client_secret = SecretKey::random(&mut OsRng);
    let client_eph_pub = client_secret.public_key();
    let client_eph_pub_bytes = client_eph_pub.to_sec1_bytes();

    let shared_key = derive_shared_key(
        node_public.to_sec1_bytes().as_ref(),
        &client_secret.to_bytes(),
    )
    .unwrap();

    let session_data = serde_json::json!({
        "jobId": "sig-test",
        "modelName": "model",
        "sessionKey": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "pricePerToken": 1000
    });
    let plaintext = session_data.to_string();
    let nonce = [5u8; 24];
    let aad = b"";
    let ciphertext = encrypt_with_aead(plaintext.as_bytes(), &nonce, aad, &shared_key).unwrap();

    // Client signs with known key
    let client_signing_key = SigningKey::random(&mut OsRng);
    let ciphertext_hash = Sha256::digest(&ciphertext);
    let signature: k256::ecdsa::Signature = client_signing_key.sign(&ciphertext);
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&signature.to_bytes());

    // Try both recovery IDs to find the correct one
    let mut expected_address = None;
    for recovery_id in 0..2 {
        sig_bytes[64] = recovery_id;
        if let Ok(addr) = recover_client_address(&sig_bytes, ciphertext_hash.as_slice()) {
            expected_address = Some(addr);
            break;
        }
    }
    assert!(
        expected_address.is_some(),
        "Should recover address with valid signature"
    );

    let payload = EncryptedSessionPayload {
        eph_pub: client_eph_pub_bytes.to_vec(),
        ciphertext,
        nonce: nonce.to_vec(),
        signature: sig_bytes.to_vec(),
        aad: aad.to_vec(),
    };

    let result = decrypt_session_init(&payload, &node_priv_bytes).unwrap();
    assert_eq!(result.client_address, expected_address.unwrap());
}

#[test]
fn test_invalid_signature() {
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();
    let node_public = node_secret.public_key();

    let client_secret = SecretKey::random(&mut OsRng);
    let client_eph_pub = client_secret.public_key();
    let client_eph_pub_bytes = client_eph_pub.to_sec1_bytes();

    let shared_key = derive_shared_key(
        node_public.to_sec1_bytes().as_ref(),
        &client_secret.to_bytes(),
    )
    .unwrap();

    let session_data = serde_json::json!({
        "jobId": "invalid-sig-test",
        "modelName": "model",
        "sessionKey": "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "pricePerToken": 500
    });
    let plaintext = session_data.to_string();
    let nonce = [7u8; 24];
    let aad = b"";
    let ciphertext = encrypt_with_aead(plaintext.as_bytes(), &nonce, aad, &shared_key).unwrap();

    // Create invalid signature (all zeros)
    let invalid_signature = vec![0u8; 65];

    let payload = EncryptedSessionPayload {
        eph_pub: client_eph_pub_bytes.to_vec(),
        ciphertext,
        nonce: nonce.to_vec(),
        signature: invalid_signature,
        aad: aad.to_vec(),
    };

    let result = decrypt_session_init(&payload, &node_priv_bytes);
    assert!(result.is_err(), "Invalid signature should fail");
}

#[test]
fn test_corrupted_ciphertext() {
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();
    let node_public = node_secret.public_key();

    let client_secret = SecretKey::random(&mut OsRng);
    let client_eph_pub = client_secret.public_key();
    let client_eph_pub_bytes = client_eph_pub.to_sec1_bytes();

    let shared_key = derive_shared_key(
        node_public.to_sec1_bytes().as_ref(),
        &client_secret.to_bytes(),
    )
    .unwrap();

    let session_data = serde_json::json!({
        "jobId": "corrupt-test",
        "modelName": "model",
        "sessionKey": "0xcccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "pricePerToken": 300
    });
    let plaintext = session_data.to_string();
    let nonce = [9u8; 24];
    let aad = b"";
    let mut ciphertext = encrypt_with_aead(plaintext.as_bytes(), &nonce, aad, &shared_key).unwrap();

    // Corrupt the ciphertext
    if ciphertext.len() > 10 {
        ciphertext[5] ^= 0xFF;
        ciphertext[10] ^= 0xFF;
    }

    let client_signing_key = SigningKey::random(&mut OsRng);
    let ciphertext_hash = Sha256::digest(&ciphertext);
    let signature: k256::ecdsa::Signature = client_signing_key.sign(&ciphertext);
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&signature.to_bytes());
    sig_bytes[64] = 0;

    let payload = EncryptedSessionPayload {
        eph_pub: client_eph_pub_bytes.to_vec(),
        ciphertext,
        nonce: nonce.to_vec(),
        signature: sig_bytes.to_vec(),
        aad: aad.to_vec(),
    };

    let result = decrypt_session_init(&payload, &node_priv_bytes);
    assert!(
        result.is_err(),
        "Corrupted ciphertext should fail decryption"
    );
}

#[test]
fn test_wrong_node_key() {
    // Generate two different node keys
    let node_secret1 = SecretKey::random(&mut OsRng);
    let node_public1 = node_secret1.public_key();

    let node_secret2 = SecretKey::random(&mut OsRng);
    let node_priv_bytes2 = node_secret2.to_bytes();

    let client_secret = SecretKey::random(&mut OsRng);
    let client_eph_pub = client_secret.public_key();
    let client_eph_pub_bytes = client_eph_pub.to_sec1_bytes();

    // Client derives key with node1's public key
    let shared_key = derive_shared_key(
        node_public1.to_sec1_bytes().as_ref(),
        &client_secret.to_bytes(),
    )
    .unwrap();

    let session_data = serde_json::json!({
        "jobId": "wrong-key-test",
        "modelName": "model",
        "sessionKey": "0xdddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        "pricePerToken": 200
    });
    let plaintext = session_data.to_string();
    let nonce = [11u8; 24];
    let aad = b"";
    let ciphertext = encrypt_with_aead(plaintext.as_bytes(), &nonce, aad, &shared_key).unwrap();

    let client_signing_key = SigningKey::random(&mut OsRng);
    let ciphertext_hash = Sha256::digest(&ciphertext);
    let signature: k256::ecdsa::Signature = client_signing_key.sign(&ciphertext);
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&signature.to_bytes());
    sig_bytes[64] = 0;

    let payload = EncryptedSessionPayload {
        eph_pub: client_eph_pub_bytes.to_vec(),
        ciphertext,
        nonce: nonce.to_vec(),
        signature: sig_bytes.to_vec(),
        aad: aad.to_vec(),
    };

    // Try to decrypt with node2's private key (wrong key)
    let result = decrypt_session_init(&payload, &node_priv_bytes2);
    assert!(
        result.is_err(),
        "Wrong node private key should fail decryption"
    );
}

#[test]
fn test_extract_session_key() {
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();
    let node_public = node_secret.public_key();

    let client_secret = SecretKey::random(&mut OsRng);
    let client_eph_pub = client_secret.public_key();
    let client_eph_pub_bytes = client_eph_pub.to_sec1_bytes();

    let shared_key = derive_shared_key(
        node_public.to_sec1_bytes().as_ref(),
        &client_secret.to_bytes(),
    )
    .unwrap();

    // Use a specific session key
    let expected_session_key = [0xAB; 32];
    let session_key_hex = format!("0x{}", hex::encode(expected_session_key));

    let session_data = serde_json::json!({
        "jobId": "key-extract-test",
        "modelName": "model",
        "sessionKey": session_key_hex,
        "pricePerToken": 100
    });
    let plaintext = session_data.to_string();
    let nonce = [13u8; 24];
    let aad = b"";
    let ciphertext = encrypt_with_aead(plaintext.as_bytes(), &nonce, aad, &shared_key).unwrap();

    let client_signing_key = SigningKey::random(&mut OsRng);
    let ciphertext_hash = Sha256::digest(&ciphertext);
    let signature: k256::ecdsa::Signature = client_signing_key.sign(&ciphertext);
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&signature.to_bytes());
    sig_bytes[64] = 0;

    let payload = EncryptedSessionPayload {
        eph_pub: client_eph_pub_bytes.to_vec(),
        ciphertext,
        nonce: nonce.to_vec(),
        signature: sig_bytes.to_vec(),
        aad: aad.to_vec(),
    };

    let result = decrypt_session_init(&payload, &node_priv_bytes).unwrap();
    assert_eq!(result.session_key, expected_session_key);
    assert_eq!(result.session_key.len(), 32);
}

#[test]
fn test_invalid_json_in_plaintext() {
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();
    let node_public = node_secret.public_key();

    let client_secret = SecretKey::random(&mut OsRng);
    let client_eph_pub = client_secret.public_key();
    let client_eph_pub_bytes = client_eph_pub.to_sec1_bytes();

    let shared_key = derive_shared_key(
        node_public.to_sec1_bytes().as_ref(),
        &client_secret.to_bytes(),
    )
    .unwrap();

    // Invalid JSON
    let invalid_json = "{ this is not valid json }";
    let nonce = [15u8; 24];
    let aad = b"";
    let ciphertext = encrypt_with_aead(invalid_json.as_bytes(), &nonce, aad, &shared_key).unwrap();

    let client_signing_key = SigningKey::random(&mut OsRng);
    let ciphertext_hash = Sha256::digest(&ciphertext);
    let signature: k256::ecdsa::Signature = client_signing_key.sign(&ciphertext);
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&signature.to_bytes());
    sig_bytes[64] = 0;

    let payload = EncryptedSessionPayload {
        eph_pub: client_eph_pub_bytes.to_vec(),
        ciphertext,
        nonce: nonce.to_vec(),
        signature: sig_bytes.to_vec(),
        aad: aad.to_vec(),
    };

    let result = decrypt_session_init(&payload, &node_priv_bytes);
    assert!(result.is_err(), "Invalid JSON should fail parsing");
}

#[test]
fn test_missing_fields_in_payload() {
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();
    let node_public = node_secret.public_key();

    let client_secret = SecretKey::random(&mut OsRng);
    let client_eph_pub = client_secret.public_key();
    let client_eph_pub_bytes = client_eph_pub.to_sec1_bytes();

    let shared_key = derive_shared_key(
        node_public.to_sec1_bytes().as_ref(),
        &client_secret.to_bytes(),
    )
    .unwrap();

    // Missing sessionKey field
    let incomplete_data = serde_json::json!({
        "jobId": "incomplete-test",
        "modelName": "model",
        "pricePerToken": 100
    });
    let plaintext = incomplete_data.to_string();
    let nonce = [17u8; 24];
    let aad = b"";
    let ciphertext = encrypt_with_aead(plaintext.as_bytes(), &nonce, aad, &shared_key).unwrap();

    let client_signing_key = SigningKey::random(&mut OsRng);
    let ciphertext_hash = Sha256::digest(&ciphertext);
    let signature: k256::ecdsa::Signature = client_signing_key.sign(&ciphertext);
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&signature.to_bytes());
    sig_bytes[64] = 0;

    let payload = EncryptedSessionPayload {
        eph_pub: client_eph_pub_bytes.to_vec(),
        ciphertext,
        nonce: nonce.to_vec(),
        signature: sig_bytes.to_vec(),
        aad: aad.to_vec(),
    };

    let result = decrypt_session_init(&payload, &node_priv_bytes);
    assert!(result.is_err(), "Missing sessionKey field should fail");
}

#[test]
fn test_decrypt_with_vector_database() {
    // Test that vector_database field is properly encrypted and decrypted
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();
    let node_public = node_secret.public_key();

    let client_secret = SecretKey::random(&mut OsRng);
    let client_eph_pub = client_secret.public_key();
    let client_eph_pub_bytes = client_eph_pub.to_sec1_bytes();

    let shared_key = derive_shared_key(
        node_public.to_sec1_bytes().as_ref(),
        &client_secret.to_bytes(),
    )
    .unwrap();

    // Create session data WITH vector_database field
    let session_data = serde_json::json!({
        "jobId": "test-job-with-vectors",
        "modelName": "llama-3",
        "sessionKey": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        "pricePerToken": 2000,
        "vectorDatabase": {
            "manifestPath": "home/vector-databases/0xABC123/my-docs/manifest.json",
            "userAddress": "0xABC123"
        }
    });
    let plaintext = session_data.to_string();

    let nonce = [99u8; 24];
    let aad = b"session-init-with-vectors";
    let ciphertext = encrypt_with_aead(plaintext.as_bytes(), &nonce, aad, &shared_key).unwrap();

    let client_signing_key = SigningKey::random(&mut OsRng);
    let ciphertext_hash = Sha256::digest(&ciphertext);
    let signature: k256::ecdsa::Signature = client_signing_key.sign(&ciphertext);
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&signature.to_bytes());
    sig_bytes[64] = 0;

    let payload = EncryptedSessionPayload {
        eph_pub: client_eph_pub_bytes.to_vec(),
        ciphertext,
        nonce: nonce.to_vec(),
        signature: sig_bytes.to_vec(),
        aad: aad.to_vec(),
    };

    // Decrypt and verify vector_database field
    let result = decrypt_session_init(&payload, &node_priv_bytes);
    assert!(
        result.is_ok(),
        "Decryption with vector_database should succeed"
    );

    let session_init = result.unwrap();
    assert_eq!(session_init.job_id, "test-job-with-vectors");
    assert_eq!(session_init.model_name, "llama-3");
    assert_eq!(session_init.price_per_token, 2000);

    // Verify vector_database field
    assert!(
        session_init.vector_database.is_some(),
        "vector_database should be present"
    );
    let vector_db = session_init.vector_database.unwrap();
    assert_eq!(
        vector_db.manifest_path,
        "home/vector-databases/0xABC123/my-docs/manifest.json"
    );
    assert_eq!(vector_db.user_address, "0xABC123");
}

#[test]
fn test_decrypt_without_vector_database_backward_compat() {
    // Test backward compatibility - session data WITHOUT vector_database field
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();
    let node_public = node_secret.public_key();

    let client_secret = SecretKey::random(&mut OsRng);
    let client_eph_pub = client_secret.public_key();
    let client_eph_pub_bytes = client_eph_pub.to_sec1_bytes();

    let shared_key = derive_shared_key(
        node_public.to_sec1_bytes().as_ref(),
        &client_secret.to_bytes(),
    )
    .unwrap();

    // Create session data WITHOUT vector_database field (old SDK format)
    let session_data = serde_json::json!({
        "jobId": "test-job-no-vectors",
        "modelName": "llama-3",
        "sessionKey": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
        "pricePerToken": 1500
    });
    let plaintext = session_data.to_string();

    let nonce = [88u8; 24];
    let aad = b"session-init-no-vectors";
    let ciphertext = encrypt_with_aead(plaintext.as_bytes(), &nonce, aad, &shared_key).unwrap();

    let client_signing_key = SigningKey::random(&mut OsRng);
    let ciphertext_hash = Sha256::digest(&ciphertext);
    let signature: k256::ecdsa::Signature = client_signing_key.sign(&ciphertext);
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&signature.to_bytes());
    sig_bytes[64] = 0;

    let payload = EncryptedSessionPayload {
        eph_pub: client_eph_pub_bytes.to_vec(),
        ciphertext,
        nonce: nonce.to_vec(),
        signature: sig_bytes.to_vec(),
        aad: aad.to_vec(),
    };

    // Decrypt and verify - should work without vector_database field
    let result = decrypt_session_init(&payload, &node_priv_bytes);
    assert!(
        result.is_ok(),
        "Decryption without vector_database should succeed (backward compat)"
    );

    let session_init = result.unwrap();
    assert_eq!(session_init.job_id, "test-job-no-vectors");
    assert_eq!(session_init.model_name, "llama-3");
    assert_eq!(session_init.price_per_token, 1500);

    // Verify vector_database is None
    assert!(
        session_init.vector_database.is_none(),
        "vector_database should be None for backward compatibility"
    );
}

/// T5.3 / interface E.2: the session-scoped `lora` field must survive the
/// encrypted session-init decrypt intact.
///
/// The trap this pins: `SessionDataJson` carries `rename_all = "camelCase"`,
/// which would map the CID field to `manifestCid` while the interface spells
/// it **`manifestCID`**. Serde would then drop it silently and the session
/// would serve the BASE model while the customer believed their fine-tune was
/// loaded. `LoraRequest` carries explicit renames for exactly that reason.
fn seal(session_data: serde_json::Value) -> (EncryptedSessionPayload, [u8; 32]) {
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();
    let node_public = node_secret.public_key();
    let client_secret = SecretKey::random(&mut OsRng);
    let client_eph_pub_bytes = client_secret.public_key().to_sec1_bytes();
    let shared_key = derive_shared_key(
        node_public.to_sec1_bytes().as_ref(),
        &client_secret.to_bytes(),
    )
    .unwrap();

    let nonce = [7u8; 24];
    let aad = b"session-init";
    let ciphertext =
        encrypt_with_aead(session_data.to_string().as_bytes(), &nonce, aad, &shared_key).unwrap();
    let signature: k256::ecdsa::Signature = SigningKey::random(&mut OsRng).sign(&ciphertext);
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&signature.to_bytes());

    let mut key = [0u8; 32];
    key.copy_from_slice(&node_priv_bytes);
    (
        EncryptedSessionPayload {
            eph_pub: client_eph_pub_bytes.to_vec(),
            ciphertext,
            nonce: nonce.to_vec(),
            signature: sig_bytes.to_vec(),
            aad: aad.to_vec(),
        },
        key,
    )
}

fn base_session_data() -> serde_json::Value {
    serde_json::json!({
        "jobId": "lora-job",
        "modelName": "qwen3.8-27b",
        "sessionKey": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        "pricePerToken": 904
    })
}

#[test]
fn session_init_carries_the_e2_lora_field() {
    let mut data = base_session_data();
    data["lora"] = serde_json::json!({
        "manifestCID": "u-capability-cid",
        "manifestSha256": "0xabc",
        "file": "adapter.gguf",
    });
    let (payload, node_key) = seal(data);

    let session_init = decrypt_session_init(&payload, &node_key).expect("decrypts");
    let lora = session_init
        .lora
        .expect("the E.2 lora field must survive the decrypt");
    assert_eq!(lora.manifest_cid, "u-capability-cid");
    assert_eq!(lora.manifest_sha256, "0xabc");
    assert_eq!(lora.file, "adapter.gguf");
}

#[test]
fn an_ordinary_session_init_carries_no_lora() {
    let (payload, node_key) = seal(base_session_data());
    let session_init = decrypt_session_init(&payload, &node_key).expect("decrypts");
    assert!(
        session_init.lora.is_none(),
        "serve-back must be opt-in; every ordinary session stays on the base model"
    );
}

#[test]
fn a_miscased_manifest_cid_is_refused_rather_than_silently_dropped() {
    // `manifestCid` is what a `rename_all = "camelCase"` would emit. It must
    // FAIL the session init loudly. Silently ignoring it would stage nothing
    // and answer the customer's prompts from the base model on a paid session.
    let mut data = base_session_data();
    data["lora"] = serde_json::json!({
        "manifestCid": "u-capability-cid",
        "manifestSha256": "0xabc",
        "file": "adapter.gguf",
    });
    let (payload, node_key) = seal(data);
    assert!(
        decrypt_session_init(&payload, &node_key).is_err(),
        "a misspelled lora.manifestCID must fail the session init, not vanish"
    );
}
