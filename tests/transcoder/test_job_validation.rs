// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for format spec hashing and modelId generation.

use fabstir_llm_node::transcoder::job_validation::{
    canonical_format_spec, compute_transcode_model_id,
};
use fabstir_llm_node::transcoder::types::VideoFormat;

fn make_format(id: u32, ext: &str, vcodec: &str) -> VideoFormat {
    VideoFormat {
        id,
        ext: ext.into(),
        label: None,
        type_: None,
        vcodec: Some(vcodec.into()),
        acodec: None,
        preset: None,
        profile: None,
        ch: None,
        vf: None,
        b_v: None,
        ar: None,
        b_a: None,
        c_a: None,
        minrate: None,
        maxrate: None,
        bufsize: None,
        gpu: None,
        compression_level: None,
        dest: None,
        encrypt: None,
    }
}

#[test]
fn test_format_spec_hash_deterministic() {
    let fmts = vec![
        make_format(1, "mp4", "libx264"),
        make_format(2, "webm", "libvpx"),
    ];
    let h1 = compute_transcode_model_id(&fmts);
    let h2 = compute_transcode_model_id(&fmts);
    assert_eq!(h1, h2);
    assert_ne!(h1, [0u8; 32]);
}

#[test]
fn test_format_spec_hash_order_independent() {
    let fmt1 = make_format(1, "mp4", "libx264");
    let fmt2 = make_format(2, "webm", "libvpx");
    let h_a = compute_transcode_model_id(&[fmt1.clone(), fmt2.clone()]);
    let h_b = compute_transcode_model_id(&[fmt2, fmt1]);
    assert_eq!(h_a, h_b);
}

#[test]
fn test_format_spec_hash_different_formats() {
    let fmts_a = vec![make_format(1, "mp4", "libx264")];
    let fmts_b = vec![make_format(1, "mp4", "libx265")];
    assert_ne!(
        compute_transcode_model_id(&fmts_a),
        compute_transcode_model_id(&fmts_b)
    );
}

#[test]
fn test_transcode_model_id_generation() {
    let fmts = vec![make_format(1, "mp4", "libx264")];
    let spec = canonical_format_spec(&fmts);
    assert!(!spec.is_empty());
    // modelId should be keccak256 of the canonical spec
    let model_id = compute_transcode_model_id(&fmts);
    let mut hasher = tiny_keccak::Keccak::v256();
    let mut expected = [0u8; 32];
    tiny_keccak::Hasher::update(&mut hasher, spec.as_bytes());
    tiny_keccak::Hasher::finalize(hasher, &mut expected);
    assert_eq!(model_id, expected);
}
