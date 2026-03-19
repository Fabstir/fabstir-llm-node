// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for trustless verification types and quality threshold constants.

use fabstir_llm_node::transcoder::{
    GOPInfo, GOPProof, QualityMetrics, TranscodeProofTree, PSNR_HIGH_THRESHOLD,
    PSNR_STANDARD_THRESHOLD, SSIM_HIGH_THRESHOLD, SSIM_STANDARD_THRESHOLD,
};

#[test]
fn test_quality_metrics_serialization() {
    let m = QualityMetrics {
        psnr_db: 42.3,
        ssim: Some(0.96),
        actual_bitrate: 4850,
    };
    let json = serde_json::to_value(&m).unwrap();
    assert_eq!(json["psnr_db"], 42.3);
    assert_eq!(json["ssim"], 0.96);
    assert_eq!(json["actual_bitrate"], 4850);
}

#[test]
fn test_gop_proof_serialization() {
    let p = GOPProof {
        gop_index: 5,
        input_gop_hash: "aabb".into(),
        output_gop_hash: "ccdd".into(),
        psnr_db: 40.0,
        ssim: Some(0.94),
        actual_bitrate: 5000,
        stark_proof_hash: "eeff".into(),
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["gop_index"], 5);
    assert_eq!(json["input_gop_hash"], "aabb");
    assert_eq!(json["stark_proof_hash"], "eeff");
}

#[test]
fn test_proof_tree_serialization() {
    let t = TranscodeProofTree {
        root_hash: "0xabc".into(),
        gop_count: 60,
        tree_cid: "bafyxyz".into(),
        spot_check_hashes: vec!["h1".into(), "h2".into()],
    };
    let json = serde_json::to_value(&t).unwrap();
    assert_eq!(json["root_hash"], "0xabc");
    assert_eq!(json["gop_count"], 60);
    assert_eq!(json["tree_cid"], "bafyxyz");
    assert_eq!(json["spot_check_hashes"].as_array().unwrap().len(), 2);
}

#[test]
fn test_gop_info_serialization() {
    let g = GOPInfo {
        current_gop: 10,
        total_gops: 60,
        elapsed_seconds: 12.5,
    };
    let json = serde_json::to_value(&g).unwrap();
    assert_eq!(json["current_gop"], 10);
    assert_eq!(json["total_gops"], 60);
    assert_eq!(json["elapsed_seconds"], 12.5);
}

#[test]
fn test_psnr_threshold_constants() {
    assert_eq!(PSNR_STANDARD_THRESHOLD, 38.0);
    assert_eq!(PSNR_HIGH_THRESHOLD, 42.0);
}

#[test]
fn test_ssim_threshold_constants() {
    assert_eq!(SSIM_STANDARD_THRESHOLD, 0.90);
    assert_eq!(SSIM_HIGH_THRESHOLD, 0.95);
}
