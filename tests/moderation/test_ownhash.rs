// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 3.2 — own-hash list (locally-confirmed material auto-blocks re-uploads). 🚨

use fabstir_llm_node::moderation::csam::ownhash::OwnHashList;

#[test]
fn add_then_contains() {
    let mut l = OwnHashList::new();
    let h = [0x11u8; 32];
    assert!(!l.contains(&h));
    l.add(h);
    assert!(l.contains(&h));
}

#[test]
fn reupload_of_confirmed_blocks() {
    // After a confirmed match is added, a bit-identical re-upload is recognised.
    let mut l = OwnHashList::new();
    let confirmed = [0x42u8; 32];
    l.add(confirmed);
    assert!(
        l.contains(&confirmed),
        "a re-upload of confirmed material must hit the own-hash list"
    );
}

#[test]
fn open_rejects_misaligned_plaintext() {
    // A validly-sealed blob whose plaintext is NOT a whole number of 32-byte hashes
    // must be rejected (fail-closed), never silently truncated.
    use fabstir_llm_node::moderation::csam::atrest;
    let key = b"own-hash-key";
    let blob = atrest::seal(key, &[0u8; 33]).expect("seal misaligned");
    assert!(
        OwnHashList::open(key, &blob).is_err(),
        "a misaligned own-hash blob must be rejected, not truncated"
    );
}

#[test]
fn persistence_roundtrip() {
    let key = b"own-hash-key";
    let mut l = OwnHashList::new();
    l.add([1u8; 32]);
    l.add([2u8; 32]);
    let sealed = l.seal(key).expect("seal");
    let restored = OwnHashList::open(key, &sealed).expect("open");
    assert!(restored.contains(&[1u8; 32]));
    assert!(restored.contains(&[2u8; 32]));
    assert_eq!(restored.len(), 2);
}
