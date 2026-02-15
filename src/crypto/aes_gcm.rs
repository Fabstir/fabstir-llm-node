// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! AES-GCM Decryption for S5 Vector Databases
//!
//! Implements decryption for data encrypted with Web Crypto API's AES-GCM,
//! as used by the SDK's EncryptionManager for S5 vector database storage.
//!
//! **Encryption Format** (Web Crypto API standard):
//! ```text
//! [nonce (12 bytes) | ciphertext+tag (variable length)]
//! ```
//!
//! - Nonce: 12 bytes (96 bits) - unique per encryption
//! - Ciphertext+Tag: Encrypted data + 16-byte authentication tag
//! - Algorithm: AES-256-GCM
//! - No Additional Authenticated Data (AAD) by default

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use anyhow::{anyhow, Result};

/// Decrypt data encrypted with Web Crypto API's AES-GCM
///
/// # Format
///
/// The encrypted data must follow Web Crypto API format:
/// ```text
/// [nonce (12 bytes) | ciphertext+tag]
/// ```
///
/// # Arguments
///
/// * `encrypted` - Encrypted data (nonce + ciphertext+tag)
/// * `key` - 32-byte (256-bit) AES key
///
/// # Returns
///
/// Decrypted plaintext as UTF-8 string
///
/// # Errors
///
/// Returns error if:
/// - Encrypted data is less than 12 bytes (no nonce)
/// - Key size is not 32 bytes
/// - Authentication tag verification fails (wrong key or tampered data)
/// - Decrypted bytes are not valid UTF-8
///
/// # Example
///
/// ```rust,ignore
/// use fabstir_llm_node::crypto::aes_gcm::decrypt_aes_gcm;
///
/// let key = [0u8; 32]; // 256-bit key
/// let encrypted = vec![/* encrypted data */];
///
/// let plaintext = decrypt_aes_gcm(&encrypted, &key)?;
/// let manifest: Manifest = serde_json::from_str(&plaintext)?;
/// ```
pub fn decrypt_aes_gcm(encrypted: &[u8], key: &[u8]) -> Result<String> {
    // 1. Validate input sizes
    if encrypted.len() < 12 {
        return Err(anyhow!(
            "Encrypted data too short: expected at least 12 bytes for nonce, got {}",
            encrypted.len()
        ));
    }

    if key.len() != 32 {
        return Err(anyhow!(
            "Invalid key size: expected 32 bytes (256 bits), got {}",
            key.len()
        ));
    }

    // 2. Extract nonce (first 12 bytes)
    let nonce = Nonce::from_slice(&encrypted[0..12]);

    // 3. Extract ciphertext+tag (remaining bytes after nonce)
    let ciphertext = &encrypted[12..];

    // 4. Create cipher instance
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| anyhow!("Failed to create AES-GCM cipher: {}", e))?;

    // 5. Decrypt and verify authentication tag
    // No AAD (Additional Authenticated Data) - matches Web Crypto API default
    let plaintext_bytes = cipher
        .decrypt(
            nonce,
            Payload {
                msg: ciphertext,
                aad: b"", // Empty AAD
            },
        )
        .map_err(|e| {
            anyhow!(
                "AES-GCM decryption failed (authentication error - wrong key or corrupted data): {}",
                e
            )
        })?;

    // 6. Convert decrypted bytes to UTF-8 string
    let plaintext = String::from_utf8(plaintext_bytes)
        .map_err(|e| anyhow!("Decrypted data is not valid UTF-8: {}", e))?;

    Ok(plaintext)
}

/// Extract nonce from encrypted data
///
/// # Arguments
///
/// * `encrypted` - Encrypted data (nonce + ciphertext+tag)
///
/// # Returns
///
/// 12-byte nonce as a slice
///
/// # Errors
///
/// Returns error if encrypted data is less than 12 bytes
///
/// # Example
///
/// ```rust,ignore
/// use fabstir_llm_node::crypto::aes_gcm::extract_nonce;
///
/// let encrypted = vec![/* encrypted data */];
/// let nonce = extract_nonce(&encrypted)?;
/// assert_eq!(nonce.len(), 12);
/// ```
pub fn extract_nonce(encrypted: &[u8]) -> Result<&[u8]> {
    if encrypted.len() < 12 {
        return Err(anyhow!(
            "Cannot extract nonce: data too short (expected at least 12 bytes, got {})",
            encrypted.len()
        ));
    }

    Ok(&encrypted[0..12])
}

/// Decrypt S5 vector database manifest
///
/// Convenience wrapper around decrypt_aes_gcm that also parses JSON
///
/// # Arguments
///
/// * `encrypted` - Encrypted manifest data
/// * `key` - 32-byte AES key (derived from session key)
///
/// # Returns
///
/// Decrypted and parsed Manifest struct
///
/// # Example
///
/// ```rust,ignore
/// use fabstir_llm_node::crypto::aes_gcm::decrypt_manifest;
///
/// let encrypted_manifest = s5_client.get("path/to/manifest.json").await?;
/// let session_key = [/* session key */];
/// let manifest = decrypt_manifest(&encrypted_manifest, &session_key)?;
/// ```
pub fn decrypt_manifest(
    encrypted: &[u8],
    key: &[u8],
) -> Result<crate::storage::manifest::Manifest> {
    let json = decrypt_aes_gcm(encrypted, key)?;
    let manifest = serde_json::from_str(&json)
        .map_err(|e| anyhow!("Failed to parse decrypted manifest JSON: {}", e))?;
    Ok(manifest)
}

/// Decrypt S5 vector database chunk
///
/// Convenience wrapper around decrypt_aes_gcm that also parses JSON
///
/// # Arguments
///
/// * `encrypted` - Encrypted chunk data
/// * `key` - 32-byte AES key (derived from session key)
///
/// # Returns
///
/// Decrypted and parsed VectorChunk struct
///
/// # Example
///
/// ```rust,ignore
/// use fabstir_llm_node::crypto::aes_gcm::decrypt_chunk;
///
/// let encrypted_chunk = s5_client.get("path/to/chunk-0.json").await?;
/// let session_key = [/* session key */];
/// let chunk = decrypt_chunk(&encrypted_chunk, &session_key)?;
/// ```
pub fn decrypt_chunk(
    encrypted: &[u8],
    key: &[u8],
) -> Result<crate::storage::manifest::VectorChunk> {
    let json = decrypt_aes_gcm(encrypted, key)?;
    let chunk = serde_json::from_str(&json)
        .map_err(|e| anyhow!("Failed to parse decrypted chunk JSON: {}", e))?;
    Ok(chunk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decrypt_aes_gcm_basic() {
        // Create a simple encrypted payload manually for testing
        let key = [0u8; 32];
        let plaintext = "Hello, World!";

        // Encrypt using the same format
        let nonce_bytes = [1u8; 12];
        let nonce = Nonce::from_slice(&nonce_bytes);

        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let ciphertext = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: b"",
                },
            )
            .unwrap();

        // Concatenate nonce + ciphertext
        let mut encrypted = Vec::with_capacity(12 + ciphertext.len());
        encrypted.extend_from_slice(&nonce_bytes);
        encrypted.extend_from_slice(&ciphertext);

        // Decrypt
        let result = decrypt_aes_gcm(&encrypted, &key);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), plaintext);
    }

    #[test]
    fn test_extract_nonce_basic() {
        let encrypted = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        let nonce = extract_nonce(&encrypted).unwrap();
        assert_eq!(nonce, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    }

    #[test]
    fn test_decrypt_wrong_key() {
        let correct_key = [0u8; 32];
        let wrong_key = [1u8; 32];
        let plaintext = "Secret";

        let nonce_bytes = [1u8; 12];
        let nonce = Nonce::from_slice(&nonce_bytes);

        let cipher = Aes256Gcm::new_from_slice(&correct_key).unwrap();
        let ciphertext = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: b"",
                },
            )
            .unwrap();

        let mut encrypted = Vec::new();
        encrypted.extend_from_slice(&nonce_bytes);
        encrypted.extend_from_slice(&ciphertext);

        let result = decrypt_aes_gcm(&encrypted, &wrong_key);
        assert!(result.is_err());
    }
}
