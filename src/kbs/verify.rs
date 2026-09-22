// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! The pure checks (design §6 step 4, §11): `prefilter` over the UNVERIFIED
//! evidence before any egress, and `check_policy` over the VERIFIED values. Both
//! are clock-free (the policy window is checked once, in step 3), evaluate EVERY
//! row and report every failing row (design D11); the first row is the headline.

use crate::kbs::config::GpuEvidenceMode;
use crate::kbs::cpu::{self, TdxEvidence};
use crate::kbs::error::KbsError;
use crate::kbs::eventlog::{self, Replayed};
use crate::kbs::gpu::CANNED_LABEL;
use crate::kbs::nras_claims::{CcAssertion, GpuOutcome};
use crate::tee::types::{sha256_32, version_at_least, CcMode, Policy};
use serde_json::Value;

/// The request's identity and nonce binding (design §11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expected {
    pub model_id: [u8; 32],
    pub pk_att: [u8; 33],
    pub nonce: [u8; 32],
}

/// The decision record for the log and the capture (design D16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    pub tcb_status: String,
    pub advisory_ids: Vec<String>,
    pub hwmodel: Option<String>,
    pub driver_version: Option<String>,
    pub vbios_version: Option<String>,
    pub cc_mode: CcRecord,
    pub td_debug_off: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CcRecord {
    /// The verified EAT showed `secboot: true` + `dbgstat` disabled, which
    /// rules DevTools out (D14 superseded 2026-09-22 by D14a). It does NOT
    /// separate On from Off; that is still the collector's NVML reading (G-6a).
    SignedNotDevTools,
    /// The EAT verified but did not show that pair: debug enabled (DevTools)
    /// or secure boot off. A policy asking for `require_cc_mode: On` refuses
    /// here, and so does the release gate for a non-test entry.
    SignedDevToolsOrNoSecureBoot,
    Canned,
}

impl CcRecord {
    /// The ONE spelling this value is logged, captured and grepped under (the
    /// runbook and gate B-8 quote it). `Debug` is for panics, not for the day.
    pub fn label(&self) -> &'static str {
        match self {
            Self::SignedNotDevTools => crate::kbs::nras_claims::LABEL_NOT_DEVTOOLS,
            Self::SignedDevToolsOrNoSecureBoot => {
                crate::kbs::nras_claims::LABEL_DEVTOOLS_OR_NO_SECURE_BOOT
            }
            Self::Canned => "canned",
        }
    }
}

/// What the pre-filter decoded, handed on to the verified half.
#[derive(Debug, Clone)]
pub struct Prefiltered {
    pub replayed: Replayed,
    pub gpu_payload: Value,
    /// `"canned": true` was carried (required in canned mode, refused in real mode).
    pub label_present: bool,
}

/// Design §6 step 4. Decides only whether the two paid/rate-limited egresses are
/// worth making; nothing here is authoritative.
pub fn prefilter(
    policy: &Policy,
    expected: &Expected,
    quote: &[u8],
    event_log: &[u8],
    gpu_report: &[u8],
    mode: GpuEvidenceMode,
) -> Result<Prefiltered, KbsError> {
    let mut rows: Vec<String> = Vec::new();

    // CPU registers and report_data off the unverified quote.
    let td = match cpu::decode_unverified(quote) {
        Ok(td) => Some(td),
        Err(e) => {
            rows.push(e.detail);
            None
        }
    };
    if let Some(td) = &td {
        check_report_data(&td.report_data, expected, &mut rows);
        check_registers(
            &policy.cvm,
            td.mr_td,
            td.rt_mr0,
            td.rt_mr1,
            td.rt_mr2,
            &mut rows,
        );
    }

    // Event log replay + extraction, against the unverified rt_mr3.
    let replayed = match eventlog::replay_and_extract(event_log) {
        Ok(r) => Some(r),
        Err(e) => {
            rows.push(e.0);
            None
        }
    };
    if let (Some(td), Some(r)) = (&td, &replayed) {
        check_events(&policy.cvm, td.rt_mr3, r, &mut rows);
    }

    // GPU payload shape, wrapper nonce, canned label vs mode.
    let (gpu_payload, label_present) = match check_gpu_payload(gpu_report, expected.nonce, mode) {
        Ok(v) => (Some(v.0), v.1),
        Err(e) => {
            rows.push(e);
            (None, false)
        }
    };

    if !rows.is_empty() {
        return Err(KbsError::verification_rows(&rows));
    }
    Ok(Prefiltered {
        replayed: replayed.expect("checked"),
        gpu_payload: gpu_payload.expect("checked"),
        label_present,
    })
}

/// Design §11. Every row of the pre-filter again on the VERIFIED report, then
/// TCB status, advisories, the GPU rows and the canned gate.
pub fn check_policy(
    policy: &Policy,
    expected: &Expected,
    cpu: &TdxEvidence,
    events: &Replayed,
    gpu: &GpuOutcome,
    entry_test: bool,
) -> Result<Verified, KbsError> {
    let mut rows: Vec<String> = Vec::new();

    if policy.model_id != expected.model_id {
        rows.push(format!(
            "model id: policy is for {}, request is for {}",
            hex::encode(policy.model_id),
            hex::encode(expected.model_id)
        ));
    }
    check_report_data(&cpu.report_data, expected, &mut rows);
    check_registers(
        &policy.cvm,
        cpu.mr_td,
        cpu.rt_mr0,
        cpu.rt_mr1,
        cpu.rt_mr2,
        &mut rows,
    );
    check_events(&policy.cvm, cpu.rt_mr3, events, &mut rows);
    if policy.cvm.require_td_debug_off && cpu.td_debug {
        rows.push("td debug: TUD.DEBUG is set".into());
    }
    if !policy
        .cvm
        .allowed_tcb_status
        .iter()
        .any(|s| s == &cpu.tcb_status)
    {
        rows.push(format!(
            "tcb status: {} not in {:?}",
            cpu.tcb_status, policy.cvm.allowed_tcb_status
        ));
    }
    let extra: Vec<&String> = cpu
        .advisory_ids
        .iter()
        .filter(|a| !policy.cvm.allowed_advisory_ids.contains(a))
        .collect();
    if !extra.is_empty() {
        rows.push(format!(
            "advisory: {:?} not in {:?}",
            extra, policy.cvm.allowed_advisory_ids
        ));
    }

    // GPU rows.
    match policy.gpu.require_cc_mode {
        None | Some(CcMode::On) => {}
        Some(other) => rows.push(format!(
            "policy require_cc_mode {other:?} is malformed (only On or absent is meaningful)"
        )),
    }
    let mut hwmodel = None;
    let mut driver_version = None;
    let mut vbios_version = None;
    let cc_mode = match gpu {
        GpuOutcome::Real(f) => {
            if !policy.gpu.allowed_hwmodels.iter().any(|h| h == &f.hwmodel) {
                rows.push(format!(
                    "hwmodel: {:?} not in {:?}",
                    f.hwmodel, policy.gpu.allowed_hwmodels
                ));
            }
            if policy.gpu.require_secure_boot && !f.secure_boot {
                rows.push("secure boot: off".into());
            }
            if policy.gpu.require_debug_disabled && !f.debug_disabled {
                rows.push("gpu debug: enabled".into());
            }
            if let Some(floor) = &policy.gpu.min_driver_version {
                if !version_at_least(&f.driver_version, floor) {
                    rows.push(format!(
                        "driver version: {} below floor {floor}",
                        f.driver_version
                    ));
                }
            }
            if let Some(floor) = &policy.gpu.min_vbios_version {
                if !version_at_least(&f.vbios_version, floor) {
                    rows.push(format!(
                        "vbios version: {} below floor {floor}",
                        f.vbios_version
                    ));
                }
            }
            hwmodel = Some(f.hwmodel.clone());
            driver_version = Some(f.driver_version.clone());
            vbios_version = Some(f.vbios_version.clone());
            // DERIVED from the two claims just checked, never read from a
            // field stored beside them (D14a): DevTools attests with the debug
            // facilities enabled, so `dbgstat: enabled` (or secure boot off)
            // fails a policy that asks for On. On versus Off is not decided
            // here; the measured collector refuses CC-off (G-6a).
            match f.cc_assertion() {
                CcAssertion::SignedNotDevTools => CcRecord::SignedNotDevTools,
                CcAssertion::SignedDevToolsOrNoSecureBoot {
                    secure_boot: sb,
                    debug_disabled: dd,
                } => {
                    // Refused when the policy asks for On, AND for any real
                    // keyring entry whatever the policy asked: a DEK that
                    // protects a model never goes to a GPU whose signed claims
                    // leave DevTools open. Both halves of the rule live here so
                    // the capture records the failing row (D11); the release
                    // gate repeats it from the claims themselves (D4).
                    if policy.gpu.require_cc_mode == Some(CcMode::On) || !entry_test {
                        rows.push(format!(
                            "cc mode: the verified claims show secboot {sb} and debug \
                             disabled {dd}, which does not rule DevTools out (that needs \
                             secboot true and a dbgstat in the disabled family){}",
                            if policy.gpu.require_cc_mode == Some(CcMode::On) {
                                "; the policy requires On"
                            } else {
                                "; a real keyring entry requires it whatever the policy asked"
                            }
                        ));
                    }
                    CcRecord::SignedDevToolsOrNoSecureBoot
                }
            }
        }
        GpuOutcome::CannedTolerated => {
            if !entry_test {
                rows.push("canned GPU evidence tolerated only for a test keyring entry".into());
            }
            CcRecord::Canned
        }
    };

    if !rows.is_empty() {
        return Err(KbsError::verification_rows(&rows));
    }
    Ok(Verified {
        tcb_status: cpu.tcb_status.clone(),
        advisory_ids: cpu.advisory_ids.clone(),
        hwmodel,
        driver_version,
        vbios_version,
        cc_mode,
        td_debug_off: !cpu.td_debug,
    })
}

fn check_report_data(report_data: &[u8; 64], expected: &Expected, rows: &mut Vec<String>) {
    let want_id = sha256_32(&expected.pk_att);
    if report_data[..32] != want_id {
        rows.push(format!(
            "identity: report_data[..32] = {}, want sha256(pk_att) = {}",
            hex::encode(&report_data[..32]),
            hex::encode(want_id)
        ));
    }
    if report_data[32..] != expected.nonce {
        rows.push(format!(
            "nonce: report_data[32..] = {}, want {}",
            hex::encode(&report_data[32..]),
            hex::encode(expected.nonce)
        ));
    }
}

fn check_registers(
    cvm: &crate::tee::types::CvmPolicy,
    mr_td: [u8; 48],
    rt_mr0: [u8; 48],
    rt_mr1: [u8; 48],
    rt_mr2: [u8; 48],
    rows: &mut Vec<String>,
) {
    for (name, got, want) in [
        ("mrtd", mr_td, &cvm.mrtd),
        ("rtmr0", rt_mr0, &cvm.rtmr0),
        ("rtmr1", rt_mr1, &cvm.rtmr1),
        ("rtmr2", rt_mr2, &cvm.rtmr2),
    ] {
        if !hex_eq(&got, want) {
            rows.push(format!("{name}: {} != policy {want}", hex::encode(got)));
        }
    }
}

fn check_events(
    cvm: &crate::tee::types::CvmPolicy,
    rt_mr3: [u8; 48],
    r: &Replayed,
    rows: &mut Vec<String>,
) {
    if r.rtmr3 != rt_mr3 {
        rows.push(format!(
            "event log: replayed rtmr3 {} != quote {}",
            hex::encode(r.rtmr3),
            hex::encode(rt_mr3)
        ));
    }
    if !hex_eq(&r.compose_hash, &cvm.compose_hash) {
        rows.push(format!(
            "compose hash: {} != policy {}",
            hex::encode(r.compose_hash),
            cvm.compose_hash
        ));
    }
    if !hex_eq(&r.os_image_hash, &cvm.os_image_hash) {
        rows.push(format!(
            "os image: {} != policy {}",
            hex::encode(r.os_image_hash),
            cvm.os_image_hash
        ));
    }
    if let Some(want) = &cvm.app_id {
        match &r.app_id {
            Some(got) if hex_eq(got, want) => {}
            Some(got) => rows.push(format!("app id: {} != policy {want}", hex::encode(got))),
            None => rows.push("app id: event absent, policy pins one".into()),
        }
    }
    if let Some(want) = &cvm.key_provider {
        match &r.key_provider {
            Some(got) if hex_eq(got, want) => {}
            Some(got) => rows.push(format!(
                "key provider: {} != policy {want}",
                hex::encode(got)
            )),
            None => rows.push("key provider: event absent, policy pins one".into()),
        }
    }
}

/// `{nonce, evidence_list, arch, canned?}`: an object, exactly one evidence entry,
/// wrapper nonce equal to ours (matching satisfies nothing), label vs mode.
fn check_gpu_payload(
    gpu_report: &[u8],
    nonce: [u8; 32],
    mode: GpuEvidenceMode,
) -> Result<(Value, bool), String> {
    let v: Value = serde_json::from_slice(gpu_report).map_err(|e| format!("gpu payload: {e}"))?;
    let obj = v.as_object().ok_or("gpu payload: not a JSON object")?;
    let list = obj
        .get("evidence_list")
        .and_then(Value::as_array)
        .ok_or("gpu payload: evidence_list missing")?;
    if list.len() != 1 {
        return Err(format!(
            "gpu count: evidence_list has {} entries, want 1",
            list.len()
        ));
    }
    if obj.get("arch").and_then(Value::as_str).is_none() {
        return Err("gpu payload: arch missing".into());
    }
    let want = hex::encode(nonce);
    match obj.get("nonce").and_then(Value::as_str) {
        Some(n) if n.eq_ignore_ascii_case(&want) => {}
        Some(n) => return Err(format!("gpu nonce (wrapper): {n} != {want}")),
        None => return Err("gpu nonce (wrapper): missing".into()),
    }
    let label = obj.get(CANNED_LABEL);
    let label_present = label.and_then(Value::as_bool) == Some(true);
    match mode {
        GpuEvidenceMode::Real if label.is_some() => {
            return Err("gpu: canned evidence under real mode".into());
        }
        GpuEvidenceMode::Canned if !label_present => {
            return Err("gpu: canned mode requires \"canned\": true".into());
        }
        _ => {}
    }
    Ok((v, label_present))
}

/// Bytes vs the policy's hex string, case-insensitive on the hex.
fn hex_eq(bytes: &[u8], hex_str: &str) -> bool {
    hex::decode(hex_str).map(|h| h == bytes).unwrap_or(false)
}
