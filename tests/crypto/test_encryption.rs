//! TDD Tests for XChaCha20-Poly1305 Encryption/Decryption
//!
//! These tests define the expected behavior of the AEAD encryption
//! functions BEFORE implementation. Following strict TDD methodology.

use fabstir_llm_node::crypto::{decrypt_with_aead, encrypt_with_aead};
use rand::{rngs::OsRng, RngCore};

#[test]
fn test_encrypt_decrypt_roundtrip() {
    // Generate test data
    let plaintext = b"Hello, World! This is a test message.";
    let mut key = [0u8; 32];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut key);
    OsRng.fill_bytes(&mut nonce);
    let aad = b"additional authenticated data";

    // Encrypt
    let ciphertext = encrypt_with_aead(plaintext, &nonce, aad, &key);
    assert!(ciphertext.is_ok(), "Encryption should succeed");
    let ciphertext = ciphertext.unwrap();

    // Ciphertext should be different from plaintext
    assert_ne!(&ciphertext[..plaintext.len()], plaintext);

    // Decrypt
    let decrypted = decrypt_with_aead(&ciphertext, &nonce, aad, &key);
    assert!(decrypted.is_ok(), "Decryption should succeed");
    let decrypted = decrypted.unwrap();

    // Should recover original plaintext
    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_decryption_with_aad() {
    // Generate test data
    let plaintext = b"Secret message";
    let mut key = [0u8; 32];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut key);
    OsRng.fill_bytes(&mut nonce);
    let aad = b"session-id-12345";

    // Encrypt with AAD
    let ciphertext = encrypt_with_aead(plaintext, &nonce, aad, &key).unwrap();

    // Decrypt with correct AAD - should succeed
    let result = decrypt_with_aead(&ciphertext, &nonce, aad, &key);
    assert!(result.is_ok(), "Decryption with correct AAD should succeed");

    // Decrypt with wrong AAD - should fail
    let wrong_aad = b"wrong-session-id";
    let result = decrypt_with_aead(&ciphertext, &nonce, wrong_aad, &key);
    assert!(
        result.is_err(),
        "Decryption with wrong AAD should fail (authentication error)"
    );
}

#[test]
fn test_invalid_nonce_size() {
    let plaintext = b"test";
    let key = [0u8; 32];
    let aad = b"";

    // Nonce too short (should be 24 bytes)
    let short_nonce = [0u8; 12];
    let result = encrypt_with_aead(plaintext, &short_nonce, aad, &key);
    assert!(result.is_err(), "Should reject nonce that's too short");

    // Nonce too long
    let long_nonce = [0u8; 32];
    let result = encrypt_with_aead(plaintext, &long_nonce, aad, &key);
    assert!(result.is_err(), "Should reject nonce that's too long");
}

#[test]
fn test_invalid_key_size() {
    let plaintext = b"test";
    let nonce = [0u8; 24];
    let aad = b"";

    // Key too short (should be 32 bytes)
    let short_key = [0u8; 16];
    let result = encrypt_with_aead(plaintext, &nonce, aad, &short_key);
    assert!(result.is_err(), "Should reject key that's too short");

    // Key too long
    let long_key = [0u8; 64];
    let result = encrypt_with_aead(plaintext, &nonce, aad, &long_key);
    assert!(result.is_err(), "Should reject key that's too long");
}

#[test]
fn test_authentication_failure() {
    // Generate test data
    let plaintext = b"Authenticated message";
    let mut key = [0u8; 32];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut key);
    OsRng.fill_bytes(&mut nonce);
    let aad = b"";

    // Encrypt
    let ciphertext = encrypt_with_aead(plaintext, &nonce, aad, &key).unwrap();

    // Decrypt with wrong key - should fail authentication
    let mut wrong_key = [0u8; 32];
    OsRng.fill_bytes(&mut wrong_key);
    let result = decrypt_with_aead(&ciphertext, &nonce, aad, &wrong_key);
    assert!(
        result.is_err(),
        "Decryption with wrong key should fail authentication"
    );
}

#[test]
fn test_tampered_ciphertext() {
    // Generate test data
    let plaintext = b"Important message";
    let mut key = [0u8; 32];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut key);
    OsRng.fill_bytes(&mut nonce);
    let aad = b"";

    // Encrypt
    let mut ciphertext = encrypt_with_aead(plaintext, &nonce, aad, &key).unwrap();

    // Tamper with ciphertext
    if ciphertext.len() > 10 {
        ciphertext[5] ^= 0xFF; // Flip bits in the middle
    }

    // Decryption should fail due to authentication tag mismatch
    let result = decrypt_with_aead(&ciphertext, &nonce, aad, &key);
    assert!(
        result.is_err(),
        "Tampered ciphertext should fail authentication"
    );
}

#[test]
fn test_wrong_key_decryption() {
    // Generate test data
    let plaintext = b"Encrypted data";
    let mut key1 = [0u8; 32];
    let mut key2 = [0u8; 32];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut key1);
    OsRng.fill_bytes(&mut key2);
    OsRng.fill_bytes(&mut nonce);
    let aad = b"";

    // Encrypt with key1
    let ciphertext = encrypt_with_aead(plaintext, &nonce, aad, &key1).unwrap();

    // Try to decrypt with key2 - should fail
    let result = decrypt_with_aead(&ciphertext, &nonce, aad, &key2);
    assert!(result.is_err(), "Wrong key should fail decryption");
}

#[test]
fn test_empty_plaintext() {
    // Should be able to encrypt/decrypt empty messages
    let plaintext = b"";
    let mut key = [0u8; 32];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut key);
    OsRng.fill_bytes(&mut nonce);
    let aad = b"metadata";

    let ciphertext = encrypt_with_aead(plaintext, &nonce, aad, &key);
    assert!(ciphertext.is_ok(), "Should handle empty plaintext");

    let ciphertext = ciphertext.unwrap();
    let decrypted = decrypt_with_aead(&ciphertext, &nonce, aad, &key).unwrap();
    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_large_plaintext() {
    // Test with larger data (1 MB)
    let plaintext = vec![42u8; 1024 * 1024];
    let mut key = [0u8; 32];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut key);
    OsRng.fill_bytes(&mut nonce);
    let aad = b"";

    let ciphertext = encrypt_with_aead(&plaintext, &nonce, aad, &key);
    assert!(ciphertext.is_ok(), "Should handle large plaintext");

    let ciphertext = ciphertext.unwrap();
    let decrypted = decrypt_with_aead(&ciphertext, &nonce, aad, &key).unwrap();
    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_different_nonces_different_ciphertext() {
    // Same plaintext + key but different nonces should produce different ciphertexts
    let plaintext = b"Same message";
    let mut key = [0u8; 32];
    let mut nonce1 = [0u8; 24];
    let mut nonce2 = [0u8; 24];
    OsRng.fill_bytes(&mut key);
    OsRng.fill_bytes(&mut nonce1);
    OsRng.fill_bytes(&mut nonce2);
    let aad = b"";

    let ciphertext1 = encrypt_with_aead(plaintext, &nonce1, aad, &key).unwrap();
    let ciphertext2 = encrypt_with_aead(plaintext, &nonce2, aad, &key).unwrap();

    // Ciphertexts should be different (excluding auth tag)
    assert_ne!(
        ciphertext1, ciphertext2,
        "Different nonces should produce different ciphertexts"
    );

    // But both should decrypt to same plaintext
    let decrypted1 = decrypt_with_aead(&ciphertext1, &nonce1, aad, &key).unwrap();
    let decrypted2 = decrypt_with_aead(&ciphertext2, &nonce2, aad, &key).unwrap();
    assert_eq!(decrypted1, plaintext);
    assert_eq!(decrypted2, plaintext);
}

#[test]
fn test_empty_aad() {
    // AAD is optional - empty AAD should work
    let plaintext = b"Message without AAD";
    let mut key = [0u8; 32];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut key);
    OsRng.fill_bytes(&mut nonce);

    // Encrypt with empty AAD
    let ciphertext = encrypt_with_aead(plaintext, &nonce, b"", &key).unwrap();

    // Decrypt with empty AAD
    let decrypted = decrypt_with_aead(&ciphertext, &nonce, b"", &key).unwrap();
    assert_eq!(decrypted, plaintext);

    // Decrypt with non-empty AAD should fail
    let result = decrypt_with_aead(&ciphertext, &nonce, b"some-aad", &key);
    assert!(result.is_err(), "AAD mismatch should fail");
}

#[test]
fn test_ciphertext_includes_auth_tag() {
    // Ciphertext should be longer than plaintext (includes 16-byte auth tag)
    let plaintext = b"Test message";
    let mut key = [0u8; 32];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut key);
    OsRng.fill_bytes(&mut nonce);
    let aad = b"";

    let ciphertext = encrypt_with_aead(plaintext, &nonce, aad, &key).unwrap();

    // XChaCha20-Poly1305 appends 16-byte authentication tag
    assert_eq!(
        ciphertext.len(),
        plaintext.len() + 16,
        "Ciphertext should be plaintext + 16-byte auth tag"
    );
}

#[test]
fn test_deterministic_encryption() {
    // Same inputs should produce same output (deterministic for same nonce)
    let plaintext = b"Deterministic test";
    let key = [42u8; 32];
    let nonce = [99u8; 24];
    let aad = b"test-aad";

    let ciphertext1 = encrypt_with_aead(plaintext, &nonce, aad, &key).unwrap();
    let ciphertext2 = encrypt_with_aead(plaintext, &nonce, aad, &key).unwrap();

    assert_eq!(
        ciphertext1, ciphertext2,
        "Same inputs should produce same ciphertext"
    );
}
