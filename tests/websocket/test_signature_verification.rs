// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier};
use fabstir_llm_node::api::websocket::auth::{SignatureConfig, SignatureVerifier};
use rand::rngs::OsRng;
use rand::RngCore;

#[tokio::test]
async fn test_ed25519_signature_generation() {
    let config = SignatureConfig {
        enabled: true,
        algorithm: "ed25519".to_string(),
        public_key: None,  // Will be set by verifier
        private_key: None, // Will be set by verifier
    };

    let verifier = SignatureVerifier::new(config).await.unwrap();

    let message = "Test message for signing";
    let signature = verifier.sign_message(message).await.unwrap();

    // Signature should be 128 hex characters (64 bytes)
    assert_eq!(
        signature.len(),
        128,
        "Ed25519 signature should be 64 bytes (128 hex chars)"
    );

    // Should be valid hex
    assert!(
        signature.chars().all(|c| c.is_ascii_hexdigit()),
        "Signature should be hexadecimal"
    );
}

#[tokio::test]
async fn test_ed25519_signature_verification() {
    let config = SignatureConfig {
        enabled: true,
        algorithm: "ed25519".to_string(),
        public_key: None,
        private_key: None,
    };

    let verifier = SignatureVerifier::new(config).await.unwrap();

    let message = "Message to sign and verify";

    // Sign the message
    let signature = verifier.sign_message(message).await.unwrap();

    // Verify the signature
    let is_valid = verifier
        .verify_signature(message, &signature)
        .await
        .unwrap();
    assert!(is_valid, "Valid signature should verify successfully");
}

#[tokio::test]
async fn test_ed25519_invalid_signature_rejection() {
    let config = SignatureConfig {
        enabled: true,
        algorithm: "ed25519".to_string(),
        public_key: None,
        private_key: None,
    };

    let verifier = SignatureVerifier::new(config).await.unwrap();

    let message = "Original message";
    let signature = verifier.sign_message(message).await.unwrap();

    // Test with modified message
    let is_valid = verifier
        .verify_signature("Modified message", &signature)
        .await
        .unwrap();
    assert!(!is_valid, "Modified message should not verify");

    // Test with modified signature
    let mut bad_signature = signature.clone();
    bad_signature.replace_range(0..2, "FF");
    let is_valid = verifier
        .verify_signature(message, &bad_signature)
        .await
        .unwrap();
    assert!(!is_valid, "Modified signature should not verify");
}

#[tokio::test]
async fn test_ed25519_different_keys_rejection() {
    // Create two different verifiers with different keys
    let config1 = SignatureConfig {
        enabled: true,
        algorithm: "ed25519".to_string(),
        public_key: None,
        private_key: None,
    };

    let config2 = SignatureConfig {
        enabled: true,
        algorithm: "ed25519".to_string(),
        public_key: None,
        private_key: None,
    };

    let verifier1 = SignatureVerifier::new(config1).await.unwrap();
    let verifier2 = SignatureVerifier::new(config2).await.unwrap();

    let message = "Cross-key test message";

    // Sign with verifier1
    let signature = verifier1.sign_message(message).await.unwrap();

    // Try to verify with verifier2 (different keys)
    let is_valid = verifier2
        .verify_signature(message, &signature)
        .await
        .unwrap();
    assert!(!is_valid, "Signature from different key should not verify");
}

#[tokio::test]
async fn test_signature_with_external_keypair() {
    // Generate a signing key externally
    let mut key_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut key_bytes);
    let signing_key = SigningKey::from_bytes(&key_bytes);

    let public_key_hex = hex::encode(signing_key.verifying_key().to_bytes());
    let private_key_hex = hex::encode(key_bytes);

    let config = SignatureConfig {
        enabled: true,
        algorithm: "ed25519".to_string(),
        public_key: Some(public_key_hex.clone()),
        private_key: Some(private_key_hex),
    };

    let verifier = SignatureVerifier::new(config).await.unwrap();

    let message = "Message with external keys";
    let signature = verifier.sign_message(message).await.unwrap();

    // Verify with the same public key
    let config_verify = SignatureConfig {
        enabled: true,
        algorithm: "ed25519".to_string(),
        public_key: Some(public_key_hex),
        private_key: None, // Only need public key for verification
    };

    let verifier2 = SignatureVerifier::new(config_verify).await.unwrap();
    let is_valid = verifier2
        .verify_signature(message, &signature)
        .await
        .unwrap();
    assert!(is_valid, "Signature should verify with matching public key");
}

#[tokio::test]
async fn test_signature_format_validation() {
    let config = SignatureConfig {
        enabled: true,
        algorithm: "ed25519".to_string(),
        public_key: None,
        private_key: None,
    };

    let verifier = SignatureVerifier::new(config).await.unwrap();

    let message = "Test message";

    // Test invalid signature formats
    let too_long = "1".repeat(130);
    let invalid_signatures = vec![
        "not_hex",
        "ZZ1234",          // Invalid hex
        "12",              // Too short
        too_long.as_str(), // Too long
        "",
    ];

    for invalid_sig in invalid_signatures {
        let result = verifier.verify_signature(message, invalid_sig).await;
        assert!(
            result.is_err() || !result.unwrap(),
            "Invalid signature format should be rejected: {}",
            invalid_sig
        );
    }
}

#[tokio::test]
async fn test_signature_disabled_mode() {
    let config = SignatureConfig {
        enabled: false, // Disabled
        algorithm: "ed25519".to_string(),
        public_key: None,
        private_key: None,
    };

    let verifier = SignatureVerifier::new(config).await.unwrap();

    let message = "Test in disabled mode";

    // When disabled, should bypass or return mock
    let signature = verifier.sign_message(message).await.unwrap();
    let is_valid = verifier
        .verify_signature(message, &signature)
        .await
        .unwrap();

    assert!(is_valid, "Should return valid in disabled mode");
}

#[tokio::test]
async fn test_message_signing_consistency() {
    let config = SignatureConfig {
        enabled: true,
        algorithm: "ed25519".to_string(),
        public_key: None,
        private_key: None,
    };

    let verifier = SignatureVerifier::new(config).await.unwrap();

    // Same message should produce same signature (deterministic)
    let message = "Consistency test message";
    let sig1 = verifier.sign_message(message).await.unwrap();
    let sig2 = verifier.sign_message(message).await.unwrap();

    assert_eq!(sig1, sig2, "Same message should produce same signature");

    // Different messages should produce different signatures
    let sig3 = verifier.sign_message("Different message").await.unwrap();
    assert_ne!(
        sig1, sig3,
        "Different messages should produce different signatures"
    );
}

#[tokio::test]
async fn test_batch_signature_verification() {
    let config = SignatureConfig {
        enabled: true,
        algorithm: "ed25519".to_string(),
        public_key: None,
        private_key: None,
    };

    let verifier = SignatureVerifier::new(config).await.unwrap();

    // Sign multiple messages
    let messages = vec![
        "Message 1",
        "Message 2",
        "Message 3",
        "Message 4",
        "Message 5",
    ];

    let mut signatures = Vec::new();
    for msg in &messages {
        let sig = verifier.sign_message(msg).await.unwrap();
        signatures.push(sig);
    }

    // Verify all signatures
    for (msg, sig) in messages.iter().zip(signatures.iter()) {
        let is_valid = verifier.verify_signature(msg, sig).await.unwrap();
        assert!(is_valid, "Batch signature should verify: {}", msg);
    }

    // Cross-verify (should all fail)
    for i in 0..messages.len() {
        let j = (i + 1) % messages.len();
        let is_valid = verifier
            .verify_signature(messages[i], &signatures[j])
            .await
            .unwrap();
        assert!(!is_valid, "Cross-verification should fail");
    }
}

#[tokio::test]
async fn test_concurrent_signature_operations() {
    let config = SignatureConfig {
        enabled: true,
        algorithm: "ed25519".to_string(),
        public_key: None,
        private_key: None,
    };

    let verifier = SignatureVerifier::new(config).await.unwrap();

    // Spawn multiple concurrent signing operations
    let mut handles = vec![];

    for i in 0..10 {
        let verifier_clone = verifier.clone();
        let handle = tokio::spawn(async move {
            let message = format!("Concurrent message {}", i);
            let signature = verifier_clone.sign_message(&message).await.unwrap();
            let is_valid = verifier_clone
                .verify_signature(&message, &signature)
                .await
                .unwrap();
            (message, signature, is_valid)
        });
        handles.push(handle);
    }

    // Wait for all to complete
    for handle in handles {
        let (msg, sig, valid) = handle.await.unwrap();
        assert!(valid, "Concurrent signature should be valid for: {}", msg);
        assert_eq!(sig.len(), 128, "Signature length should be consistent");
    }
}
