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
/// Shape follows what a dstack node ships to its relying party (Phala's
/// reference `quote.py`: `intel_quote`, `nvidia_payload`, `event_log`,
/// `vm_config`), plus the two values the KBS needs to bind the release to a
/// key and a challenge (`pk_att`, `nonce`). The node collects; it never
/// verifies. Everything here is UNAUTHENTICATED until the verifier has checked
/// the signed quote and the GPU evidence; in particular `pk_att`, `nonce` and
/// `image_measurement` are node-asserted and only mean something once the
/// verifier has tied them to the signed `report_data`.
///
/// For Phases 1–4 `cpu_quote` is a synthetic 64-byte blob whose bytes `0..64`
/// directly carry the `report_data` field (`cpu_quote[0..64]`); Phase 5 parses
/// real TDX quotes to extract `report_data`. The `report_data` layout (identical
/// in the mock provider and every verifier) is [`report_data`]:
/// `identity(32) ‖ nonce(32)` with `identity = sha256(pk_att)`. The GPU evidence
/// carries the same 32-byte nonce inside its own signed report; the shared
/// nonce is the cross-binding between the two independently signed quotes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// GPU attestation evidence, opaque bytes. Mock: bincode of
    /// [`GpuReportFields`]. Real: the `nvidia_payload` JSON exactly as the
    /// collector emits it (`{"nonce","evidence_list":[…],"arch"}`, plus
    /// `"canned": true` in test mode), which the verifier forwards to NRAS.
    pub gpu_report: Vec<u8>,
    /// CPU TEE quote. Mock: 64 raw bytes = `report_data`. Real: the TDX quote
    /// bytes from dstack `/GetQuote` (hex-decoded).
    pub cpu_quote: Vec<u8>,
    /// dstack event log, UTF-8 JSON as returned by `/GetQuote` (`event_log`).
    /// The verifier replays it to the quote's RTMR3 and reads the
    /// `compose-hash` / `app-id` / `os-image-hash` / `key-provider` events.
    /// Empty in the mock.
    pub event_log: Vec<u8>,
    /// dstack VM configuration, UTF-8 JSON as returned by `/GetQuote`
    /// (`vm_config`): the vCPU/RAM/device spec MRTD and RTMR0 depend on, kept so
    /// a later `dstack-mr` reproduction has its inputs. Empty in the mock.
    pub vm_config: Vec<u8>,
    /// 48-byte launch measurement as the NODE reports it. Mock-era field: the
    /// mock verifier compares it to the policy; a real verifier MUST take MRTD
    /// from the verified quote body and ignore this. Removed with Policy v2.
    #[serde(with = "BigArray")]
    pub image_measurement: [u8; 48],
    /// Attestation-bound ephemeral public key (compressed secp256k1, 33 bytes).
    pub pk_att: Vec<u8>,
    /// 32-byte KBS-issued freshness nonce; the second half of `report_data`
    /// and the nonce the GPU evidence was collected under.
    pub nonce: [u8; 32],
}

/// GPU Confidential Computing mode.
///
/// **This is three states, not two, and that is the whole point of the type.**
/// NVIDIA's `nvidia_gpu_tools.py --set-cc-mode` takes `off`, `on` or
/// `devtools`, and **`devtools` attests while the memory protections are
/// DISABLED** — it exists so developers can debug inside the CC flow. A GPU in
/// `devtools` is a real, reachable, *attesting* state that provides no
/// confidentiality whatsoever.
///
/// Modelling this as a `bool` is a silent fail-open: a real report parser
/// handed a `bool` maps "not off" to `true`, `devtools` then satisfies a policy
/// that meant to demand protection, and the key is released to an unprotected
/// GPU while the check reads as though it did its job. Do not collapse this
/// back to two states.
///
/// Serialises to NVIDIA's own spelling (`"off"` / `"on"` / `"devtools"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CcMode {
    /// Confidential Computing disabled.
    Off,
    /// Enabled, protections active. The only production-safe value.
    On,
    /// Enabled for debugging with the protections OFF. Attests; protects nothing.
    DevTools,
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
    /// Required GPU CC mode, matched EXACTLY. `Some(CcMode::On)` is the
    /// production setting; `None` imposes no requirement. Accepting `devtools`
    /// must be spelled out as `Some(CcMode::DevTools)` rather than reachable by
    /// relaxing a boolean, so no policy can accept an unprotected GPU by
    /// accident.
    pub require_cc_mode: Option<CcMode>,
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
    /// The dstack guest agent could not be reached or answered out of shape
    /// (Phase 5). Node-side, before any evidence exists; always fail-closed.
    #[error("dstack guest agent: {0}")]
    Dstack(String),
    /// GPU evidence collection failed or returned an out-of-shape / mislabelled
    /// payload (Phase 5). Node-side; always fail-closed.
    #[error("gpu evidence: {0}")]
    GpuEvidence(String),
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
    /// The 32-byte challenge nonce the GPU evidence was collected under. Real
    /// evidence carries it inside the hardware-signed attestation report (NRAS
    /// checks it against the payload's `nonce`); the mock carries it here so the
    /// verifier's "same nonce on both halves" check is exercised. This, not a
    /// hash in `report_data`, is what binds the GPU half to the CPU half.
    pub nonce: [u8; 32],
    /// GPU SKU (e.g. `"H100"`, `"H200"`).
    pub sku: String,
    /// The CC mode the GPU reports. See [`CcMode`]: `DevTools` attests but
    /// protects nothing, so this must never be narrowed to a boolean.
    pub cc_mode: CcMode,
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

/// Length of a TDX/SEV `report_data` field.
pub const REPORT_DATA_LEN: usize = 64;

/// The 32-byte identity half of `report_data`: `sha256(pk_att)`.
///
/// `pk_att` is the 33-byte compressed secp256k1 key the DEK is wrapped to; it
/// does not fit 32 bytes, so it is hashed (the reference layout's "v2"
/// identity is likewise a SHA-256). A verifier recomputes this from the
/// node-asserted `Evidence::pk_att` and compares it to the SIGNED quote body,
/// which is what turns `pk_att` from a claim into a fact.
pub fn report_data_identity(pk_att: &[u8]) -> [u8; 32] {
    sha256_32(pk_att)
}

/// The 64-byte `report_data` the node asks the CPU TEE to sign:
/// `identity(32) ‖ nonce(32)`, with `identity = sha256(pk_att)` and the nonce
/// in the clear (reference: Phala's dstack node, `quote.py::_build_report_data`).
///
/// SECURITY-CRITICAL and deliberately hash-free on the nonce: the same 32
/// bytes are handed to GPU evidence collection, so the verifier can check that
/// the CPU quote and the GPU attestation report were both produced for THIS
/// challenge. Neither quote needs to exist before the other. There is no
/// domain tag because both halves are fixed-width and positional.
pub fn report_data(pk_att: &[u8], nonce: &[u8; 32]) -> [u8; REPORT_DATA_LEN] {
    let mut out = [0u8; REPORT_DATA_LEN];
    out[..32].copy_from_slice(&report_data_identity(pk_att));
    out[32..].copy_from_slice(nonce);
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
