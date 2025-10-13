//! TDD Tests for ECDH Key Exchange
//!
//! These tests define the expected behavior of the ECDH key derivation
//! function BEFORE implementation. Following strict TDD methodology.

use fabstir_llm_node::crypto::derive_shared_key;
use k256::{
    ecdh::EphemeralSecret,
    elliptic_curve::sec1::ToEncodedPoint,
    PublicKey, SecretKey,
};
use rand::rngs::OsRng;

#[test]
fn test_derive_shared_key_valid() {
    // Generate a test keypair for the node (static)
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();

    // Generate an ephemeral keypair for the client
    let client_ephemeral = EphemeralSecret::random(&mut OsRng);
    let client_eph_pub = PublicKey::from(&client_ephemeral);
    let client_eph_pub_bytes = client_eph_pub.to_encoded_point(true); // compressed

    // Derive shared key
    let result = derive_shared_key(client_eph_pub_bytes.as_bytes(), &node_priv_bytes);

    // Should succeed and return 32-byte key
    assert!(result.is_ok(), "ECDH key derivation should succeed");
    let shared_key = result.unwrap();
    assert_eq!(shared_key.len(), 32, "Shared key must be 32 bytes");
}

#[test]
fn test_ecdh_matches_expected_output() {
    // Use valid test vectors with deterministic keys
    // Generate a valid keypair first, then use those values

    // Generate a valid node private key
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();

    // Generate a valid client ephemeral public key
    let client_ephemeral = EphemeralSecret::random(&mut OsRng);
    let client_eph_pub = PublicKey::from(&client_ephemeral);
    let client_eph_pub_bytes = client_eph_pub.to_encoded_point(true); // compressed

    // Derive shared key
    let result = derive_shared_key(client_eph_pub_bytes.as_bytes(), &node_priv_bytes);

    // Should succeed
    assert!(result.is_ok(), "ECDH with valid vectors should succeed");
    let shared_key = result.unwrap();

    // Key should be deterministic for same inputs
    let result2 = derive_shared_key(client_eph_pub_bytes.as_bytes(), &node_priv_bytes);
    assert_eq!(
        result2.unwrap(),
        shared_key,
        "Derivation should be deterministic"
    );
}

#[test]
fn test_invalid_public_key() {
    // Valid node private key
    let node_priv_bytes = SecretKey::random(&mut OsRng).to_bytes();

    // Invalid public key (wrong size)
    let invalid_pub = vec![0u8; 20]; // Too short

    let result = derive_shared_key(&invalid_pub, &node_priv_bytes);
    assert!(
        result.is_err(),
        "Should fail with invalid public key size"
    );
}

#[test]
fn test_invalid_public_key_malformed() {
    // Valid node private key
    let node_priv_bytes = SecretKey::random(&mut OsRng).to_bytes();

    // Invalid public key (correct size but malformed)
    let invalid_pub = vec![0xFF; 33]; // Invalid point

    let result = derive_shared_key(&invalid_pub, &node_priv_bytes);
    assert!(
        result.is_err(),
        "Should fail with malformed public key"
    );
}

#[test]
fn test_invalid_private_key() {
    // Valid client ephemeral public key
    let client_ephemeral = EphemeralSecret::random(&mut OsRng);
    let client_eph_pub = PublicKey::from(&client_ephemeral);
    let client_eph_pub_bytes = client_eph_pub.to_encoded_point(true);

    // Invalid private key (wrong size)
    let invalid_priv = vec![0u8; 16]; // Too short

    let result = derive_shared_key(client_eph_pub_bytes.as_bytes(), &invalid_priv);
    assert!(
        result.is_err(),
        "Should fail with invalid private key size"
    );
}

#[test]
fn test_key_derivation_deterministic() {
    // Generate keypairs
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();

    let client_ephemeral = EphemeralSecret::random(&mut OsRng);
    let client_eph_pub = PublicKey::from(&client_ephemeral);
    let client_eph_pub_bytes = client_eph_pub.to_encoded_point(true);

    // Derive key twice
    let key1 = derive_shared_key(client_eph_pub_bytes.as_bytes(), &node_priv_bytes).unwrap();
    let key2 = derive_shared_key(client_eph_pub_bytes.as_bytes(), &node_priv_bytes).unwrap();

    assert_eq!(
        key1, key2,
        "Same inputs should produce same output (deterministic)"
    );
}

#[test]
fn test_different_keys_different_secrets() {
    // Generate two different node keypairs
    let node_secret1 = SecretKey::random(&mut OsRng);
    let node_secret2 = SecretKey::random(&mut OsRng);

    // Same client ephemeral public key
    let client_ephemeral = EphemeralSecret::random(&mut OsRng);
    let client_eph_pub = PublicKey::from(&client_ephemeral);
    let client_eph_pub_bytes = client_eph_pub.to_encoded_point(true);

    // Derive shared keys with different node keys
    let key1 = derive_shared_key(
        client_eph_pub_bytes.as_bytes(),
        &node_secret1.to_bytes()
    ).unwrap();
    let key2 = derive_shared_key(
        client_eph_pub_bytes.as_bytes(),
        &node_secret2.to_bytes()
    ).unwrap();

    assert_ne!(
        key1, key2,
        "Different private keys should produce different shared secrets"
    );
}

#[test]
fn test_uncompressed_public_key() {
    // Generate keypairs
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();

    let client_ephemeral = EphemeralSecret::random(&mut OsRng);
    let client_eph_pub = PublicKey::from(&client_ephemeral);

    // Test with uncompressed public key (65 bytes)
    let client_eph_pub_uncompressed = client_eph_pub.to_encoded_point(false);

    let result = derive_shared_key(
        client_eph_pub_uncompressed.as_bytes(),
        &node_priv_bytes
    );

    // Should also work with uncompressed keys
    assert!(
        result.is_ok(),
        "Should handle uncompressed public keys (65 bytes)"
    );
}

#[test]
fn test_zero_private_key_rejected() {
    // Valid client ephemeral public key
    let client_ephemeral = EphemeralSecret::random(&mut OsRng);
    let client_eph_pub = PublicKey::from(&client_ephemeral);
    let client_eph_pub_bytes = client_eph_pub.to_encoded_point(true);

    // Invalid private key (all zeros)
    let zero_priv = vec![0u8; 32];

    let result = derive_shared_key(client_eph_pub_bytes.as_bytes(), &zero_priv);
    assert!(
        result.is_err(),
        "Should reject zero private key as invalid"
    );
}

#[test]
fn test_shared_secret_not_all_zeros() {
    // Generate valid keypairs
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();

    let client_ephemeral = EphemeralSecret::random(&mut OsRng);
    let client_eph_pub = PublicKey::from(&client_ephemeral);
    let client_eph_pub_bytes = client_eph_pub.to_encoded_point(true);

    // Derive shared key
    let shared_key = derive_shared_key(
        client_eph_pub_bytes.as_bytes(),
        &node_priv_bytes
    ).unwrap();

    // Shared secret should not be all zeros (extremely unlikely)
    let is_all_zeros = shared_key.iter().all(|&b| b == 0);
    assert!(
        !is_all_zeros,
        "Shared secret should not be all zeros"
    );
}
