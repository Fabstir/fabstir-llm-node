//! ECDH Key Exchange Implementation
//!
//! Implements Elliptic Curve Diffie-Hellman key exchange using secp256k1
//! (the same curve used by Ethereum). This is used for the initial session
//! initialization where the client sends an ephemeral public key.

use anyhow::{anyhow, Result};
use k256::ecdh::EphemeralSecret;
use k256::PublicKey;

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
    // TODO: Implement ECDH key derivation
    // 1. Parse client's ephemeral public key
    // 2. Parse node's private key
    // 3. Perform ECDH multiplication
    // 4. Apply HKDF-SHA256 to derive encryption key

    Err(anyhow!("ECDH key derivation not yet implemented"))
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
