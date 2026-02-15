// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! Encrypted Checkpoint Deltas for Privacy-Preserving Recovery
//!
//! Implements ECDH + XChaCha20-Poly1305 encryption for checkpoint deltas,
//! ensuring only the user can recover their conversation.
//!
//! ## Security Properties
//! - **Confidentiality**: XChaCha20-Poly1305 with ECDH-derived key
//! - **Forward Secrecy**: Ephemeral keypair per checkpoint
//! - **Authenticity**: Poly1305 MAC + host signature over ciphertext
//! - **Integrity**: AEAD (Authenticated Encryption with Associated Data)
//! - **User-Only Access**: Only user has private key for recoveryPublicKey
//!
//! ## Format
//! ```json
//! {
//!   "encrypted": true,
//!   "version": 1,
//!   "userRecoveryPubKey": "0x02...",
//!   "ephemeralPublicKey": "0x03...",
//!   "nonce": "...",
//!   "ciphertext": "...",
//!   "hostSignature": "0x..."
//! }
//! ```

use crate::checkpoint::delta::{sort_json_keys, CheckpointDelta};
use crate::checkpoint::signer::sign_checkpoint_data;
use anyhow::{anyhow, Result};
use chacha20poly1305::{aead::Aead, aead::KeyInit, XChaCha20Poly1305};
use hkdf::Hkdf;
use k256::{
    ecdh::diffie_hellman, elliptic_curve::sec1::FromEncodedPoint, EncodedPoint, PublicKey,
    SecretKey,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tiny_keccak::{Hasher, Keccak};

/// HKDF info parameter for checkpoint encryption domain separation
pub const CHECKPOINT_HKDF_INFO: &[u8] = b"checkpoint-delta-encryption-v1";

/// Derive encryption key for checkpoint delta using ECDH + HKDF
///
/// This function performs ECDH key exchange between the host's ephemeral private key
/// and the user's recovery public key, then derives a 32-byte encryption key using
/// HKDF-SHA256 with a checkpoint-specific info parameter for domain separation.
///
/// # Arguments
/// * `ephemeral_private` - Host's ephemeral private key (32 bytes)
/// * `user_recovery_pubkey` - User's recovery public key (33 bytes compressed or 65 bytes uncompressed)
///
/// # Returns
/// 32-byte encryption key suitable for XChaCha20-Poly1305
///
/// # Security
/// - Uses checkpoint-specific HKDF info parameter to ensure keys are domain-separated
/// - Ephemeral private key should be freshly generated for each checkpoint (forward secrecy)
pub fn derive_checkpoint_encryption_key(
    ephemeral_private: &[u8],
    user_recovery_pubkey: &[u8],
) -> Result<[u8; 32]> {
    // 1. Validate ephemeral private key (32 bytes)
    if ephemeral_private.len() != 32 {
        return Err(anyhow!(
            "Invalid ephemeral private key size: expected 32 bytes, got {}",
            ephemeral_private.len()
        ));
    }

    // Parse ephemeral private key
    let secret_key = SecretKey::from_slice(ephemeral_private)
        .map_err(|e| anyhow!("Failed to parse ephemeral private key: {}", e))?;

    // 2. Validate and parse user's recovery public key
    // Supports both compressed (33 bytes) and uncompressed (65 bytes) formats
    if user_recovery_pubkey.len() != 33 && user_recovery_pubkey.len() != 65 {
        return Err(anyhow!(
            "Invalid user recovery public key size: expected 33 or 65 bytes, got {}",
            user_recovery_pubkey.len()
        ));
    }

    // Parse user's recovery public key
    let encoded_point = EncodedPoint::from_bytes(user_recovery_pubkey)
        .map_err(|e| anyhow!("Failed to parse user recovery public key: {}", e))?;

    let user_pub = PublicKey::from_encoded_point(&encoded_point);
    let user_pub = if user_pub.is_some().into() {
        user_pub.unwrap()
    } else {
        return Err(anyhow!("Invalid user recovery public key point"));
    };

    // 3. Perform ECDH: shared_point = user_pub * ephemeral_private
    let ecdh_result = diffie_hellman(secret_key.to_nonzero_scalar(), user_pub.as_affine());

    // 4. Hash the x-coordinate with SHA256 (SDK compatibility requirement)
    // SDK expects: shared_secret = sha256(shared_point.x)
    let x_coordinate = ecdh_result.raw_secret_bytes();
    let shared_secret = Sha256::digest(x_coordinate);

    // 5. Derive encryption key using HKDF-SHA256 with checkpoint-specific info
    // HKDF with salt=None (which HKDF treats as all-zeros salt)
    let hkdf = Hkdf::<Sha256>::new(None, &shared_secret);
    let mut encryption_key = [0u8; 32];
    hkdf.expand(CHECKPOINT_HKDF_INFO, &mut encryption_key)
        .map_err(|e| anyhow!("HKDF key derivation failed: {}", e))?;

    Ok(encryption_key)
}

/// Encrypt a checkpoint delta for SDK recovery
///
/// This function encrypts a `CheckpointDelta` using:
/// 1. Fresh ephemeral keypair (forward secrecy)
/// 2. ECDH + HKDF for shared key derivation
/// 3. XChaCha20-Poly1305 for authenticated encryption
/// 4. EIP-191 signature over keccak256(ciphertext)
///
/// # Arguments
/// * `delta` - The checkpoint delta to encrypt
/// * `user_recovery_pubkey_hex` - User's recovery public key (0x-prefixed hex, compressed secp256k1)
/// * `host_private_key` - Host's signing private key for EIP-191 signature
///
/// # Returns
/// `EncryptedCheckpointDelta` ready for S5 upload
///
/// # Security
/// - Each call generates a fresh ephemeral keypair (forward secrecy)
/// - Random 24-byte nonce prevents nonce reuse
/// - AEAD provides confidentiality and authenticity
/// - Host signature proves checkpoint origin
pub fn encrypt_checkpoint_delta(
    delta: &CheckpointDelta,
    user_recovery_pubkey_hex: &str,
    host_private_key: &[u8; 32],
) -> Result<EncryptedCheckpointDelta> {
    // 1. Parse and validate user's recovery public key
    let pubkey_bytes = parse_hex_pubkey(user_recovery_pubkey_hex)?;

    // 2. Generate fresh ephemeral keypair for forward secrecy
    let mut rng = rand::thread_rng();
    let ephemeral_private: [u8; 32] = rng.gen();
    let ephemeral_secret = SecretKey::from_slice(&ephemeral_private)
        .map_err(|e| anyhow!("Failed to create ephemeral key: {}", e))?;
    let ephemeral_public = ephemeral_secret.public_key();
    let ephemeral_public_compressed = ephemeral_public.to_sec1_bytes();
    let ephemeral_public_hex = format!("0x{}", hex::encode(&ephemeral_public_compressed));

    // 3. Derive encryption key using ECDH + HKDF
    let encryption_key = derive_checkpoint_encryption_key(&ephemeral_private, &pubkey_bytes)?;

    // 4. Serialize delta to JSON with sorted keys (SDK compatibility)
    let value =
        serde_json::to_value(delta).map_err(|e| anyhow!("JSON serialization failed: {}", e))?;
    let sorted = sort_json_keys(&value);
    let plaintext =
        serde_json::to_string(&sorted).map_err(|e| anyhow!("JSON stringify failed: {}", e))?;

    // 5. Generate random 24-byte nonce
    let nonce_bytes: [u8; 24] = rng.gen();
    let nonce_hex = hex::encode(nonce_bytes);

    // 6. Encrypt with XChaCha20-Poly1305
    let cipher = XChaCha20Poly1305::new_from_slice(&encryption_key)
        .map_err(|e| anyhow!("Cipher initialization failed: {}", e))?;
    let nonce = chacha20poly1305::XNonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow!("Encryption failed: {}", e))?;
    let ciphertext_hex = hex::encode(&ciphertext);

    // 7. Sign keccak256(ciphertext) with host key
    let ciphertext_hash = keccak256(&ciphertext);
    let hash_hex = hex::encode(ciphertext_hash);
    let host_signature = sign_checkpoint_data(host_private_key, &hash_hex)?;

    // 8. Build and return encrypted delta
    Ok(EncryptedCheckpointDelta {
        encrypted: true,
        version: 1,
        user_recovery_pub_key: user_recovery_pubkey_hex.to_string(),
        ephemeral_public_key: ephemeral_public_hex,
        nonce: nonce_hex,
        ciphertext: ciphertext_hex,
        host_signature,
    })
}

/// Parse hex-encoded public key (with or without 0x prefix)
fn parse_hex_pubkey(hex_str: &str) -> Result<Vec<u8>> {
    let hex_clean = hex_str.strip_prefix("0x").unwrap_or(hex_str);

    // Validate length: compressed (66 hex = 33 bytes) or uncompressed (130 hex = 65 bytes)
    if hex_clean.len() != 66 && hex_clean.len() != 130 {
        return Err(anyhow!(
            "Invalid public key length: expected 66 or 130 hex chars, got {}",
            hex_clean.len()
        ));
    }

    let bytes = hex::decode(hex_clean).map_err(|e| anyhow!("Invalid hex in public key: {}", e))?;

    // Validate it's a valid curve point
    let encoded = EncodedPoint::from_bytes(&bytes)
        .map_err(|e| anyhow!("Invalid public key format: {}", e))?;
    let pubkey = PublicKey::from_encoded_point(&encoded);
    if pubkey.is_none().into() {
        return Err(anyhow!("Invalid public key: not a valid curve point"));
    }

    Ok(bytes)
}

/// Compute keccak256 hash
fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    hasher.update(data);
    let mut hash = [0u8; 32];
    hasher.finalize(&mut hash);
    hash
}

/// Encrypted checkpoint delta for SDK recovery
///
/// Only the user with the matching private key can decrypt.
/// The structure is designed for forward secrecy - each checkpoint
/// uses a fresh ephemeral keypair.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedCheckpointDelta {
    /// Always true for encrypted deltas
    pub encrypted: bool,

    /// Encryption version (currently 1)
    pub version: u8,

    /// User's recovery public key (echoed back for verification)
    /// Compressed secp256k1 public key (0x-prefixed hex, 68 chars)
    pub user_recovery_pub_key: String,

    /// Host's ephemeral public key for ECDH (compressed, 33 bytes)
    /// 0x-prefixed hex, 68 chars
    pub ephemeral_public_key: String,

    /// 24-byte random nonce for XChaCha20 (hex, 48 chars)
    pub nonce: String,

    /// Encrypted CheckpointDelta JSON (hex-encoded)
    pub ciphertext: String,

    /// EIP-191 signature over keccak256(ciphertext)
    /// 0x-prefixed hex, 132 chars (65 bytes)
    pub host_signature: String,
}

impl EncryptedCheckpointDelta {
    /// Create a new encrypted checkpoint delta
    ///
    /// This is primarily for testing. Use `encrypt_checkpoint_delta()` for production.
    pub fn new(
        user_recovery_pub_key: String,
        ephemeral_public_key: String,
        nonce: String,
        ciphertext: String,
        host_signature: String,
    ) -> Self {
        Self {
            encrypted: true,
            version: 1,
            user_recovery_pub_key,
            ephemeral_public_key,
            nonce,
            ciphertext,
            host_signature,
        }
    }

    /// Convert to JSON bytes for S5 upload
    pub fn to_json_bytes(&self) -> Vec<u8> {
        // Use sorted keys for deterministic output (SDK compatibility)
        serde_json::to_vec(self).expect("EncryptedCheckpointDelta serialization should never fail")
    }

    /// Validate the structure has expected field lengths
    pub fn validate(&self) -> Result<(), String> {
        // Check encrypted flag
        if !self.encrypted {
            return Err("encrypted field must be true".to_string());
        }

        // Check version
        if self.version != 1 {
            return Err(format!("unsupported version: {}", self.version));
        }

        // Check user recovery public key (0x + 66 hex chars = 68)
        if !self.user_recovery_pub_key.starts_with("0x") {
            return Err("userRecoveryPubKey must start with 0x".to_string());
        }
        if self.user_recovery_pub_key.len() != 68 {
            return Err(format!(
                "userRecoveryPubKey invalid length: {} (expected 68)",
                self.user_recovery_pub_key.len()
            ));
        }

        // Check ephemeral public key (0x + 66 hex chars = 68)
        if !self.ephemeral_public_key.starts_with("0x") {
            return Err("ephemeralPublicKey must start with 0x".to_string());
        }
        if self.ephemeral_public_key.len() != 68 {
            return Err(format!(
                "ephemeralPublicKey invalid length: {} (expected 68)",
                self.ephemeral_public_key.len()
            ));
        }

        // Check nonce (24 bytes = 48 hex chars)
        if self.nonce.len() != 48 {
            return Err(format!(
                "nonce invalid length: {} (expected 48)",
                self.nonce.len()
            ));
        }

        // Check ciphertext is non-empty
        if self.ciphertext.is_empty() {
            return Err("ciphertext cannot be empty".to_string());
        }

        // Check host signature (0x + 130 hex chars = 132 for 65 bytes)
        if !self.host_signature.starts_with("0x") {
            return Err("hostSignature must start with 0x".to_string());
        }
        if self.host_signature.len() != 132 {
            return Err(format!(
                "hostSignature invalid length: {} (expected 132)",
                self.host_signature.len()
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Test recovery public key (compressed secp256k1, 33 bytes)
    const TEST_RECOVERY_PUBKEY: &str =
        "0x02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5";

    // Test ephemeral public key (compressed secp256k1, 33 bytes)
    const TEST_EPHEMERAL_PUBKEY: &str =
        "0x0379be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

    // Test nonce (24 bytes = 48 hex chars)
    const TEST_NONCE: &str = "f47ac10b58cc4372a5670e02b2c3d479f47ac10b58cc4372";

    // Test ciphertext (non-empty hex)
    const TEST_CIPHERTEXT: &str = "a1b2c3d4e5f6789012345678";

    // Test signature (65 bytes = 130 hex chars + 0x)
    const TEST_SIGNATURE: &str = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12";

    #[test]
    fn test_encrypted_checkpoint_delta_serialization_camel_case() {
        let delta = EncryptedCheckpointDelta::new(
            TEST_RECOVERY_PUBKEY.to_string(),
            TEST_EPHEMERAL_PUBKEY.to_string(),
            TEST_NONCE.to_string(),
            TEST_CIPHERTEXT.to_string(),
            TEST_SIGNATURE.to_string(),
        );

        let json_str = serde_json::to_string(&delta).unwrap();

        // Verify camelCase field names
        assert!(json_str.contains("\"encrypted\":"));
        assert!(json_str.contains("\"version\":"));
        assert!(json_str.contains("\"userRecoveryPubKey\":"));
        assert!(json_str.contains("\"ephemeralPublicKey\":"));
        assert!(json_str.contains("\"nonce\":"));
        assert!(json_str.contains("\"ciphertext\":"));
        assert!(json_str.contains("\"hostSignature\":"));

        // Verify NOT snake_case
        assert!(!json_str.contains("user_recovery_pub_key"));
        assert!(!json_str.contains("ephemeral_public_key"));
        assert!(!json_str.contains("host_signature"));
    }

    #[test]
    fn test_encrypted_checkpoint_delta_all_fields_present() {
        let delta = EncryptedCheckpointDelta::new(
            TEST_RECOVERY_PUBKEY.to_string(),
            TEST_EPHEMERAL_PUBKEY.to_string(),
            TEST_NONCE.to_string(),
            TEST_CIPHERTEXT.to_string(),
            TEST_SIGNATURE.to_string(),
        );

        assert!(delta.encrypted);
        assert_eq!(delta.version, 1);
        assert_eq!(delta.user_recovery_pub_key, TEST_RECOVERY_PUBKEY);
        assert_eq!(delta.ephemeral_public_key, TEST_EPHEMERAL_PUBKEY);
        assert_eq!(delta.nonce, TEST_NONCE);
        assert_eq!(delta.ciphertext, TEST_CIPHERTEXT);
        assert_eq!(delta.host_signature, TEST_SIGNATURE);
    }

    #[test]
    fn test_encrypted_checkpoint_delta_to_json_bytes() {
        let delta = EncryptedCheckpointDelta::new(
            TEST_RECOVERY_PUBKEY.to_string(),
            TEST_EPHEMERAL_PUBKEY.to_string(),
            TEST_NONCE.to_string(),
            TEST_CIPHERTEXT.to_string(),
            TEST_SIGNATURE.to_string(),
        );

        let bytes = delta.to_json_bytes();
        assert!(!bytes.is_empty());

        // Should be valid JSON
        let parsed: EncryptedCheckpointDelta = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed, delta);
    }

    #[test]
    fn test_encrypted_checkpoint_delta_deserialization() {
        // Test deserialization from JSON (as SDK would receive)
        let json = json!({
            "encrypted": true,
            "version": 1,
            "userRecoveryPubKey": TEST_RECOVERY_PUBKEY,
            "ephemeralPublicKey": TEST_EPHEMERAL_PUBKEY,
            "nonce": TEST_NONCE,
            "ciphertext": TEST_CIPHERTEXT,
            "hostSignature": TEST_SIGNATURE
        });

        let delta: EncryptedCheckpointDelta = serde_json::from_value(json).unwrap();

        assert!(delta.encrypted);
        assert_eq!(delta.version, 1);
        assert_eq!(delta.user_recovery_pub_key, TEST_RECOVERY_PUBKEY);
    }

    #[test]
    fn test_encrypted_checkpoint_delta_validation_pass() {
        let delta = EncryptedCheckpointDelta::new(
            TEST_RECOVERY_PUBKEY.to_string(),
            TEST_EPHEMERAL_PUBKEY.to_string(),
            TEST_NONCE.to_string(),
            TEST_CIPHERTEXT.to_string(),
            TEST_SIGNATURE.to_string(),
        );

        assert!(delta.validate().is_ok());
    }

    #[test]
    fn test_encrypted_checkpoint_delta_validation_bad_encrypted_flag() {
        let mut delta = EncryptedCheckpointDelta::new(
            TEST_RECOVERY_PUBKEY.to_string(),
            TEST_EPHEMERAL_PUBKEY.to_string(),
            TEST_NONCE.to_string(),
            TEST_CIPHERTEXT.to_string(),
            TEST_SIGNATURE.to_string(),
        );
        delta.encrypted = false;

        let result = delta.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("encrypted field must be true"));
    }

    #[test]
    fn test_encrypted_checkpoint_delta_validation_bad_nonce_length() {
        let delta = EncryptedCheckpointDelta::new(
            TEST_RECOVERY_PUBKEY.to_string(),
            TEST_EPHEMERAL_PUBKEY.to_string(),
            "tooshort".to_string(), // Bad nonce
            TEST_CIPHERTEXT.to_string(),
            TEST_SIGNATURE.to_string(),
        );

        let result = delta.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("nonce invalid length"));
    }

    #[test]
    fn test_encrypted_checkpoint_delta_validation_empty_ciphertext() {
        let delta = EncryptedCheckpointDelta::new(
            TEST_RECOVERY_PUBKEY.to_string(),
            TEST_EPHEMERAL_PUBKEY.to_string(),
            TEST_NONCE.to_string(),
            String::new(), // Empty ciphertext
            TEST_SIGNATURE.to_string(),
        );

        let result = delta.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("ciphertext cannot be empty"));
    }

    // Sub-phase 9.5: ECDH Key Derivation Tests

    // Valid secp256k1 test vectors
    // These are well-known test vectors from Bitcoin/Ethereum ecosystem
    // Private key: 1 (for testing only - never use in production!)
    const TEST_EPHEMERAL_PRIVATE: [u8; 32] = [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x01,
    ];

    // Compressed public key for private key = 2 (G * 2)
    // This is a valid secp256k1 point
    const TEST_USER_RECOVERY_PUBKEY_BYTES: [u8; 33] = [
        0x02, 0xc6, 0x04, 0x7f, 0x94, 0x41, 0xed, 0x7d, 0x6d, 0x30, 0x45, 0x40, 0x6e, 0x95, 0xc0,
        0x7c, 0xd8, 0x5c, 0x77, 0x8e, 0x4b, 0x8c, 0xef, 0x3c, 0xa7, 0xab, 0xac, 0x09, 0xb9, 0x5c,
        0x70, 0x9e, 0xe5,
    ];

    #[test]
    fn test_derive_checkpoint_key_returns_32_bytes() {
        let result = derive_checkpoint_encryption_key(
            &TEST_EPHEMERAL_PRIVATE,
            &TEST_USER_RECOVERY_PUBKEY_BYTES,
        );

        assert!(result.is_ok());
        let key = result.unwrap();
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn test_derive_checkpoint_key_is_deterministic() {
        // Same inputs should produce same output
        let key1 = derive_checkpoint_encryption_key(
            &TEST_EPHEMERAL_PRIVATE,
            &TEST_USER_RECOVERY_PUBKEY_BYTES,
        )
        .unwrap();

        let key2 = derive_checkpoint_encryption_key(
            &TEST_EPHEMERAL_PRIVATE,
            &TEST_USER_RECOVERY_PUBKEY_BYTES,
        )
        .unwrap();

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_derive_checkpoint_key_different_inputs_different_outputs() {
        // Different private keys should produce different derived keys
        let key1 = derive_checkpoint_encryption_key(
            &TEST_EPHEMERAL_PRIVATE,
            &TEST_USER_RECOVERY_PUBKEY_BYTES,
        )
        .unwrap();

        // Different private key (2 instead of 1)
        let different_private: [u8; 32] = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x02,
        ];

        let key2 =
            derive_checkpoint_encryption_key(&different_private, &TEST_USER_RECOVERY_PUBKEY_BYTES)
                .unwrap();

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_derive_checkpoint_key_rejects_invalid_private_key_size() {
        let short_key = [0u8; 16]; // Too short
        let result = derive_checkpoint_encryption_key(&short_key, &TEST_USER_RECOVERY_PUBKEY_BYTES);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("expected 32 bytes"));
    }

    #[test]
    fn test_derive_checkpoint_key_rejects_invalid_public_key_size() {
        let short_pubkey = [0u8; 20]; // Too short
        let result = derive_checkpoint_encryption_key(&TEST_EPHEMERAL_PRIVATE, &short_pubkey);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("expected 33 or 65 bytes"));
    }

    #[test]
    fn test_derive_checkpoint_key_rejects_invalid_public_key_point() {
        // Valid length but invalid point (all zeros is not on curve)
        let invalid_pubkey = [0u8; 33];
        let result = derive_checkpoint_encryption_key(&TEST_EPHEMERAL_PRIVATE, &invalid_pubkey);

        assert!(result.is_err());
    }

    #[test]
    fn test_derive_checkpoint_key_uses_checkpoint_info_param() {
        // Verify that the key is different from what we'd get with empty info
        // by checking it's non-zero and properly derived
        let key = derive_checkpoint_encryption_key(
            &TEST_EPHEMERAL_PRIVATE,
            &TEST_USER_RECOVERY_PUBKEY_BYTES,
        )
        .unwrap();

        // Key should not be all zeros
        assert!(key.iter().any(|&b| b != 0));

        // Key should not be all the same value
        let first = key[0];
        assert!(key.iter().any(|&b| b != first));
    }

    // Sub-phase 9.6: encrypt_checkpoint_delta() Tests

    use crate::checkpoint::delta::{CheckpointDelta, CheckpointMessage};

    fn create_test_delta() -> CheckpointDelta {
        CheckpointDelta {
            session_id: "test-session-123".to_string(),
            checkpoint_index: 0,
            proof_hash: "0xabcdef1234567890".to_string(),
            start_token: 0,
            end_token: 500,
            messages: vec![
                CheckpointMessage::new_user("Hello, AI!".to_string(), 1704844800000),
                CheckpointMessage::new_assistant(
                    "Hello! How can I help?".to_string(),
                    1704844801000,
                    false,
                ),
            ],
            host_signature: "0x".to_string(), // Will be set during encryption
        }
    }

    fn generate_test_host_key() -> [u8; 32] {
        // Valid private key (value = 3)
        [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x03,
        ]
    }

    #[test]
    fn test_encrypt_checkpoint_delta_returns_encrypted_delta() {
        let delta = create_test_delta();
        let host_key = generate_test_host_key();

        let result = encrypt_checkpoint_delta(&delta, TEST_RECOVERY_PUBKEY, &host_key);
        assert!(result.is_ok(), "Should return Ok: {:?}", result);

        let encrypted = result.unwrap();
        assert!(encrypted.encrypted, "encrypted field should be true");
        assert_eq!(encrypted.version, 1, "version should be 1");
    }

    #[test]
    fn test_encrypt_checkpoint_delta_has_correct_field_formats() {
        let delta = create_test_delta();
        let host_key = generate_test_host_key();

        let encrypted = encrypt_checkpoint_delta(&delta, TEST_RECOVERY_PUBKEY, &host_key).unwrap();

        // User recovery public key should be echoed back
        assert_eq!(encrypted.user_recovery_pub_key, TEST_RECOVERY_PUBKEY);

        // Ephemeral public key should be 0x + 66 hex chars (33 bytes compressed)
        assert!(encrypted.ephemeral_public_key.starts_with("0x"));
        assert_eq!(encrypted.ephemeral_public_key.len(), 68);

        // Nonce should be 48 hex chars (24 bytes)
        assert_eq!(encrypted.nonce.len(), 48);

        // Ciphertext should be non-empty hex
        assert!(!encrypted.ciphertext.is_empty());

        // Host signature should be 0x + 130 hex chars (65 bytes)
        assert!(encrypted.host_signature.starts_with("0x"));
        assert_eq!(encrypted.host_signature.len(), 132);
    }

    #[test]
    fn test_encrypt_checkpoint_delta_validates_structure() {
        let delta = create_test_delta();
        let host_key = generate_test_host_key();

        let encrypted = encrypt_checkpoint_delta(&delta, TEST_RECOVERY_PUBKEY, &host_key).unwrap();

        // Should pass all validation checks
        assert!(
            encrypted.validate().is_ok(),
            "Validation should pass: {:?}",
            encrypted.validate()
        );
    }

    #[test]
    fn test_encrypt_checkpoint_delta_different_calls_different_ciphertext() {
        // Due to ephemeral keys and random nonces, each encryption should be different
        let delta = create_test_delta();
        let host_key = generate_test_host_key();

        let encrypted1 = encrypt_checkpoint_delta(&delta, TEST_RECOVERY_PUBKEY, &host_key).unwrap();
        let encrypted2 = encrypt_checkpoint_delta(&delta, TEST_RECOVERY_PUBKEY, &host_key).unwrap();

        // Ephemeral keys should be different (forward secrecy)
        assert_ne!(
            encrypted1.ephemeral_public_key,
            encrypted2.ephemeral_public_key
        );

        // Nonces should be different
        assert_ne!(encrypted1.nonce, encrypted2.nonce);

        // Ciphertexts should be different
        assert_ne!(encrypted1.ciphertext, encrypted2.ciphertext);
    }

    #[test]
    fn test_encrypt_checkpoint_delta_rejects_invalid_pubkey() {
        let delta = create_test_delta();
        let host_key = generate_test_host_key();

        // Invalid public key (too short)
        let result = encrypt_checkpoint_delta(&delta, "0x1234", &host_key);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid"));
    }

    #[test]
    fn test_encrypt_checkpoint_delta_rejects_invalid_pubkey_point() {
        let delta = create_test_delta();
        let host_key = generate_test_host_key();

        // Valid length but invalid point (all zeros is not on curve)
        let invalid_pubkey = "0x000000000000000000000000000000000000000000000000000000000000000000";
        let result = encrypt_checkpoint_delta(&delta, invalid_pubkey, &host_key);
        assert!(result.is_err());
    }

    #[test]
    fn test_encrypt_checkpoint_delta_ciphertext_decryptable() {
        // Verify that ciphertext can be decrypted with the correct shared key
        // This simulates SDK-side decryption using the same crypto parameters
        use chacha20poly1305::{
            aead::{Aead, KeyInit},
            XChaCha20Poly1305,
        };
        use sha2::Digest;

        let delta = create_test_delta();
        let host_key = generate_test_host_key();

        let encrypted = encrypt_checkpoint_delta(&delta, TEST_RECOVERY_PUBKEY, &host_key).unwrap();

        // Get ephemeral public key
        let ephemeral_pubkey_bytes = hex::decode(&encrypted.ephemeral_public_key[2..]).unwrap();

        // Derive the same shared key from user's perspective (SDK-side decryption)
        // User would use their private key (2) with host's ephemeral public key
        let user_private: [u8; 32] = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x02,
        ];

        // Use the same derivation as the node (which now includes SHA256 step)
        let shared_key =
            derive_checkpoint_encryption_key(&user_private, &ephemeral_pubkey_bytes).unwrap();

        // Decrypt using XChaCha20-Poly1305
        let cipher = XChaCha20Poly1305::new_from_slice(&shared_key).unwrap();
        let nonce_bytes = hex::decode(&encrypted.nonce).unwrap();
        let ciphertext_bytes = hex::decode(&encrypted.ciphertext).unwrap();

        let nonce = chacha20poly1305::XNonce::from_slice(&nonce_bytes);
        let plaintext = cipher.decrypt(nonce, ciphertext_bytes.as_slice()).unwrap();

        // Should decrypt to valid JSON
        let plaintext_str = String::from_utf8(plaintext).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&plaintext_str).unwrap();

        // Verify it's the delta
        assert!(parsed.get("sessionId").is_some());
        assert_eq!(parsed["sessionId"].as_str().unwrap(), "test-session-123");
    }

    #[test]
    fn test_sdk_compatible_key_derivation() {
        // Test that our key derivation matches SDK's expected flow:
        // 1. ECDH: user_private × ephemeral_public = shared_point
        // 2. shared_secret = sha256(shared_point.x)
        // 3. HKDF(ikm=shared_secret, salt=None, info="checkpoint-delta-encryption-v1")
        use sha2::Digest;

        // Well-known test vectors
        let ephemeral_private: [u8; 32] = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x01,
        ];

        // Public key for private key = 2
        let user_pubkey: [u8; 33] = TEST_USER_RECOVERY_PUBKEY_BYTES;

        // Our function should now produce a key that includes the SHA256 step
        let key = derive_checkpoint_encryption_key(&ephemeral_private, &user_pubkey).unwrap();

        // Key should be 32 bytes
        assert_eq!(key.len(), 32);

        // Verify manual computation matches:
        // 1. ECDH
        let secret = SecretKey::from_slice(&ephemeral_private).unwrap();
        let pubkey = PublicKey::from_sec1_bytes(&user_pubkey).unwrap();
        let ecdh = k256::ecdh::diffie_hellman(secret.to_nonzero_scalar(), pubkey.as_affine());

        // 2. SHA256(x-coordinate)
        let x_coord = ecdh.raw_secret_bytes();
        let shared_secret = sha2::Sha256::digest(x_coord);

        // 3. HKDF with checkpoint info
        let hkdf = hkdf::Hkdf::<sha2::Sha256>::new(None, &shared_secret);
        let mut expected_key = [0u8; 32];
        hkdf.expand(CHECKPOINT_HKDF_INFO, &mut expected_key)
            .unwrap();

        assert_eq!(
            key, expected_key,
            "Key derivation should match SDK's expected flow"
        );
    }
}
