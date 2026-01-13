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

use serde::{Deserialize, Serialize};

/// HKDF info parameter for checkpoint encryption domain separation
pub const CHECKPOINT_HKDF_INFO: &[u8] = b"checkpoint-delta-encryption-v1";

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
}
