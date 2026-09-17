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
    /// New provider reporting `sku`, image `measurement`, and GPU `cc_mode`
    /// (production TCB, age 0 by default — adjust with the builder setters).
    pub fn new(sku: impl Into<String>, measurement: [u8; 48], cc_mode: CcMode) -> Self {
        Self {
            report: GpuReportFields {
                nonce: [0u8; 32], // replaced per call by the challenge nonce
                sku: sku.into(),
                cc_mode,
                production_tcb: true,
                tcb_age_days: 0,
            },
            measurement,
        }
    }

    /// Override the reported CPU TCB age (days) — for stale-TCB tests.
    pub fn with_tcb_age_days(mut self, days: u32) -> Self {
        self.report.tcb_age_days = days;
        self
    }

    /// Override whether the CPU TCB is production — for non-production tests.
    pub fn with_production_tcb(mut self, production: bool) -> Self {
        self.report.production_tcb = production;
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
    async fn challenge(&self, _model_id: [u8; 32]) -> TeeResult<[u8; 32]> {
        // model_id is intentionally not bound into the nonce: a nonce minted for one
        // model and replayed against another grants no capability — `request_key` selects
        // (dek, policy) by the request's `model_id`, that per-model policy must still pass,
        // and the identity check pins release to the attested `pk_att`; nonces stay
        // single-use + TTL-bounded regardless. Phase 5 SHOULD bind nonce→model_id for
        // explicit domain separation.
        let mut nonce = [0u8; 32];
        OsRng.fill_bytes(&mut nonce);
        self.nonces.lock().expect("kbs nonces poisoned").insert(
            nonce,
            NonceRecord {
                issued_at: now_unix(),
                consumed: false,
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
        // unconsumed. Burned up-front so any attempt (even a failing verify below)
        // consumes it — no nonce can be retried.
        {
            let mut nonces = self.nonces.lock().expect("kbs nonces poisoned");
            let rec = nonces
                .get_mut(&ev.nonce)
                .ok_or(TeeError::FreshnessFailure)?;
            if rec.consumed || now_unix() > rec.issued_at.saturating_add(self.ttl_seconds as u64) {
                return Err(TeeError::FreshnessFailure);
            }
            rec.consumed = true;
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
