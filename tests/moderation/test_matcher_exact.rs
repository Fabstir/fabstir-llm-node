// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 3.2 — Track-1 exact-match engine (SHA-256 + own-hash). 🚨
//!
//! Exact-match is bit-identical only — it does NOT detect re-encoded CSAM (§3.5);
//! that is PDQ's job (Phase 4). Fail-closed: an unavailable NCMEC list ⇒ Err.

use fabstir_llm_node::moderation::csam::hashlist::{HashListSnapshot, HashListSource};
use fabstir_llm_node::moderation::csam::matcher::Matcher;
use fabstir_llm_node::moderation::csam::mock_source::MockHashListSource;
use fabstir_llm_node::moderation::csam::ownhash::OwnHashList;

#[test]
fn sha256_exact_hit_matches() {
    let snap = MockHashListSource::with_test_vectors().refresh().unwrap();
    let own = OwnHashList::new();
    let m = Matcher::new(&snap, &own);
    // [0x11; 32] is a known-bad vector in the mock list.
    assert!(m.match_sha256(&[0x11u8; 32]).unwrap().is_match);
}

#[test]
fn benign_control_no_match() {
    let snap = MockHashListSource::with_test_vectors().refresh().unwrap();
    let own = OwnHashList::new();
    let m = Matcher::new(&snap, &own);
    let r = m
        .match_sha256(&MockHashListSource::BENIGN_CONTROL_SHA256)
        .unwrap();
    assert!(!r.is_match, "a benign control must not match");
}

#[test]
fn own_hash_hit_matches() {
    let snap = MockHashListSource::with_test_vectors().refresh().unwrap();
    let mut own = OwnHashList::new();
    let confirmed = [0x99u8; 32]; // not in NCMEC list, but locally confirmed
    own.add(confirmed);
    let m = Matcher::new(&snap, &own);
    assert!(
        m.match_sha256(&confirmed).unwrap().is_match,
        "an own-hash hit must match"
    );
}

#[test]
fn own_hash_hits_even_when_list_unavailable_but_others_hold() {
    // Own-hash is a definitive local block regardless of NCMEC availability; any
    // other hash against an Unavailable list fails-closed (Err).
    let snap = HashListSnapshot::unavailable();
    let mut own = OwnHashList::new();
    let confirmed = [0x77u8; 32];
    own.add(confirmed);
    let m = Matcher::new(&snap, &own);
    assert!(
        m.match_sha256(&confirmed).unwrap().is_match,
        "own-hash hit must block even when the NCMEC list is down"
    );
    assert!(
        m.match_sha256(&[0x00u8; 32]).is_err(),
        "a non-own-hash against an Unavailable list must hold (Err)"
    );
}

#[test]
fn match_bytes_hashes_then_matches() {
    // match_bytes computes SHA-256 then matches; verify a byte string whose hash is
    // added to the own-hash list is recognised.
    let snap = MockHashListSource::with_test_vectors().refresh().unwrap();
    let bytes = b"known-bad-bytes";
    let h = Matcher::sha256(bytes);
    let mut own = OwnHashList::new();
    own.add(h);
    let m = Matcher::new(&snap, &own);
    assert!(m.match_bytes(bytes).unwrap().is_match);
}
