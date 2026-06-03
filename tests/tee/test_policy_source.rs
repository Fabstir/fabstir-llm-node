// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 4.1a — fetch + fully validate a model's signed policy: fetch → signer ==
//! expected provider → validity window. All fail-closed.

use async_trait::async_trait;
use fabstir_llm_node::crypto::recover_client_address;
use fabstir_llm_node::tee::policy::{
    canonical_policy_bytes, policy_signature_digest, SignedModelPolicy,
};
use fabstir_llm_node::tee::policy_source::{
    fetch_validated_policy, PolicySource, ProviderRegistry,
};
use fabstir_llm_node::tee::types::{Policy, TeeError, TeeResult};
use k256::ecdsa::{signature::hazmat::PrehashSigner, RecoveryId, Signature, SigningKey};
use std::collections::HashMap;

fn a_policy(model_id: [u8; 32], version: u32, not_before: u64, expiry: u64) -> Policy {
    Policy {
        policy_version: version,
        allowed_skus: vec!["H100".to_string()],
        expected_measurement: [0x11u8; 48],
        require_cc_on: true,
        require_production_tcb: true,
        max_tcb_age_days: 30,
        not_before,
        expiry,
        model_id,
    }
}

/// Sign a policy as the provider; return the blob + the provider's recovered address.
fn sign(policy: &Policy, sk: &SigningKey) -> (SignedModelPolicy, String) {
    let canonical = canonical_policy_bytes(policy).unwrap();
    let digest = policy_signature_digest(&canonical);
    let (sig, recid): (Signature, RecoveryId) = sk.sign_prehash_recoverable(&digest).unwrap();
    let mut sig65 = vec![0u8; 65];
    sig65[..64].copy_from_slice(&sig.to_bytes());
    sig65[64] = recid.to_byte() + 27;
    let provider = recover_client_address(&sig65, &digest).unwrap();
    (
        SignedModelPolicy {
            policy: policy.clone(),
            encrypted_ref: "s5://blob".to_string(),
            signer: provider.clone(),
            signature: sig65,
        },
        provider,
    )
}

/// Mock policy source: serves stored policies; an unknown model = fetch failure.
struct MockSource {
    policies: HashMap<[u8; 32], SignedModelPolicy>,
}
#[async_trait]
impl PolicySource for MockSource {
    async fn fetch_policy(&self, model_id: [u8; 32]) -> TeeResult<SignedModelPolicy> {
        self.policies.get(&model_id).cloned().ok_or_else(|| {
            TeeError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "policy fetch failed",
            ))
        })
    }
}

fn setup(policy: &Policy, sk: &SigningKey) -> (MockSource, ProviderRegistry, String) {
    let (signed, provider) = sign(policy, sk);
    let source = MockSource {
        policies: HashMap::from([(policy.model_id, signed)]),
    };
    let providers = ProviderRegistry::new().with_provider(policy.model_id, provider.clone());
    (source, providers, provider)
}

#[tokio::test]
async fn fetch_validated_ok() {
    let sk = SigningKey::random(&mut rand::rngs::OsRng);
    let p = a_policy([1u8; 32], 1, 0, u64::MAX - 1);
    let (src, providers, _) = setup(&p, &sk);
    let got = fetch_validated_policy(&src, &providers, p.model_id)
        .await
        .expect("a correctly-signed, in-window policy from the bound provider validates");
    assert_eq!(got.policy.model_id, p.model_id);
}

#[tokio::test]
async fn rejects_signer_mismatch() {
    let sk = SigningKey::random(&mut rand::rngs::OsRng);
    let p = a_policy([1u8; 32], 1, 0, u64::MAX - 1);
    let (src, _providers, _) = setup(&p, &sk);
    // Bind the model to a DIFFERENT provider than the one who signed.
    let providers = ProviderRegistry::new()
        .with_provider(p.model_id, "0x000000000000000000000000000000000000dEaD");
    let err = fetch_validated_policy(&src, &providers, p.model_id)
        .await
        .expect_err("a policy signed by a non-provider must fail closed");
    assert!(
        matches!(err, TeeError::VerificationFailed(_)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn rejects_expired_policy() {
    let sk = SigningKey::random(&mut rand::rngs::OsRng);
    let p = a_policy([1u8; 32], 1, 0, 0); // expiry=0 ⇒ revoked/expired
    let (src, providers, _) = setup(&p, &sk);
    let err = fetch_validated_policy(&src, &providers, p.model_id)
        .await
        .expect_err("expired policy must fail closed");
    assert!(
        matches!(err, TeeError::VerificationFailed(_)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn rejects_not_yet_valid() {
    let sk = SigningKey::random(&mut rand::rngs::OsRng);
    let p = a_policy([1u8; 32], 1, u64::MAX - 1, u64::MAX); // not_before in the far future
    let (src, providers, _) = setup(&p, &sk);
    let err = fetch_validated_policy(&src, &providers, p.model_id)
        .await
        .expect_err("not-yet-valid policy must fail closed");
    assert!(
        matches!(err, TeeError::VerificationFailed(_)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn rejects_fetch_failure() {
    let sk = SigningKey::random(&mut rand::rngs::OsRng);
    let p = a_policy([1u8; 32], 1, 0, u64::MAX - 1);
    let (src, providers, _) = setup(&p, &sk);
    // Ask for a model the source doesn't have → fetch fails.
    let err = fetch_validated_policy(&src, &providers, [2u8; 32])
        .await
        .expect_err("a failed fetch must return Err");
    assert!(
        matches!(err, TeeError::Io(_) | TeeError::NoProviderBound(_)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn rejects_unknown_provider() {
    let sk = SigningKey::random(&mut rand::rngs::OsRng);
    let p = a_policy([1u8; 32], 1, 0, u64::MAX - 1);
    let (signed, _provider) = sign(&p, &sk);
    let src = MockSource {
        policies: HashMap::from([(p.model_id, signed)]),
    };
    let providers = ProviderRegistry::new(); // no binding for this model
    let err = fetch_validated_policy(&src, &providers, p.model_id)
        .await
        .expect_err("no provider binding must fail closed");
    assert!(matches!(err, TeeError::NoProviderBound(_)), "got {err:?}");
}
