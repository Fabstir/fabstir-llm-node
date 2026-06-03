// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 4.1a — policy fetch + provider resolution + full validation.
//!
//! [`fetch_validated_policy`] is the single fail-closed entry the model-load path
//! uses to obtain a model's [`SignedModelPolicy`]: fetch it from a [`PolicySource`],
//! confirm it was signed by the model's **expected provider**, and confirm its
//! validity window. Any failure → `Err`, no policy released.
//!
//! **Provider resolution (Q2, Phase-4 interim).** The expected provider comes from
//! a [`ProviderRegistry`]:
//!   - **on-chain** (production): the model's `proposals(modelId).proposer`
//!     (wired in Phase 4.3.1 via `ModelRegistryClient::get_model_provider`); but the
//!     current deployment's approved models are direct/legacy approvals with an empty
//!     `proposer`, so —
//!   - **config fallback** (this module): a node-config `model_id → provider` map for
//!     testing / provider-operated nodes. **THREAT MODEL:** a config override is only
//!     sound if the config is *measured into the attestation* (else a host authorizes
//!     its own policy — §1). Phase 5 replaces the fallback with the on-chain binding.
//!
//! The real HTTP `PolicySource` (provider endpoint + retry/timeout + TTL cache,
//! Phase 4.1a.2) is a thin adapter over this trait, added at node-wiring time.

use crate::tee::policy::{check_policy_validity, SignedModelPolicy};
use crate::tee::types::{TeeError, TeeResult};
use async_trait::async_trait;
use std::collections::HashMap;

/// Source of provider-signed model policies (HTTP/S5 in production; mock in tests).
#[async_trait]
pub trait PolicySource: Send + Sync {
    /// Fetch the current signed policy for `model_id` (fail-closed on any error).
    async fn fetch_policy(&self, model_id: [u8; 32]) -> TeeResult<SignedModelPolicy>;

    /// Latest policy version for `model_id` (for version-based revocation, Phase 4.3.1a).
    /// Defaults to the fetched policy's own version.
    async fn latest_policy_version(&self, model_id: [u8; 32]) -> TeeResult<u32> {
        Ok(self.fetch_policy(model_id).await?.policy.policy_version)
    }
}

/// Resolves a model's expected provider address (the authority allowed to sign its policy).
///
/// Phase-4 config fallback (see module docs); Phase 4.3.1 layers the on-chain
/// `proposals().proposer` lookup in front of this.
#[derive(Default, Clone)]
pub struct ProviderRegistry {
    config: HashMap<[u8; 32], String>,
}

impl ProviderRegistry {
    /// Empty registry (every model resolves to `NoProviderBound` until bound).
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind `model_id` to a provider `0x` address (config fallback).
    pub fn with_provider(mut self, model_id: [u8; 32], provider: impl Into<String>) -> Self {
        self.config.insert(model_id, provider.into());
        self
    }

    /// The provider authorized to sign `model_id`'s policy, or `NoProviderBound`.
    pub fn expected_provider(&self, model_id: &[u8; 32]) -> TeeResult<String> {
        self.config
            .get(model_id)
            .cloned()
            .ok_or(TeeError::NoProviderBound(*model_id))
    }
}

/// Fetch + fully validate `model_id`'s policy: fetch → signer == expected provider
/// → validity window. Fail-closed: any failure returns `Err` and releases nothing.
pub async fn fetch_validated_policy(
    source: &dyn PolicySource,
    providers: &ProviderRegistry,
    model_id: [u8; 32],
) -> TeeResult<SignedModelPolicy> {
    let signed = source.fetch_policy(model_id).await?;
    let provider = providers.expected_provider(&model_id)?;
    signed.verify_signer(&provider)?;
    check_policy_validity(&signed.policy)?;
    Ok(signed)
}
