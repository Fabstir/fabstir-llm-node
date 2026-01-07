// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Proof Signing for Security Audit Compliance
//!
//! Signs proof data for `submitProofOfWork` contract calls.
//! Required after January 2026 security audit.
//!
//! ## Signature Formula
//!
//! ```text
//! 1. dataHash = keccak256(abi.encodePacked(proofHash, hostAddress, tokensClaimed))
//! 2. signature = personal_sign(dataHash)  // 65 bytes: r(32) + s(32) + v(1)
//! ```
//!
//! ## Security Requirements
//!
//! - Host wallet must sign (matches session.host in contract)
//! - Signature must be exactly 65 bytes (r + s + v format)
//! - Each proofHash can only be used once (replay protection)
//! - v value must be 27 or 28 (Ethereum standard)
//!
//! ## Usage
//!
//! ```ignore
//! use fabstir_llm_node::crypto::sign_proof_data;
//! use ethers::types::Address;
//!
//! let signature = sign_proof_data(
//!     &private_key,
//!     proof_hash,
//!     host_address,
//!     tokens_claimed,
//! )?;
//! ```

use anyhow::{anyhow, Result};
use ethers::types::Address;
use k256::ecdsa::SigningKey;
use tiny_keccak::{Hasher, Keccak};
use tracing::debug;

/// Error types for proof signing operations
#[derive(Debug, thiserror::Error)]
pub enum ProofSigningError {
    #[error("Invalid private key: {0}")]
    InvalidPrivateKey(String),

    #[error("Signing failed: {0}")]
    SigningFailed(String),

    #[error("Invalid signature format: {0}")]
    InvalidSignatureFormat(String),
}

/// Sign proof data for contract submission
///
/// Creates a 65-byte ECDSA signature that proves the host authorized
/// this specific proof submission. The signature binds together:
/// - The proof hash (proves work was done)
/// - The host address (proves authorization)
/// - The token count (prevents manipulation)
///
/// # Arguments
///
/// * `private_key` - 32-byte host private key (from HOST_PRIVATE_KEY)
/// * `proof_hash` - 32-byte keccak256 hash of the proof data
/// * `host_address` - 20-byte Ethereum address of the host
/// * `tokens_claimed` - Number of tokens being claimed in this submission
///
/// # Returns
///
/// 65-byte signature in r(32) + s(32) + v(1) format
///
/// # Errors
///
/// Returns error if:
/// - Private key is invalid
/// - Signing operation fails
///
/// # Example
///
/// ```ignore
/// let signature = sign_proof_data(
///     &private_key,
///     proof_hash,
///     host_address,
///     1000,
/// )?;
/// assert_eq!(signature.len(), 65);
/// ```
pub fn sign_proof_data(
    private_key: &[u8; 32],
    proof_hash: [u8; 32],
    host_address: Address,
    tokens_claimed: u64,
) -> Result<[u8; 65]> {
    // 1. Encode the data (Solidity abi.encodePacked equivalent)
    let encoded = encode_proof_data(proof_hash, host_address, tokens_claimed);
    debug!(
        "Encoded proof data: {} bytes (proofHash + address + tokens)",
        encoded.len()
    );

    // 2. Hash the encoded data with keccak256
    let data_hash = hash_data(&encoded);
    debug!("Data hash: 0x{}", hex::encode(&data_hash));

    // 3. Sign the hash using ECDSA
    let signing_key = SigningKey::from_bytes(private_key.into())
        .map_err(|e| anyhow!(ProofSigningError::InvalidPrivateKey(e.to_string())))?;

    let (signature, recovery_id) = signing_key
        .sign_prehash_recoverable(&data_hash)
        .map_err(|e| anyhow!(ProofSigningError::SigningFailed(e.to_string())))?;

    // 4. Format as 65-byte signature (r + s + v)
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&signature.to_bytes());
    // Ethereum uses v = 27 or 28 (recovery_id + 27)
    sig_bytes[64] = recovery_id.to_byte() + 27;

    debug!(
        "Generated 65-byte signature, v={}",
        sig_bytes[64]
    );

    Ok(sig_bytes)
}

/// Verify a proof signature locally (for debugging)
///
/// Recovers the signer address from the signature and compares
/// it to the expected host address.
///
/// # Arguments
///
/// * `signature` - 65-byte signature to verify
/// * `proof_hash` - 32-byte proof hash that was signed
/// * `host_address` - Expected signer address
/// * `tokens_claimed` - Token count that was signed
///
/// # Returns
///
/// `true` if signature was created by host_address for this data
pub fn verify_proof_signature(
    signature: &[u8; 65],
    proof_hash: [u8; 32],
    host_address: Address,
    tokens_claimed: u64,
) -> Result<bool> {
    // 1. Encode and hash the data (same as signing)
    let encoded = encode_proof_data(proof_hash, host_address, tokens_claimed);
    let data_hash = hash_data(&encoded);

    // 2. Recover address from signature using existing helper
    let recovered = crate::crypto::signature::recover_client_address(signature, &data_hash)?;

    // 3. Compare addresses (case-insensitive)
    let expected = format!("{:?}", host_address).to_lowercase();
    let recovered_lower = recovered.to_lowercase();

    Ok(recovered_lower == expected)
}

/// Encode proof data for signing (Solidity abi.encodePacked equivalent)
///
/// Packs the data in the same order as the contract expects:
/// - proofHash: 32 bytes
/// - hostAddress: 20 bytes
/// - tokensClaimed: 32 bytes (uint256, big-endian, zero-padded)
fn encode_proof_data(proof_hash: [u8; 32], host_address: Address, tokens_claimed: u64) -> Vec<u8> {
    let mut data = Vec::with_capacity(32 + 20 + 32); // 84 bytes total

    // proofHash: 32 bytes
    data.extend_from_slice(&proof_hash);

    // hostAddress: 20 bytes
    data.extend_from_slice(host_address.as_bytes());

    // tokensClaimed: 32 bytes (uint256, big-endian, zero-padded left)
    let mut tokens_bytes = [0u8; 32];
    tokens_bytes[24..].copy_from_slice(&tokens_claimed.to_be_bytes()); // Last 8 bytes
    data.extend_from_slice(&tokens_bytes);

    data
}

/// Hash data with keccak256
fn hash_data(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    let mut hash = [0u8; 32];
    hasher.update(data);
    hasher.finalize(&mut hash);
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use k256::ecdsa::{SigningKey, VerifyingKey};
    use tiny_keccak::{Hasher, Keccak};

    /// Compute keccak256 hash of data
    fn keccak256(data: &[u8]) -> [u8; 32] {
        let mut hasher = Keccak::v256();
        let mut output = [0u8; 32];
        hasher.update(data);
        hasher.finalize(&mut output);
        output
    }

    /// Generate a test keypair: (private_key, ethereum_address)
    ///
    /// Derives the Ethereum address from the public key using keccak256.
    fn generate_test_keypair() -> ([u8; 32], Address) {
        // Use a deterministic key for reproducible tests
        let private_key_bytes: [u8; 32] = [
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
            0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c,
            0x1d, 0x1e, 0x1f, 0x20,
        ];

        let signing_key = SigningKey::from_bytes((&private_key_bytes).into()).unwrap();
        let verifying_key = VerifyingKey::from(&signing_key);

        // Get uncompressed public key (65 bytes: 0x04 prefix + 32 bytes X + 32 bytes Y)
        let public_key_bytes = verifying_key.to_encoded_point(false);
        let public_key_uncompressed = public_key_bytes.as_bytes();

        // Ethereum address = last 20 bytes of keccak256(public_key[1..65])
        let hash = keccak256(&public_key_uncompressed[1..]); // Skip 0x04 prefix
        let address_bytes: [u8; 20] = hash[12..32].try_into().unwrap();

        (private_key_bytes, Address::from(address_bytes))
    }

    /// Generate a second test keypair for comparison tests
    fn generate_test_keypair_2() -> ([u8; 32], Address) {
        let private_key_bytes: [u8; 32] = [
            0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
            0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55,
            0x66, 0x77, 0x88, 0x99,
        ];

        let signing_key = SigningKey::from_bytes((&private_key_bytes).into()).unwrap();
        let verifying_key = VerifyingKey::from(&signing_key);

        let public_key_bytes = verifying_key.to_encoded_point(false);
        let public_key_uncompressed = public_key_bytes.as_bytes();

        let hash = keccak256(&public_key_uncompressed[1..]);
        let address_bytes: [u8; 20] = hash[12..32].try_into().unwrap();

        (private_key_bytes, Address::from(address_bytes))
    }

    // === Sub-phase 1.1 Tests ===

    #[test]
    fn test_encode_proof_data_length() {
        let proof_hash = [0u8; 32];
        let host_address = Address::zero();
        let tokens = 1000u64;

        let encoded = encode_proof_data(proof_hash, host_address, tokens);
        assert_eq!(encoded.len(), 84); // 32 + 20 + 32
    }

    #[test]
    fn test_hash_data_produces_32_bytes() {
        let data = b"test data";
        let hash = hash_data(data);
        assert_eq!(hash.len(), 32);
    }

    // === Sub-phase 1.2 Tests (TDD) ===

    #[test]
    fn test_sign_proof_data_returns_65_bytes() {
        let (private_key, host_address) = generate_test_keypair();
        let proof_hash = [0xab; 32];
        let tokens = 1000u64;

        let signature = sign_proof_data(&private_key, proof_hash, host_address, tokens).unwrap();

        assert_eq!(signature.len(), 65, "Signature must be exactly 65 bytes");
    }

    #[test]
    fn test_sign_proof_data_recoverable_address() {
        let (private_key, host_address) = generate_test_keypair();
        let proof_hash = [0xcd; 32];
        let tokens = 5000u64;

        let signature = sign_proof_data(&private_key, proof_hash, host_address, tokens).unwrap();

        // Verify the signature using our verify function
        let is_valid =
            verify_proof_signature(&signature, proof_hash, host_address, tokens).unwrap();

        assert!(
            is_valid,
            "Recovered address should match the signing address"
        );
    }

    #[test]
    fn test_sign_proof_data_different_tokens_different_signature() {
        let (private_key, host_address) = generate_test_keypair();
        let proof_hash = [0xef; 32];

        let signature_1000 =
            sign_proof_data(&private_key, proof_hash, host_address, 1000).unwrap();
        let signature_2000 =
            sign_proof_data(&private_key, proof_hash, host_address, 2000).unwrap();

        assert_ne!(
            signature_1000, signature_2000,
            "Different token counts must produce different signatures"
        );
    }

    #[test]
    fn test_sign_proof_data_different_proof_hash_different_signature() {
        let (private_key, host_address) = generate_test_keypair();
        let tokens = 1000u64;

        let proof_hash_1 = [0x11; 32];
        let proof_hash_2 = [0x22; 32];

        let signature_1 =
            sign_proof_data(&private_key, proof_hash_1, host_address, tokens).unwrap();
        let signature_2 =
            sign_proof_data(&private_key, proof_hash_2, host_address, tokens).unwrap();

        assert_ne!(
            signature_1, signature_2,
            "Different proof hashes must produce different signatures"
        );
    }

    #[test]
    fn test_sign_proof_data_v_value_is_27_or_28() {
        let (private_key, host_address) = generate_test_keypair();
        let proof_hash = [0x99; 32];
        let tokens = 100u64;

        let signature = sign_proof_data(&private_key, proof_hash, host_address, tokens).unwrap();

        let v = signature[64];
        assert!(
            v == 27 || v == 28,
            "v value must be 27 or 28 for Ethereum compatibility, got {}",
            v
        );
    }

    #[test]
    fn test_sign_proof_data_wrong_address_fails_verification() {
        let (private_key, host_address) = generate_test_keypair();
        let (_, wrong_address) = generate_test_keypair_2();
        let proof_hash = [0x55; 32];
        let tokens = 100u64;

        let signature = sign_proof_data(&private_key, proof_hash, host_address, tokens).unwrap();

        // Verify with wrong address should fail
        let is_valid =
            verify_proof_signature(&signature, proof_hash, wrong_address, tokens).unwrap();

        assert!(
            !is_valid,
            "Verification should fail with wrong host address"
        );
    }

    #[test]
    fn test_sign_proof_data_wrong_tokens_fails_verification() {
        let (private_key, host_address) = generate_test_keypair();
        let proof_hash = [0x66; 32];

        let signature = sign_proof_data(&private_key, proof_hash, host_address, 1000).unwrap();

        // Verify with wrong token count should fail
        let is_valid =
            verify_proof_signature(&signature, proof_hash, host_address, 2000).unwrap();

        assert!(
            !is_valid,
            "Verification should fail with wrong token count"
        );
    }

    #[test]
    fn test_encode_proof_data_tokens_big_endian() {
        let proof_hash = [0u8; 32];
        let host_address = Address::zero();
        let tokens = 0x0102030405060708u64;

        let encoded = encode_proof_data(proof_hash, host_address, tokens);

        // Tokens should be in the last 32 bytes, big-endian, zero-padded left
        // Position: 32 (proof_hash) + 20 (address) = 52, then 32 bytes for tokens
        let tokens_portion = &encoded[52..84];

        // First 24 bytes should be zeros (padding)
        assert_eq!(&tokens_portion[0..24], &[0u8; 24]);

        // Last 8 bytes should be the u64 in big-endian
        assert_eq!(
            &tokens_portion[24..32],
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
        );
    }
}
