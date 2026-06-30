// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 3.1 — shared CSAM at-rest encryption (aes-gcm + hkdf, D5). 🚨
//!
//! Used by both the NCMEC hash store (3.1) and quarantine (6.1). Verifies a real
//! round-trip plus fail-closed tamper / wrong-key rejection.

use fabstir_llm_node::moderation::csam::atrest::{open, seal};

#[test]
fn seal_open_roundtrip() {
    let key = b"some-key-material-32-bytes-or-not";
    let plaintext = b"sensitive at-rest payload";
    let sealed = seal(key, plaintext).expect("seal");
    assert_ne!(
        &sealed[..],
        &plaintext[..],
        "ciphertext must differ from plaintext"
    );
    let opened = open(key, &sealed).expect("open");
    assert_eq!(opened, plaintext, "roundtrip must recover the plaintext");
}

#[test]
fn tampered_ciphertext_is_rejected() {
    let key = b"key-material";
    let sealed = seal(key, b"payload").expect("seal");
    let mut tampered = sealed.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xFF; // flip a tag/ciphertext byte
    assert!(
        open(key, &tampered).is_err(),
        "tampered at-rest data must fail-closed (GCM tag mismatch)"
    );
}

#[test]
fn wrong_key_is_rejected() {
    let sealed = seal(b"right-key", b"payload").expect("seal");
    assert!(
        open(b"wrong-key", &sealed).is_err(),
        "a wrong key must fail to decrypt"
    );
}

#[test]
fn too_short_blob_is_rejected() {
    assert!(
        open(b"k", &[0u8; 4]).is_err(),
        "a blob shorter than the nonce must error"
    );
}
