// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Attestation provider trait (Phase 1.2).

use crate::tee::types::{Evidence, TeeResult};
use async_trait::async_trait;

/// Gathers hardware attestation evidence from inside the confidential VM.
///
/// Implemented by `MockAttestationProvider` (Phases 1–4, tests/dev) and, in
/// Phase 5, `NvidiaCcProvider` (real GPU attestation report + CPU TDX/SNP quote)
/// — the swap is behind this trait, with no call-site changes.
#[async_trait]
pub trait AttestationProvider: Send + Sync {
    /// Produce [`Evidence`] binding the KBS freshness `nonce` and the
    /// attestation key `pk_att` (compressed secp256k1, 33 bytes) into the
    /// hardware-signed cross-binding.
    async fn gather_evidence(&self, nonce: [u8; 32], pk_att: &[u8]) -> TeeResult<Evidence>;
}
