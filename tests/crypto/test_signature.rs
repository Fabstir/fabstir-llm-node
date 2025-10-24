// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! TDD Tests for ECDSA Signature Recovery
//!
//! These tests define the expected behavior of signature recovery
//! functions BEFORE implementation. Following strict TDD methodology.

use fabstir_llm_node::crypto::recover_client_address;
use k256::{
    ecdsa::{signature::Signer, Signature, SigningKey},
    elliptic_curve::sec1::ToEncodedPoint,
};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use tiny_keccak::{Hasher, Keccak};

/// Helper to create Ethereum address from public key using Keccak-256
fn pubkey_to_address(public_key: &k256::PublicKey) -> String {
    // Get uncompressed public key (65 bytes: 0x04 + x + y)
    let encoded_point = public_key.to_encoded_point(false);
    let uncompressed = encoded_point.as_bytes();

    // Remove 0x04 prefix, hash with Keccak-256, take last 20 bytes
    let mut hasher = Keccak::v256();
    let mut hash = [0u8; 32];
    hasher.update(&uncompressed[1..]); // Skip 0x04 prefix
    hasher.finalize(&mut hash);

    let address_bytes = &hash[12..]; // Take last 20 bytes
    format!("0x{}", hex::encode(address_bytes))
}

#[test]
fn test_recover_client_address_valid() {
    // Generate a test keypair
    let signing_key = SigningKey::random(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    let public_key = k256::PublicKey::from(verifying_key);
    let expected_address = pubkey_to_address(&public_key);

    // Create a message and sign it
    let message = b"test message for signature recovery";
    let message_hash = Sha256::digest(message);

    // Sign the message
    let signature: Signature = signing_key.sign(message);
    let signature_bytes = signature.to_bytes();

    // Create compact signature (64 bytes r+s) + recovery_id (1 byte)
    let mut compact_sig = [0u8; 65];
    compact_sig[..64].copy_from_slice(&signature_bytes[..]);

    // Try both recovery IDs (0 and 1) to find which one works
    for recovery_id in 0..2 {
        compact_sig[64] = recovery_id;

        // Attempt to recover address
        let result = recover_client_address(&compact_sig, message_hash.as_slice());

        if let Ok(recovered_address) = result {
            // Check if this recovery ID produces the correct address
            if recovered_address == expected_address {
                // Success! Recovery ID worked
                return;
            }
        }
    }

    // If we get here, neither recovery ID worked
    panic!("Failed to recover correct address with either recovery ID");
}

#[test]
fn test_ethereum_address_format() {
    // Generate test keypair
    let signing_key = SigningKey::random(&mut OsRng);
    let verifying_key = signing_key.verifying_key();

    // Create and sign a message
    let message = b"address format test";
    let message_hash = Sha256::digest(message);
    let signature: Signature = signing_key.sign(message);
    let signature_bytes = signature.to_bytes();

    // Create compact signature
    let mut compact_sig = [0u8; 65];
    compact_sig[..64].copy_from_slice(&signature_bytes[..]);
    compact_sig[64] = 0; // Try recovery ID 0

    // Recover address
    let result = recover_client_address(&compact_sig, message_hash.as_slice());

    if let Ok(address) = result {
        // Ethereum address format checks
        assert!(address.starts_with("0x"), "Address should start with 0x");
        assert_eq!(
            address.len(),
            42,
            "Address should be 42 characters (0x + 40 hex)"
        );

        // Check all characters after 0x are valid hex
        let hex_part = &address[2..];
        assert!(
            hex_part.chars().all(|c| c.is_ascii_hexdigit()),
            "Address should only contain hex digits"
        );
    } else {
        // Try recovery ID 1
        compact_sig[64] = 1;
        let result = recover_client_address(&compact_sig, message_hash.as_slice());
        assert!(
            result.is_ok(),
            "Signature recovery should succeed with valid signature"
        );

        let address = result.unwrap();
        assert!(address.starts_with("0x"));
        assert_eq!(address.len(), 42);
    }
}

#[test]
fn test_invalid_signature_size() {
    // Test with signature that's too short
    let short_sig = [0u8; 32];
    let message_hash = Sha256::digest(b"test");

    let result = recover_client_address(&short_sig, message_hash.as_slice());
    assert!(result.is_err(), "Should reject signature that's too short");
    assert!(result.unwrap_err().to_string().contains("65 bytes"));
}

#[test]
fn test_invalid_signature_too_long() {
    // Test with signature that's too long
    let long_sig = [0u8; 100];
    let message_hash = Sha256::digest(b"test");

    let result = recover_client_address(&long_sig, message_hash.as_slice());
    assert!(result.is_err(), "Should reject signature that's too long");
}

#[test]
fn test_invalid_recovery_id() {
    // Create a valid signature but with invalid recovery ID
    let signing_key = SigningKey::random(&mut OsRng);
    let message = b"test message";
    let message_hash = Sha256::digest(message);
    let signature: Signature = signing_key.sign(message);
    let signature_bytes = signature.to_bytes();

    let mut compact_sig = [0u8; 65];
    compact_sig[..64].copy_from_slice(&signature_bytes[..]);
    compact_sig[64] = 5; // Invalid recovery ID (should be 0, 1, 2, or 3)

    let result = recover_client_address(&compact_sig, message_hash.as_slice());
    assert!(result.is_err(), "Should reject invalid recovery ID");
}

#[test]
fn test_signature_deterministic() {
    // Same signature and message should always produce same address
    let signing_key = SigningKey::random(&mut OsRng);
    let message = b"deterministic test";
    let message_hash = Sha256::digest(message);

    // NOTE: ECDSA signatures are not deterministic by default (use RFC 6979 for that)
    // So we sign once and recover twice from the same signature
    let signature: Signature = signing_key.sign(message);
    let signature_bytes = signature.to_bytes();

    let mut compact_sig = [0u8; 65];
    compact_sig[..64].copy_from_slice(&signature_bytes[..]);

    // Try both recovery IDs to find the correct one
    let mut address1 = None;
    for recovery_id in 0..2 {
        compact_sig[64] = recovery_id;
        if let Ok(addr) = recover_client_address(&compact_sig, message_hash.as_slice()) {
            address1 = Some((addr, recovery_id));
            break;
        }
    }

    assert!(
        address1.is_some(),
        "Should recover address on first attempt"
    );
    let (address1, recovery_id) = address1.unwrap();

    // Recover again with same signature and recovery ID
    compact_sig[64] = recovery_id;
    let address2 = recover_client_address(&compact_sig, message_hash.as_slice()).unwrap();

    assert_eq!(
        address1, address2,
        "Same signature should produce same address"
    );
}

#[test]
fn test_different_messages_different_addresses() {
    // NOTE: This test verifies that different messages produce different signatures,
    // but both recover to the same signer address
    let signing_key = SigningKey::random(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    let public_key = k256::PublicKey::from(verifying_key);
    let expected_address = pubkey_to_address(&public_key);

    let message1 = b"message one";
    let message2 = b"message two";

    let hash1 = Sha256::digest(message1);
    let hash2 = Sha256::digest(message2);

    let sig1: Signature = signing_key.sign(message1);
    let sig2: Signature = signing_key.sign(message2);

    // Signatures should be different
    assert_ne!(
        sig1.to_bytes(),
        sig2.to_bytes(),
        "Different messages should produce different signatures"
    );

    // But both should recover to the same signer address
    let sig1_bytes = sig1.to_bytes();
    let sig2_bytes = sig2.to_bytes();

    let mut compact_sig1 = [0u8; 65];
    let mut compact_sig2 = [0u8; 65];
    compact_sig1[..64].copy_from_slice(&sig1_bytes[..]);
    compact_sig2[..64].copy_from_slice(&sig2_bytes[..]);

    // Find correct recovery IDs that match expected address
    let mut addr1 = None;
    let mut addr2 = None;

    for recovery_id in 0..4 {
        compact_sig1[64] = recovery_id;
        if let Ok(addr) = recover_client_address(&compact_sig1, hash1.as_slice()) {
            if addr == expected_address {
                addr1 = Some(addr);
                break;
            }
        }
    }

    for recovery_id in 0..4 {
        compact_sig2[64] = recovery_id;
        if let Ok(addr) = recover_client_address(&compact_sig2, hash2.as_slice()) {
            if addr == expected_address {
                addr2 = Some(addr);
                break;
            }
        }
    }

    assert!(
        addr1.is_some(),
        "First signature should recover successfully"
    );
    assert!(
        addr2.is_some(),
        "Second signature should recover successfully"
    );

    let addr1_value = addr1.unwrap();
    let addr2_value = addr2.unwrap();

    assert_eq!(
        addr1_value, addr2_value,
        "Both should recover to same signer address"
    );
    assert_eq!(
        addr1_value, expected_address,
        "Recovered address should match expected"
    );
}

#[test]
fn test_corrupted_signature() {
    // Create a valid signature then corrupt it
    let signing_key = SigningKey::random(&mut OsRng);
    let message = b"test message";
    let message_hash = Sha256::digest(message);
    let signature: Signature = signing_key.sign(message);
    let signature_bytes = signature.to_bytes();

    let mut compact_sig = [0u8; 65];
    compact_sig[..64].copy_from_slice(&signature_bytes[..]);
    compact_sig[64] = 0;

    // Corrupt the signature by flipping bits
    compact_sig[10] ^= 0xFF;
    compact_sig[20] ^= 0xFF;

    // Recovery should fail or produce wrong address
    let result = recover_client_address(&compact_sig, message_hash.as_slice());

    if let Ok(corrupted_address) = result {
        // If it succeeds, it should produce a different address than expected
        let verifying_key = signing_key.verifying_key();
        let public_key = k256::PublicKey::from(verifying_key);
        let expected_address = pubkey_to_address(&public_key);

        assert_ne!(
            corrupted_address, expected_address,
            "Corrupted signature should not recover correct address"
        );
    }
    // Alternatively, recovery might fail entirely, which is also acceptable
}

#[test]
fn test_wrong_message_hash() {
    // Sign one message but try to recover with different message hash
    let signing_key = SigningKey::random(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    let public_key = k256::PublicKey::from(verifying_key);
    let expected_address = pubkey_to_address(&public_key);

    let original_message = b"original message";
    let wrong_message = b"wrong message";

    let original_hash = Sha256::digest(original_message);
    let wrong_hash = Sha256::digest(wrong_message);

    let signature: Signature = signing_key.sign(original_message);
    let signature_bytes = signature.to_bytes();

    let mut compact_sig = [0u8; 65];
    compact_sig[..64].copy_from_slice(&signature_bytes[..]);

    // Try to recover with wrong message hash
    let mut recovered_with_wrong = None;
    for recovery_id in 0..2 {
        compact_sig[64] = recovery_id;
        if let Ok(addr) = recover_client_address(&compact_sig, wrong_hash.as_slice()) {
            recovered_with_wrong = Some(addr);
            break;
        }
    }

    if let Some(wrong_address) = recovered_with_wrong {
        // If recovery succeeds, it should produce wrong address
        assert_ne!(
            wrong_address, expected_address,
            "Wrong message hash should not recover correct address"
        );
    }
    // Alternatively, recovery might fail, which is also acceptable behavior
}

#[test]
fn test_recovery_id_affects_result() {
    // Same signature with different recovery IDs should produce different results
    let signing_key = SigningKey::random(&mut OsRng);
    let message = b"test message";
    let message_hash = Sha256::digest(message);
    let signature: Signature = signing_key.sign(message);
    let signature_bytes = signature.to_bytes();

    let mut compact_sig = [0u8; 65];
    compact_sig[..64].copy_from_slice(&signature_bytes[..]);

    // Try recovery with ID 0
    compact_sig[64] = 0;
    let result0 = recover_client_address(&compact_sig, message_hash.as_slice());

    // Try recovery with ID 1
    compact_sig[64] = 1;
    let result1 = recover_client_address(&compact_sig, message_hash.as_slice());

    // At least one should succeed, and if both succeed they should differ
    // (unless by coincidence both IDs recover to same address, which is unlikely)
    assert!(
        result0.is_ok() || result1.is_ok(),
        "At least one recovery ID should work"
    );

    if result0.is_ok() && result1.is_ok() {
        // Both succeeded - they should produce different addresses
        // (one will be the correct signer, one will be a different public key)
        let addr0 = result0.unwrap();
        let addr1 = result1.unwrap();

        // Exactly one should match the expected address
        let verifying_key = signing_key.verifying_key();
        let public_key = k256::PublicKey::from(verifying_key);
        let expected_address = pubkey_to_address(&public_key);

        let matches0 = addr0 == expected_address;
        let matches1 = addr1 == expected_address;

        assert!(
            matches0 ^ matches1,
            "Exactly one recovery ID should produce correct address"
        );
    }
}

#[test]
fn test_empty_message_hash() {
    // Test with empty message hash
    let signing_key = SigningKey::random(&mut OsRng);
    let empty_message = b"";
    let message_hash = Sha256::digest(empty_message);
    let signature: Signature = signing_key.sign(empty_message);
    let signature_bytes = signature.to_bytes();

    let mut compact_sig = [0u8; 65];
    compact_sig[..64].copy_from_slice(&signature_bytes[..]);

    // Should be able to recover even from empty message signature
    let mut recovered = false;
    for recovery_id in 0..2 {
        compact_sig[64] = recovery_id;
        if recover_client_address(&compact_sig, message_hash.as_slice()).is_ok() {
            recovered = true;
            break;
        }
    }

    assert!(
        recovered,
        "Should be able to recover from empty message signature"
    );
}
