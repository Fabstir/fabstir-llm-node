// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for ffmpeg PSNR/SSIM parsing and quality threshold checking.

use fabstir_llm_node::transcoder::quality::{
    check_quality_threshold, parse_psnr_output, parse_ssim_output,
};
use fabstir_llm_node::transcoder::QualityMetrics;

#[test]
fn test_parse_psnr_output_valid() {
    let stderr = "[Parsed_psnr_0 @ 0x55f3a1] PSNR y:42.315 u:45.123 v:44.891 average:42.315 min:38.200 max:48.100\n";
    let result = parse_psnr_output(stderr);
    assert_eq!(result, Some(42.315));
}

#[test]
fn test_parse_psnr_output_no_match() {
    let stderr = "frame=  120 fps=45 q=28.0 size=    256kB time=00:00:04.00\n";
    assert_eq!(parse_psnr_output(stderr), None);
}

#[test]
fn test_parse_ssim_output_valid() {
    let stderr = "[Parsed_ssim_0 @ 0x55f3a1] SSIM Y:0.962345 (14.20) U:0.975432 V:0.971234 All:0.962345 (14.20)\n";
    let result = parse_ssim_output(stderr);
    assert_eq!(result, Some(0.962345));
}

#[test]
fn test_parse_ssim_output_no_match() {
    let stderr = "Stream mapping: Stream #0:0 -> #0:0 (h264 -> libx264)\n";
    assert_eq!(parse_ssim_output(stderr), None);
}

#[test]
fn test_check_quality_standard_pass() {
    let m = QualityMetrics {
        psnr_db: 42.0,
        ssim: Some(0.95),
        actual_bitrate: 5000,
    };
    assert!(check_quality_threshold(&m, "standard"));
}

#[test]
fn test_check_quality_standard_fail() {
    let m = QualityMetrics {
        psnr_db: 35.0,
        ssim: Some(0.95),
        actual_bitrate: 5000,
    };
    assert!(!check_quality_threshold(&m, "standard"));
}

#[test]
fn test_check_quality_high_pass() {
    let m = QualityMetrics {
        psnr_db: 43.0,
        ssim: Some(0.96),
        actual_bitrate: 5000,
    };
    assert!(check_quality_threshold(&m, "high"));
}

#[test]
fn test_check_quality_high_fail_ssim() {
    let m = QualityMetrics {
        psnr_db: 43.0,
        ssim: Some(0.93),
        actual_bitrate: 5000,
    };
    assert!(!check_quality_threshold(&m, "high"));
}
