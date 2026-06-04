// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Sub-phase 4.2 — PDQ near-match wired into the Track-1 engine. 🚨

use fabstir_llm_node::moderation::csam::hashlist::HashListSource;
use fabstir_llm_node::moderation::csam::matcher::Matcher;
use fabstir_llm_node::moderation::csam::mock_source::MockHashListSource;
use fabstir_llm_node::moderation::csam::ownhash::OwnHashList;
use fabstir_llm_node::moderation::csam::pdq;
use fabstir_llm_node::moderation::types::Pdq256;

/// A PDQ hash at exactly Hamming distance `n` from `base`.
fn flip_bits(base: &Pdq256, n: usize) -> Pdq256 {
    let mut b = base.0;
    for i in 0..n {
        b[i / 8] ^= 1 << (i % 8);
    }
    Pdq256(b)
}

#[test]
fn pdq_within_threshold_matches() {
    let listed = Pdq256([0u8; 32]);
    let snap = MockHashListSource::loaded(vec![], vec![listed])
        .refresh()
        .unwrap();
    let own = OwnHashList::new();
    let m = Matcher::new(&snap, &own);
    let query = flip_bits(&listed, 20); // distance 20 ≤ 31
    let r = m.match_pdq(&query, 31).unwrap();
    assert!(r.is_match, "a PDQ within threshold must match");
    assert_eq!(r.distance, Some(20), "distance must be reported");
}

#[test]
fn pdq_above_threshold_no_match() {
    let listed = Pdq256([0u8; 32]);
    let snap = MockHashListSource::loaded(vec![], vec![listed])
        .refresh()
        .unwrap();
    let own = OwnHashList::new();
    let m = Matcher::new(&snap, &own);
    let query = flip_bits(&listed, 50); // distance 50 > 31
    assert!(
        !m.match_pdq(&query, 31).unwrap().is_match,
        "beyond threshold ⇒ no match"
    );
}

#[test]
fn threshold_is_config_driven() {
    let listed = Pdq256([0u8; 32]);
    let snap = MockHashListSource::loaded(vec![], vec![listed])
        .refresh()
        .unwrap();
    let own = OwnHashList::new();
    let m = Matcher::new(&snap, &own);
    let query = flip_bits(&listed, 20);
    let d = pdq::hamming(&listed, &query);
    assert_eq!(d, 20);
    assert!(
        !m.match_pdq(&query, d - 1).unwrap().is_match,
        "tighter threshold ⇒ no match"
    );
    assert!(
        m.match_pdq(&query, d).unwrap().is_match,
        "looser threshold ⇒ match"
    );
}

#[test]
fn exact_prefilter_short_circuits_pdq() {
    // When the exact SHA-256 prefilter hits, PDQ is NOT consulted: the result is the
    // exact hit (distance 0), even though the supplied PDQ is far from the list PDQ.
    let known_sha = [0x55u8; 32];
    let snap = MockHashListSource::loaded(vec![known_sha], vec![Pdq256([0xFFu8; 32])])
        .refresh()
        .unwrap();
    let own = OwnHashList::new();
    let m = Matcher::new(&snap, &own);
    let far_pdq = Pdq256([0u8; 32]); // distance 256 from the list PDQ
    let r = m.match_content(&known_sha, Some(&far_pdq), 31).unwrap();
    assert!(r.is_match);
    assert_eq!(
        r.distance,
        Some(0),
        "exact prefilter must short-circuit PDQ (distance 0, not the far PDQ distance)"
    );
}

#[test]
fn pdq_match_against_unavailable_list_holds() {
    // Fail-closed: PDQ near-match against an Unavailable list errors, never "no match".
    let snap = fabstir_llm_node::moderation::csam::hashlist::HashListSnapshot::unavailable();
    let own = OwnHashList::new();
    let m = Matcher::new(&snap, &own);
    assert!(m.match_pdq(&Pdq256([0u8; 32]), 31).is_err());
}
