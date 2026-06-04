// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 3.2 — attestation-gated key broker (KBS) client + node-side flow.
//!
//! The KBS releases a model's DEK only after a passing, *fresh* attestation, and
//! returns it ECIES-wrapped ([`crate::tee::keywrap`]) to the CVM's attestation key
//! `pk_att`. [`NodeAttestationClient::obtain_dek`] drives the node side:
//! challenge → generate `pk_att` → gather evidence → request the wrapped key →
//! unwrap. The mock KBS lives in [`crate::tee::mock`]; the real (Phase 5) KBS slots
//! in behind [`KeyBrokerClient`] with no call-site changes.

use crate::tee::keywrap::{generate_ephemeral_keypair, unwrap_key};
use crate::tee::provider::AttestationProvider;
use crate::tee::types::{Evidence, TeeResult, WrappedKey};
use async_trait::async_trait;

/// Client of the Key Broker Service.
#[async_trait]
pub trait KeyBrokerClient: Send + Sync {
    /// Request a fresh freshness nonce for `model_id` (the challenge step).
    async fn challenge(&self, model_id: [u8; 32]) -> TeeResult<[u8; 32]>;
    /// Submit `ev`; if it attests against the model's policy and the nonce is fresh,
    /// receive the model's DEK wrapped to `ev.pk_att`.
    async fn request_key(&self, model_id: [u8; 32], ev: &Evidence) -> TeeResult<WrappedKey>;
    /// TTL (seconds) of an issued challenge nonce. Non-async with a default body.
    fn challenge_nonce_ttl_seconds(&self) -> u32 {
        300
    }
}

/// Node-side attestation → key-release flow.
pub struct NodeAttestationClient;

impl NodeAttestationClient {
    /// Obtain the cleartext DEK for `model_id`: challenge → generate `pk_att` →
    /// gather evidence (binding `pk_att` + the issued nonce into the cross-binding)
    /// → request the wrapped key → unwrap with the matching `pk_att` secret.
    ///
    /// The `pk_att` secret never leaves this function; the DEK arrives wrapped and
    /// is unwrapped locally. Fail-closed: any step's error propagates unchanged.
    pub async fn obtain_dek(
        provider: &dyn AttestationProvider,
        kbs: &dyn KeyBrokerClient,
        model_id: [u8; 32],
    ) -> TeeResult<[u8; 32]> {
        let nonce = kbs.challenge(model_id).await?;
        let (pk_att_secret, pk_att_pub) = generate_ephemeral_keypair();
        let evidence = provider.gather_evidence(nonce, &pk_att_pub).await?;
        let wrapped = kbs.request_key(model_id, &evidence).await?;
        unwrap_key(&wrapped, &pk_att_secret)
    }
}
