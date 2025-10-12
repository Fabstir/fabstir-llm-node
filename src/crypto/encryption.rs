//! XChaCha20-Poly1305 Encryption/Decryption
//!
//! Implements authenticated encryption using XChaCha20-Poly1305 AEAD
//! (Authenticated Encryption with Additional Data). This provides both
//! confidentiality and authenticity for messages.

use anyhow::{anyhow, Result};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};

/// Decrypt data using XChaCha20-Poly1305 AEAD
///
/// # Arguments
///
/// * `ciphertext` - Encrypted data (includes authentication tag)
/// * `nonce` - 24-byte nonce (unique per encryption)
/// * `aad` - Additional authenticated data (optional, can be empty)
/// * `key` - 32-byte encryption key
///
/// # Returns
///
/// Decrypted plaintext as a byte vector
///
/// # Errors
///
/// Returns error if:
/// - Authentication tag verification fails (tampered data)
/// - Nonce size is not 24 bytes
/// - Key size is not 32 bytes
pub fn decrypt_with_aead(
    ciphertext: &[u8],
    nonce: &[u8],
    aad: &[u8],
    key: &[u8],
) -> Result<Vec<u8>> {
    // 1. Validate nonce size (24 bytes for XChaCha20)
    if nonce.len() != 24 {
        return Err(anyhow!(
            "Invalid nonce size: expected 24 bytes, got {}",
            nonce.len()
        ));
    }

    // 2. Validate key size (32 bytes / 256 bits)
    if key.len() != 32 {
        return Err(anyhow!(
            "Invalid key size: expected 32 bytes, got {}",
            key.len()
        ));
    }

    // 3. Create cipher instance
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| anyhow!("Failed to create cipher: {}", e))?;

    // 4. Prepare nonce
    let xnonce = XNonce::from_slice(nonce);

    // 5. Prepare payload with AAD
    let payload = Payload {
        msg: ciphertext,
        aad,
    };

    // 6. Decrypt and verify authentication tag
    let plaintext = cipher
        .decrypt(xnonce, payload)
        .map_err(|e| anyhow!("Decryption failed (authentication error): {}", e))?;

    Ok(plaintext)
}

/// Encrypt data using XChaCha20-Poly1305 AEAD
///
/// # Arguments
///
/// * `plaintext` - Data to encrypt
/// * `nonce` - 24-byte nonce (must be unique for this key)
/// * `aad` - Additional authenticated data (optional, can be empty)
/// * `key` - 32-byte encryption key
///
/// # Returns
///
/// Encrypted ciphertext with authentication tag appended
///
/// # Security
///
/// **CRITICAL**: Never reuse the same nonce with the same key!
/// Use a random nonce generator or counter.
pub fn encrypt_with_aead(
    plaintext: &[u8],
    nonce: &[u8],
    aad: &[u8],
    key: &[u8],
) -> Result<Vec<u8>> {
    // 1. Validate nonce size (24 bytes for XChaCha20)
    if nonce.len() != 24 {
        return Err(anyhow!(
            "Invalid nonce size: expected 24 bytes, got {}",
            nonce.len()
        ));
    }

    // 2. Validate key size (32 bytes / 256 bits)
    if key.len() != 32 {
        return Err(anyhow!(
            "Invalid key size: expected 32 bytes, got {}",
            key.len()
        ));
    }

    // 3. Create cipher instance
    let cipher = XChaCha20Poly1305::new_from_slice(key)
        .map_err(|e| anyhow!("Failed to create cipher: {}", e))?;

    // 4. Prepare nonce
    let xnonce = XNonce::from_slice(nonce);

    // 5. Prepare payload with AAD
    let payload = Payload {
        msg: plaintext,
        aad,
    };

    // 6. Encrypt and append 16-byte authentication tag
    let ciphertext = cipher
        .encrypt(xnonce, payload)
        .map_err(|e| anyhow!("Encryption failed: {}", e))?;

    Ok(ciphertext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::OsRng, RngCore};

    #[test]
    fn test_encrypt_decrypt_basic() {
        // Basic encrypt/decrypt roundtrip
        let plaintext = b"test message";
        let mut key = [0u8; 32];
        let mut nonce = [0u8; 24];
        OsRng.fill_bytes(&mut key);
        OsRng.fill_bytes(&mut nonce);

        let ciphertext = encrypt_with_aead(plaintext, &nonce, b"", &key).unwrap();
        let decrypted = decrypt_with_aead(&ciphertext, &nonce, b"", &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_invalid_nonce_size() {
        let plaintext = b"test";
        let key = [0u8; 32];
        let short_nonce = [0u8; 12];

        let result = encrypt_with_aead(plaintext, &short_nonce, b"", &key);
        assert!(result.is_err());
    }
}
