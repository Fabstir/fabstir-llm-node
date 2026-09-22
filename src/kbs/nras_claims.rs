// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! The NRAS claim-name table (design §10.1), one place, versioned by the overall
//! token's `x-nvidia-ver`. Missing claim = refuse. Every row is evaluated and every
//! failing row is reported (design D11) so the first real EAT on the paid day
//! yields every pin at once.
//!
//! The "never" rows (the chain that makes the answer NVIDIA's and this nonce's)
//! are hardcoded literals: the overall-token ones in `gpu.rs`, the three per-GPU
//! ones (`eat_nonce`, the absent error details, the report-nonce match) in
//! `map_per_gpu` below, reading through this table's fields. The table maps the
//! per-GPU claims that B-4 may show under another name ("renameable"): a runbook
//! patch renames a key here and nothing else.

use serde_json::{Map, Value};

/// The ONE spelling of each state, so the log line, both halves of a capture
/// and gate B-8's grep cannot drift apart (`CcRecord::label` returns these too).
pub const LABEL_NOT_DEVTOOLS: &str = "signed-not-devtools";
pub const LABEL_DEVTOOLS_OR_NO_SECURE_BOOT: &str = "signed-devtools-or-no-secure-boot";

/// What the SIGNED per-GPU claims say about the GPU's protection state
/// (design D14, superseded 2026-09-22 by D14a, Phala's answer to the G-6
/// question).
///
/// NVIDIA publishes no literal "CC mode" claim, but its own relying-party
/// policy example reads the pair `secboot` and `dbgstat`: DevTools mode
/// attests with the debug facilities enabled and therefore reports
/// `dbgstat: enabled`, so the pair RULES DEVTOOLS OUT. Both claims come from
/// the EAT this broker has already verified against NVIDIA's JWKS for this
/// challenge nonce, so they are signed evidence rather than the node's word.
/// The mapping itself is NVIDIA's documentation plus Phala's confirmation;
/// nobody in this chain has yet seen an EAT from a GPU actually in DevTools
/// mode, which gate B-8 captures on the first real H200 (gap G-6).
///
/// What the pair does NOT do is separate CC mode On from Off: a GPU with
/// confidential computing disabled is expected to report secure boot on and
/// debug disabled too. That premise is unobserved here, exactly like the
/// DevTools mapping above; it errs safe, since it only ever makes the pair
/// weaker and never releases a key the pair would refuse. Off is refused only
/// by the measured in-guest collector's NVML reading (`collect_gpu_evidence.py`,
/// exit 75 when `cc_enabled` is false), which is node-asserted; that residual
/// is gap G-6a.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CcAssertion {
    /// `secboot: true` and a `dbgstat` in the disabled family: DevTools is
    /// ruled out by signed evidence. On versus Off is not (G-6a).
    SignedNotDevTools,
    /// The pair is absent: the debug facilities are enabled (DevTools) or
    /// secure boot is off. Carries both so a refusal can name them.
    SignedDevToolsOrNoSecureBoot {
        secure_boot: bool,
        debug_disabled: bool,
    },
}

impl CcAssertion {
    /// The pair as NVIDIA's relying-party example reads it. Derive it at every
    /// use site from the two mapped claims; never store it beside them, or the
    /// derived value can disagree with what it was derived from.
    pub fn from_claims(secure_boot: bool, debug_disabled: bool) -> Self {
        if secure_boot && debug_disabled {
            Self::SignedNotDevTools
        } else {
            Self::SignedDevToolsOrNoSecureBoot {
                secure_boot,
                debug_disabled,
            }
        }
    }

    /// The word this state is logged and captured under.
    pub fn label(&self) -> &'static str {
        match self {
            Self::SignedNotDevTools => LABEL_NOT_DEVTOOLS,
            Self::SignedDevToolsOrNoSecureBoot { .. } => LABEL_DEVTOOLS_OR_NO_SECURE_BOOT,
        }
    }
}

/// What the broker decides on (design D16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuFields {
    pub nonce: [u8; 32],
    pub hwmodel: String,
    pub secure_boot: bool,
    pub debug_disabled: bool,
    pub driver_version: String,
    pub vbios_version: String,
}

impl GpuFields {
    /// The signed debug/boot verdict, DERIVED here rather than stored: a
    /// stored copy beside the two claims it comes from can disagree with them.
    pub fn cc_assertion(&self) -> CcAssertion {
        CcAssertion::from_claims(self.secure_boot, self.debug_disabled)
    }
}

/// The GPU half's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuOutcome {
    Real(GpuFields),
    /// Canned mode only: overall `false` + per-GPU `NONCE_NOT_MATCHING` on the
    /// labelled sample evidence. The release step refuses it unless the keyring
    /// entry is `test: true` (design D4).
    CannedTolerated,
}

/// Claim names for one claims version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimNames {
    pub version: &'static str,
    pub nonce: &'static str,
    pub error_details: &'static str,
    pub nonce_match: &'static str,
    pub hwmodel: &'static str,
    pub secboot: &'static str,
    pub dbgstat: &'static str,
    pub driver_version: &'static str,
    pub vbios_version: &'static str,
    pub cert_chain_validated: &'static str,
    pub signature_verified: &'static str,
    pub arch_check: &'static str,
    pub measres: &'static str,
}

/// NVIDIA's documented v3 GPU claim set, claims version 2.0 (the version the
/// A-17 capture shows).
pub const CLAIMS_2_0: ClaimNames = ClaimNames {
    version: "2.0",
    nonce: "eat_nonce",
    error_details: "x-nvidia-error-details",
    nonce_match: "x-nvidia-gpu-attestation-report-nonce-match",
    hwmodel: "hwmodel",
    secboot: "secboot",
    dbgstat: "dbgstat",
    driver_version: "x-nvidia-gpu-driver-version",
    vbios_version: "x-nvidia-gpu-vbios-version",
    cert_chain_validated: "x-nvidia-gpu-attestation-report-cert-chain-validated",
    signature_verified: "x-nvidia-gpu-attestation-report-signature-verified",
    arch_check: "x-nvidia-gpu-arch-check",
    measres: "measres",
};

/// The table for `version`, or `None` (a claims-version bump refuses until this
/// table is extended; design §2 `KBS_NRAS_CLAIMS_VERSION`).
pub fn table_for(version: &str) -> Option<&'static ClaimNames> {
    match version {
        "2.0" => Some(&CLAIMS_2_0),
        _ => None,
    }
}

/// The RFC 9711 `dbgstat` vocabulary: every `disabled*` value means debug is off.
pub fn dbgstat_is_disabled(v: &str) -> Option<bool> {
    match v {
        "disabled"
        | "disabled-since-boot"
        | "disabled-permanently"
        | "disabled-fully-and-permanently" => Some(true),
        "enabled" => Some(false),
        _ => None,
    }
}

/// Map the VERIFIED per-GPU claims to `GpuFields`. Every row is evaluated; on any
/// failure every failing row is returned (first = headline).
pub fn map_per_gpu(
    claims: &Map<String, Value>,
    names: &ClaimNames,
    issued_nonce: [u8; 32],
) -> Result<GpuFields, Vec<String>> {
    let mut rows: Vec<String> = Vec::new();
    let want_nonce = hex::encode(issued_nonce);

    let nonce_ok = match claims.get(names.nonce).and_then(Value::as_str) {
        Some(s) if s.eq_ignore_ascii_case(&want_nonce) => true,
        Some(s) => {
            rows.push(format!("gpu nonce (signed): got {s}, want {want_nonce}"));
            false
        }
        None => {
            rows.push(format!("gpu nonce (signed): claim {} missing", names.nonce));
            false
        }
    };
    if claims.contains_key(names.error_details) {
        rows.push(format!(
            "gpu error details present: {}",
            claims
                .get(names.error_details)
                .map(|v| v.to_string())
                .unwrap_or_default()
        ));
    }
    for (name, what) in [
        (names.nonce_match, "nonce match"),
        (names.cert_chain_validated, "cert chain validated"),
        (names.signature_verified, "signature verified"),
        (names.arch_check, "arch check"),
    ] {
        match claims.get(name) {
            Some(Value::Bool(true)) => {}
            Some(other) => rows.push(format!("gpu {what}: {name} = {other}, want true")),
            None => rows.push(format!("gpu {what}: claim {name} missing")),
        }
    }
    match claims.get(names.measres).and_then(Value::as_str) {
        Some("success") => {}
        Some(other) => rows.push(format!("gpu measres: {other}, want success")),
        None => rows.push(format!("gpu measres: claim {} missing", names.measres)),
    }
    let hwmodel = str_claim(claims, names.hwmodel, "hwmodel", &mut rows);
    let driver_version = str_claim(claims, names.driver_version, "driver version", &mut rows);
    let vbios_version = str_claim(claims, names.vbios_version, "vbios version", &mut rows);
    let secure_boot = match claims.get(names.secboot) {
        Some(Value::Bool(b)) => Some(*b),
        Some(other) => {
            rows.push(format!("gpu secboot: {other} is not a boolean"));
            None
        }
        None => {
            rows.push(format!("gpu secboot: claim {} missing", names.secboot));
            None
        }
    };
    let debug_disabled = match claims.get(names.dbgstat).and_then(Value::as_str) {
        Some(s) => match dbgstat_is_disabled(s) {
            Some(b) => Some(b),
            None => {
                rows.push(format!("gpu dbgstat: unknown value {s:?}"));
                None
            }
        },
        None => {
            rows.push(format!("gpu dbgstat: claim {} missing", names.dbgstat));
            None
        }
    };

    if !rows.is_empty() || !nonce_ok {
        return Err(rows);
    }
    let secure_boot = secure_boot.expect("checked");
    let debug_disabled = debug_disabled.expect("checked");
    Ok(GpuFields {
        nonce: issued_nonce,
        hwmodel: hwmodel.expect("checked"),
        secure_boot,
        debug_disabled,
        driver_version: driver_version.expect("checked"),
        vbios_version: vbios_version.expect("checked"),
    })
}

fn str_claim(
    claims: &Map<String, Value>,
    name: &str,
    what: &str,
    rows: &mut Vec<String>,
) -> Option<String> {
    match claims.get(name) {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(other) => {
            rows.push(format!(
                "gpu {what}: {name} = {other} is not a non-empty string"
            ));
            None
        }
        None => {
            rows.push(format!("gpu {what}: claim {name} missing"));
            None
        }
    }
}
