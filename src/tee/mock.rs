// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Mock attestation backend (Phases 1–4, tests/dev).
//!
//! [`MockAttestationProvider`] produces synthetic but **layout-correct**
//! [`Evidence`]: it bincode-encodes its [`GpuReportFields`] (carrying the
//! challenge nonce, as a real GPU attestation report does) into
//! `Evidence::gpu_report` and sets the CPU-quote `report_data` to
//! `sha256(pk_att) ‖ nonce` ([`crate::tee::types::report_data`]), so
//! `DefaultVerifier` exercises the real identity + shared-nonce checks. The real
//! `DstackAttestationProvider` replaces this in Phase 5 behind the
//! [`AttestationProvider`] trait.

use crate::tee::key_broker::KeyBrokerClient;
use crate::tee::keywrap::wrap_key;
use crate::tee::provider::AttestationProvider;
use crate::tee::types::{
    now_unix, report_data, CcMode, Evidence, GpuReportFields, Policy, TeeError, TeeResult,
    WrappedKey,
};
use crate::tee::verifier::{AttestationVerifier, DefaultVerifier};
use async_trait::async_trait;
use rand::{rngs::OsRng, RngCore};
use std::collections::HashMap;
use std::sync::Mutex;

/// Configurable mock attestation provider.
pub struct MockAttestationProvider {
    report: GpuReportFields,
    measurement: [u8; 48],
}

impl MockAttestationProvider {
    /// New provider reporting `hwmodel`, an image `measurement` (the mock's
    /// stand-in for MRTD), and GPU `cc_mode`; everything else at the
    /// production-good values (secure boot on, debug off, TD debug off, TCB
    /// `UpToDate`, driver `580.95.05`, VBIOS `96.00.9f.00.01`). Adjust with the
    /// builder setters.
    pub fn new(hwmodel: impl Into<String>, measurement: [u8; 48], cc_mode: CcMode) -> Self {
        Self {
            report: GpuReportFields {
                nonce: [0u8; 32], // replaced per call by the challenge nonce
                hwmodel: hwmodel.into(),
                cc_mode,
                secure_boot: true,
                debug_disabled: true,
                driver_version: "580.95.05".into(),
                vbios_version: "96.00.9f.00.01".into(),
                td_debug_off: true,
                tcb_status: "UpToDate".into(),
            },
            measurement,
        }
    }

    /// Override the reported TCB status (e.g. `"OutOfDate"`).
    pub fn with_tcb_status(mut self, status: impl Into<String>) -> Self {
        self.report.tcb_status = status.into();
        self
    }

    /// Override the TD DEBUG attribute (`false` = debug TD).
    pub fn with_td_debug_off(mut self, off: bool) -> Self {
        self.report.td_debug_off = off;
        self
    }

    pub fn with_secure_boot(mut self, on: bool) -> Self {
        self.report.secure_boot = on;
        self
    }

    pub fn with_debug_disabled(mut self, disabled: bool) -> Self {
        self.report.debug_disabled = disabled;
        self
    }

    pub fn with_driver_version(mut self, v: impl Into<String>) -> Self {
        self.report.driver_version = v.into();
        self
    }

    pub fn with_vbios_version(mut self, v: impl Into<String>) -> Self {
        self.report.vbios_version = v.into();
        self
    }
}

#[async_trait]
impl AttestationProvider for MockAttestationProvider {
    async fn gather_evidence(&self, nonce: [u8; 32], pk_att: &[u8]) -> TeeResult<Evidence> {
        // The GPU half is collected under the challenge nonce, exactly as the real
        // collector passes `nonce_hex` to nvtrust; a real report carries it signed.
        let report = GpuReportFields {
            nonce,
            ..self.report.clone()
        };
        let gpu_report = bincode::serialize(&report)
            .map_err(|e| TeeError::Crypto(format!("mock gpu_report serialize: {e}")))?;
        // The CPU half: report_data = sha256(pk_att) ‖ nonce, as the mock "quote".
        let cpu_quote = report_data(pk_att, &nonce).to_vec();
        Ok(Evidence {
            gpu_report,
            cpu_quote,
            // The mock has no dstack behind it: an empty event log and VM
            // config, spelled as valid JSON so a verifier that parses them
            // sees "nothing recorded" rather than a parse error.
            event_log: b"[]".to_vec(),
            vm_config: b"{}".to_vec(),
            image_measurement: self.measurement,
            pk_att: pk_att.to_vec(),
            nonce,
        })
    }
}

/// State of one KBS-issued challenge nonce.
struct NonceRecord {
    issued_at: u64,
    consumed: bool,
    /// Who the nonce was minted for (gate A-8): redeemable only by this model
    /// and this key.
    model_id: [u8; 32],
    pk_att: Vec<u8>,
}

/// Mock attestation-gated Key Broker Service (Phases 1–4, tests/dev).
///
/// Holds `model_id → (dek, policy)`, mints **one-time-use** freshness nonces
/// (v1 DECISION: Option A — `challenge` issues, `request_key` requires the nonce to
/// be issued, unexpired, and unconsumed, then burns it), verifies submitted
/// evidence with [`DefaultVerifier`], and on success wraps the DEK to the attested
/// `ev.pk_att`. The verifier's identity check ties `pk_att` to the signed
/// `report_data` and the shared-nonce check ties both quotes to the issued
/// nonce, so the DEK is released only to the key the attestation committed to. The
/// real (Phase 5) KBS replaces this behind [`KeyBrokerClient`].
pub struct MockKeyBroker {
    entries: HashMap<[u8; 32], ([u8; 32], Policy)>,
    nonces: Mutex<HashMap<[u8; 32], NonceRecord>>,
    ttl_seconds: u32,
}

impl MockKeyBroker {
    /// New broker serving `entries` (`model_id → (dek, policy)`), default 300 s TTL.
    pub fn new(entries: HashMap<[u8; 32], ([u8; 32], Policy)>) -> Self {
        Self {
            entries,
            nonces: Mutex::new(HashMap::new()),
            ttl_seconds: 300,
        }
    }

    /// Override the challenge-nonce TTL (seconds).
    pub fn with_ttl(mut self, ttl_seconds: u32) -> Self {
        self.ttl_seconds = ttl_seconds;
        self
    }
}

#[async_trait]
impl KeyBrokerClient for MockKeyBroker {
    async fn challenge(&self, model_id: [u8; 32], pk_att: &[u8]) -> TeeResult<[u8; 32]> {
        // Phase 5 (gate A-8): the nonce is bound to (model_id, pk_att) at mint time.
        // `request_key` refuses it for any other model or any other key, so a nonce
        // is a capability for exactly one release attempt by exactly one requester.
        if pk_att.len() != 33 {
            return Err(TeeError::Crypto(format!(
                "challenge: pk_att must be a 33-byte compressed key, got {}",
                pk_att.len()
            )));
        }
        let mut nonce = [0u8; 32];
        OsRng.fill_bytes(&mut nonce);
        self.nonces.lock().expect("kbs nonces poisoned").insert(
            nonce,
            NonceRecord {
                issued_at: now_unix(),
                consumed: false,
                model_id,
                pk_att: pk_att.to_vec(),
            },
        );
        Ok(nonce)
    }

    async fn request_key(&self, model_id: [u8; 32], ev: &Evidence) -> TeeResult<WrappedKey> {
        let (dek, policy) = self
            .entries
            .get(&model_id)
            .ok_or(TeeError::NoProviderBound(model_id))?;
        // Nonce lifecycle — Option A (one-time-use): must be issued, unexpired, and
        // unconsumed, AND minted for this model and this pk_att. Burned up-front so any
        // attempt (even a failing verify below) consumes it — no nonce can be retried.
        {
            let mut nonces = self.nonces.lock().expect("kbs nonces poisoned");
            let rec = nonces
                .get_mut(&ev.nonce)
                .ok_or(TeeError::FreshnessFailure)?;
            if rec.consumed || now_unix() > rec.issued_at.saturating_add(self.ttl_seconds as u64) {
                return Err(TeeError::FreshnessFailure);
            }
            rec.consumed = true;
            if rec.model_id != model_id || rec.pk_att != ev.pk_att {
                // Minted for someone else: burned (above) and refused.
                return Err(TeeError::FreshnessFailure);
            }
        }
        // Verify against the model's policy. The identity check
        // (`report_data[0..32] == sha256(ev.pk_att)`) and the shared-nonce check
        // (`report_data[32..64] == nonce == GPU evidence nonce`) tie `ev.pk_att` to
        // this KBS-issued nonce, so wrapping to `ev.pk_att` releases the DEK only
        // to the key the attestation committed to.
        DefaultVerifier.verify(ev, policy, ev.nonce)?;
        wrap_key(dek, &ev.pk_att)
    }

    fn challenge_nonce_ttl_seconds(&self) -> u32 {
        self.ttl_seconds
    }
}
