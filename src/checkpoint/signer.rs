// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! EIP-191 signing for checkpoint data
//!
//! Signs messages and checkpoints arrays for SDK verification.
//!
//! ## Signature Format
//! - 65 bytes: r (32) + s (32) + v (1)
//! - Hex string with 0x prefix: `0x` + 130 hex characters
//! - v value must be 27 or 28 (Ethereum standard)

use anyhow::{anyhow, Result};
use k256::ecdsa::{signature::hazmat::PrehashSigner, RecoveryId, Signature, SigningKey};
use tiny_keccak::{Hasher, Keccak};

/// Sign data using EIP-191 personal_sign
///
/// # Arguments
/// * `private_key` - 32-byte host private key
/// * `data` - JSON string to sign (messages or checkpoints array)
///
/// # Returns
/// 65-byte signature (r + s + v) as hex string with 0x prefix
///
/// # Example
/// ```ignore
/// let sig = sign_checkpoint_data(&private_key, &messages_json)?;
/// assert_eq!(sig.len(), 132); // 0x + 130 hex chars
/// ```
pub fn sign_checkpoint_data(private_key: &[u8; 32], data: &str) -> Result<String> {
    // 1. Create EIP-191 message hash
    let message_hash = eip191_hash(data.as_bytes());

    // 2. Sign with ECDSA
    let signing_key = SigningKey::from_bytes(private_key.into())
        .map_err(|e| anyhow!("Invalid private key: {}", e))?;

    let (signature, recovery_id) = signing_key
        .sign_prehash_recoverable(&message_hash)
        .map_err(|e| anyhow!("Signing failed: {}", e))?;

    // 3. Format as 65-byte signature with v = 27 or 28
    let sig_bytes = format_signature(signature, recovery_id);

    // 4. Return as hex with 0x prefix
    Ok(format!("0x{}", hex::encode(sig_bytes)))
}

/// Create EIP-191 message hash
/// prefix = "\x19Ethereum Signed Message:\n" + len(message)
fn eip191_hash(message: &[u8]) -> [u8; 32] {
    let prefix = format!("\x19Ethereum Signed Message:\n{}", message.len());

    let mut hasher = Keccak::v256();
    hasher.update(prefix.as_bytes());
    hasher.update(message);

    let mut hash = [0u8; 32];
    hasher.finalize(&mut hash);
    hash
}

/// Format signature as 65 bytes (r + s + v)
fn format_signature(signature: Signature, recovery_id: RecoveryId) -> [u8; 65] {
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&signature.to_bytes());
    sig_bytes[64] = recovery_id.to_byte() + 27; // Ethereum v value
    sig_bytes
}

/// Verify a checkpoint signature locally (for testing/debugging)
///
/// Returns the recovered address if signature is valid
#[allow(dead_code)]
pub fn recover_signer_address(signature: &str, data: &str) -> Result<String> {
    use k256::ecdsa::{RecoveryId, VerifyingKey};

    // Parse signature
    let sig_bytes = hex::decode(signature.trim_start_matches("0x"))
        .map_err(|e| anyhow!("Invalid signature hex: {}", e))?;

    if sig_bytes.len() != 65 {
        return Err(anyhow!(
            "Signature must be 65 bytes, got {}",
            sig_bytes.len()
        ));
    }

    // Extract r, s, v
    let r_s = &sig_bytes[..64];
    let v = sig_bytes[64];

    // v must be 27 or 28
    let recovery_id = match v {
        27 => RecoveryId::try_from(0u8).map_err(|e| anyhow!("Invalid recovery ID: {}", e))?,
        28 => RecoveryId::try_from(1u8).map_err(|e| anyhow!("Invalid recovery ID: {}", e))?,
        _ => return Err(anyhow!("Invalid v value: {}", v)),
    };

    // Create signature from r, s
    let signature = Signature::from_slice(r_s).map_err(|e| anyhow!("Invalid signature: {}", e))?;

    // Hash the message with EIP-191
    let message_hash = eip191_hash(data.as_bytes());

    // Recover the public key
    let verifying_key = VerifyingKey::recover_from_prehash(&message_hash, &signature, recovery_id)
        .map_err(|e| anyhow!("Recovery failed: {}", e))?;

    // Convert to Ethereum address (last 20 bytes of keccak256(pubkey))
    let pubkey_bytes = verifying_key.to_encoded_point(false);
    let pubkey_uncompressed = &pubkey_bytes.as_bytes()[1..]; // Skip 0x04 prefix

    let mut hasher = Keccak::v256();
    hasher.update(pubkey_uncompressed);
    let mut pubkey_hash = [0u8; 32];
    hasher.finalize(&mut pubkey_hash);

    // Address is last 20 bytes
    let address = &pubkey_hash[12..];
    Ok(format!("0x{}", hex::encode(address)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use k256::ecdsa::SigningKey;
    use rand::rngs::OsRng;

    fn generate_test_key() -> [u8; 32] {
        let signing_key = SigningKey::random(&mut OsRng);
        signing_key.to_bytes().into()
    }

    #[test]
    fn test_sign_checkpoint_data_returns_correct_length() {
        let key = generate_test_key();
        let data = r#"[{"role":"user","content":"hello"}]"#;
        let sig = sign_checkpoint_data(&key, data).unwrap();

        // 0x + 130 hex chars = 132 total
        assert_eq!(sig.len(), 132);
        assert!(sig.starts_with("0x"));
    }

    #[test]
    fn test_sign_checkpoint_data_v_27_or_28() {
        let key = generate_test_key();
        let sig = sign_checkpoint_data(&key, "test message").unwrap();

        let sig_bytes = hex::decode(&sig[2..]).unwrap();
        let v = sig_bytes[64];
        assert!(v == 27 || v == 28, "v should be 27 or 28, got {}", v);
    }

    #[test]
    fn test_different_data_different_signature() {
        let key = generate_test_key();
        let sig1 = sign_checkpoint_data(&key, "message1").unwrap();
        let sig2 = sign_checkpoint_data(&key, "message2").unwrap();
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn test_same_data_same_signature() {
        let key = generate_test_key();
        let data = "test message";
        let sig1 = sign_checkpoint_data(&key, data).unwrap();
        let sig2 = sign_checkpoint_data(&key, data).unwrap();
        // Note: ECDSA with k256 is deterministic (RFC 6979)
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn test_signature_is_recoverable() {
        let key = generate_test_key();
        let data = r#"[{"content":"Hello","role":"user","timestamp":123}]"#;
        let sig = sign_checkpoint_data(&key, data).unwrap();

        // Should be able to recover the signer address
        let recovered = recover_signer_address(&sig, data);
        assert!(recovered.is_ok(), "Should recover address: {:?}", recovered);

        let address = recovered.unwrap();
        assert!(address.starts_with("0x"));
        assert_eq!(address.len(), 42); // 0x + 40 hex chars
    }

    #[test]
    fn test_eip191_hash_format() {
        // Test that EIP-191 prefix is applied correctly
        let message = "hello";
        let hash = eip191_hash(message.as_bytes());

        // The hash should be 32 bytes
        assert_eq!(hash.len(), 32);

        // Same message should produce same hash
        let hash2 = eip191_hash(message.as_bytes());
        assert_eq!(hash, hash2);

        // Different message should produce different hash
        let hash3 = eip191_hash("world".as_bytes());
        assert_ne!(hash, hash3);
    }

    #[test]
    fn test_invalid_private_key() {
        let invalid_key = [0u8; 32]; // All zeros is invalid
        let result = sign_checkpoint_data(&invalid_key, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_recover_signer_invalid_signature_length() {
        let result = recover_signer_address("0x1234", "test");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("65 bytes"));
    }

    #[test]
    fn test_recover_signer_invalid_v_value() {
        // Valid length but invalid v value (should be 27 or 28)
        let invalid_sig = format!("0x{}", hex::encode([0u8; 65])); // v = 0
        let result = recover_signer_address(&invalid_sig, "test");
        assert!(result.is_err());
    }
}
