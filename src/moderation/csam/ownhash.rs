// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Own-hash list — locally-confirmed known-bad SHA-256 hashes (B6).
//!
//! When a match is human-confirmed (Phase 6), its hash is added here so any
//! bit-identical re-upload auto-blocks, independent of the NCMEC list. Persisted
//! encrypted-at-rest via the shared [`super::atrest`] helper.

use std::collections::HashSet;

use crate::moderation::types::{ModerationError, Result};

#[derive(Default)]
pub struct OwnHashList {
    sha256: HashSet<[u8; 32]>,
}

impl OwnHashList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, sha256: [u8; 32]) {
        self.sha256.insert(sha256);
    }

    pub fn contains(&self, sha256: &[u8; 32]) -> bool {
        self.sha256.contains(sha256)
    }

    pub fn len(&self) -> usize {
        self.sha256.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sha256.is_empty()
    }

    /// Serialize (flat 32-byte hashes) + encrypt for at-rest persistence.
    pub fn seal(&self, key_material: &[u8]) -> Result<Vec<u8>> {
        let plain: Vec<u8> = self.sha256.iter().flatten().copied().collect();
        super::atrest::seal(key_material, &plain)
    }

    /// Decrypt + load a persisted list (plaintext = a flat sequence of 32-byte hashes).
    /// Fail-closed: a decrypted plaintext whose length is not a whole number of
    /// 32-byte hashes is REJECTED (never silently truncated via `chunks_exact`).
    pub fn open(key_material: &[u8], sealed: &[u8]) -> Result<Self> {
        let plain = super::atrest::open(key_material, sealed)?;
        if !plain.len().is_multiple_of(32) {
            return Err(ModerationError::StoreError(format!(
                "own-hash blob length {} is not a whole number of 32-byte hashes",
                plain.len()
            )));
        }
        let mut sha256 = HashSet::new();
        for chunk in plain.chunks_exact(32) {
            let mut h = [0u8; 32];
            h.copy_from_slice(chunk);
            sha256.insert(h);
        }
        Ok(Self { sha256 })
    }
}
