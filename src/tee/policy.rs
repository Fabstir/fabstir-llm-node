// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 4.1 — off-chain signed model policy (decision D3).
//!
//! A model provider publishes its DEK-release [`Policy`] off-chain, signed with
//! its Ethereum wallet key (EIP-191 `personal_sign`, so standard wallets like
//! MetaMask can sign). The node recovers the signer and requires it to equal the
//! model's on-chain provider (the contract lookup is the caller's job — Phase 4.3;
//! [`SignedModelPolicy::verify_signer`] takes the resolved provider address).
//!
//! **Two distinct hashes (not an error):** `policy_hash` = SHA-256 of the canonical
//! policy bytes (bound into the container AAD for integrity); the *signature* is
//! over `keccak256("\x19Ethereum Signed Message:\n"+len+canonical)` (EIP-191), the
//! Ethereum-wallet standard that [`recover_client_address`] expects.
//!
//! **Canonical bytes (locked):** `serde_json::to_value` → [`sort_json_keys`]
//! (alphabetical object keys) → `to_string` → bytes, so provider and node derive
//! byte-identical input.

use crate::checkpoint::delta::sort_json_keys;
use crate::crypto::recover_client_address;
use crate::tee::types::{now_unix, sha256_32, Policy, TeeError, TeeResult};
use serde::{Deserialize, Serialize};
use tiny_keccak::{Hasher, Keccak};

/// A provider-signed model policy + a pointer to the encrypted container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedModelPolicy {
    /// The DEK-release policy this signature authenticates.
    pub policy: Policy,
    /// S5 path / CID of the encrypted-model container.
    pub encrypted_ref: String,
    /// Claimed signer address (0x hex); authoritative check is [`Self::verify_signer`].
    pub signer: String,
    /// 65-byte recoverable ECDSA signature over the EIP-191 digest of the canonical policy.
    pub signature: Vec<u8>,
}

/// Canonical, byte-stable serialization of a [`Policy`]: JSON with alphabetically
/// sorted object keys. Provider and node MUST produce identical bytes here.
pub fn canonical_policy_bytes(policy: &Policy) -> TeeResult<Vec<u8>> {
    let value = serde_json::to_value(policy)
        .map_err(|e| TeeError::Crypto(format!("policy to_value: {e}")))?;
    let sorted = sort_json_keys(&value);
    let s = serde_json::to_string(&sorted)
        .map_err(|e| TeeError::Crypto(format!("policy to_string: {e}")))?;
    Ok(s.into_bytes())
}

/// EIP-191 personal-sign digest of the canonical policy:
/// `keccak256("\x19Ethereum Signed Message:\n" + len(canonical) + canonical)`,
/// the 32-byte value [`recover_client_address`] recovers against.
pub fn policy_signature_digest(canonical: &[u8]) -> [u8; 32] {
    let prefix = format!("\x19Ethereum Signed Message:\n{}", canonical.len());
    let mut k = Keccak::v256();
    k.update(prefix.as_bytes());
    k.update(canonical);
    let mut out = [0u8; 32];
    k.finalize(&mut out);
    out
}

/// Synchronously enforce the policy validity window (`not_before ≤ now ≤ expiry`,
/// inclusive). Fail-closed: expired / not-yet-valid / broken-clock all return `Err`
/// so the model load is refused with no plaintext written. Revocation is signaled
/// by a policy with `expiry ≤ now` (e.g. `expiry = 0`). Phase 4.1a.3.
pub fn check_policy_validity(policy: &Policy) -> TeeResult<()> {
    let now = now_unix();
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
    Ok(())
}

impl SignedModelPolicy {
    /// SHA-256 of the canonical policy bytes — the value bound into the container AAD.
    pub fn policy_hash(&self) -> TeeResult<[u8; 32]> {
        Ok(sha256_32(&canonical_policy_bytes(&self.policy)?))
    }

    /// Recover the 0x-address that signed this policy (fail-closed on a bad signature).
    pub fn recover_signer(&self) -> TeeResult<String> {
        let canonical = canonical_policy_bytes(&self.policy)?;
        let digest = policy_signature_digest(&canonical);
        recover_client_address(&self.signature, &digest)
            .map_err(|e| TeeError::VerificationFailed(format!("policy signature recovery: {e}")))
    }

    /// Verify the policy was signed by `expected_provider` (the model's on-chain
    /// provider, resolved by the caller). Fail-closed `VerificationFailed` otherwise.
    pub fn verify_signer(&self, expected_provider: &str) -> TeeResult<()> {
        let recovered = self.recover_signer()?;
        if recovered.to_lowercase() != expected_provider.to_lowercase() {
            return Err(TeeError::VerificationFailed(format!(
                "policy signer {recovered} != expected provider {expected_provider}"
            )));
        }
        Ok(())
    }
}
