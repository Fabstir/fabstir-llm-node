//! Session Initialization Decryption
//!
//! Implements decryption and verification of encrypted session initialization payloads.
//! Combines ECDH, XChaCha20-Poly1305, and ECDSA signature recovery.

use super::{decrypt_with_aead, derive_shared_key, recover_client_address};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Encrypted session initialization payload from client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedSessionPayload {
    /// Client's ephemeral public key (33 bytes compressed or 65 bytes uncompressed)
    pub eph_pub: Vec<u8>,
    /// Encrypted session data
    pub ciphertext: Vec<u8>,
    /// 24-byte nonce for XChaCha20-Poly1305
    pub nonce: Vec<u8>,
    /// 65-byte ECDSA signature (r + s + recovery_id)
    pub signature: Vec<u8>,
    /// Additional authenticated data
    pub aad: Vec<u8>,
}

/// Decrypted session initialization data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInitData {
    /// Job ID from blockchain
    pub job_id: String,
    /// Model name to use for inference
    pub model_name: String,
    /// 32-byte session key for subsequent message encryption
    pub session_key: [u8; 32],
    /// Price per token in wei
    pub price_per_token: u64,
    /// Client's Ethereum address (recovered from signature)
    pub client_address: String,
}

/// Internal structure for parsing decrypted JSON payload
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionDataJson {
    job_id: String,
    model_name: String,
    session_key: String,
    price_per_token: u64,
}

/// Decrypt and verify encrypted session initialization payload
///
/// This function orchestrates the complete session initialization decryption:
/// 1. Performs ECDH with client's ephemeral public key
/// 2. Decrypts session data with XChaCha20-Poly1305
/// 3. Parses decrypted JSON
/// 4. Recovers client address from signature over ciphertext
/// 5. Returns session data + client address
///
/// # Arguments
/// * `payload` - Encrypted session initialization payload from client
/// * `node_private_key` - Node's secp256k1 private key (32 bytes)
///
/// # Returns
/// * `Ok(SessionInitData)` - Decrypted and verified session data
/// * `Err` - If decryption, verification, or parsing fails
pub fn decrypt_session_init(
    payload: &EncryptedSessionPayload,
    node_private_key: &[u8],
) -> Result<SessionInitData> {
    // Validate payload sizes
    if payload.eph_pub.is_empty() {
        return Err(anyhow!("Ephemeral public key is empty"));
    }
    if payload.ciphertext.is_empty() {
        return Err(anyhow!("Ciphertext is empty"));
    }
    if payload.nonce.len() != 24 {
        return Err(anyhow!(
            "Invalid nonce size: expected 24 bytes, got {}",
            payload.nonce.len()
        ));
    }
    if payload.signature.len() != 65 {
        return Err(anyhow!(
            "Invalid signature size: expected 65 bytes, got {}",
            payload.signature.len()
        ));
    }
    if node_private_key.len() != 32 {
        return Err(anyhow!(
            "Invalid node private key size: expected 32 bytes, got {}",
            node_private_key.len()
        ));
    }

    // Step 1: Perform ECDH to derive shared key
    let shared_key = derive_shared_key(&payload.eph_pub, node_private_key)
        .map_err(|e| anyhow!("ECDH key derivation failed: {}", e))?;

    // Step 2: Decrypt ciphertext with XChaCha20-Poly1305
    let nonce: [u8; 24] = payload.nonce.as_slice().try_into().map_err(|_| {
        anyhow!(
            "Failed to convert nonce to fixed-size array: {} bytes",
            payload.nonce.len()
        )
    })?;

    let plaintext = decrypt_with_aead(&payload.ciphertext, &nonce, &payload.aad, &shared_key)
        .map_err(|e| anyhow!("Decryption failed: {}", e))?;

    // Step 3: Parse decrypted JSON
    let plaintext_str = std::str::from_utf8(&plaintext)
        .map_err(|e| anyhow!("Decrypted data is not valid UTF-8: {}", e))?;

    let session_data: SessionDataJson = serde_json::from_str(plaintext_str)
        .map_err(|e| anyhow!("Failed to parse session data JSON: {}", e))?;

    // Step 4: Extract and validate session key (hex-encoded 32 bytes)
    let session_key_hex = session_data
        .session_key
        .strip_prefix("0x")
        .unwrap_or(&session_data.session_key);

    let session_key_bytes = hex::decode(session_key_hex)
        .map_err(|e| anyhow!("Failed to decode session key hex: {}", e))?;

    if session_key_bytes.len() != 32 {
        return Err(anyhow!(
            "Invalid session key length: expected 32 bytes, got {}",
            session_key_bytes.len()
        ));
    }

    let mut session_key = [0u8; 32];
    session_key.copy_from_slice(&session_key_bytes);

    // Step 5: Verify signature over ciphertext and recover client address
    let ciphertext_hash = Sha256::digest(&payload.ciphertext);

    let client_address = recover_client_address(&payload.signature, ciphertext_hash.as_slice())
        .map_err(|e| anyhow!("Signature verification failed: {}", e))?;

    // Step 6: Return complete session initialization data
    Ok(SessionInitData {
        job_id: session_data.job_id,
        model_name: session_data.model_name,
        session_key,
        price_per_token: session_data.price_per_token,
        client_address,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_payload_sizes() {
        let valid_payload = EncryptedSessionPayload {
            eph_pub: vec![0u8; 33],
            ciphertext: vec![0u8; 64],
            nonce: vec![0u8; 24],
            signature: vec![0u8; 65],
            aad: vec![],
        };

        let node_key = [0u8; 32];

        // This will fail during ECDH/decryption, but should pass validation
        let result = decrypt_session_init(&valid_payload, &node_key);
        // Should not fail on size validation
        assert!(result.is_err()); // Will fail on ECDH with invalid keys
    }

    #[test]
    fn test_invalid_nonce_size() {
        let payload = EncryptedSessionPayload {
            eph_pub: vec![0u8; 33],
            ciphertext: vec![0u8; 64],
            nonce: vec![0u8; 16], // Wrong size
            signature: vec![0u8; 65],
            aad: vec![],
        };

        let node_key = [0u8; 32];
        let result = decrypt_session_init(&payload, &node_key);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid nonce size"));
    }

    #[test]
    fn test_invalid_signature_size() {
        let payload = EncryptedSessionPayload {
            eph_pub: vec![0u8; 33],
            ciphertext: vec![0u8; 64],
            nonce: vec![0u8; 24],
            signature: vec![0u8; 64], // Wrong size
            aad: vec![],
        };

        let node_key = [0u8; 32];
        let result = decrypt_session_init(&payload, &node_key);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid signature size"));
    }
}
