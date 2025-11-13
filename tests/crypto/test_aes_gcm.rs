// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// Tests for AES-GCM decryption (Sub-phase 2.3)
// Format matches Web Crypto API's AES-GCM used by SDK

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};

#[cfg(test)]
mod aes_gcm_tests {
    use super::*;

    /// Helper: Encrypt with Web Crypto API format for testing
    /// Format: [nonce (12 bytes) | ciphertext+tag]
    fn encrypt_web_crypto_format(plaintext: &str, key: &[u8]) -> Vec<u8> {
        // Generate a random 12-byte nonce
        let nonce_bytes = [1u8; 12]; // Fixed nonce for deterministic tests
        let nonce = Nonce::from_slice(&nonce_bytes);

        let cipher = Aes256Gcm::new_from_slice(key).unwrap();
        let ciphertext = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: b"", // No additional data (matches Web Crypto API default)
                },
            )
            .unwrap();

        // Concatenate: nonce + ciphertext+tag
        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);
        result
    }

    /// Test 1: Successful decryption with correct key
    #[test]
    fn test_decrypt_aes_gcm_success() {
        use fabstir_llm_node::crypto::aes_gcm::decrypt_aes_gcm;

        let key = [0u8; 32]; // 256-bit key
        let plaintext = "Hello, S5 Vector Database!";

        // Encrypt with Web Crypto API format
        let encrypted = encrypt_web_crypto_format(plaintext, &key);

        // Decrypt
        let result = decrypt_aes_gcm(&encrypted, &key);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), plaintext);
    }

    /// Test 2: Decrypt JSON manifest
    #[test]
    fn test_decrypt_json_manifest() {
        use fabstir_llm_node::crypto::aes_gcm::decrypt_aes_gcm;

        let key = [1u8; 32];
        let json_plaintext = r#"{"name":"my-docs","owner":"0xABC","dimensions":384}"#;

        let encrypted = encrypt_web_crypto_format(json_plaintext, &key);

        let result = decrypt_aes_gcm(&encrypted, &key);
        assert!(result.is_ok());
        let decrypted = result.unwrap();
        assert_eq!(decrypted, json_plaintext);

        // Verify it's valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&decrypted).unwrap();
        assert_eq!(parsed["name"], "my-docs");
        assert_eq!(parsed["dimensions"], 384);
    }

    /// Test 3: Decryption with wrong key fails
    #[test]
    fn test_decrypt_wrong_key() {
        use fabstir_llm_node::crypto::aes_gcm::decrypt_aes_gcm;

        let correct_key = [0u8; 32];
        let wrong_key = [1u8; 32];
        let plaintext = "Secret data";

        let encrypted = encrypt_web_crypto_format(plaintext, &correct_key);

        // Try to decrypt with wrong key
        let result = decrypt_aes_gcm(&encrypted, &wrong_key);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("authentication"));
    }

    /// Test 4: Corrupted ciphertext fails authentication
    #[test]
    fn test_decrypt_corrupted_ciphertext() {
        use fabstir_llm_node::crypto::aes_gcm::decrypt_aes_gcm;

        let key = [0u8; 32];
        let plaintext = "Data to corrupt";

        let mut encrypted = encrypt_web_crypto_format(plaintext, &key);

        // Corrupt one byte in the ciphertext (after nonce)
        if encrypted.len() > 12 {
            encrypted[15] ^= 0xFF;
        }

        // Decryption should fail
        let result = decrypt_aes_gcm(&encrypted, &key);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("authentication"));
    }

    /// Test 5: Corrupted nonce fails decryption
    #[test]
    fn test_decrypt_corrupted_nonce() {
        use fabstir_llm_node::crypto::aes_gcm::decrypt_aes_gcm;

        let key = [0u8; 32];
        let plaintext = "Data with corrupted nonce";

        let mut encrypted = encrypt_web_crypto_format(plaintext, &key);

        // Corrupt one byte in the nonce
        encrypted[5] ^= 0xFF;

        // Decryption should fail (wrong nonce)
        let result = decrypt_aes_gcm(&encrypted, &key);
        assert!(result.is_err());
    }

    /// Test 6: Too short data (less than 12 bytes for nonce)
    #[test]
    fn test_decrypt_too_short() {
        use fabstir_llm_node::crypto::aes_gcm::decrypt_aes_gcm;

        let key = [0u8; 32];
        let encrypted = vec![1, 2, 3, 4, 5]; // Only 5 bytes, need at least 12

        let result = decrypt_aes_gcm(&encrypted, &key);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("12 bytes"));
    }

    /// Test 7: Invalid key size
    #[test]
    fn test_decrypt_invalid_key_size() {
        use fabstir_llm_node::crypto::aes_gcm::decrypt_aes_gcm;

        let short_key = [0u8; 16]; // Only 128-bit, need 256-bit
        let plaintext = "Test data";

        // Create valid encrypted data with correct key
        let correct_key = [0u8; 32];
        let encrypted = encrypt_web_crypto_format(plaintext, &correct_key);

        // Try to decrypt with wrong key size
        let result = decrypt_aes_gcm(&encrypted, &short_key);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("key size"));
    }

    /// Test 8: UTF-8 conversion from decrypted bytes
    #[test]
    fn test_decrypt_utf8_conversion() {
        use fabstir_llm_node::crypto::aes_gcm::decrypt_aes_gcm;

        let key = [0u8; 32];
        let plaintext = "UTF-8 test: Hello 世界 🚀";

        let encrypted = encrypt_web_crypto_format(plaintext, &key);

        let result = decrypt_aes_gcm(&encrypted, &key);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), plaintext);
    }

    /// Test 9: Invalid UTF-8 in decrypted data
    #[test]
    fn test_decrypt_invalid_utf8() {
        use fabstir_llm_node::crypto::aes_gcm::decrypt_aes_gcm;

        let key = [2u8; 32];

        // Create encrypted data that will decrypt to invalid UTF-8
        // We'll encrypt raw bytes that aren't valid UTF-8
        let invalid_utf8_bytes = vec![0xFF, 0xFE, 0xFD]; // Invalid UTF-8 sequence
        let nonce_bytes = [1u8; 12];
        let nonce = Nonce::from_slice(&nonce_bytes);

        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let ciphertext = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: &invalid_utf8_bytes,
                    aad: b"",
                },
            )
            .unwrap();

        let mut encrypted = Vec::with_capacity(12 + ciphertext.len());
        encrypted.extend_from_slice(&nonce_bytes);
        encrypted.extend_from_slice(&ciphertext);

        // Decryption should fail at UTF-8 conversion
        let result = decrypt_aes_gcm(&encrypted, &key);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("UTF-8"));
    }

    /// Test 10: Large JSON chunk decryption (simulates vector chunk)
    #[test]
    fn test_decrypt_large_chunk() {
        use fabstir_llm_node::crypto::aes_gcm::decrypt_aes_gcm;

        let key = [3u8; 32];

        // Create a large JSON structure (simulating vector chunk)
        let vectors: Vec<serde_json::Value> = (0..100)
            .map(|i| {
                serde_json::json!({
                    "id": format!("vec{}", i),
                    "vector": vec![0.1; 384],
                    "metadata": {"source": "test.pdf", "page": i}
                })
            })
            .collect();

        let chunk_json = serde_json::json!({
            "chunkId": 0,
            "vectors": vectors
        });

        let plaintext = serde_json::to_string(&chunk_json).unwrap();

        let encrypted = encrypt_web_crypto_format(&plaintext, &key);

        // Decrypt
        let result = decrypt_aes_gcm(&encrypted, &key);
        assert!(result.is_ok());
        let decrypted = result.unwrap();

        // Verify it's valid JSON and matches original
        let parsed: serde_json::Value = serde_json::from_str(&decrypted).unwrap();
        assert_eq!(parsed["chunkId"], 0);
        assert_eq!(parsed["vectors"].as_array().unwrap().len(), 100);
    }

    /// Test 11: Nonce extraction function
    #[test]
    fn test_extract_nonce() {
        use fabstir_llm_node::crypto::aes_gcm::extract_nonce;

        let encrypted = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

        let result = extract_nonce(&encrypted);
        assert!(result.is_ok());
        let nonce = result.unwrap();
        assert_eq!(nonce.len(), 12);
        assert_eq!(nonce, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    }

    /// Test 12: Nonce extraction from too-short data
    #[test]
    fn test_extract_nonce_too_short() {
        use fabstir_llm_node::crypto::aes_gcm::extract_nonce;

        let encrypted = vec![1, 2, 3]; // Only 3 bytes

        let result = extract_nonce(&encrypted);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("12 bytes"));
    }

    /// Test 13: Empty ciphertext after nonce
    #[test]
    fn test_decrypt_empty_ciphertext() {
        use fabstir_llm_node::crypto::aes_gcm::decrypt_aes_gcm;

        let key = [0u8; 32];
        let encrypted = vec![0u8; 12]; // Only nonce, no ciphertext

        let result = decrypt_aes_gcm(&encrypted, &key);
        assert!(result.is_err());
    }

    /// Test 14: Round-trip with real session key derivation
    #[test]
    fn test_decrypt_with_derived_key() {
        use fabstir_llm_node::crypto::aes_gcm::decrypt_aes_gcm;
        use sha2::{Digest, Sha256};

        // Simulate deriving a key from session key (like SDK does)
        let session_key = b"test-session-key-12345678901234567890";
        let mut hasher = Sha256::new();
        hasher.update(session_key);
        let key = hasher.finalize();

        let plaintext = "Data encrypted with derived key";
        let encrypted = encrypt_web_crypto_format(plaintext, &key);

        let result = decrypt_aes_gcm(&encrypted, &key);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), plaintext);
    }

    /// Test 15: Web Crypto API format validation
    #[test]
    fn test_web_crypto_api_format() {
        use fabstir_llm_node::crypto::aes_gcm::decrypt_aes_gcm;

        let key = [0u8; 32];
        let plaintext = "Format: [12-byte nonce][ciphertext+16-byte tag]";

        let encrypted = encrypt_web_crypto_format(plaintext, &key);

        // Verify format: should be at least 12 (nonce) + 16 (tag) = 28 bytes
        assert!(encrypted.len() >= 28);

        // First 12 bytes should be nonce
        let nonce_part = &encrypted[0..12];
        assert_eq!(nonce_part, &[1u8; 12]); // Our test nonce

        // Decrypt and verify
        let result = decrypt_aes_gcm(&encrypted, &key);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), plaintext);
    }
}
