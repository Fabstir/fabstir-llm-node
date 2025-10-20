// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! ECDH Key Exchange Implementation
//!
//! Implements Elliptic Curve Diffie-Hellman key exchange using secp256k1
//! (the same curve used by Ethereum). This is used for the initial session
//! initialization where the client sends an ephemeral public key.

use anyhow::{anyhow, Result};
use hkdf::Hkdf;
use k256::{
    elliptic_curve::sec1::FromEncodedPoint,
    EncodedPoint, PublicKey, SecretKey,
};
use sha2::Sha256;

/// Derive a shared encryption key using ECDH
///
/// Performs ECDH key exchange between the client's ephemeral public key
/// and the node's static private key, then derives a 32-byte encryption
/// key using HKDF-SHA256.
///
/// # Arguments
///
/// * `client_eph_pub` - Client's ephemeral public key (33 bytes compressed or 65 bytes uncompressed)
/// * `node_priv_key` - Node's static private key (32 bytes)
///
/// # Returns
///
/// A 32-byte encryption key suitable for XChaCha20-Poly1305
///
/// # Example
///
/// ```ignore
/// let shared_key = derive_shared_key(&client_pub_bytes, &node_priv_bytes)?;
/// // Use shared_key for decrypting session init payload
/// ```
pub fn derive_shared_key(client_eph_pub: &[u8], node_priv_key: &[u8]) -> Result<[u8; 32]> {
    // 1. Validate and parse node's private key (32 bytes)
    if node_priv_key.len() != 32 {
        return Err(anyhow!(
            "Invalid node private key size: expected 32 bytes, got {}",
            node_priv_key.len()
        ));
    }

    // Parse node's private key
    let node_secret = SecretKey::from_slice(node_priv_key)
        .map_err(|e| anyhow!("Failed to parse node private key: {}", e))?;

    // 2. Validate and parse client's ephemeral public key
    // Supports both compressed (33 bytes) and uncompressed (65 bytes) formats
    if client_eph_pub.len() != 33 && client_eph_pub.len() != 65 {
        return Err(anyhow!(
            "Invalid client public key size: expected 33 or 65 bytes, got {}",
            client_eph_pub.len()
        ));
    }

    // Parse client's ephemeral public key
    let encoded_point = EncodedPoint::from_bytes(client_eph_pub)
        .map_err(|e| anyhow!("Failed to parse client public key: {}", e))?;

    let client_pub = PublicKey::from_encoded_point(&encoded_point);
    let client_pub = if client_pub.is_some().into() {
        client_pub.unwrap()
    } else {
        return Err(anyhow!("Invalid client public key point"));
    };

    // 3. Perform ECDH: shared_point = client_pub * node_secret
    let shared_secret = k256::ecdh::diffie_hellman(
        node_secret.to_nonzero_scalar(),
        client_pub.as_affine(),
    );

    // 4. Derive encryption key using HKDF-SHA256
    // Extract entropy from shared secret and expand to 32-byte key
    let hkdf = Hkdf::<Sha256>::new(None, shared_secret.raw_secret_bytes());
    let mut derived_key = [0u8; 32];
    hkdf.expand(&[], &mut derived_key)
        .map_err(|e| anyhow!("HKDF key derivation failed: {}", e))?;

    Ok(derived_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_shared_key_placeholder() {
        // Placeholder test - will be replaced with proper TDD tests
        let client_pub = vec![0u8; 33]; // Placeholder
        let node_priv = vec![0u8; 32]; // Placeholder

        let result = derive_shared_key(&client_pub, &node_priv);
        assert!(result.is_err()); // Should fail until implemented
    }
}
