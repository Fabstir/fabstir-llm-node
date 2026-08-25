// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Mock attestation backend (Phases 1–4, tests/dev).
//!
//! [`MockAttestationProvider`] produces synthetic but **cross-binding-correct**
//! [`Evidence`]: it bincode-encodes its [`GpuReportFields`] into
//! `Evidence::gpu_report` and sets the CPU-quote `report_data` to the canonical
//! `sha256(pk_att ‖ gpu_report_hash ‖ nonce)`, so `DefaultVerifier` exercises the
//! real cross-binding path. The real `NvidiaCcProvider` replaces this in Phase 5
//! behind the [`AttestationProvider`] trait.

use crate::tee::key_broker::KeyBrokerClient;
use crate::tee::keywrap::wrap_key;
use crate::tee::provider::AttestationProvider;
use crate::tee::types::{
    cross_bind_report_data, now_unix, sha256_32, CcMode, Evidence, GpuReportFields, Policy,
    TeeError, TeeResult, WrappedKey,
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
        let gpu_report = bincode::serialize(&self.report)
            .map_err(|e| TeeError::Crypto(format!("mock gpu_report serialize: {e}")))?;
        let gpu_report_hash = sha256_32(&gpu_report);
        // report_data[0..32] = cross-binding commitment; [32..64] = zero padding.
        let report_data = cross_bind_report_data(pk_att, &gpu_report_hash, &nonce);
        let mut cpu_quote = vec![0u8; 64];
        cpu_quote[..32].copy_from_slice(&report_data);
        Ok(Evidence {
            gpu_report,
            cpu_quote,
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
/// `ev.pk_att`. The verifier's cross-binding check ties `pk_att` to the issued
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
        // and the cross-binding pins release to the attested `pk_att`; nonces stay
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
        // Verify against the model's policy. The cross-binding check
        // (`report_data == hash(pk_att ‖ gpu_report_hash ‖ nonce)`) ties `ev.pk_att`
        // to this KBS-issued nonce, so wrapping to `ev.pk_att` releases the DEK only
        // to the key the attestation committed to.
        DefaultVerifier.verify(ev, policy, ev.nonce)?;
        wrap_key(dek, &ev.pk_att)
    }

    fn challenge_nonce_ttl_seconds(&self) -> u32 {
        self.ttl_seconds
    }
}
