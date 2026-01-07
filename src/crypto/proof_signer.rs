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

    // Test will be implemented in Sub-phase 1.2
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
}
