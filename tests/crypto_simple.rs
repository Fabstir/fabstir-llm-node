//! Simple crypto tests that can run without linking issues

use fabstir_llm_node::crypto::{decrypt_with_aead, derive_shared_key, encrypt_with_aead};
use k256::{ecdh::EphemeralSecret, PublicKey, SecretKey};
use rand::{rngs::OsRng, RngCore};

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
