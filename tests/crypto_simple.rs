//! Simple crypto tests that can run without linking issues

use fabstir_llm_node::crypto::{decrypt_with_aead, derive_shared_key, encrypt_with_aead, recover_client_address};
use k256::{
    ecdh::EphemeralSecret,
    ecdsa::{SigningKey, signature::Signer},
    elliptic_curve::sec1::ToEncodedPoint,
    PublicKey,
    SecretKey,
};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use tiny_keccak::{Hasher, Keccak};

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

#[test]
fn test_encryption_basic() {
    // Generate test data
    let plaintext = b"Hello, encryption!";
    let mut key = [0u8; 32];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut key);
    OsRng.fill_bytes(&mut nonce);
    let aad = b"test-aad";

    // Encrypt
    let ciphertext = encrypt_with_aead(plaintext, &nonce, aad, &key);
    assert!(ciphertext.is_ok(), "Encryption should succeed");
    let ciphertext = ciphertext.unwrap();

    // Decrypt
    let decrypted = decrypt_with_aead(&ciphertext, &nonce, aad, &key);
    assert!(decrypted.is_ok(), "Decryption should succeed");
    assert_eq!(decrypted.unwrap(), plaintext, "Should decrypt to original");
}

#[test]
fn test_encryption_wrong_key() {
    let plaintext = b"Secret";
    let mut key1 = [0u8; 32];
    let mut key2 = [0u8; 32];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut key1);
    OsRng.fill_bytes(&mut key2);
    OsRng.fill_bytes(&mut nonce);

    let ciphertext = encrypt_with_aead(plaintext, &nonce, b"", &key1).unwrap();
    let result = decrypt_with_aead(&ciphertext, &nonce, b"", &key2);
    assert!(result.is_err(), "Wrong key should fail");
}

#[test]
fn test_signature_recovery_basic() {
    // Helper to create Ethereum address from public key
    fn pubkey_to_address(public_key: &k256::PublicKey) -> String {
        let encoded_point = public_key.to_encoded_point(false);
        let uncompressed = encoded_point.as_bytes();

        let mut hasher = Keccak::v256();
        let mut hash = [0u8; 32];
        hasher.update(&uncompressed[1..]); // Skip 0x04 prefix
        hasher.finalize(&mut hash);

        let address_bytes = &hash[12..];
        format!("0x{}", hex::encode(address_bytes))
    }

    // Generate test keypair
    let signing_key = SigningKey::random(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    let public_key = k256::PublicKey::from(verifying_key);
    let expected_address = pubkey_to_address(&public_key);

    // Sign a message
    let message = b"test signature recovery";
    let message_hash = Sha256::digest(message);
    let signature: k256::ecdsa::Signature = signing_key.sign(message);
    let signature_bytes = signature.to_bytes();

    // Create compact signature with recovery ID
    let mut compact_sig = [0u8; 65];
    compact_sig[..64].copy_from_slice(&signature_bytes[..]);

    // Try both recovery IDs to find the correct one
    for recovery_id in 0..2 {
        compact_sig[64] = recovery_id;

        if let Ok(recovered_address) = recover_client_address(&compact_sig, message_hash.as_slice()) {
            if recovered_address == expected_address {
                // Success!
                assert_eq!(recovered_address.len(), 42, "Address should be 42 chars");
                assert!(recovered_address.starts_with("0x"), "Address should start with 0x");
                return;
            }
        }
    }

    panic!("Failed to recover correct address with either recovery ID");
}

#[test]
fn test_signature_invalid_size() {
    let short_sig = [0u8; 32];
    let message_hash = Sha256::digest(b"test");

    let result = recover_client_address(&short_sig, message_hash.as_slice());
    assert!(result.is_err(), "Should reject invalid signature size");
    assert!(result.unwrap_err().to_string().contains("65 bytes"));
}
