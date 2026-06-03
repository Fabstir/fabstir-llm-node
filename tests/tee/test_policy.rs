// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 4.1 — off-chain signed model policy: provider signs (EIP-191 over the
//! canonical policy bytes); the node recovers + verifies the signer == provider.

use fabstir_llm_node::crypto::recover_client_address;
use fabstir_llm_node::tee::policy::{
    canonical_policy_bytes, check_policy_validity, policy_signature_digest, SignedModelPolicy,
};
use fabstir_llm_node::tee::types::{Policy, TeeError};
use k256::ecdsa::{signature::hazmat::PrehashSigner, RecoveryId, Signature, SigningKey};

fn a_policy() -> Policy {
    Policy {
        policy_version: 1,
        allowed_skus: vec!["H100".to_string(), "H200".to_string()],
        expected_measurement: [0x11u8; 48],
        require_cc_on: true,
        require_production_tcb: true,
        max_tcb_age_days: 30,
        not_before: 0,
        expiry: u64::MAX - 1,
        model_id: [9u8; 32],
    }
}

/// Sign `policy` as the provider would, returning the signed blob + the provider's
/// recovered 0x address (derived from the signature, so no address-derivation here).
fn sign_policy(
    policy: &Policy,
    encrypted_ref: &str,
    sk: &SigningKey,
) -> (SignedModelPolicy, String) {
    let canonical = canonical_policy_bytes(policy).expect("canonical");
    let digest = policy_signature_digest(&canonical);
    let (sig, recid): (Signature, RecoveryId) = sk.sign_prehash_recoverable(&digest).expect("sign");
    let mut sig65 = vec![0u8; 65];
    sig65[..64].copy_from_slice(&sig.to_bytes());
    sig65[64] = recid.to_byte() + 27; // Ethereum v
    let provider = recover_client_address(&sig65, &digest).expect("recover provider address");
    let signed = SignedModelPolicy {
        policy: policy.clone(),
        encrypted_ref: encrypted_ref.to_string(),
        signer: provider.clone(),
        signature: sig65,
    };
    (signed, provider)
}

#[test]
fn sign_and_verify_ok() {
    let sk = SigningKey::random(&mut rand::rngs::OsRng);
    let (signed, provider) = sign_policy(&a_policy(), "s5://blob", &sk);
    signed
        .verify_signer(&provider)
        .expect("a valid provider signature must verify");
    assert_eq!(
        signed.recover_signer().unwrap().to_lowercase(),
        provider.to_lowercase(),
        "recover_signer returns the provider address"
    );
}

#[test]
fn verify_rejects_wrong_provider() {
    let sk = SigningKey::random(&mut rand::rngs::OsRng);
    let (signed, _provider) = sign_policy(&a_policy(), "s5://blob", &sk);
    let err = signed
        .verify_signer("0x000000000000000000000000000000000000dEaD")
        .expect_err("a signer that isn't the on-chain provider must fail closed");
    assert!(
        matches!(err, TeeError::VerificationFailed(_)),
        "got {err:?}"
    );
}

#[test]
fn verify_rejects_tampered_policy() {
    let sk = SigningKey::random(&mut rand::rngs::OsRng);
    let (mut signed, provider) = sign_policy(&a_policy(), "s5://blob", &sk);
    signed.policy.max_tcb_age_days = 999; // tamper after signing
    let err = signed
        .verify_signer(&provider)
        .expect_err("a tampered policy must fail signature verification");
    assert!(
        matches!(err, TeeError::VerificationFailed(_)),
        "got {err:?}"
    );
}

#[test]
fn check_policy_validity_accepts_current_and_rejects_window_violations() {
    // not_before=0, expiry=u64::MAX-1 ⇒ valid now.
    check_policy_validity(&a_policy()).expect("a current policy must be valid");

    // Revocation via expiry=0 ⇒ expired.
    let mut expired = a_policy();
    expired.expiry = 0;
    let err = check_policy_validity(&expired).expect_err("expired policy must be refused");
    assert!(
        matches!(err, TeeError::VerificationFailed(_)),
        "got {err:?}"
    );

    // not_before far in the future ⇒ not yet valid.
    let mut future = a_policy();
    future.not_before = u64::MAX - 1;
    let err = check_policy_validity(&future).expect_err("not-yet-valid policy must be refused");
    assert!(
        matches!(err, TeeError::VerificationFailed(_)),
        "got {err:?}"
    );
}

#[test]
fn policy_hash_is_deterministic_and_content_addressed() {
    let sk = SigningKey::random(&mut rand::rngs::OsRng);
    let p = a_policy();
    // policy_hash depends ONLY on the Policy (not encrypted_ref/signer/signature).
    let (s1, _) = sign_policy(&p, "ref-one", &sk);
    let (s2, _) = sign_policy(&p, "ref-two", &sk);
    assert_eq!(
        s1.policy_hash().unwrap(),
        s2.policy_hash().unwrap(),
        "policy_hash is deterministic over the Policy alone"
    );
    // A different Policy hashes differently.
    let mut p2 = p.clone();
    p2.policy_version = 2;
    let (s3, _) = sign_policy(&p2, "ref-one", &sk);
    assert_ne!(
        s1.policy_hash().unwrap(),
        s3.policy_hash().unwrap(),
        "a changed Policy must change policy_hash"
    );
}
