// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Attestation verifier: the [`AttestationVerifier`] trait and the
//! `DefaultVerifier` policy logic (Phase 1.3).
//!
//! The `report_data` layout helpers ([`report_data_identity`], [`sha256_32`])
//! and the decoded [`GpuReportFields`] live in [`crate::tee::types`] — the
//! neutral shared home, so the provider and verifier compute the
//! security-critical layout identically. The real quote/NRAS verifier lands in
//! Phase 5 behind this same trait, reusing the identity, shared-nonce and
//! policy-validity logic here.

use crate::tee::types::{
    now_unix, report_data_identity, sha256_32, version_at_least, Claims, Evidence, GpuReportFields,
    Policy, TeeError, TeeResult, REPORT_DATA_LEN,
};

/// Verifies attestation [`Evidence`] against a model-provider [`Policy`].
///
/// Synchronous and HW-agnostic at the trait level: implementations perform pure
/// checks (identity, shared nonce, measurement, SKU, CC-on, TCB, policy validity) and, in
/// Phase 5, RIM and CPU/GPU certificate-chain verification.
pub trait AttestationVerifier: Send + Sync {
    /// Verify `ev` against `policy`, requiring both quotes to be bound to
    /// `expected_nonce`. Returns [`Claims`] on success, else [`TeeError`].
    fn verify(&self, ev: &Evidence, policy: &Policy, expected_nonce: [u8; 32])
        -> TeeResult<Claims>;
}

/// Pure, hardware-agnostic verifier for the mock-backed pipeline (Phases 1–4).
///
/// Performs the identity, shared-nonce, measurement, SKU, CC-on, TCB, and policy-validity
/// checks in fail-closed order: any check that does not pass returns
/// [`TeeError::VerificationFailed`] with no [`Claims`].
pub struct DefaultVerifier;

impl AttestationVerifier for DefaultVerifier {
    fn verify(
        &self,
        ev: &Evidence,
        policy: &Policy,
        expected_nonce: [u8; 32],
    ) -> TeeResult<Claims> {
        self.verify_at(ev, policy, expected_nonce, now_unix())
    }
}

impl DefaultVerifier {
    /// Verify with an explicit `now` (unix seconds). Production calls
    /// [`AttestationVerifier::verify`], which supplies the real clock; this
    /// overload makes the policy-validity boundary (`not_before <= now <=
    /// expiry`, inclusive) deterministically testable.
    pub fn verify_at(
        &self,
        ev: &Evidence,
        policy: &Policy,
        expected_nonce: [u8; 32],
        now: u64,
    ) -> TeeResult<Claims> {
        // 0. The policy itself must be schema 2 in canonical spelling; nothing is
        //    coerced (the provider signed these exact bytes).
        policy.validate()?;

        // 1. Freshness: evidence must be bound to the KBS-issued nonce. (Nonce issuance,
        //    single-use, and TTL are enforced by the KBS in Phase 3.2; here we only bind the
        //    evidence to the caller's expected nonce.)
        if ev.nonce != expected_nonce {
            return Err(TeeError::VerificationFailed("nonce mismatch".into()));
        }

        // 1b. Real evidence from `DstackAttestationProvider` (an `nvidia_payload` JSON
        //     object with an `evidence_list`) cannot be judged by this mock-only verifier
        //     and is refused here, BEFORE the report_data checks: a real TDX quote is
        //     thousands of bytes whose first 32 are the DCAP header, so the identity
        //     check below would otherwise fail first with a misleading "pk_att" error.
        //     A full JSON parse, not a byte sniff (bincode puts the nonce at byte 0).
        if is_nvidia_payload(&ev.gpu_report) {
            return Err(TeeError::VerificationFailed(
                "real GPU evidence (nvidia_payload JSON) needs the Phase-5 verifier; \
                 DefaultVerifier is mock-only"
                    .into(),
            ));
        }

        // 2. The mock CPU quote must carry a 64-byte report_data field. (Phase 5: a real
        //    TDX quote is longer; the real verifier extracts report_data from the
        //    dcap-qvl-verified TD report body — this `[..64]` view is mock-only.)
        if ev.cpu_quote.len() < REPORT_DATA_LEN {
            return Err(TeeError::VerificationFailed("cpu_quote too short".into()));
        }
        let report_data = &ev.cpu_quote[..REPORT_DATA_LEN];

        // 3. Identity: report_data[0..32] == sha256(pk_att). This is what makes the
        //    node-asserted `ev.pk_att` mean something: the CPU TEE signed a commitment to
        //    exactly this key, so wrapping the DEK to it releases to the attested CVM only.
        //    (The mock merely echoes `pk_att`; the real verifier compares against the
        //    SIGNED quote body. The key-wrap layer rejects a non-parsing point.)
        if report_data[..32] != report_data_identity(&ev.pk_att) {
            return Err(TeeError::VerificationFailed(
                "report_data identity mismatch".into(),
            ));
        }

        // 4. Shared nonce, CPU half: report_data[32..64] == the KBS-issued nonce, in the
        //    clear. Catches a replayed quote and a quote produced for another challenge.
        if report_data[32..] != expected_nonce {
            return Err(TeeError::VerificationFailed(
                "report_data nonce mismatch".into(),
            ));
        }

        // 5. Decode the GPU-report fields (Phase 5 forwards the real payload to NRAS and
        //    maps the EAT claims into these fields instead). Real payloads were refused at
        //    1b, so a failure here is a malformed mock report.
        let fields: GpuReportFields = bincode::deserialize(&ev.gpu_report)
            .map_err(|e| TeeError::VerificationFailed(format!("gpu report decode: {e}")))?;

        // 6. Shared nonce, GPU half: the GPU evidence was collected under the SAME
        //    challenge. Two independently signed quotes, one nonce: this is the whole
        //    cross-binding, and it catches a genuine CPU quote paired with GPU evidence
        //    collected for a different challenge (a different session, box or replay).
        if fields.nonce != expected_nonce {
            return Err(TeeError::VerificationFailed(
                "gpu evidence nonce mismatch".into(),
            ));
        }
        let gpu_report_hash = sha256_32(&ev.gpu_report);

        // 7. Measurement (mock-era): the mock's 48-byte `image_measurement` stands in
        //    for MRTD and is compared with `policy.cvm.mrtd`. The real verifier compares
        //    MRTD, RTMR0–2 from the VERIFIED quote and replays RTMR3 for os_image_hash /
        //    compose_hash; the mock cannot, so those fields are validated for form
        //    (step 0) but not compared here. Never trust this field on a real path.
        if hex::encode(ev.image_measurement) != policy.cvm.mrtd {
            return Err(TeeError::VerificationFailed("mrtd mismatch".into()));
        }

        // 8. Hardware model must be allowed by the policy.
        if !policy
            .gpu
            .allowed_hwmodels
            .iter()
            .any(|s| s == &fields.hwmodel)
        {
            return Err(TeeError::VerificationFailed(format!(
                "disallowed hwmodel: {}",
                fields.hwmodel
            )));
        }

        // 9. CC mode must match the policy EXACTLY when it requires one.
        //    Exact, not "at least on": `devtools` attests with the protections
        //    disabled, so anything looser than equality releases the key to an
        //    unprotected GPU. A policy that genuinely wants devtools has to name
        //    it.
        if let Some(required) = policy.gpu.require_cc_mode {
            if fields.cc_mode != required {
                return Err(TeeError::VerificationFailed(format!(
                    "cc mode {:?}, policy requires {:?}",
                    fields.cc_mode, required
                )));
            }
        }

        // 10. TD DEBUG attribute clear when the policy requires it.
        if policy.cvm.require_td_debug_off && !fields.td_debug_off {
            return Err(TeeError::VerificationFailed("td debug enabled".into()));
        }

        // 11. TCB status in the allow-list (exact string, e.g. "UpToDate").
        if !policy
            .cvm
            .allowed_tcb_status
            .iter()
            .any(|s| s == &fields.tcb_status)
        {
            return Err(TeeError::VerificationFailed(format!(
                "tcb status {} not allowed",
                fields.tcb_status
            )));
        }

        // 11b. GPU secure boot / debug status.
        if policy.gpu.require_secure_boot && !fields.secure_boot {
            return Err(TeeError::VerificationFailed("gpu secure boot off".into()));
        }
        if policy.gpu.require_debug_disabled && !fields.debug_disabled {
            return Err(TeeError::VerificationFailed("gpu debug enabled".into()));
        }

        // 11c. Version floors (dotted numeric compare).
        if let Some(min) = &policy.gpu.min_driver_version {
            if !version_at_least(&fields.driver_version, min) {
                return Err(TeeError::VerificationFailed(format!(
                    "driver {} below policy minimum {min}",
                    fields.driver_version
                )));
            }
        }
        if let Some(min) = &policy.gpu.min_vbios_version {
            if !version_at_least(&fields.vbios_version, min) {
                return Err(TeeError::VerificationFailed(format!(
                    "vbios {} below policy minimum {min}",
                    fields.vbios_version
                )));
            }
        }

        // 12. Policy validity window: not_before <= now <= expiry (inclusive).
        //     A broken (pre-epoch) clock surfaces as now == u64::MAX (see `now_unix`); fail
        //     closed unconditionally so even an `expiry == u64::MAX` ("never expires") policy
        //     is rejected when the clock is untrustworthy.
        if now == u64::MAX {
            return Err(TeeError::VerificationFailed(
                "system clock unavailable".into(),
            ));
        }
        if now < policy.not_before {
            return Err(TeeError::VerificationFailed("policy not yet valid".into()));
        }
        if now > policy.expiry {
            return Err(TeeError::VerificationFailed("policy expired".into()));
        }

        Ok(Claims {
            verified_at: now,
            gpu_report_hash,
            measurement_verified: true,
        })
    }
}

/// Is this the real thing: an `nvidia_payload` JSON object carrying an
/// `evidence_list`? A full parse, so mock bincode bytes (nonce first) can never
/// match by accident.
fn is_nvidia_payload(gpu_report: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(gpu_report)
        .map(|v| v.get("evidence_list").is_some())
        .unwrap_or(false)
}
