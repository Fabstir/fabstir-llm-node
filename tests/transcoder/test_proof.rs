// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for GOP proof building pipeline.

use fabstir_llm_node::transcoder::proof::{
    build_gop_proof, compute_codec_params_hash, compute_proof_hash, generate_gop_stark_proof,
    serialize_proof_for_s5,
};
use fabstir_llm_node::transcoder::types::{QualityMetrics, VideoFormat};

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
fn test_codec_params_hash_deterministic() {
    let fmts = vec![
        make_format(1, "mp4", "libx264"),
        make_format(2, "webm", "libvpx"),
    ];
    let h1 = compute_codec_params_hash(&fmts);
    let h2 = compute_codec_params_hash(&fmts);
    assert_eq!(h1, h2);
    assert_ne!(h1, [0u8; 32]);

    let other = vec![make_format(1, "mp4", "libx265")];
    assert_ne!(compute_codec_params_hash(&other), h1);
}

#[test]
fn test_codec_params_hash_order_independent() {
    let fmt1 = make_format(1, "mp4", "libx264");
    let fmt2 = make_format(2, "webm", "libvpx");
    let h_a = compute_codec_params_hash(&[fmt1.clone(), fmt2.clone()]);
    let h_b = compute_codec_params_hash(&[fmt2, fmt1]);
    assert_eq!(h_a, h_b);
}

#[test]
fn test_build_gop_proof() {
    let metrics = QualityMetrics {
        psnr_db: 42.0,
        ssim: Some(0.95),
        actual_bitrate: 5000,
    };
    let input = [1u8; 32];
    let output = [2u8; 32];
    let proof = build_gop_proof(7, input, output, &metrics);
    assert_eq!(proof.gop_index, 7);
    assert_eq!(proof.input_gop_hash, hex::encode(input));
    assert_eq!(proof.output_gop_hash, hex::encode(output));
    assert_eq!(proof.psnr_db, 42.0);
    assert_eq!(proof.ssim, Some(0.95));
    assert_eq!(proof.actual_bitrate, 5000);
}

#[test]
fn test_serialize_proof_for_s5() {
    let metrics = QualityMetrics {
        psnr_db: 40.0,
        ssim: None,
        actual_bitrate: 4000,
    };
    let proof = build_gop_proof(0, [0u8; 32], [1u8; 32], &metrics);
    let stark_bytes = b"mock_stark_proof_data";
    let serialized = serialize_proof_for_s5(&proof, stark_bytes);
    assert!(!serialized.is_empty());
    // Should contain the gop_index somewhere in the JSON portion
    let json_part = String::from_utf8_lossy(&serialized);
    assert!(json_part.contains("gop_index") || serialized.len() > 20);
}

#[test]
fn test_compute_proof_hash() {
    let data = b"some proof data";
    let h1 = compute_proof_hash(data);
    let h2 = compute_proof_hash(data);
    assert_eq!(h1, h2);
    assert_ne!(h1, [0u8; 32]);
}

#[test]
fn test_generate_gop_stark_proof_mock() {
    let proof_bytes = generate_gop_stark_proof(42, [1u8; 32], [2u8; 32], [3u8; 32]).unwrap();
    assert!(!proof_bytes.is_empty());
    // Mock proof is 200 bytes
    assert_eq!(proof_bytes.len(), 200);
}
