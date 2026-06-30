// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! `MockHashListSource` — provider-style test vectors + a benign control, so the
//! pipeline goes green before any real NCMEC credential exists.

use std::collections::HashSet;

use crate::moderation::csam::hashlist::{HashListSnapshot, HashListSource, ListState};
use crate::moderation::types::{Pdq256, Result};

/// In-memory `HashListSource` for tests.
pub struct MockHashListSource {
    sha256: HashSet<[u8; 32]>,
    pdq: Vec<Pdq256>,
    version: u64,
}

impl MockHashListSource {
    /// A benign SHA-256 guaranteed NOT in [`Self::with_test_vectors`]'s bad set.
    pub const BENIGN_CONTROL_SHA256: [u8; 32] = [0xAA; 32];

    /// A loaded list built from explicit known-bad hashes (version 1).
    pub fn loaded(sha256: Vec<[u8; 32]>, pdq: Vec<Pdq256>) -> Self {
        Self {
            sha256: sha256.into_iter().collect(),
            pdq,
            version: 1,
        }
    }

    /// A small deterministic set of known-bad test vectors.
    pub fn with_test_vectors() -> Self {
        Self::loaded(vec![[0x11; 32], [0x22; 32]], vec![Pdq256([0x33; 32])])
    }
}

impl HashListSource for MockHashListSource {
    fn refresh(&self) -> Result<HashListSnapshot> {
        Ok(HashListSnapshot {
            state: ListState::Loaded,
            sha256: self.sha256.clone(),
            pdq: self.pdq.clone(),
            version: self.version,
        })
    }

    fn version(&self) -> u64 {
        self.version
    }
}
