// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 3.1 — ECIES key-wrap: seal a model DEK to the TEE's attestation key.
//!
//! The KBS wraps the model's data-encryption key (DEK) to `pk_att` — the
//! attestation-bound public key the CVM proved it holds — so only that CVM can
//! [`unwrap_key`] it. Each wrap uses a fresh ephemeral keypair (forward secrecy)
//! and a fresh nonce, and the shared key is derived with a **wrap-specific HKDF
//! domain tag** so it can never collide with other key-derivation contexts.
//!
//! The three domain-separated HKDF uses in this crate:
//!   1. session-init (`crypto::ecdh::derive_shared_key`): empty info (existing);
//!   2. checkpoint    (`checkpoint::encryption`): `b"checkpoint-delta-encryption-v1"`;
//!   3. key-wrap (here): [`KEY_WRAP_HKDF_INFO`] = `b"key-wrap-v1"`.
//!
//! SECURITY: we deliberately do **not** reuse `crypto::derive_shared_key` — it
//! HKDF-expands the raw ECDH secret with empty info and cannot inject domain
//! separation. We follow the proven `checkpoint::encryption` pattern instead:
//! own ECDH → SHA256(x-coordinate) → HKDF-SHA256 with a wrap-specific info.

use crate::crypto::{decrypt_with_aead, encrypt_with_aead};
use crate::tee::types::{TeeError, TeeResult, WrappedKey};
use hkdf::Hkdf;
use k256::{
    ecdh::diffie_hellman,
    elliptic_curve::sec1::{FromEncodedPoint, ToEncodedPoint},
    EncodedPoint, PublicKey, SecretKey,
};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};

/// HKDF `info` for key-wrap domain separation (distinct from session-init/checkpoint).
pub const KEY_WRAP_HKDF_INFO: &[u8] = b"key-wrap-v1";

/// Generate a fresh ephemeral secp256k1 keypair: `(raw 32-byte secret, compressed 33-byte pub)`.
///
/// Draws CSPRNG bytes and retries on the ~2⁻¹²⁸-probability invalid scalar
/// (matches the crate's `SecretKey::from_slice` keying idiom).
pub fn generate_ephemeral_keypair() -> (Vec<u8>, Vec<u8>) {
    loop {
        let mut sk = [0u8; 32];
        OsRng.fill_bytes(&mut sk);
        if let Ok(secret) = SecretKey::from_slice(&sk) {
            let pub_bytes = secret
                .public_key()
                .to_encoded_point(true)
                .as_bytes()
                .to_vec();
            return (sk.to_vec(), pub_bytes);
        }
    }
}

/// Derive the 32-byte wrap key via ECDH(`local_secret`, `peer_pub`) → SHA256(x) →
/// HKDF-SHA256 with [`KEY_WRAP_HKDF_INFO`]. Wrap and unwrap reach the same key by
/// ECDH symmetry (wrap: ephemeral_secret × recipient_pub; unwrap: recipient_secret
/// × ephemeral_pub).
fn derive_wrap_key(local_secret: &[u8], peer_pub: &[u8]) -> TeeResult<[u8; 32]> {
    if local_secret.len() != 32 {
        return Err(TeeError::Crypto(format!(
            "wrap secret must be 32 bytes, got {}",
            local_secret.len()
        )));
    }
    let secret = SecretKey::from_slice(local_secret)
        .map_err(|e| TeeError::Crypto(format!("parse wrap secret: {e}")))?;
    if peer_pub.len() != 33 && peer_pub.len() != 65 {
        return Err(TeeError::Crypto(format!(
            "peer public key must be 33 or 65 bytes, got {}",
            peer_pub.len()
        )));
    }
    let encoded = EncodedPoint::from_bytes(peer_pub)
        .map_err(|e| TeeError::Crypto(format!("parse peer public key: {e}")))?;
    let peer = PublicKey::from_encoded_point(&encoded);
    let peer = if peer.is_some().into() {
        peer.unwrap()
    } else {
        return Err(TeeError::Crypto("invalid peer public key point".into()));
    };
    let ecdh = diffie_hellman(secret.to_nonzero_scalar(), peer.as_affine());
    let shared = Sha256::digest(ecdh.raw_secret_bytes());
    let hkdf = Hkdf::<Sha256>::new(None, &shared);
    let mut key = [0u8; 32];
    hkdf.expand(KEY_WRAP_HKDF_INFO, &mut key)
        .map_err(|e| TeeError::Crypto(format!("wrap-key HKDF: {e}")))?;
    Ok(key)
}

/// Wrap `dek` to `recipient_pub` (the TEE's `pk_att`) — ECIES over secp256k1.
///
/// Fresh ephemeral keypair + fresh 24-byte nonce per call. The wrapped DEK is
/// bound to its ephemeral key via `aad = eph_pub`, so substituting `eph_pub`
/// breaks authentication (defense-in-depth atop the ECDH binding).
pub fn wrap_key(dek: &[u8; 32], recipient_pub: &[u8]) -> TeeResult<WrappedKey> {
    let (eph_secret, eph_pub) = generate_ephemeral_keypair();
    let wrap_key = derive_wrap_key(&eph_secret, recipient_pub)?;
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = encrypt_with_aead(dek, &nonce, &eph_pub, &wrap_key)
        .map_err(|e| TeeError::Crypto(format!("key wrap: {e}")))?;
    Ok(WrappedKey {
        eph_pub,
        nonce,
        ciphertext,
    })
}

/// Unwrap a [`WrappedKey`] with `recipient_secret` (the `pk_att` private key).
///
/// Fail-closed: a wrong secret, a tampered ciphertext/nonce/`eph_pub`, or a
/// non-32-byte plaintext all return `Err`.
pub fn unwrap_key(w: &WrappedKey, recipient_secret: &[u8]) -> TeeResult<[u8; 32]> {
    let wrap_key = derive_wrap_key(recipient_secret, &w.eph_pub)?;
    let pt = decrypt_with_aead(&w.ciphertext, &w.nonce, &w.eph_pub, &wrap_key)
        .map_err(|e| TeeError::Crypto(format!("key unwrap: {e}")))?;
    if pt.len() != 32 {
        return Err(TeeError::Crypto(format!(
            "unwrapped DEK must be 32 bytes, got {}",
            pt.len()
        )));
    }
    let mut dek = [0u8; 32];
    dek.copy_from_slice(&pt);
    Ok(dek)
}
