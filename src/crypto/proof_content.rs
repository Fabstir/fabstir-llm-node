// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Keyed content commitments for the proof journal (NP2, plan D20, 8.67.0).
//!
//! The Risc0 guest commits the 32-byte input and output hashes it is given to
//! the public journal unchanged. Before 8.67.0 the node gave it plain
//! `SHA-256(prompt)` and `SHA-256(response)`, which anyone holding a guess of
//! the content could confirm. Now it gives
//!
//! ```text
//! k          = HKDF-SHA256(ikm = session key, salt = none,
//!                          info = "fabstir-proof-content-v1:" + decimal job id)  (32 bytes)
//! commitment = HMAC-SHA256(k, content bytes)
//! ```
//!
//! The client holds the same session key, so it can recompute both fields;
//! nobody else can. A job with no session key gets a random per-job key held
//! only in memory (never logged, stored or published), so its commitment is
//! opaque to everyone. The guest program and its image id are unchanged.

use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;

/// HKDF `info` prefix; the decimal job id follows it.
pub const PROOF_CONTENT_INFO_PREFIX: &str = "fabstir-proof-content-v1:";

/// A per-job commitment key. Not `Copy`, not `Clone`: one owner, wiped on
/// drop; `Debug` never shows the bytes.
pub struct ProofContentKey([u8; 32]);

impl Drop for ProofContentKey {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.0);
    }
}

impl std::fmt::Debug for ProofContentKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProofContentKey(<redacted>)")
    }
}

impl ProofContentKey {
    /// `k` for a job whose client holds `session_key`.
    pub fn from_session_key(session_key: &[u8; 32], job_id: u64) -> Self {
        let info = format!("{PROOF_CONTENT_INFO_PREFIX}{job_id}");
        let hk = Hkdf::<Sha256>::new(None, session_key);
        let mut key = Self([0u8; 32]);
        hk.expand(info.as_bytes(), &mut key.0)
            .expect("32 bytes is a valid HKDF-SHA256 output length");
        key
    }

    /// A random key for a job with no session key. Held in memory only.
    pub fn random() -> Self {
        let mut key = Self([0u8; 32]);
        rand::rngs::OsRng.fill_bytes(&mut key.0);
        key
    }

    /// `HMAC-SHA256(k, content)`.
    pub fn commit(&self, content: &[u8]) -> [u8; 32] {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.0)
            .expect("HMAC-SHA256 accepts a 32-byte key");
        mac.update(content);
        mac.finalize().into_bytes().into()
    }
}

/// The commitment a client recomputes: HKDF from its session key and the job
/// id, then HMAC over the content. Same bytes as the node's journal field.
pub fn proof_content_commitment(session_key: &[u8; 32], job_id: u64, content: &[u8]) -> [u8; 32] {
    ProofContentKey::from_session_key(session_key, job_id).commit(content)
}
