// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! The pure checks (design §6 step 4, §11): `prefilter` over the UNVERIFIED
//! evidence before any egress, and `check_policy` over the VERIFIED values. Both
//! evaluate EVERY row and report every failing row (design D11); the first row is
//! the headline.
//!
//! Policy schema 3 (`IMPLEMENTATION-POLICY-V3-HOST-PROFILES.md` D7–D9): the host
//! identity rows (registers, OS image, compose, app id, key provider, hwmodel)
//! are judged per CANDIDATE profile of the root's authority bundle by the shared
//! matcher; the profile-independent rows (report data, the RTMR3 replay, model
//! id, TD debug, TCB, advisories, CC and the policy's GPU rows) are evaluated
//! once. A refusal with no matching profile names the closest candidate with its
//! rows, the candidate counts, and every EXCLUDED or REMOVED profile whose
//! identity rows all hold, with the reason it was excluded (D8). The prefilter
//! never feeds the authoritative check: step 7 re-runs identity on the verified
//! values over the FULL candidate list (D9).

use crate::kbs::authority::Authority;
use crate::kbs::config::GpuEvidenceMode;
use crate::kbs::cpu::{self, TdxEvidence};
use crate::kbs::error::KbsError;
use crate::kbs::eventlog::{self, Replayed};
use crate::kbs::gpu::CANNED_LABEL;
use crate::kbs::nras_claims::{CcAssertion, GpuOutcome};
use crate::tee::host_profile::{match_profile, HostProfile, HwModel, Observed};
use crate::tee::types::{sha256_32, version_at_least, CcMode, Policy};
use serde_json::Value;

/// The request's identity and nonce binding (design §11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expected {
    pub model_id: [u8; 32],
    pub pk_att: [u8; 33],
    pub nonce: [u8; 32],
}

/// Which profile(s) released this request and under which documents (D12).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedProfiles {
    /// The first matching profile by `id`: the one the log names.
    pub headline: String,
    /// Every matching profile, sorted by `id`.
    pub all: Vec<String>,
    /// The headline profile's class.
    pub class: String,
    pub epoch: u64,
    pub manifest_version: u32,
    /// The distinct current signers counted on the set.
    pub counted_signers: Vec<String>,
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
    pub profile: MatchedProfiles,
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
/// worth making; nothing here is authoritative (D9). The identity rows run over
/// the candidates on the UNVERIFIED quote with the hwmodel not yet known; zero
/// survivors refuse before PCCS/NRAS with the same self-explaining rows step 7
/// would print (D8: a boot with zero candidates never reaches NRAS).
#[allow(clippy::too_many_arguments)]
pub fn prefilter(
    policy: &Policy,
    auth: &Authority,
    expected: &Expected,
    quote: &[u8],
    event_log: &[u8],
    gpu_report: &[u8],
    mode: GpuEvidenceMode,
    now: u64,
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
        rtmr3_row(td.rt_mr3, r, &mut rows);
        let o = observed(
            td.mr_td,
            td.rt_mr0,
            td.rt_mr1,
            td.rt_mr2,
            r,
            HwModel::NotYetKnown,
        );
        rows.extend(judge_identity(auth, policy, &o, now).refusal);
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

/// Design §11 + schema 3 D7. The profile-independent rows on the VERIFIED
/// report once, the identity rows over the FULL candidate list (never the
/// prefilter's survivors, D9), then the floor: the policy's GPU rows as before
/// plus, for EVERY matched profile, its own floors applied independently (a
/// profile only adds requirements). In `GpuOutcome::CannedTolerated` (test
/// entries only) no GPU evidence row runs at all, profile floors and the
/// hwmodel row included: there is no GPU data.
#[allow(clippy::too_many_arguments)]
pub fn check_policy(
    policy: &Policy,
    auth: &Authority,
    expected: &Expected,
    cpu: &TdxEvidence,
    events: &Replayed,
    gpu: &GpuOutcome,
    entry_test: bool,
    now: u64,
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
    rtmr3_row(cpu.rt_mr3, events, &mut rows);
    let hw = match gpu {
        GpuOutcome::Real(f) => HwModel::Known(f.hwmodel.clone()),
        GpuOutcome::CannedTolerated => HwModel::CannedTestEntry,
    };
    let o = observed(cpu.mr_td, cpu.rt_mr0, cpu.rt_mr1, cpu.rt_mr2, events, hw);
    let identity = judge_identity(auth, policy, &o, now);
    rows.extend(identity.refusal.iter().cloned());
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
    let profiles = auth.profiles();
    let mut hwmodel = None;
    let mut driver_version = None;
    let mut vbios_version = None;
    let cc_mode = match gpu {
        GpuOutcome::Real(f) => {
            // The hwmodel rows are identity rows now (profile list AND policy
            // list, D7), judged above per candidate.
            if policy.gpu.require_secure_boot && !f.secure_boot {
                rows.push("secure boot: off".into());
            }
            if policy.gpu.require_debug_disabled && !f.debug_disabled {
                rows.push("gpu debug: enabled".into());
            }
            let floors = [(
                None,
                &policy.gpu.min_driver_version,
                &policy.gpu.min_vbios_version,
            )]
            .into_iter()
            .chain(identity.matched.iter().map(|&i| {
                let p = &profiles[i];
                (
                    Some(p.id.as_str()),
                    &p.gpu.min_driver_version,
                    &p.gpu.min_vbios_version,
                )
            }));
            // Every floor independently (D7): no stricter-of arithmetic, so a
            // malformed floor refuses rather than being skipped.
            for (whose, driver, vbios) in floors {
                let of = whose.map_or_else(String::new, |id| format!(" of profile {id}"));
                if let Some(floor) = driver {
                    if !version_at_least(&f.driver_version, floor) {
                        rows.push(format!(
                            "driver version: {} below floor {floor}{of}",
                            f.driver_version
                        ));
                    }
                }
                if let Some(floor) = vbios {
                    if !version_at_least(&f.vbios_version, floor) {
                        rows.push(format!(
                            "vbios version: {} below floor {floor}{of}",
                            f.vbios_version
                        ));
                    }
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
    let head = &profiles[identity.matched[0]];
    Ok(Verified {
        tcb_status: cpu.tcb_status.clone(),
        advisory_ids: cpu.advisory_ids.clone(),
        hwmodel,
        driver_version,
        vbios_version,
        cc_mode,
        td_debug_off: !cpu.td_debug,
        profile: MatchedProfiles {
            headline: head.id.clone(),
            all: identity
                .matched
                .iter()
                .map(|&i| profiles[i].id.clone())
                .collect(),
            class: head.class.clone(),
            epoch: auth.set.set.epoch,
            manifest_version: auth.manifest().manifest_version,
            counted_signers: auth.counted.signers.clone(),
        },
    })
}

/// The outcome of judging the host identity against the candidates (D7, D8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityOutcome {
    /// Indices into the set of the candidates whose identity rows all hold,
    /// sorted by profile `id` (the first is the headline).
    pub matched: Vec<usize>,
    /// When nothing matched: the refusal rows (closest candidate + its rows,
    /// the counts, the excluded/removed hints). Empty when something matched.
    pub refusal: Vec<String>,
}

/// Judge `o` against every candidate of `auth` (D7) and, when none matches,
/// build the self-explaining refusal (D8): the closest candidate (fewest
/// failing identity rows; ties by `id`) with ALL its rows, the counts, and a
/// hint for every excluded or removed profile whose identity rows all hold.
/// Pure and clock-free apart from `now` for the expiry wording; the removed
/// profiles come from the cached ledger in `auth`, so no IO happens here.
pub fn judge_identity(
    auth: &Authority,
    policy: &Policy,
    o: &Observed,
    now: u64,
) -> IdentityOutcome {
    let profiles = auth.profiles();
    let hw = &policy.gpu.allowed_hwmodels;
    let judged: Vec<(usize, Vec<String>)> = auth
        .candidates
        .list
        .iter()
        .map(|&i| (i, match_profile(&profiles[i], o, hw)))
        .collect();
    let mut matched: Vec<usize> = judged
        .iter()
        .filter(|(_, rows)| rows.is_empty())
        .map(|(i, _)| *i)
        .collect();
    matched.sort_by(|a, b| profiles[*a].id.cmp(&profiles[*b].id));
    if !matched.is_empty() {
        return IdentityOutcome {
            matched,
            refusal: vec![],
        };
    }
    let mut refusal = Vec::new();
    if let Some((i, rows)) = judged.iter().min_by(|(a, ra), (b, rb)| {
        ra.len()
            .cmp(&rb.len())
            .then(profiles[*a].id.cmp(&profiles[*b].id))
    }) {
        refusal.push(format!(
            "no profile matches: closest is profile {} ({} failing row(s))",
            profiles[*i].id,
            rows.len()
        ));
        refusal.extend(rows.iter().cloned());
    } else {
        refusal.push("no profile matches: no candidate profile".into());
    }
    let c = &auth.candidates;
    refusal.push(format!(
        "candidates={} of {} (expired={}, class_filtered={}, unpinned={}) in epoch {} manifest v{}",
        c.list.len(),
        profiles.len(),
        c.expired.len(),
        c.class_filtered.len(),
        c.unpinned.len(),
        auth.set.set.epoch,
        auth.manifest().manifest_version
    ));
    let matches = |p: &HostProfile| match_profile(p, o, hw).is_empty();
    for &i in &c.expired {
        let p = &profiles[i];
        if matches(p) {
            refusal.push(if now < p.not_before {
                format!(
                    "matches {} but it is not valid before {}",
                    p.id, p.not_before
                )
            } else {
                format!("matches {} but expired at {}", p.id, p.expiry)
            });
        }
    }
    for &i in &c.class_filtered {
        let p = &profiles[i];
        if matches(p) {
            refusal.push(format!(
                "matches {} but class {} is not allowed by the policy",
                p.id, p.class
            ));
        }
    }
    for &i in &c.unpinned {
        let p = &profiles[i];
        if matches(p) {
            refusal.push(format!(
                "matches {} but the policy requires a pinned {}",
                p.id,
                if policy.cvm.require_app_id_pinned && p.cvm.app_id.is_none() {
                    "app id"
                } else {
                    "key provider"
                }
            ));
        }
    }
    for r in &auth.removed {
        if matches(&r.profile) {
            refusal.push(format!(
                "matches {}, present in the archived bundle {} but absent from epoch {}",
                r.profile.id, r.archive_file, r.epoch
            ));
        }
    }
    IdentityOutcome {
        matched: vec![],
        refusal,
    }
}

/// The identity the evidence shows, as the matcher reads it.
pub fn observed(
    mr_td: [u8; 48],
    rt_mr0: [u8; 48],
    rt_mr1: [u8; 48],
    rt_mr2: [u8; 48],
    r: &Replayed,
    hwmodel: HwModel,
) -> Observed {
    Observed {
        mr_td,
        rt_mr0,
        rt_mr1,
        rt_mr2,
        compose_hash: r.compose_hash,
        os_image_hash: r.os_image_hash,
        app_id: r.app_id.clone(),
        key_provider: r.key_provider.clone(),
        hwmodel,
    }
}

fn rtmr3_row(rt_mr3: [u8; 48], r: &Replayed, rows: &mut Vec<String>) {
    if r.rtmr3 != rt_mr3 {
        rows.push(format!(
            "event log: replayed rtmr3 {} != quote {}",
            hex::encode(r.rtmr3),
            hex::encode(rt_mr3)
        ));
    }
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
