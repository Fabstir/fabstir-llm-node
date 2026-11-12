// Copyright (c) 2026 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! D9 frames index and header record, D9a padding. Mutations: unsorted or non-canonical index accepted,
//! class not checked, record field dropped, `needs_pad` off by one.
mod common;
use common::*;
use fabstir_provenance::class::Class;
use fabstir_provenance::exr_record::{needs_pad, ExrRecord};
use fabstir_provenance::frames_index;

fn frames() -> Vec<(String, Vec<u8>)> {
    vec![
        ("frame_000002.exr".into(), b"two".to_vec()),
        ("frame_000001.exr".into(), b"one".to_vec()),
    ]
}

#[test]
fn frames_index_is_golden_and_canonical() {
    let b = frames_index::build(Class::Generated, &frames()).unwrap();
    golden("frames-index", &serde_json::from_slice(&b).unwrap());
    assert_eq!(frames_index::validate(&b, Class::Generated), Ok(2));
    let mut rev = frames();
    rev.reverse();
    assert_eq!(
        frames_index::build(Class::Generated, &rev).unwrap(),
        b,
        "input order must not matter"
    );
}

#[test]
fn frames_index_refuses_bad_input() {
    assert!(frames_index::build(Class::Generated, &[]).is_err());
    let dup = vec![
        ("a.exr".to_string(), b"x".to_vec()),
        ("a.exr".to_string(), b"y".to_vec()),
    ];
    assert!(frames_index::build(Class::Generated, &dup).is_err());
    assert!(frames_index::build(Class::Generated, &[("../a.exr".into(), b"x".to_vec())]).is_err());
    // names are fixed by the format: no gap, no other stem, no text
    assert!(
        frames_index::build(
            Class::Generated,
            &[("frame_000002.exr".into(), b"x".to_vec())]
        )
        .is_err(),
        "gap"
    );
    assert!(
        frames_index::build(
            Class::Generated,
            &[("prompt_a_cat.exr".into(), b"x".to_vec())]
        )
        .is_err(),
        "text"
    );
    assert!(
        frames_index::build(
            Class::Generated,
            &[
                ("frame_000001.exr".into(), b"x".to_vec()),
                ("frame_000003.exr".into(), b"y".to_vec())
            ]
        )
        .is_err(),
        "hole"
    );
    let many: Vec<(String, Vec<u8>)> = (1..=frames_index::MAX_FRAMES + 1)
        .map(|n| (frames_index::frame_name(n), vec![]))
        .collect();
    assert!(
        frames_index::build(Class::Generated, &many).is_err(),
        "count cap"
    );
    assert!(frames_index::build(Class::Generated, &[(".a".into(), b"x".to_vec())]).is_err());
}

#[test]
fn frames_index_validation_refuses_tampering() {
    let b = frames_index::build(Class::Modified, &frames()).unwrap();
    assert!(
        frames_index::validate(&b, Class::Generated).is_err(),
        "class mismatch"
    );
    let pretty =
        serde_json::to_vec_pretty(&serde_json::from_slice::<serde_json::Value>(&b).unwrap())
            .unwrap();
    assert!(
        frames_index::validate(&pretty, Class::Modified).is_err(),
        "non-canonical"
    );
    let s = String::from_utf8(b).unwrap();
    let extra = s.replacen("{", "{\"note\":\"x\",", 1);
    assert!(
        frames_index::validate(extra.as_bytes(), Class::Modified).is_err(),
        "extra key"
    );
    let upper = s.replace(
        &s[s.find("\"frame_000001.exr\":\"").unwrap() + 20..][..4],
        "ABCD",
    );
    assert!(
        frames_index::validate(upper.as_bytes(), Class::Modified).is_err(),
        "upper-case hash"
    );
}

#[test]
fn exr_record_is_golden() {
    let r = ExrRecord {
        class: Class::Modified,
        node_version: NODE_VERSION.into(),
        time: "2026-11-09T12:00:00Z".into(),
        manifest_uuid: UUID_A.into(),
        sequence_id: SEQ_ID.into(),
        sequence_manifest_uuid: UUID_B.into(),
        resolver_url: URL.into(),
    };
    let b = r.to_bytes(0);
    golden("exr-record", &serde_json::from_slice(&b).unwrap());
    let padded = r.to_bytes(1);
    assert_eq!(padded.len(), b.len() + 1);
    assert_eq!(*padded.last().unwrap(), b' ');
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&padded).unwrap(),
        serde_json::from_slice::<serde_json::Value>(&b).unwrap()
    );
}

#[test]
fn needs_pad_only_at_exact_256k_multiples() {
    assert!(needs_pad(262_144));
    assert!(needs_pad(3 * 262_144));
    assert!(!needs_pad(262_143));
    assert!(!needs_pad(262_145));
    assert!(!needs_pad(0));
}
