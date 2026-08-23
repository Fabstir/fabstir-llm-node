// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Deep-conform manifest parsing (v8.44.2, transport v2): the strict JSON
//! manifest fails closed on everything outside "billed or billed-1 unique
//! frame CIDs", and the per-frame gate rejects non-EXR bytes.

use fabstir_llm_node::ltx::deep_input::{
    check_deep_frame, parse_deep_manifest, EXR_MAGIC, MAX_FRAME_BYTES,
};

fn manifest(n: usize) -> Vec<u8> {
    let cids: Vec<String> = (1..=n).map(|i| format!("uFrameCid{i:05}")).collect();
    serde_json::to_vec(&serde_json::json!({ "deepFrames": cids })).unwrap()
}

#[test]
fn parses_and_preserves_delivery_order() {
    let cids = parse_deep_manifest(&manifest(3), 3).expect("valid manifest parses");
    assert_eq!(cids, ["uFrameCid00001", "uFrameCid00002", "uFrameCid00003"]);
}

#[test]
fn count_honours_the_conform_plus_minus_one() {
    // The job bills fps*d + 1; a content-true conform carries fps*d — the
    // SAME tolerance the mp4 path has always had (caught live: a 9 s render
    // conformed 216 frames against 217 billed and was wrongly refused).
    assert!(
        parse_deep_manifest(&manifest(216), 217).is_ok(),
        "billed-1 accepted"
    );
    assert!(
        parse_deep_manifest(&manifest(217), 217).is_ok(),
        "exact accepted"
    );
    let err = parse_deep_manifest(&manifest(215), 217).unwrap_err();
    assert!(
        err.contains("lists 215") && err.contains("bills 217"),
        "{err}"
    );
    let err = parse_deep_manifest(&manifest(218), 217).unwrap_err();
    assert!(err.contains("lists 218"), "{err}");
}

#[test]
fn rejects_junk_json_duplicates_and_empty_cids() {
    assert!(parse_deep_manifest(b"not json", 1)
        .unwrap_err()
        .contains("not valid JSON"));
    assert!(parse_deep_manifest(b"{}", 1)
        .unwrap_err()
        .contains("not valid JSON"));

    let dup = serde_json::to_vec(&serde_json::json!({ "deepFrames": ["uA", "uA"] })).unwrap();
    assert!(parse_deep_manifest(&dup, 2)
        .unwrap_err()
        .contains("repeats"));

    let empty = serde_json::to_vec(&serde_json::json!({ "deepFrames": ["uA", ""] })).unwrap();
    assert!(parse_deep_manifest(&empty, 2)
        .unwrap_err()
        .contains("empty CID"));
}

#[test]
fn per_frame_gate_rejects_non_exr_and_oversize() {
    let mut good = EXR_MAGIC.to_vec();
    good.extend_from_slice(&[0u8; 64]);
    assert!(check_deep_frame(0, &good).is_ok());

    assert!(check_deep_frame(3, b"not an exr")
        .unwrap_err()
        .contains("frame 3"));

    // an oversize frame fails without allocating one: size check uses len()
    let mut big = EXR_MAGIC.to_vec();
    big.resize(MAX_FRAME_BYTES + 1, 0);
    assert!(check_deep_frame(1, &big).unwrap_err().contains("ceiling"));
}
