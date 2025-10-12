//! Simple crypto tests that can run without linking issues

use fabstir_llm_node::crypto::{
    decrypt_session_init, decrypt_with_aead, derive_shared_key, encrypt_with_aead,
    recover_client_address, EncryptedSessionPayload,
};
use k256::{
    ecdh::EphemeralSecret,
    ecdsa::{SigningKey, signature::Signer},
    elliptic_curve::sec1::ToEncodedPoint,
    PublicKey,
    SecretKey,
};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use tiny_keccak::{Hasher, Keccak};

#[test]
fn test_ecdh_basic() {
    // Generate test keys
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();

    let client_ephemeral = EphemeralSecret::random(&mut OsRng);
    let client_eph_pub = PublicKey::from(&client_ephemeral);
    let client_eph_pub_bytes = client_eph_pub.to_sec1_bytes();

    // Derive shared key
    let result = derive_shared_key(&client_eph_pub_bytes, &node_priv_bytes);

    assert!(result.is_ok(), "ECDH derivation should succeed");
    let key = result.unwrap();
    assert_eq!(key.len(), 32, "Shared key must be 32 bytes");
}

#[test]
fn test_ecdh_deterministic() {
    // Generate keys once
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();

    let client_ephemeral = EphemeralSecret::random(&mut OsRng);
    let client_eph_pub = PublicKey::from(&client_ephemeral);
    let client_eph_pub_bytes = client_eph_pub.to_sec1_bytes();

    // Derive twice
    let key1 = derive_shared_key(&client_eph_pub_bytes, &node_priv_bytes);
    let key2 = derive_shared_key(&client_eph_pub_bytes, &node_priv_bytes);

    assert!(key1.is_ok() && key2.is_ok(), "Derivations should succeed");
    assert_eq!(key1.unwrap(), key2.unwrap(), "Should be deterministic");
}

#[test]
fn test_encryption_basic() {
    // Generate test data
    let plaintext = b"Hello, encryption!";
    let mut key = [0u8; 32];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut key);
    OsRng.fill_bytes(&mut nonce);
    let aad = b"test-aad";

    // Encrypt
    let ciphertext = encrypt_with_aead(plaintext, &nonce, aad, &key);
    assert!(ciphertext.is_ok(), "Encryption should succeed");
    let ciphertext = ciphertext.unwrap();

    // Decrypt
    let decrypted = decrypt_with_aead(&ciphertext, &nonce, aad, &key);
    assert!(decrypted.is_ok(), "Decryption should succeed");
    assert_eq!(decrypted.unwrap(), plaintext, "Should decrypt to original");
}

#[test]
fn test_encryption_wrong_key() {
    let plaintext = b"Secret";
    let mut key1 = [0u8; 32];
    let mut key2 = [0u8; 32];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut key1);
    OsRng.fill_bytes(&mut key2);
    OsRng.fill_bytes(&mut nonce);

    let ciphertext = encrypt_with_aead(plaintext, &nonce, b"", &key1).unwrap();
    let result = decrypt_with_aead(&ciphertext, &nonce, b"", &key2);
    assert!(result.is_err(), "Wrong key should fail");
}

#[test]
fn test_signature_recovery_basic() {
    // Helper to create Ethereum address from public key
    fn pubkey_to_address(public_key: &k256::PublicKey) -> String {
        let encoded_point = public_key.to_encoded_point(false);
        let uncompressed = encoded_point.as_bytes();

        let mut hasher = Keccak::v256();
        let mut hash = [0u8; 32];
        hasher.update(&uncompressed[1..]); // Skip 0x04 prefix
        hasher.finalize(&mut hash);

        let address_bytes = &hash[12..];
        format!("0x{}", hex::encode(address_bytes))
    }

    // Generate test keypair
    let signing_key = SigningKey::random(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    let public_key = k256::PublicKey::from(verifying_key);
    let expected_address = pubkey_to_address(&public_key);

    // Sign a message
    let message = b"test signature recovery";
    let message_hash = Sha256::digest(message);
    let signature: k256::ecdsa::Signature = signing_key.sign(message);
    let signature_bytes = signature.to_bytes();

    // Create compact signature with recovery ID
    let mut compact_sig = [0u8; 65];
    compact_sig[..64].copy_from_slice(&signature_bytes[..]);

    // Try both recovery IDs to find the correct one
    for recovery_id in 0..2 {
        compact_sig[64] = recovery_id;

        if let Ok(recovered_address) = recover_client_address(&compact_sig, message_hash.as_slice()) {
            if recovered_address == expected_address {
                // Success!
                assert_eq!(recovered_address.len(), 42, "Address should be 42 chars");
                assert!(recovered_address.starts_with("0x"), "Address should start with 0x");
                return;
            }
        }
    }

    panic!("Failed to recover correct address with either recovery ID");
}

#[test]
fn test_signature_invalid_size() {
    let short_sig = [0u8; 32];
    let message_hash = Sha256::digest(b"test");

    let result = recover_client_address(&short_sig, message_hash.as_slice());
    assert!(result.is_err(), "Should reject invalid signature size");
    assert!(result.unwrap_err().to_string().contains("65 bytes"));
}

#[test]
fn test_session_init_integration() {
    // Helper to create Ethereum address from public key
    fn pubkey_to_address(public_key: &k256::PublicKey) -> String {
        let encoded_point = public_key.to_encoded_point(false);
        let uncompressed = encoded_point.as_bytes();

        let mut hasher = tiny_keccak::Keccak::v256();
        let mut hash = [0u8; 32];
        hasher.update(&uncompressed[1..]); // Skip 0x04 prefix
        hasher.finalize(&mut hash);

        let address_bytes = &hash[12..];
        format!("0x{}", hex::encode(address_bytes))
    }

    // Generate node keypair
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();
    let node_public = node_secret.public_key();

    // Generate client ephemeral keypair
    // Note: In a real scenario, client would use diffie_hellman() directly
    // Here we simulate both sides by using a SecretKey for the client too
    let client_secret = SecretKey::random(&mut OsRng);
    let client_eph_pub = client_secret.public_key();
    let client_eph_pub_bytes = client_eph_pub.to_sec1_bytes();

    // Client derives shared key (simulating client-side ECDH)
    let shared_key = derive_shared_key(node_public.to_sec1_bytes().as_ref(), &client_secret.to_bytes()).unwrap();

    // Create session data JSON
    let session_data = serde_json::json!({
        "jobId": "integration-test-job-456",
        "modelName": "llama-3-8b",
        "sessionKey": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
        "pricePerToken": 3500
    });
    let plaintext = session_data.to_string();

    // Encrypt with XChaCha20-Poly1305
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut nonce);
    let aad = b"integration-test-aad";
    let ciphertext = encrypt_with_aead(plaintext.as_bytes(), &nonce, aad, &shared_key).unwrap();

    // Client signs the ciphertext
    let client_signing_key = SigningKey::random(&mut OsRng);
    let client_verifying_key = client_signing_key.verifying_key();
    let client_public_key = k256::PublicKey::from(client_verifying_key);
    let expected_client_address = pubkey_to_address(&client_public_key);

    let ciphertext_hash = Sha256::digest(&ciphertext);
    let signature: k256::ecdsa::Signature = client_signing_key.sign(&ciphertext);
    let signature_bytes = signature.to_bytes();

    // Try both recovery IDs to find the correct one
    let mut correct_sig = None;
    for recovery_id in 0..2 {
        let mut compact_sig = [0u8; 65];
        compact_sig[..64].copy_from_slice(&signature_bytes[..]);
        compact_sig[64] = recovery_id;

        if let Ok(addr) = recover_client_address(&compact_sig, ciphertext_hash.as_slice()) {
            if addr == expected_client_address {
                correct_sig = Some(compact_sig);
                break;
            }
        }
    }

    let signature_with_recovery = correct_sig.expect("Should find correct recovery ID");

    // Create encrypted payload
    let payload = EncryptedSessionPayload {
        eph_pub: client_eph_pub_bytes.to_vec(),
        ciphertext: ciphertext.clone(),
        nonce: nonce.to_vec(),
        signature: signature_with_recovery.to_vec(),
        aad: aad.to_vec(),
    };

    // Node decrypts and verifies
    let result = decrypt_session_init(&payload, &node_priv_bytes);
    assert!(result.is_ok(), "Session init decryption should succeed");

    let session_init = result.unwrap();
    assert_eq!(session_init.job_id, "integration-test-job-456");
    assert_eq!(session_init.model_name, "llama-3-8b");
    assert_eq!(session_init.price_per_token, 3500);
    assert_eq!(session_init.session_key.len(), 32);
    assert_eq!(
        hex::encode(&session_init.session_key),
        "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
    );
    assert_eq!(session_init.client_address, expected_client_address);
    assert!(session_init.client_address.starts_with("0x"));
    assert_eq!(session_init.client_address.len(), 42);
}

#[test]
fn test_session_init_invalid_signature() {
    // Generate node keypair
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();
    let node_public = node_secret.public_key();

    // Generate client ephemeral keypair
    // Note: In a real scenario, client would use diffie_hellman() directly
    // Here we simulate both sides by using a SecretKey for the client too
    let client_secret = SecretKey::random(&mut OsRng);
    let client_eph_pub = client_secret.public_key();
    let client_eph_pub_bytes = client_eph_pub.to_sec1_bytes();

    // Client derives shared key (simulating client-side ECDH)
    let shared_key = derive_shared_key(node_public.to_sec1_bytes().as_ref(), &client_secret.to_bytes()).unwrap();

    // Create session data JSON
    let session_data = serde_json::json!({
        "jobId": "test-job",
        "modelName": "llama-3",
        "sessionKey": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        "pricePerToken": 2000
    });
    let plaintext = session_data.to_string();

    // Encrypt
    let nonce = [1u8; 24];
    let aad = b"test-aad";
    let ciphertext = encrypt_with_aead(plaintext.as_bytes(), &nonce, aad, &shared_key).unwrap();

    // Create invalid signature (all zeros)
    let invalid_signature = vec![0u8; 65];

    // Create encrypted payload with invalid signature
    let payload = EncryptedSessionPayload {
        eph_pub: client_eph_pub_bytes.to_vec(),
        ciphertext,
        nonce: nonce.to_vec(),
        signature: invalid_signature,
        aad: aad.to_vec(),
    };

    // Node should reject invalid signature
    let result = decrypt_session_init(&payload, &node_priv_bytes);
    assert!(result.is_err(), "Should reject invalid signature");
    assert!(result.unwrap_err().to_string().contains("Signature verification failed"));
}
