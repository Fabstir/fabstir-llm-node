// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 5 (P2.5) — the real [`AttestationProvider`]: a dstack CVM quote plus
//! NVIDIA GPU evidence, both bound to one challenge.
//!
//! Drop-in behind the trait the mock filled in Phases 1–4; no call site
//! changes. For a challenge `nonce` and attestation key `pk_att`:
//!
//! 1. `report_data = sha256(pk_att) ‖ nonce` (`types::report_data`);
//! 2. `POST /GetQuote` on the dstack socket signs it into a TDX quote, returning
//!    the quote, the event log and the VM config ([`DstackClient`]);
//! 3. the GPU collector produces NVIDIA's `nvidia_payload` for the same `nonce`
//!    ([`GpuEvidenceCollector`]);
//! 4. the four are packaged as [`Evidence`], unverified, for the key broker.
//!
//! The two quotes are independent; neither needs the other first. The order
//! here (CPU quote, then GPU) only means a dead dstack socket fails before the
//! slower NVML round trip.
//!
//! **Consumer:** only the Phase-5 broker verifier (P4.2/P4.3: dcap-qvl + NRAS)
//! can judge this evidence. `DefaultVerifier` is mock-only and refuses it with a
//! message that says so; wiring this provider into `prepare_attested_model`
//! against `MockKeyBroker` therefore fails closed, by design, until P4 lands.

use crate::tee::dstack::DstackClient;
use crate::tee::gpu_evidence::GpuEvidenceCollector;
use crate::tee::provider::AttestationProvider;
use crate::tee::types::{report_data, Evidence, TeeError, TeeResult};
use async_trait::async_trait;

/// Real attestation on a Phala/dstack CVM with an NVIDIA CC GPU.
#[derive(Debug, Clone)]
pub struct DstackAttestationProvider {
    dstack: DstackClient,
    gpu: GpuEvidenceCollector,
}

impl DstackAttestationProvider {
    pub fn new(dstack: DstackClient, gpu: GpuEvidenceCollector) -> Self {
        Self { dstack, gpu }
    }

    /// Wire everything from the container environment (`DSTACK_SIMULATOR_ENDPOINT`,
    /// `TEE_GPU_EVIDENCE_SCRIPT`, `TEE_GPU_EVIDENCE`). Fails at construction if
    /// the collector script is missing or the mode is misspelt.
    pub fn from_env() -> TeeResult<Self> {
        Ok(Self::new(
            DstackClient::from_env()?,
            GpuEvidenceCollector::from_env()?,
        ))
    }

    pub fn dstack(&self) -> &DstackClient {
        &self.dstack
    }

    pub fn gpu(&self) -> &GpuEvidenceCollector {
        &self.gpu
    }
}

#[async_trait]
impl AttestationProvider for DstackAttestationProvider {
    async fn gather_evidence(&self, nonce: [u8; 32], pk_att: &[u8]) -> TeeResult<Evidence> {
        if pk_att.len() != 33 {
            // The identity half commits to the key the DEK is wrapped to; only the
            // canonical compressed form is ever asked for, so anything else is a
            // caller bug, refused before a quote is spent on it.
            return Err(TeeError::Crypto(format!(
                "pk_att must be a 33-byte compressed secp256k1 key, got {} bytes",
                pk_att.len()
            )));
        }
        let rd = report_data(pk_att, &nonce);
        let quote = self.dstack.get_quote(&rd).await?;
        let gpu_report = self.gpu.collect(&nonce).await?;
        tracing::info!(
            target: "tee",
            "evidence gathered: quote {} B, event_log {} B, vm_config {} B, gpu payload {} B, mode {:?}",
            quote.quote.len(),
            quote.event_log.len(),
            quote.vm_config.len(),
            gpu_report.len(),
            self.gpu.mode()
        );
        Ok(Evidence {
            gpu_report,
            cpu_quote: quote.quote,
            event_log: quote.event_log.into_bytes(),
            vm_config: quote.vm_config.into_bytes(),
            // Mock-era field; the real verifier takes MRTD from the verified quote
            // body and ignores this. Zero, never a value the node could choose.
            image_measurement: [0u8; 48],
            pk_att: pk_att.to_vec(),
            nonce,
        })
    }
}
