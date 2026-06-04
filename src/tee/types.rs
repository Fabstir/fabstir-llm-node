// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Core TEE / confidential-inference types (Phase 1.1).
//!
//! Data structures shared across the attestation pipeline: hardware [`Evidence`],
//! the model provider's DEK-release [`Policy`], verified [`Claims`], a
//! [`WrappedKey`] (the DEK bound to the TEE's attestation key), and the module
//! error [`TeeError`].
//!
//! The 48-byte measurement fields (`Evidence::image_measurement`,
//! `Policy::expected_measurement`) are SHA-384 launch measurements (AMD SEV-SNP
//! `LAUNCH_MEASUREMENT` / Intel TDX `MRTD`); serde does not derive for arrays
//! larger than `[T; 32]`, so they use `#[serde(with = "BigArray")]`.

use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;
use sha2::{Digest, Sha256};

/// Module result type — every TEE operation returns [`TeeError`] on failure.
///
/// Named (not a bare `Result`) to match the crate's convention (`EzklResult`,
/// `AuthResult`, `ClaimResult`, …) and avoid shadowing `std::result::Result`.
pub type TeeResult<T> = std::result::Result<T, TeeError>;

/// Hardware attestation evidence gathered inside the CVM, sent to the KBS.
///
/// For Phases 1–4 `cpu_quote` is a synthetic 64-byte blob whose bytes `0..64`
/// directly carry the `report_data` field (`cpu_quote[0..64]`); Phase 5 parses
/// real TDX/SNP quotes to extract `report_data`. The canonical cross-binding
/// commitment (identical in the mock provider and `DefaultVerifier`) is
/// `report_data[0..32] = sha256(pk_att ‖ gpu_report_hash ‖ nonce)` with
/// `report_data[32..64] = 0x00…00`, where `gpu_report_hash = sha256(gpu_report)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// GPU attestation report (opaque bytes; `sha256` of this is the cross-bind input).
    pub gpu_report: Vec<u8>,
    /// CPU TEE quote. Mock: 64 raw bytes = `report_data`. Real: vendor quote (Phase 5).
    pub cpu_quote: Vec<u8>,
    /// 48-byte SHA-384 launch measurement of the node-CVM image.
    #[serde(with = "BigArray")]
    pub image_measurement: [u8; 48],
    /// Attestation-bound ephemeral public key (compressed secp256k1, 33 bytes).
    pub pk_att: Vec<u8>,
    /// 32-byte KBS-issued freshness nonce, bound into the cross-binding.
    pub nonce: [u8; 32],
}

/// Model-provider DEK-release policy (off-chain, signed — decision D3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    /// Monotonic policy version (enables rotation + version-based revocation).
    pub policy_version: u32,
    /// GPU SKUs the provider permits (e.g. `"H100"`, `"H200"`).
    pub allowed_skus: Vec<String>,
    /// Expected 48-byte node-CVM launch measurement (pinned by the provider).
    #[serde(with = "BigArray")]
    pub expected_measurement: [u8; 48],
    /// Require the GPU to report CC = ON.
    pub require_cc_on: bool,
    /// Require a production (non-debug) CPU TCB.
    pub require_production_tcb: bool,
    /// Maximum acceptable CPU TCB age, in days.
    pub max_tcb_age_days: u32,
    /// Policy validity start (unix seconds) — anti-replay / rotation.
    pub not_before: u64,
    /// Policy expiry (unix seconds), **inclusive**: valid while `now <= expiry`
    /// (matches the plan's `not_before ≤ now ≤ expiry`). Revoke by setting it in
    /// the past (e.g. `0`) — see also version-based revocation, [`Policy::policy_version`].
    pub expiry: u64,
    /// The model this policy governs.
    pub model_id: [u8; 32],
}

/// Result of a successful attestation verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    /// Verification timestamp (unix seconds).
    pub verified_at: u64,
    /// `sha256(gpu_report)` of the verified evidence.
    pub gpu_report_hash: [u8; 32],
    /// Whether the image measurement matched the policy's expected value.
    pub measurement_verified: bool,
}

/// A DEK wrapped (ECIES over k256) to the TEE's attestation key `pk_att`.
///
/// Produced/consumed by `keywrap` (Phase 3), which must build on the existing
/// `crypto::{encrypt_with_aead, decrypt_with_aead, derive_shared_key}` rather
/// than forking the ECDH/AEAD primitives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrappedKey {
    /// Ephemeral public key of the wrapper (compressed secp256k1, 33 bytes).
    pub eph_pub: Vec<u8>,
    /// 24-byte XChaCha20-Poly1305 nonce.
    pub nonce: [u8; 24],
    /// AEAD-sealed DEK (32-byte key + 16-byte tag).
    pub ciphertext: Vec<u8>,
}

/// All error conditions across the TEE pipeline.
///
/// Fail-closed posture: every error path writes no plaintext and returns one of
/// these variants. Phase 5 adds a richer `VerificationError`, folded in later as
/// a dedicated variant.
#[derive(Debug, thiserror::Error)]
pub enum TeeError {
    /// Encryption would need ≥ 2^32 chunks (would overflow the chunk index).
    #[error("encrypted container too large (chunk count would exceed u32)")]
    ContainerTooLarge,
    /// No model→provider binding found on-chain for this model.
    #[error("no provider bound for model {0:?}")]
    NoProviderBound([u8; 32]),
    /// A non-TEE node refused to load an encrypted (proprietary) model.
    #[error("non-TEE node refuses to load encrypted model (HOST_TEE_ENABLED=false)")]
    NonTeeNodeRefusesEncrypted,
    /// Attestation nonce was unissued, stale, or already consumed.
    #[error("attestation freshness check failed (nonce unissued/stale/consumed)")]
    FreshnessFailure,
    /// A newer policy version exists — the cached policy is revoked.
    #[error("policy revoked (a newer policy version exists)")]
    PolicyRevoked,
    /// Decrypted weights did not match the on-chain-approved model hash (4.3.2).
    #[error("model hash mismatch: expected {expected}, got {got}")]
    ModelHashMismatch { expected: String, got: String },
    /// Attestation verification rejected the evidence (reason in the string).
    #[error("attestation verification failed: {0}")]
    VerificationFailed(String),
    /// Cryptographic operation failed (wrap/unwrap, AEAD, key parsing).
    #[error("crypto error: {0}")]
    Crypto(String),
    /// Filesystem / IO error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// The security-relevant fields a GPU attestation report yields.
///
/// In Phases 1–4 the mock provider bincode-encodes this into
/// `Evidence::gpu_report` and `DefaultVerifier` decodes it; in Phase 5 the real
/// verifier parses these same logical fields from the real DER attestation
/// report. It lives here (the neutral shared home), not in `verifier`/`mock`, so
/// the real verifier can reuse the policy checks without depending on the mock.
///
/// Phase 5 note: `production_tcb`/`tcb_age_days` describe the **CPU** TCB and
/// MUST then be sourced from the CPU quote, not the GPU report (the mock
/// conflates them — a real host's GPU report cannot attest CPU-TCB state).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuReportFields {
    /// GPU SKU (e.g. `"H100"`, `"H200"`).
    pub sku: String,
    /// Whether the GPU reports Confidential Computing = ON.
    pub cc_on: bool,
    /// Whether the CPU TCB is a production (non-debug) TCB.
    pub production_tcb: bool,
    /// CPU TCB age, in days.
    pub tcb_age_days: u32,
}

/// `sha256(data)` as a 32-byte array.
pub fn sha256_32(data: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(&Sha256::digest(data));
    out
}

/// Canonical cross-binding commitment `sha256(pk_att ‖ gpu_report_hash ‖ nonce)`.
///
/// SECURITY-CRITICAL: the provider and verifier MUST use this identical
/// construction — it lives here, in the neutral shared home next to [`Evidence`],
/// so a host cannot pair a genuine CPU quote with a *different* GPU's report
/// (both individually valid, yet the pairing is forged).
///
/// The inputs are concatenated without length prefixes; this is safe only
/// because `pk_att` is the sole variable-length input and is followed by two
/// fixed-length fields (a boundary shift would require a SHA-256 second-preimage).
/// Phase 5 should add an explicit domain-separation tag + length prefixes when
/// finalizing `report_data` against the real CPU-quote semantics.
pub fn cross_bind_report_data(
    pk_att: &[u8],
    gpu_report_hash: &[u8; 32],
    nonce: &[u8; 32],
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(pk_att);
    h.update(gpu_report_hash);
    h.update(nonce);
    let mut out = [0u8; 32];
    out.copy_from_slice(&h.finalize());
    out
}

/// Current unix time (seconds). On a mis-set (pre-epoch) clock returns `u64::MAX`
/// — never `0` (which would bypass a `not_before == 0` window) — so a broken clock
/// always fails **closed** wherever it gates a validity window. Shared by
/// `DefaultVerifier`, `MockKeyBroker`, and `policy::check_policy_validity`.
pub(crate) fn now_unix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(u64::MAX)
}
