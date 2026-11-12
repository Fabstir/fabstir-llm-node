// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! The node <-> provenance-signer contract (`docs/development/AP-SIGNER-CONTRACT.md`, frozen at AP1).

use crate::defs::Params;
use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tiny_keccak::{Hasher, Keccak};

pub const API_VERSION: u32 = 1;
pub const DOMAIN: &str = "platformlessai-provenance/1";
pub const WINDOW_SECS: u64 = 120;
pub const NONCE_MEMORY_SECS: u64 = 240;
pub const MAX_SIGN_BODY: usize = 256 * 1024;
pub const MAX_SEQUENCE_BODY: usize = 1024 * 1024;
pub const MAX_RESERVE_BODY: usize = 4 * 1024;
pub const HDR_TIMESTAMP: &str = "x-pai-timestamp";
pub const HDR_NONCE: &str = "x-pai-nonce";
pub const HDR_SIGNATURE: &str = "x-pai-signature";
pub const HDR_MANIFEST_ID: &str = "x-pai-manifest-id";
pub const HDR_MANIFEST_SHA256: &str = "x-pai-manifest-sha256";
pub const HDR_CHAIN_EPOCH: &str = "x-pai-chain-epoch";
pub const HDR_TSA: &str = "x-pai-tsa";
pub const HDR_REPLAY: &str = "x-pai-replay";
/// The detector's shared secret on the internal index listener.
pub const HDR_INDEX_SECRET: &str = "x-pai-index-secret";
/// Extra time a reservation lives past the job deadline: the 10-minute retry window of D10 plus 5 minutes.
pub const RESERVATION_GRACE_SECS: u64 = 900;
pub const MAX_DEADLINE_SECS: u64 = 6 * 3600;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub api: u32,
    pub c2pa_crate: String,
    pub provenance_crate: String,
    pub spec_version: String,
    pub time: u64,
    pub reserve_min: usize,
    pub reserve_max: usize,
    pub reserve_recommended: usize,
    pub chain_epoch: u64,
    pub chain_sha256: String,
    pub max_body_bytes: usize,
    pub max_sequence_body_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slots {
    pub mp4: u8,
    pub sequence: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReserveRequest {
    pub template_id: String,
    pub bundle_version: u32,
    pub node_version: String,
    pub slots: Slots,
    pub deadline_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequenceGrant {
    pub id: String,
    pub manifest_uuid: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReserveResponse {
    pub reservation_id: String,
    pub expires_at: u64,
    pub params: Params,
    pub sequence: Option<SequenceGrant>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignRequest {
    pub reservation_id: String,
    /// always "mp4"
    pub slot: String,
    pub reserve_size: usize,
    pub placeholder_len: usize,
    pub bmff_hash_sha256_hex: String,
    pub builder_archive_b64: String,
    /// The packed signature set (`fingerprint` module); required for a Picture output, absent for Data.
    pub fingerprint_b64: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignSequenceRequest {
    pub reservation_id: String,
    /// always "sequence"
    pub slot: String,
    pub frames_index_b64: String,
}

/// Every refusal the signer can give (contract section 8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    MalformedRequest,
    ArchiveUnreadable,
    Unauthorized,
    StaleRequest,
    ReservationUnknown,
    ReservationWrongHost,
    SlotConsumed,
    ReserveOutOfRange,
    ReserveTooSmall,
    VersionMismatch,
    ReservationExpired,
    BodyTooLarge,
    AssertionMismatch,
    ForbiddenStructure,
    PlaceholderLengthMismatch,
    ClassMismatch,
    FingerprintDigestMismatch,
    TsaUnavailable,
    KeyUnavailable,
    NotFound,
    RateLimited,
    Internal,
}

impl ErrorCode {
    pub fn status(self) -> u16 {
        use ErrorCode::*;
        match self {
            MalformedRequest | ArchiveUnreadable => 400,
            Unauthorized | StaleRequest => 401,
            ReservationWrongHost => 403,
            ReservationUnknown | NotFound => 404,
            SlotConsumed | ReserveOutOfRange | ReserveTooSmall | VersionMismatch => 409,
            ReservationExpired => 410,
            BodyTooLarge => 413,
            AssertionMismatch
            | ForbiddenStructure
            | PlaceholderLengthMismatch
            | ClassMismatch
            | FingerprintDigestMismatch => 422,
            RateLimited => 429,
            Internal => 500,
            TsaUnavailable | KeyUnavailable => 503,
        }
    }
    pub fn retryable(self) -> bool {
        matches!(
            self,
            ErrorCode::TsaUnavailable
                | ErrorCode::KeyUnavailable
                | ErrorCode::Internal
                | ErrorCode::RateLimited
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorDetail {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: ErrorDetail,
    /// The current limits, on `reserve_out_of_range` / `reserve_too_small` (redo the placeholder once).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<Limits>,
}

impl ErrorBody {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        ErrorBody {
            error: ErrorDetail {
                code,
                message: message.into(),
                retryable: code.retryable(),
            },
            limits: None,
        }
    }
}

// ------------------------------------------------------------------------------------------- request auth

pub fn keccak(b: &[u8]) -> [u8; 32] {
    let mut k = Keccak::v256();
    k.update(b);
    let mut out = [0u8; 32];
    k.finalize(&mut out);
    out
}

/// Canonical request bytes (UTF-8, LF line ends, no trailing newline).
pub fn canonical_bytes(
    method: &str,
    target: &str,
    ts: u64,
    nonce_hex: &str,
    body: &[u8],
) -> Vec<u8> {
    format!(
        "{DOMAIN}\n{}\n{}\nts:{}\nnonce:{}\nbody-sha256:{}",
        method.to_ascii_uppercase(),
        target,
        ts,
        nonce_hex.to_ascii_lowercase(),
        hex::encode(Sha256::digest(body))
    )
    .into_bytes()
}

/// EIP-191 `personal_sign` framing over a 32-byte digest (same as the node's `proof_signer::sign_eip191_digest`).
pub fn eip191_hash(digest: &[u8; 32]) -> [u8; 32] {
    let mut m = b"\x19Ethereum Signed Message:\n32".to_vec();
    m.extend_from_slice(digest);
    keccak(&m)
}

pub fn address_of(vk: &VerifyingKey) -> [u8; 20] {
    let pt = vk.to_encoded_point(false);
    let h = keccak(&pt.as_bytes()[1..]);
    h[12..].try_into().expect("20 bytes")
}

/// The three auth headers for one request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthHeaders {
    pub timestamp: String,
    pub nonce: String,
    pub signature: String,
}

/// Sign a request (node side; tests). `nonce` = 16 random bytes.
pub fn sign_request(
    sk: &SigningKey,
    method: &str,
    target: &str,
    ts: u64,
    nonce: [u8; 16],
    body: &[u8],
) -> AuthHeaders {
    let nonce_hex = hex::encode(nonce);
    let digest = keccak(&canonical_bytes(method, target, ts, &nonce_hex, body));
    let (sig, rid) = sk
        .sign_prehash_recoverable(&eip191_hash(&digest))
        .expect("prehash signing cannot fail for 32 bytes");
    let mut s65 = [0u8; 65];
    s65[..64].copy_from_slice(&sig.to_bytes());
    s65[64] = 27 + rid.to_byte();
    AuthHeaders {
        timestamp: ts.to_string(),
        nonce: nonce_hex,
        signature: format!("0x{}", hex::encode(s65)),
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("malformed auth header: {0}")]
    Malformed(&'static str),
    #[error("stale request")]
    Stale,
}

/// Parse the headers and recover the signing address. Window check included; allow-list and nonce
/// consumption are the signer's (in that order, after this).
pub fn recover_caller(
    now: u64,
    method: &str,
    target: &str,
    ts: &str,
    nonce_hex: &str,
    sig_hex: &str,
    body: &[u8],
) -> Result<[u8; 20], AuthError> {
    if ts.is_empty() || ts.len() > 12 || !ts.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AuthError::Malformed("timestamp"));
    }
    let ts: u64 = ts.parse().map_err(|_| AuthError::Malformed("timestamp"))?;
    if nonce_hex.len() != 32
        || !nonce_hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(AuthError::Malformed(
            "nonce must be 32 lower-case hex chars",
        ));
    }
    let s = sig_hex
        .strip_prefix("0x")
        .ok_or(AuthError::Malformed("signature must start with 0x"))?;
    if s.len() != 130 {
        return Err(AuthError::Malformed("signature must be 65 bytes"));
    }
    let sig = hex::decode(s).map_err(|_| AuthError::Malformed("signature hex"))?;
    if now.abs_diff(ts) > WINDOW_SECS {
        return Err(AuthError::Stale);
    }
    let digest = keccak(&canonical_bytes(method, target, ts, nonce_hex, body));
    recover(&sig, &eip191_hash(&digest))
        .map_err(|_| AuthError::Malformed("unrecoverable signature"))
}

pub fn recover(sig65: &[u8], eth_hash: &[u8; 32]) -> Result<[u8; 20], &'static str> {
    if sig65.len() != 65 {
        return Err("signature must be 65 bytes");
    }
    let v = sig65[64];
    // contract section 1: v = 27 or 28 only
    let v = match v {
        27 | 28 => v - 27,
        _ => return Err("bad v"),
    };
    let rid = RecoveryId::from_byte(v).ok_or("bad v")?;
    let sig = Signature::from_slice(&sig65[..64]).map_err(|_| "bad r||s")?;
    // reject high-s (malleable) signatures
    if sig.normalize_s().is_some() {
        return Err("high s");
    }
    let vk = VerifyingKey::recover_from_prehash(eth_hash, &sig, rid).map_err(|_| "recover")?;
    Ok(address_of(&vk))
}

pub fn parse_address(s: &str) -> Option<[u8; 20]> {
    let h = s.strip_prefix("0x").unwrap_or(s);
    let b = hex::decode(h).ok()?;
    b.try_into().ok()
}
