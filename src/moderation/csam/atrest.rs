// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Shared CSAM at-rest encryption (AES-256-GCM + HKDF-SHA256, D5).
//!
//! Used by the NCMEC hash store (3.1) and quarantine (6.1). Format mirrors the
//! repo's Web-Crypto AES-GCM convention: `[nonce(12) | ciphertext+tag]`. A wrong
//! key or any tampering fails-closed (GCM tag mismatch ⇒ `Err`), never silently
//! returning garbage.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;

use crate::moderation::types::{ModerationError, Result};

const NONCE_LEN: usize = 12;
/// HKDF domain separation for the CSAM at-rest store.
const ATREST_HKDF_INFO: &[u8] = b"fabstir-moderation-csam-at-rest-v1";

/// Derive the 32-byte AES-256-GCM key from arbitrary key material (HKDF-SHA256).
fn derive_key(key_material: &[u8]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(None, key_material);
    let mut key = [0u8; 32];
    hk.expand(ATREST_HKDF_INFO, &mut key)
        .expect("HKDF expand of 32 bytes is infallible");
    key
}

/// Encrypt `plaintext` for at-rest storage: `[nonce(12) | ciphertext+tag]`.
pub fn seal(key_material: &[u8], plaintext: &[u8]) -> Result<Vec<u8>> {
    let key = derive_key(key_material);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| ModerationError::StoreError(format!("cipher init: {e}")))?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| ModerationError::StoreError(format!("seal: {e}")))?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Decrypt at-rest data produced by [`seal`]. Fails-closed on a wrong key or any
/// tampering (GCM tag mismatch).
pub fn open(key_material: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
    if sealed.len() < NONCE_LEN {
        return Err(ModerationError::StoreError("sealed blob too short".into()));
    }
    let key = derive_key(key_material);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| ModerationError::StoreError(format!("cipher init: {e}")))?;
    let (nonce_bytes, ct) = sealed.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ct)
        .map_err(|e| ModerationError::StoreError(format!("open (tampered or wrong key): {e}")))
}
