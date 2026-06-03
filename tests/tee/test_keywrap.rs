// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 3.1 — ECIES key-wrap tests: wrap a DEK to the TEE's attestation key
//! `pk_att` so only the attested CVM (holder of the matching secret) can unwrap.

use fabstir_llm_node::tee::keywrap::{generate_ephemeral_keypair, unwrap_key, wrap_key};
use fabstir_llm_node::tee::types::TeeError;

/// A stand-in for the TEE's attestation keypair `pk_att` (secret, compressed pub).
fn recipient() -> (Vec<u8>, Vec<u8>) {
    generate_ephemeral_keypair()
}

#[test]
fn wrap_unwrap_roundtrip() {
    let (sec, pubk) = recipient();
    let dek = [0x42u8; 32];
    let w = wrap_key(&dek, &pubk).expect("wrap");
    assert_eq!(w.eph_pub.len(), 33, "ephemeral pub is compressed secp256k1");
    assert_eq!(w.nonce.len(), 24);
    let got = unwrap_key(&w, &sec).expect("unwrap");
    assert_eq!(
        got, dek,
        "unwrap∘wrap is the identity for the right recipient"
    );
}

#[test]
fn unwrap_with_wrong_recipient_secret_fails() {
    let (_sec, pubk) = recipient();
    let (other_sec, _other_pub) = recipient();
    let w = wrap_key(&[7u8; 32], &pubk).unwrap();
    let err = unwrap_key(&w, &other_sec).expect_err("wrong recipient must fail");
    assert!(matches!(err, TeeError::Crypto(_)), "got {err:?}");
}

#[test]
fn unwrap_tampered_ciphertext_fails() {
    let (sec, pubk) = recipient();
    let mut w = wrap_key(&[1u8; 32], &pubk).unwrap();
    w.ciphertext[0] ^= 0x01;
    let err = unwrap_key(&w, &sec).expect_err("tampered ciphertext must fail the AEAD");
    assert!(matches!(err, TeeError::Crypto(_)), "got {err:?}");
}

#[test]
fn unwrap_tampered_eph_pub_fails() {
    let (sec, pubk) = recipient();
    let mut w = wrap_key(&[2u8; 32], &pubk).unwrap();
    w.eph_pub[1] ^= 0x01; // changes both the ECDH input and the bound AAD
    let err = unwrap_key(&w, &sec).expect_err("tampered eph_pub must fail");
    assert!(matches!(err, TeeError::Crypto(_)), "got {err:?}");
}

#[test]
fn wrap_uses_fresh_ephemeral_and_nonce_each_call() {
    let (_sec, pubk) = recipient();
    let dek = [9u8; 32];
    let a = wrap_key(&dek, &pubk).unwrap();
    let b = wrap_key(&dek, &pubk).unwrap();
    assert_ne!(
        a.eph_pub, b.eph_pub,
        "fresh ephemeral keypair per wrap (forward secrecy)"
    );
    assert_ne!(a.nonce, b.nonce, "fresh nonce per wrap");
    assert_ne!(
        a.ciphertext, b.ciphertext,
        "same DEK wraps to different ciphertext"
    );
}

#[test]
fn wrap_key_is_domain_separated_from_session_init() {
    // The wrap key uses HKDF info=b"key-wrap-v1" over SHA256(ECDH.x); session-init
    // (`derive_shared_key`) uses empty info over the raw ECDH secret. For identical
    // ECDH inputs the keys differ, so a key-wrap blob must NOT decrypt under the
    // session-init key — this proves the mandated domain separation.
    use fabstir_llm_node::crypto::{decrypt_with_aead, derive_shared_key};
    let (sec, pubk) = recipient();
    let w = wrap_key(&[5u8; 32], &pubk).unwrap();
    let session_key = derive_shared_key(&w.eph_pub, &sec).expect("session key");
    let r = decrypt_with_aead(&w.ciphertext, &w.nonce, &w.eph_pub, &session_key);
    assert!(
        r.is_err(),
        "domain separation: the session-init key must not unwrap a key-wrap blob"
    );
}
