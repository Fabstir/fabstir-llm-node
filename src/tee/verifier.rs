// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Attestation verifier: the [`AttestationVerifier`] trait and the
//! `DefaultVerifier` policy logic (Phase 1.3).
//!
//! The canonical cross-binding helpers ([`cross_bind_report_data`],
//! [`sha256_32`]) and the decoded [`GpuReportFields`] live in
//! [`crate::tee::types`] — the neutral shared home, so the provider and verifier
//! compute the security-critical commitment identically. The real RIM/cert-chain
//! verifier lands in Phase 5 behind this same trait, reusing the cross-binding
//! and policy-validity logic here.

use crate::tee::types::{
    cross_bind_report_data, now_unix, sha256_32, Claims, Evidence, GpuReportFields, Policy,
    TeeError, TeeResult,
};

/// Verifies attestation [`Evidence`] against a model-provider [`Policy`].
///
/// Synchronous and HW-agnostic at the trait level: implementations perform pure
/// checks (cross-binding, measurement, SKU, CC-on, TCB, policy validity) and, in
/// Phase 5, RIM and CPU/GPU certificate-chain verification.
pub trait AttestationVerifier: Send + Sync {
    /// Verify `ev` against `policy`, requiring the cross-binding to commit to
    /// `expected_nonce`. Returns [`Claims`] on success, else [`TeeError`].
    fn verify(&self, ev: &Evidence, policy: &Policy, expected_nonce: [u8; 32])
        -> TeeResult<Claims>;
}

/// Pure, hardware-agnostic verifier for the mock-backed pipeline (Phases 1–4).
///
/// Performs the cross-binding, measurement, SKU, CC-on, TCB, and policy-validity
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
        // 1. Freshness: evidence must be bound to the KBS-issued nonce. (Nonce issuance,
        //    single-use, and TTL are enforced by the KBS in Phase 3.2; here we only bind the
        //    evidence to the caller's expected nonce.)
        if ev.nonce != expected_nonce {
            return Err(TeeError::VerificationFailed("nonce mismatch".into()));
        }

        // 2. The mock CPU quote must carry a 64-byte report_data field. (Phase 5: a real
        //    TDX/SNP quote is longer; bytes beyond report_data are validated by the real
        //    quote parser, not here — this `[..64]` view is mock-only.)
        if ev.cpu_quote.len() < 64 {
            return Err(TeeError::VerificationFailed("cpu_quote too short".into()));
        }

        // 3. Cross-binding: report_data[0..32] == sha256(pk_att ‖ sha256(gpu_report) ‖ nonce)
        //    and report_data[32..64] == 0. Catches a genuine quote paired with a swapped report.
        // SECURITY (Phase 3/5): `ev.pk_att` is unauthenticated here — the mock merely echoes
        // it. Phase 3 (key-wrap) MUST wrap the DEK only to a `pk_att` the *hardware* quote
        // bound (Phase 5 derives/confirms it from the real report) and validate it is a
        // canonical 33-byte compressed secp256k1 point; otherwise a host could attest with a
        // genuine quote but a substituted key and decrypt the model outside the TEE.
        let gpu_report_hash = sha256_32(&ev.gpu_report);
        let expected = cross_bind_report_data(&ev.pk_att, &gpu_report_hash, &ev.nonce);
        if ev.cpu_quote[..32] != expected || ev.cpu_quote[32..64] != [0u8; 32] {
            return Err(TeeError::VerificationFailed(
                "cross-binding mismatch".into(),
            ));
        }

        // 4. Decode the GPU-report fields (Phase 5 parses a real DER report here instead).
        //    Trailing / non-canonical bytes are harmless: step 3 already hashed the *entire*
        //    gpu_report, so any extra bytes would have failed the cross-binding above.
        let fields: GpuReportFields = bincode::deserialize(&ev.gpu_report)
            .map_err(|e| TeeError::VerificationFailed(format!("gpu report decode: {e}")))?;

        // 5. Image measurement must match the provider's pinned value.
        if ev.image_measurement != policy.expected_measurement {
            return Err(TeeError::VerificationFailed("measurement mismatch".into()));
        }

        // 6. SKU must be allowed by the policy.
        if !policy.allowed_skus.iter().any(|s| s == &fields.sku) {
            return Err(TeeError::VerificationFailed(format!(
                "disallowed sku: {}",
                fields.sku
            )));
        }

        // 7. CC mode must match the policy EXACTLY when it requires one.
        //    Exact, not "at least on": `devtools` attests with the protections
        //    disabled, so anything looser than equality releases the key to an
        //    unprotected GPU. A policy that genuinely wants devtools has to name
        //    it.
        if let Some(required) = policy.require_cc_mode {
            if fields.cc_mode != required {
                return Err(TeeError::VerificationFailed(format!(
                    "cc mode {:?}, policy requires {:?}",
                    fields.cc_mode, required
                )));
            }
        }

        // 8. Production TCB when the policy requires it.
        if policy.require_production_tcb && !fields.production_tcb {
            return Err(TeeError::VerificationFailed("non-production tcb".into()));
        }

        // 9. TCB age within the policy bound.
        if fields.tcb_age_days > policy.max_tcb_age_days {
            return Err(TeeError::VerificationFailed(format!(
                "stale tcb: {} days",
                fields.tcb_age_days
            )));
        }

        // 10. Policy validity window: not_before <= now <= expiry (inclusive).
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
