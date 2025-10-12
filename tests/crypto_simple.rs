//! Simple crypto tests that can run without linking issues

use fabstir_llm_node::crypto::derive_shared_key;
use k256::{ecdh::EphemeralSecret, PublicKey, SecretKey};
use rand::rngs::OsRng;

#[test]
fn test_ecdh_basic() {
    // Generate test keys
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();

    let client_ephemeral = EphemeralSecret::random(&mut OsRng);
    let client_eph_pub = PublicKey::from(&client_ephemeral);
    let client_eph_pub_bytes = client_eph_pub.to_sec1_bytes();

    // Derive shared key
    let result = derive_shared_key(&client_eph_pub_bytes, &node_priv_bytes);

    assert!(result.is_ok(), "ECDH derivation should succeed");
    let key = result.unwrap();
    assert_eq!(key.len(), 32, "Shared key must be 32 bytes");
}

#[test]
fn test_ecdh_deterministic() {
    // Generate keys once
    let node_secret = SecretKey::random(&mut OsRng);
    let node_priv_bytes = node_secret.to_bytes();

    let client_ephemeral = EphemeralSecret::random(&mut OsRng);
    let client_eph_pub = PublicKey::from(&client_ephemeral);
    let client_eph_pub_bytes = client_eph_pub.to_sec1_bytes();

    // Derive twice
    let key1 = derive_shared_key(&client_eph_pub_bytes, &node_priv_bytes);
    let key2 = derive_shared_key(&client_eph_pub_bytes, &node_priv_bytes);

    assert!(key1.is_ok() && key2.is_ok(), "Derivations should succeed");
    assert_eq!(key1.unwrap(), key2.unwrap(), "Should be deterministic");
}
