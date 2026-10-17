// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Core TEE / confidential-inference types (Phase 1.1).
//!
//! Data structures shared across the attestation pipeline: hardware [`Evidence`],
//! the model provider's DEK-release [`Policy`], verified [`Claims`], a
//! [`WrappedKey`] (the DEK bound to the TEE's attestation key), and the module
//! error [`TeeError`].
//!
//! `Evidence::image_measurement` is a 48-byte SHA-384 launch measurement (Intel
//! TDX `MRTD`; serde does not derive for arrays larger than `[T; 32]`, so it
//! uses `#[serde(with = "BigArray")]`). Under policy schema 3 the registers live
//! in host profiles (`crate::tee::host_profile`) as lowercase hex strings.

use serde::{Deserialize, Serialize};

/// The reserved TEST model-id prefix (`t5t:`, bytes `74 35 74 3a`): a keyring entry is
/// `test: true` iff its `model_id` starts with it (broker, at keyring load), and a
/// node accepts a `test_release: true` release iff its `TEE_MODEL_ID` does (node, at
/// `request_key`). A labelling convention on both sides; the security boundary is the
/// broker's keyring-flag × evidence-mode coupling and the node's
/// `TEE_ACCEPT_TEST_RELEASE` opt-in.
pub const TEST_ID_PREFIX: &[u8; 4] = b"t5t:";
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

/// Model-provider DEK-release policy, **schema 3** (Phase 5; off-chain, signed,
/// decision D3; `docs/development/IMPLEMENTATION-POLICY-V3-HOST-PROFILES.md`).
///
/// Schema 3 separates WHO may vouch for host machines from WHICH machines they
/// are. This policy, the only document bound into the container (its
/// `policy_hash` is in every chunk's AAD), carries the provider's own
/// requirements and names one authority root (`host_authority.root`); the host
/// identities (registers, OS image, compose hashes, app id, key provider, GPU
/// models and floors) live in a separately signed, epoch-versioned
/// [`HostProfileSet`](crate::tee::host_profile::HostProfileSet) the broker
/// checks. A host change is a new epoch, never a reseal.
///
/// Values are refused, never coerced: the signature covers the canonical JSON
/// exactly as written. Unknown fields are refused too, so a schema-2 file can
/// never half-parse as schema 3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// Always [`POLICY_DOC`]: the document-type tag inside the signed body.
    pub doc: String,
    /// Always [`POLICY_SCHEMA_VERSION`]; anything else is refused.
    pub schema_version: u32,
    /// Monotonic per model; a newer version revokes older ones.
    pub policy_version: u32,
    /// The model this policy governs (bytes; bound into the container header).
    pub model_id: [u8; 32],
    /// Validity start (unix seconds).
    pub not_before: u64,
    /// Validity end (unix seconds), **inclusive**. Revoke by setting it in the past.
    pub expiry: u64,
    /// The provider's confidential-VM requirements.
    pub cvm: CvmPolicy,
    /// The provider's GPU requirements.
    pub gpu: GpuPolicy,
    /// Who may approve host machines, and the provider's floor on that authority.
    pub host_authority: HostAuthority,
}

/// The policy schema this code reads and writes. Schema 2 is refused everywhere.
pub const POLICY_SCHEMA_VERSION: u32 = 3;

/// The `doc` tag of a model release policy.
pub const POLICY_DOC: &str = "fabstir-model-release-policy";

/// The provider's Intel TDX requirements. The measurements themselves are a
/// host profile's (schema 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CvmPolicy {
    /// TD attributes: the DEBUG bit must be clear.
    pub require_td_debug_off: bool,
    /// dcap-qvl `VerifiedReport.status` allow-list, e.g. `["UpToDate"]`.
    pub allowed_tcb_status: Vec<String>,
    /// Intel advisory ids tolerated. Empty = none tolerated.
    pub allowed_advisory_ids: Vec<String>,
    /// Only profiles that pin the dstack `app-id` are candidates. Implies
    /// `require_key_provider_pinned`: an app id is host-asserted in dstack's
    /// KMS mode and means something only with the key provider pinned (D18).
    pub require_app_id_pinned: bool,
    /// Only profiles that pin the `key-provider` event payload are candidates.
    pub require_key_provider_pinned: bool,
}

/// NVIDIA GPU expectations, checked against the NRAS EAT (claim-name mapping
/// lives in the verifier, never here).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GpuPolicy {
    /// Accepted hardware model strings (EAT `hwmodel`), non-empty. A GPU must
    /// be in this list AND in the matched profile's `hwmodels`: the
    /// Hopper/Blackwell boundary (gap G-6a) stays the PROVIDER's.
    pub allowed_hwmodels: Vec<String>,
    /// Required CC mode, matched EXACTLY. See [`CcMode`]. Two consumers read
    /// it and they are backed differently:
    ///
    /// * the BROKER (`kbs::verify`) enforces `Some(On)` against the signed
    ///   per-GPU EAT pair `secboot` + `dbgstat`, which rules DevTools out
    ///   (design D14a, gap G-6 closed 2026-09-22) but does not separate On
    ///   from Off (gap G-6a). Its release gate refuses a non-test entry whose
    ///   claims leave DevTools open EVEN WHEN this field is `None`, so absent
    ///   is not don't-care there;
    /// * the NODE does NOT check it on the Phase-5 path. `DefaultVerifier`
    ///   compares it with [`GpuReportFields::cc_mode`], but that verifier is
    ///   the mock-backed pipeline (Phases 1–4) and refuses a real payload at
    ///   its step 1b, so the comparison never runs against real evidence.
    ///   CC-OFF is refused by the measured in-guest collector
    ///   (`collect_gpu_evidence.py`, exit 75 when `cc_enabled` is false),
    ///   unconditionally and without consulting this policy at all: setting
    ///   `Some(On)` does not change the Off behaviour anywhere (gap G-6a).
    pub require_cc_mode: Option<CcMode>,
    /// GPU secure boot / debug status.
    pub require_secure_boot: bool,
    pub require_debug_disabled: bool,
    /// Minimum driver / VBIOS versions (dotted numeric compare); `None` = any
    /// that NRAS accepts. The mainnet policy seals these as `None` and puts
    /// per-family floors on the profiles (D15).
    pub min_driver_version: Option<String>,
    pub min_vbios_version: Option<String>,
}

/// The one authority this policy trusts to approve host machines, and the
/// provider's floor on its documents (D3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostAuthority {
    /// The authority root, `0x` + 40 lowercase hex: the only key whose
    /// manifest the broker accepts for this model.
    pub root: String,
    /// The broker refuses a manifest below this version.
    pub min_manifest_version: u32,
    /// The broker refuses a manifest whose threshold is below this (`≥ 1`).
    pub min_threshold: u32,
    /// The broker refuses a profile set below this epoch.
    pub min_profile_epoch: u64,
    /// Host classes this model may run on, non-empty; a profile of another
    /// class is never a candidate (D7).
    pub allowed_classes: Vec<String>,
}

fn shown(s: &str) -> String {
    if s.chars().count() > 20 {
        format!("{}…", s.chars().take(20).collect::<String>())
    } else {
        s.to_string()
    }
}

fn is_lower_hex(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `s` is exactly `bytes * 2` lowercase hex characters, no `0x`. `what` names
/// the field in the refusal (e.g. `profile p1 cvm.mrtd`).
pub(crate) fn check_hex(what: &str, s: &str, bytes: usize) -> TeeResult<()> {
    if s.len() == bytes * 2 && is_lower_hex(s) {
        Ok(())
    } else {
        Err(TeeError::VerificationFailed(format!(
            "{what}: expected {} lowercase hex chars without 0x, got {:?}",
            bytes * 2,
            shown(s)
        )))
    }
}

/// Variable-length lowercase hex, even length, `min_bytes..=max_bytes` bytes.
pub(crate) fn check_hex_var(
    what: &str,
    s: &str,
    min_bytes: usize,
    max_bytes: usize,
) -> TeeResult<()> {
    let ok = s.len().is_multiple_of(2)
        && (min_bytes * 2..=max_bytes * 2).contains(&s.len())
        && is_lower_hex(s);
    if ok {
        Ok(())
    } else {
        Err(TeeError::VerificationFailed(format!(
            "{what}: expected an even number ({}..={}) of lowercase hex chars without 0x, got {} chars {:?}",
            min_bytes * 2,
            max_bytes * 2,
            s.len(),
            shown(s)
        )))
    }
}

/// An Ethereum address as every schema-3 document spells it: `0x` + 40
/// lowercase hex.
pub(crate) fn check_address(what: &str, s: &str) -> TeeResult<()> {
    match s.strip_prefix("0x") {
        Some(rest) if rest.len() == 40 && is_lower_hex(rest) => Ok(()),
        _ => Err(TeeError::VerificationFailed(format!(
            "{what}: expected 0x + 40 lowercase hex chars, got {:?}",
            shown(s)
        ))),
    }
}

/// A class token: `[a-z0-9][a-z0-9-]{0,62}`.
pub(crate) fn check_token(what: &str, s: &str) -> TeeResult<()> {
    let b = s.as_bytes();
    let ok = !b.is_empty()
        && b.len() <= 63
        && matches!(b[0], b'a'..=b'z' | b'0'..=b'9')
        && b[1..]
            .iter()
            .all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(TeeError::VerificationFailed(format!(
            "{what}: expected [a-z0-9][a-z0-9-]{{0,62}}, got {:?}",
            shown(s)
        )))
    }
}

/// A version floor must be well-formed by the rule [`version_at_least`]
/// applies (dotted, every component non-empty hex), or every real driver
/// string would fail the floor on the GPU day, one burned nonce per try.
pub(crate) fn check_version_floor(what: &str, floor: &Option<String>) -> TeeResult<()> {
    if let Some(v) = floor {
        let well_formed = !v.trim().is_empty()
            && v.split('.')
                .all(|c| !c.trim().is_empty() && u64::from_str_radix(c.trim(), 16).is_ok());
        if !well_formed {
            return Err(TeeError::VerificationFailed(format!(
                "{what}: expected dotted hex components (e.g. 580.95.05 or 96.00.9f.00.01), got {v:?}"
            )));
        }
    }
    Ok(())
}

impl Policy {
    /// Refuse a policy that is not schema 3 or breaks a D3 rule. No coercion:
    /// the provider signed these bytes.
    pub fn validate(&self) -> TeeResult<()> {
        if self.doc != POLICY_DOC {
            return Err(TeeError::VerificationFailed(format!(
                "policy doc {:?} is not {POLICY_DOC:?}",
                self.doc
            )));
        }
        if self.schema_version != POLICY_SCHEMA_VERSION {
            return Err(TeeError::VerificationFailed(format!(
                "policy schema_version {} is not {POLICY_SCHEMA_VERSION}",
                self.schema_version
            )));
        }
        if self.cvm.allowed_tcb_status.is_empty() {
            return Err(TeeError::VerificationFailed(
                "policy cvm.allowed_tcb_status is empty: no TCB status could ever pass".into(),
            ));
        }
        if self.cvm.require_app_id_pinned && !self.cvm.require_key_provider_pinned {
            return Err(TeeError::VerificationFailed(
                "policy cvm.require_app_id_pinned without require_key_provider_pinned: an app id \
                 is host-asserted unless the key provider is pinned too"
                    .into(),
            ));
        }
        if self.gpu.allowed_hwmodels.is_empty() {
            return Err(TeeError::VerificationFailed(
                "policy gpu.allowed_hwmodels is empty: no GPU could ever pass".into(),
            ));
        }
        check_version_floor(
            "policy gpu.min_driver_version",
            &self.gpu.min_driver_version,
        )?;
        check_version_floor("policy gpu.min_vbios_version", &self.gpu.min_vbios_version)?;
        let a = &self.host_authority;
        check_address("policy host_authority.root", &a.root)?;
        if a.min_threshold == 0 {
            return Err(TeeError::VerificationFailed(
                "policy host_authority.min_threshold is 0: must be at least 1".into(),
            ));
        }
        if a.allowed_classes.is_empty() {
            return Err(TeeError::VerificationFailed(
                "policy host_authority.allowed_classes is empty: no host could ever pass".into(),
            ));
        }
        for (i, c) in a.allowed_classes.iter().enumerate() {
            check_token(&format!("policy host_authority.allowed_classes[{i}]"), c)?;
        }
        Ok(())
    }
}

/// `a >= b` for dotted versions. Each component is compared as a
/// **hexadecimal** integer, case-insensitively: NVIDIA VBIOS strings carry hex
/// components (`96.00.9f.00.01`) and driver strings are decimal
/// (`580.95.05`); parsing decimal digit strings as hex preserves their order
/// (same digit order, longer = larger), so one rule serves both. A component
/// that is not hex at all, on either side, makes the answer `false` (refuse;
/// never a lexical fallback, which would pass `unknown` or `r580` above any
/// floor). Missing trailing components and EMPTY components (`580.95.05.`,
/// `580..95`) count as zero, so a formatting quirk never silently refuses a
/// GPU.
pub fn version_at_least(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<String> {
        v.split('.')
            .map(|p| {
                let p = p.trim().to_ascii_lowercase();
                if p.is_empty() {
                    "0".to_string()
                } else {
                    p
                }
            })
            .collect()
    };
    let (pa, pb) = (parts(a), parts(b));
    for i in 0..pa.len().max(pb.len()) {
        let (x, y) = (
            pa.get(i).map(String::as_str).unwrap_or("0"),
            pb.get(i).map(String::as_str).unwrap_or("0"),
        );
        // A component that is not hex on EITHER side refuses: a lexical
        // fallback would put letters above digits (`unknown`, `r580` > any
        // floor) and pass the check open.
        let ord = match (u64::from_str_radix(x, 16), u64::from_str_radix(y, 16)) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            _ => return false,
        };
        match ord {
            std::cmp::Ordering::Greater => return true,
            std::cmp::Ordering::Less => return false,
            std::cmp::Ordering::Equal => {}
        }
    }
    true
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
    /// The key broker could not be reached, refused TLS, or answered out of
    /// contract (Phase 5). Transport-class; the contract errors (freshness,
    /// verification, no provider) map to their own variants instead.
    #[error("key broker: {0}")]
    Kbs(String),
    /// A policy or container fetch failed or was refused (Phase 5): transport,
    /// non-2xx, oversize, or an insecure URL. Always fail-closed.
    #[error("fetch: {0}")]
    Fetch(String),
    /// Cryptographic operation failed (wrap/unwrap, AEAD, key parsing).
    #[error("crypto error: {0}")]
    Crypto(String),
    /// Filesystem / IO error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// The mock-shaped view of the evidence the mock verifier judges (Phases 1–4
/// and the gate rounds). The mock provider bincode-encodes this into
/// `Evidence::gpu_report`; the Phase-5 broker verifier builds the same logical
/// fields from the NRAS EAT (GPU half) and the dcap-qvl report (CVM half)
/// instead. It lives here, the neutral shared home, so the policy checks are
/// written once against these names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuReportFields {
    /// The 32-byte challenge nonce the GPU evidence was collected under. Real
    /// evidence carries it inside the hardware-signed attestation report (NRAS
    /// checks it against the payload's `nonce`); the mock carries it here so the
    /// verifier's "same nonce on both halves" check is exercised. This, not a
    /// hash in `report_data`, is what binds the GPU half to the CPU half.
    pub nonce: [u8; 32],
    /// Hardware model string (EAT `hwmodel`).
    pub hwmodel: String,
    /// The CC mode the GPU reports. See [`CcMode`]: `DevTools` attests but
    /// protects nothing, so this must never be narrowed to a boolean.
    pub cc_mode: CcMode,
    /// GPU secure boot and debug status (EAT `secboot`, `dbgstat`).
    pub secure_boot: bool,
    pub debug_disabled: bool,
    /// Driver and VBIOS versions (EAT claims).
    pub driver_version: String,
    pub vbios_version: String,
    /// CVM side (the mock conflates it here; the real verifier takes these from
    /// the verified TDX quote): TD DEBUG attribute clear, and the TCB status
    /// string dcap-qvl reports.
    pub td_debug_off: bool,
    pub tcb_status: String,
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
