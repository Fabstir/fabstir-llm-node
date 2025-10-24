// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! ECDSA Signature Recovery
//!
//! Recovers Ethereum addresses from ECDSA signatures. This is used to
//! authenticate clients during session initialization by verifying that
//! the signature was created by the claimed wallet address.

use anyhow::{anyhow, Result};
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use k256::elliptic_curve::sec1::ToEncodedPoint;
use tiny_keccak::{Hasher, Keccak};

/// Recover client's Ethereum address from ECDSA signature
///
/// Uses ECDSA signature recovery to obtain the public key that signed
/// the message, then derives the Ethereum address from that public key
/// using Keccak-256 hashing (Ethereum standard).
///
/// # Arguments
///
/// * `signature` - 65-byte compact signature (r + s + v)
///   - Bytes 0-31: r component (big-endian)
///   - Bytes 32-63: s component (big-endian)
///   - Byte 64: recovery ID (v), typically 0 or 1 (or 27/28 in some formats)
/// * `message_hash` - 32-byte hash of the message that was signed
///
/// # Returns
///
/// The Ethereum address (0x-prefixed hex string, 42 characters) of the signer
///
/// # Errors
///
/// Returns error if:
/// - Signature is not exactly 65 bytes
/// - Message hash is not exactly 32 bytes
/// - Recovery ID is invalid (> 3)
/// - Signature recovery fails (invalid signature)
///
/// # Example
///
/// ```ignore
/// let signature = &signature_bytes[..]; // 65 bytes
/// let message_hash = &hash_bytes[..];   // 32 bytes
/// let address = recover_client_address(signature, message_hash)?;
/// // address = "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb"
/// ```
pub fn recover_client_address(signature: &[u8], message_hash: &[u8]) -> Result<String> {
    // 1. Validate signature size (65 bytes: 32 + 32 + 1)
    if signature.len() != 65 {
        return Err(anyhow!(
            "Invalid signature size: expected 65 bytes, got {}",
            signature.len()
        ));
    }

    // 2. Validate message hash size (32 bytes / 256 bits)
    if message_hash.len() != 32 {
        return Err(anyhow!(
            "Invalid message hash size: expected 32 bytes, got {}",
            message_hash.len()
        ));
    }

    // 3. Parse signature components
    // Bytes 0-63: r and s components (64 bytes total)
    let signature_bytes = &signature[..64];

    // Byte 64: recovery ID (v parameter)
    let mut recovery_id = signature[64];

    // Handle Ethereum-style recovery IDs (27/28) by normalizing to 0/1
    if recovery_id >= 27 {
        recovery_id -= 27;
    }

    // Validate recovery ID (must be 0, 1, 2, or 3)
    if recovery_id > 3 {
        return Err(anyhow!(
            "Invalid recovery ID: expected 0-3, got {}",
            recovery_id
        ));
    }

    // 4. Recover public key from signature
    let recovery_id = RecoveryId::try_from(recovery_id)
        .map_err(|e| anyhow!("Failed to create recovery ID: {}", e))?;

    let signature = Signature::try_from(signature_bytes)
        .map_err(|e| anyhow!("Failed to parse signature: {}", e))?;

    let verifying_key = VerifyingKey::recover_from_prehash(message_hash, &signature, recovery_id)
        .map_err(|e| anyhow!("Failed to recover public key: {}", e))?;

    // 5. Derive Ethereum address from public key
    // Get uncompressed public key (65 bytes: 0x04 + x + y coordinates)
    let public_key = verifying_key.to_encoded_point(false);
    let public_key_bytes = public_key.as_bytes();

    // Skip the 0x04 prefix byte, hash the remaining 64 bytes with Keccak-256
    let mut hasher = Keccak::v256();
    let mut hash = [0u8; 32];
    hasher.update(&public_key_bytes[1..]); // Skip first byte (0x04 prefix)
    hasher.finalize(&mut hash);

    // Take the last 20 bytes of the Keccak-256 hash as the Ethereum address
    let address_bytes = &hash[12..]; // Last 20 bytes
    let address = format!("0x{}", hex::encode(address_bytes));

    Ok(address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use k256::ecdsa::{signature::Signer, SigningKey};
    use rand::rngs::OsRng;
    use sha2::{Digest, Sha256};
    use tiny_keccak::{Hasher, Keccak};

    /// Helper to create Ethereum address from public key using Keccak-256
    fn pubkey_to_address(public_key: &k256::PublicKey) -> String {
        let encoded_point = public_key.to_encoded_point(false);
        let uncompressed = encoded_point.as_bytes();

        // Hash with Keccak-256
        let mut hasher = Keccak::v256();
        let mut hash = [0u8; 32];
        hasher.update(&uncompressed[1..]); // Skip 0x04 prefix
        hasher.finalize(&mut hash);

        // Take last 20 bytes
        let address_bytes = &hash[12..];
        format!("0x{}", hex::encode(address_bytes))
    }

    #[test]
    fn test_signature_recovery_basic() {
        // Generate test keypair
        let signing_key = SigningKey::random(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let public_key = k256::PublicKey::from(verifying_key);
        let expected_address = pubkey_to_address(&public_key);

        // Sign a message
        let message = b"test message";
        let message_hash = Sha256::digest(message);
        let signature: k256::ecdsa::Signature = signing_key.sign(message);
        let signature_bytes = signature.to_bytes();

        // Create compact signature with recovery ID
        let mut compact_sig = [0u8; 65];
        compact_sig[..64].copy_from_slice(&signature_bytes[..]);

        // Try both recovery IDs to find the correct one
        for recovery_id in 0..2 {
            compact_sig[64] = recovery_id;

            if let Ok(recovered_address) =
                recover_client_address(&compact_sig, message_hash.as_slice())
            {
                if recovered_address == expected_address {
                    // Success!
                    assert_eq!(recovered_address.len(), 42);
                    assert!(recovered_address.starts_with("0x"));
                    return;
                }
            }
        }

        panic!("Failed to recover correct address");
    }

    #[test]
    fn test_invalid_signature_size() {
        let short_sig = [0u8; 32];
        let message_hash = Sha256::digest(b"test");

        let result = recover_client_address(&short_sig, message_hash.as_slice());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("65 bytes"));
    }
}
