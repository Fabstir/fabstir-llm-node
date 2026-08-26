// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Session Initialization Decryption
//!
//! Implements decryption and verification of encrypted session initialization payloads.
//! Combines ECDH, XChaCha20-Poly1305, and ECDSA signature recovery.

use super::{decrypt_with_aead, derive_shared_key, recover_client_address};
use crate::api::websocket::message_types::VectorDatabaseInfo;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::info;

/// Encrypted session initialization payload from client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedSessionPayload {
    /// Client's ephemeral public key (33 bytes compressed or 65 bytes uncompressed)
    pub eph_pub: Vec<u8>,
    /// Encrypted session data
    pub ciphertext: Vec<u8>,
    /// 24-byte nonce for XChaCha20-Poly1305
    pub nonce: Vec<u8>,
    /// 65-byte ECDSA signature (r + s + recovery_id)
    pub signature: Vec<u8>,
    /// Additional authenticated data
    pub aad: Vec<u8>,
}

/// The parts of the client's HKDF context that are also SIGNED, and so must be
/// reproduced exactly to recover the signer. They travel on the wire; when a
/// client omits them the SDK's own defaults apply (32 zero bytes, empty info).
#[derive(Debug, Clone)]
pub struct SigContext {
    pub salt: Vec<u8>,
    pub info: Vec<u8>,
}

impl Default for SigContext {
    fn default() -> Self {
        Self {
            salt: vec![0u8; 32],
            info: Vec::new(),
        }
    }
}

/// Decrypted session initialization data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInitData {
    /// Job ID from blockchain
    pub job_id: String,
    /// Model name to use for inference
    pub model_name: String,
    /// 32-byte session key for subsequent message encryption
    pub session_key: [u8; 32],
    /// Price per token with PRICE_PRECISION multiplier (1000x).
    /// To get USD per million tokens, divide by 1000.
    /// Example: price_per_token=5000 means $5/million tokens.
    pub price_per_token: u64,
    /// Client's Ethereum address (recovered from signature)
    pub client_address: String,
    /// Optional S5 vector database information for RAG
    pub vector_database: Option<VectorDatabaseInfo>,
    /// User's recovery public key for encrypted checkpoint deltas (SDK v1.8.7+)
    /// Compressed secp256k1 public key (33 bytes = 66 hex chars + 0x prefix)
    pub recovery_public_key: Option<String>,
    /// Interface E.2 serve-back: the session-scoped LoRA adapter to apply to
    /// THIS session's requests only. Absent on every ordinary session.
    pub lora: Option<crate::training::serve::LoraRequest>,
}

/// Internal structure for parsing decrypted JSON payload
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionDataJson {
    job_id: String,
    model_name: String,
    session_key: String,
    price_per_token: u64,
    vector_database: Option<VectorDatabaseInfo>,
    /// User's recovery public key for encrypted checkpoint deltas (SDK v1.8.7+)
    recovery_public_key: Option<String>,
    /// E.2 `lora`. NOTE the wire spells the CID field `manifestCID`, which
    /// this struct's `rename_all = "camelCase"` would mangle to `manifestCid`
    /// and then silently drop — `LoraRequest` carries its own explicit
    /// `#[serde(rename)]` for exactly that reason. Do not "tidy" it away.
    lora: Option<crate::training::serve::LoraRequest>,
}

/// Rebuild the message the client signed (E2EE v1).
///
/// The SDK signs `sha256("E2EEv1|" ‖ ephPub ‖ "|" ‖ recipientPub ‖ "|" ‖ salt ‖
/// "|" ‖ nonce ‖ "|" ‖ info [‖ "|" ‖ aad])` with the client's STATIC key —
/// @fabstir/sdk-core `makeSigMessage`. The `aad` separator and value are
/// appended ONLY when aad is non-empty, which is load bearing: appending an
/// empty one changes the hash.
///
/// This node previously recovered over `sha256(ciphertext)` instead. That is a
/// different message, so recovery returned a well-formed but meaningless
/// address that changed with every ciphertext — an "identity" that was never
/// the client's. It went unnoticed because the only consumer is the FC1.6 gate,
/// which was not enforced anywhere until a vault address was configured.
pub fn e2ee_sig_message(
    eph_pub: &[u8],
    recipient_pub: &[u8],
    salt: &[u8],
    nonce: &[u8],
    info: &[u8],
    aad: &[u8],
) -> [u8; 32] {
    let mut message: Vec<u8> = Vec::new();
    message.extend_from_slice(b"E2EEv1|");
    message.extend_from_slice(eph_pub);
    message.push(b'|');
    message.extend_from_slice(recipient_pub);
    message.push(b'|');
    message.extend_from_slice(salt);
    message.push(b'|');
    message.extend_from_slice(nonce);
    message.push(b'|');
    message.extend_from_slice(info);
    if !aad.is_empty() {
        message.push(b'|');
        message.extend_from_slice(aad);
    }
    let digest = Sha256::digest(&message);
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_slice());
    out
}

/// Decrypt and verify encrypted session initialization payload
///
/// This function orchestrates the complete session initialization decryption:
/// 1. Performs ECDH with client's ephemeral public key
/// 2. Decrypts session data with XChaCha20-Poly1305
/// 3. Parses decrypted JSON
/// 4. Recovers client address from signature over ciphertext
/// 5. Returns session data + client address
///
/// # Arguments
/// * `payload` - Encrypted session initialization payload from client
/// * `node_private_key` - Node's secp256k1 private key (32 bytes)
///
/// # Returns
/// * `Ok(SessionInitData)` - Decrypted and verified session data
/// * `Err` - If decryption, verification, or parsing fails
pub fn decrypt_session_init(
    payload: &EncryptedSessionPayload,
    node_private_key: &[u8],
) -> Result<SessionInitData> {
    decrypt_session_init_with_context(payload, node_private_key, &SigContext::default())
}

/// As `decrypt_session_init`, but with the client's actual salt/info rather
/// than the SDK defaults. Both are part of the SIGNED message, so a client that
/// sends its own must be verified against those values or the recovered address
/// is meaningless.
pub fn decrypt_session_init_with_context(
    payload: &EncryptedSessionPayload,
    node_private_key: &[u8],
    sig_context: &SigContext,
) -> Result<SessionInitData> {
    // Validate payload sizes
    if payload.eph_pub.is_empty() {
        return Err(anyhow!("Ephemeral public key is empty"));
    }
    if payload.ciphertext.is_empty() {
        return Err(anyhow!("Ciphertext is empty"));
    }
    if payload.nonce.len() != 24 {
        return Err(anyhow!(
            "Invalid nonce size: expected 24 bytes, got {}",
            payload.nonce.len()
        ));
    }
    if payload.signature.len() != 65 {
        return Err(anyhow!(
            "Invalid signature size: expected 65 bytes, got {}",
            payload.signature.len()
        ));
    }
    if node_private_key.len() != 32 {
        return Err(anyhow!(
            "Invalid node private key size: expected 32 bytes, got {}",
            node_private_key.len()
        ));
    }

    // Step 1: Perform ECDH to derive shared key
    let shared_key = derive_shared_key(&payload.eph_pub, node_private_key)
        .map_err(|e| anyhow!("ECDH key derivation failed: {}", e))?;

    // Step 2: Decrypt ciphertext with XChaCha20-Poly1305
    let nonce: [u8; 24] = payload.nonce.as_slice().try_into().map_err(|_| {
        anyhow!(
            "Failed to convert nonce to fixed-size array: {} bytes",
            payload.nonce.len()
        )
    })?;

    let plaintext = decrypt_with_aead(&payload.ciphertext, &nonce, &payload.aad, &shared_key)
        .map_err(|e| anyhow!("Decryption failed: {}", e))?;

    // Step 3: Parse decrypted JSON
    let plaintext_str = std::str::from_utf8(&plaintext)
        .map_err(|e| anyhow!("Decrypted data is not valid UTF-8: {}", e))?;

    let session_data: SessionDataJson = serde_json::from_str(plaintext_str)
        .map_err(|e| anyhow!("Failed to parse session data JSON: {}", e))?;

    // Log whether recovery_public_key was provided (determines checkpoint encryption)
    if session_data.recovery_public_key.is_some() {
        info!("🔑 Session init contains recoveryPublicKey - encrypted checkpoints enabled");
    }

    // Step 4: Extract and validate session key (hex-encoded 32 bytes)
    let session_key_hex = session_data
        .session_key
        .strip_prefix("0x")
        .unwrap_or(&session_data.session_key);

    let session_key_bytes = hex::decode(session_key_hex)
        .map_err(|e| anyhow!("Failed to decode session key hex: {}", e))?;

    if session_key_bytes.len() != 32 {
        return Err(anyhow!(
            "Invalid session key length: expected 32 bytes, got {}",
            session_key_bytes.len()
        ));
    }

    let mut session_key = [0u8; 32];
    session_key.copy_from_slice(&session_key_bytes);

    // Step 5: Recover the client's STATIC address from the signature over the
    // E2EE v1 message (NOT over the ciphertext — see e2ee_sig_message).
    let recipient_pub = crate::crypto::public_key_from_private(node_private_key)
        .map_err(|e| anyhow!("Could not derive this node's public key: {}", e))?;
    let sig_message = e2ee_sig_message(
        &payload.eph_pub,
        &recipient_pub,
        &sig_context.salt,
        &payload.nonce,
        &sig_context.info,
        &payload.aad,
    );

    let client_address = recover_client_address(&payload.signature, &sig_message)
        .map_err(|e| anyhow!("Signature verification failed: {}", e))?;

    // Step 6: Return complete session initialization data
    Ok(SessionInitData {
        job_id: session_data.job_id,
        model_name: session_data.model_name,
        session_key,
        price_per_token: session_data.price_per_token,
        client_address,
        vector_database: session_data.vector_database,
        lora: session_data.lora,
        recovery_public_key: session_data.recovery_public_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_1129_payload_shape_parses_with_lora() {
        // Session 1129 (2026-08-26): the client CAPTURED this exact shape
        // leaving the browser — keys, casing, nesting — while the node
        // behaved as though lora was absent. This test settles whether the
        // PARSE is the quiet arm: if it passes, the loss is elsewhere.
        let plaintext = r#"{
            "sessionKey": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "jobId": "1129",
            "modelName": "0x892310a339a9c5faaf43c53b8a90fb2a1a1e008ad3f0e455202f4b60878bd650",
            "pricePerToken": 10000,
            "recoveryPublicKey": "0x02aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899",
            "lora": {
                "manifestCID": "urqYSH0QfQhz5Kg182z-Kocp",
                "manifestSha256": "0x0843be66f0c926ac6c6395823023644f5873505315f6cb5a36114087a245e44f",
                "file": "adapter.gguf"
            }
        }"#;
        let parsed: SessionDataJson = serde_json::from_str(plaintext).expect("must parse");
        let lora = parsed.lora.expect("lora must survive the parse — session 1129");
        assert_eq!(lora.file, "adapter.gguf");
        assert_eq!(lora.manifest_cid, "urqYSH0QfQhz5Kg182z-Kocp");
        assert!(lora.manifest_sha256.starts_with("0x0843be66"));
    }

    // Cross-runtime vectors for the E2EE v1 signed message. Generated with the
    // construction copied VERBATIM from @fabstir/sdk-core 1.34.0
    // (dist/index.js:15200 `makeSigMessage`), signed with a known key, so this
    // pins the node to the CLIENT's format rather than to our reading of it.
    //
    // This is the bug these vectors exist to prevent recurring: the node used to
    // recover over sha256(ciphertext), a message no client ever signed, which
    // produced a different bogus address on every attempt.
    const EPH_PUB: &str = "034f355bdcb7cc0af728ef3cceb9615d90684bb5b2ca5f859ab0f0b704075871aa";
    const RECIPIENT_PUB: &str =
        "02466d7fcae563e5cb09a0d1870bb580344804617879a14949cf22285f1bae3f27";
    const NONCE_HEX: &str = "333333333333333333333333333333333333333333333333";
    const EXPECTED_ADDRESS: &str = "0x17c5185167401ed00cf5f5b2fc97d9bbfdb7d025";

    #[test]
    fn sig_message_matches_the_sdk_vector_without_aad() {
        let digest = e2ee_sig_message(
            &hex::decode(EPH_PUB).unwrap(),
            &hex::decode(RECIPIENT_PUB).unwrap(),
            &[0u8; 32],
            &hex::decode(NONCE_HEX).unwrap(),
            &[],
            &[],
        );
        assert_eq!(
            hex::encode(digest),
            "dc280aec449c4f9a887ed1eb3f5a03b0cbbc63db2174d8a4dc2b7c0851caa36c"
        );
    }

    #[test]
    fn sig_message_matches_the_sdk_vector_with_aad() {
        // aad appends a SEPARATOR and the value; appending an empty one would
        // change the hash, which is why the emptiness check is load bearing.
        let digest = e2ee_sig_message(
            &hex::decode(EPH_PUB).unwrap(),
            &hex::decode(RECIPIENT_PUB).unwrap(),
            &[0u8; 32],
            &hex::decode(NONCE_HEX).unwrap(),
            &[],
            b"job-1106",
        );
        assert_eq!(
            hex::encode(digest),
            "a086c48c7b37f9f7af376ba9a67046f01837b32d9ac345df55a1d3072aa34666"
        );
    }

    #[test]
    fn recovers_the_clients_static_address_from_the_sdk_signature() {
        let sig = hex::decode(
            "3a42c8921c8e91303507f7565bc2b843f0cfb1c30532ed91ebaf4fe4637c861609298aa99b553f8471f9278f41bdc69bb3736a30c6055dfbcde8d9494da56da000",
        )
        .unwrap();
        let digest = e2ee_sig_message(
            &hex::decode(EPH_PUB).unwrap(),
            &hex::decode(RECIPIENT_PUB).unwrap(),
            &[0u8; 32],
            &hex::decode(NONCE_HEX).unwrap(),
            &[],
            &[],
        );
        let recovered = super::recover_client_address(&sig, &digest).unwrap();
        assert!(
            recovered.eq_ignore_ascii_case(EXPECTED_ADDRESS),
            "recovered {recovered}, expected {EXPECTED_ADDRESS}"
        );
    }

    #[test]
    fn recovering_over_the_wrong_message_gives_a_different_address() {
        // The old behaviour, pinned so nobody restores it thinking it equivalent.
        let sig = hex::decode(
            "3a42c8921c8e91303507f7565bc2b843f0cfb1c30532ed91ebaf4fe4637c861609298aa99b553f8471f9278f41bdc69bb3736a30c6055dfbcde8d9494da56da000",
        )
        .unwrap();
        let wrong = Sha256::digest(b"any other message");
        let recovered = super::recover_client_address(&sig, wrong.as_slice());
        // It "succeeds" and yields a plausible-looking address that is NOT the
        // client's. That silent plausibility is what made the bug survive.
        if let Ok(addr) = recovered {
            assert!(!addr.eq_ignore_ascii_case(EXPECTED_ADDRESS));
        }
    }

    #[test]
    fn node_public_key_is_compressed_33_bytes() {
        // The client signs the recipient key COMPRESSED; a 65-byte encoding here
        // would hash differently and break every recovery.
        let pk = crate::crypto::public_key_from_private(&[0x22u8; 32]).unwrap();
        assert_eq!(pk.len(), 33);
        assert_eq!(hex::encode(&pk), RECIPIENT_PUB);
    }

    #[test]
    fn test_validate_payload_sizes() {
        let valid_payload = EncryptedSessionPayload {
            eph_pub: vec![0u8; 33],
            ciphertext: vec![0u8; 64],
            nonce: vec![0u8; 24],
            signature: vec![0u8; 65],
            aad: vec![],
        };

        let node_key = [0u8; 32];

        // This will fail during ECDH/decryption, but should pass validation
        let result = decrypt_session_init(&valid_payload, &node_key);
        // Should not fail on size validation
        assert!(result.is_err()); // Will fail on ECDH with invalid keys
    }

    #[test]
    fn test_invalid_nonce_size() {
        let payload = EncryptedSessionPayload {
            eph_pub: vec![0u8; 33],
            ciphertext: vec![0u8; 64],
            nonce: vec![0u8; 16], // Wrong size
            signature: vec![0u8; 65],
            aad: vec![],
        };

        let node_key = [0u8; 32];
        let result = decrypt_session_init(&payload, &node_key);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid nonce size"));
    }

    #[test]
    fn test_invalid_signature_size() {
        let payload = EncryptedSessionPayload {
            eph_pub: vec![0u8; 33],
            ciphertext: vec![0u8; 64],
            nonce: vec![0u8; 24],
            signature: vec![0u8; 64], // Wrong size
            aad: vec![],
        };

        let node_key = [0u8; 32];
        let result = decrypt_session_init(&payload, &node_key);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid signature size"));
    }
}
