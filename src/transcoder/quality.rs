// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Quality measurement — ffmpeg PSNR/SSIM output parsing and threshold checking.

use std::sync::OnceLock;

use regex::Regex;

use super::types::{
    QualityMetrics, PSNR_HIGH_THRESHOLD, PSNR_STANDARD_THRESHOLD, SSIM_HIGH_THRESHOLD,
    SSIM_STANDARD_THRESHOLD,
};

static PSNR_RE: OnceLock<Regex> = OnceLock::new();
static SSIM_RE: OnceLock<Regex> = OnceLock::new();

fn extract_f64(re: &Regex, text: &str) -> Option<f64> {
    re.captures(text)?.get(1)?.as_str().parse().ok()
}

/// Parse PSNR average from ffmpeg stderr output.
pub fn parse_psnr_output(stderr: &str) -> Option<f64> {
    let re = PSNR_RE.get_or_init(|| Regex::new(r"PSNR.*average:([\d]+\.[\d]+)").unwrap());
    extract_f64(re, stderr)
}

/// Parse SSIM All value from ffmpeg stderr output.
pub fn parse_ssim_output(stderr: &str) -> Option<f64> {
    let re = SSIM_RE.get_or_init(|| Regex::new(r"SSIM.*All:([\d]+\.[\d]+)").unwrap());
    extract_f64(re, stderr)
}

/// Check if quality metrics meet the given tier threshold ("standard" or "high").
pub fn check_quality_threshold(metrics: &QualityMetrics, tier: &str) -> bool {
    let (psnr_min, ssim_min) = match tier {
        "high" => (PSNR_HIGH_THRESHOLD, Some(SSIM_HIGH_THRESHOLD)),
        _ => (PSNR_STANDARD_THRESHOLD, None),
    };
    if metrics.psnr_db < psnr_min {
        return false;
    }
    if let Some(threshold) = ssim_min {
        return metrics.ssim.is_some_and(|s| s >= threshold);
    }
    true
}
