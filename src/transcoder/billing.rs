// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Billing calculations and job tracking for transcoding.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Extract resolution billing factor from a video filter string like "scale=WxH".
/// Supports both `x` (API format) and `:` (ffmpeg format) separators.
/// Tier-based: height ≤480→0.25, ≤720→0.5, ≤1080→1.0, >1080→2.0.
/// Returns 1.0 if unparseable.
pub fn resolution_factor_from_vf(vf: &str) -> f64 {
    if let Some(scale) = vf.strip_prefix("scale=") {
        if let Some((_w, h_str)) = scale.split_once('x').or_else(|| scale.split_once(':')) {
            if let Ok(height) = h_str.parse::<u32>() {
                return match height {
                    0..=480 => 0.25,
                    481..=720 => 0.5,
                    721..=1080 => 1.0,
                    _ => 2.0,
                };
            }
        }
    }
    1.0
}

/// Get codec billing factor. AV1 is 1.5x, HEVC/H.265 is 1.2x, others 1.0x.
pub fn codec_factor(vcodec: &str) -> f64 {
    if vcodec.starts_with("av1") {
        1.5
    } else if vcodec.starts_with("hevc") || vcodec.starts_with("h265") {
        1.2
    } else {
        1.0
    }
}

/// Calculate transcoding billing units.
/// Formula: duration × resolution_factor × codec_factor × encryption_factor.
pub fn calculate_transcode_units(
    duration: f64,
    resolution_factor: f64,
    codec_factor: f64,
    is_encrypted: bool,
) -> f64 {
    let encryption_factor = if is_encrypted { 1.1 } else { 1.0 };
    duration * resolution_factor * codec_factor * encryption_factor
}

/// Info about a tracked transcoding job.
#[derive(Debug, Clone)]
pub struct TranscodingJobInfo {
    pub job_id: u64,
    pub session_id: Option<String>,
    pub total_units: f64,
    pub format_count: u32,
    pub last_update: Instant,
}

/// Tracks transcoding jobs for billing.
pub struct TranscodingTracker {
    jobs: Arc<RwLock<HashMap<u64, TranscodingJobInfo>>>,
}

impl TranscodingTracker {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Record transcoding units for a job.
    pub async fn track(&self, job_id: u64, session_id: Option<String>, units: f64) {
        let mut jobs = self.jobs.write().await;
        let entry = jobs.entry(job_id).or_insert_with(|| TranscodingJobInfo {
            job_id,
            session_id: session_id.clone(),
            total_units: 0.0,
            format_count: 0,
            last_update: Instant::now(),
        });
        entry.total_units += units;
        entry.format_count += 1;
        entry.last_update = Instant::now();
    }

    /// Get tracking info for a job.
    pub async fn get_job_info(&self, job_id: u64) -> Option<TranscodingJobInfo> {
        self.jobs.read().await.get(&job_id).cloned()
    }
}

impl Default for TranscodingTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The unit price a customer is quoted from: at pricePerToken 5000, one unit
    /// (a second of 1080p H.264, unencrypted) costs 5000 micro-USDC / 1000 = 5
    /// micro, i.e. half a cent. Every other shape is a multiplier off that, and
    /// these factors are what the quote depends on — change one and someone's
    /// deposit sizing silently stops covering their clip.
    #[test]
    fn resolution_factors_are_the_quoted_bands() {
        assert_eq!(resolution_factor_from_vf("scale=854x480"), 0.25);
        assert_eq!(resolution_factor_from_vf("scale=1280x720"), 0.5);
        assert_eq!(resolution_factor_from_vf("scale=1920x1080"), 1.0);
        assert_eq!(resolution_factor_from_vf("scale=3840x2160"), 2.0);
        // Band edges, since 480/720/1080 sit ON the boundaries.
        assert_eq!(resolution_factor_from_vf("scale=640x481"), 0.5);
        assert_eq!(resolution_factor_from_vf("scale=1280x721"), 1.0);
        assert_eq!(resolution_factor_from_vf("scale=1920x1081"), 2.0);
        // Unparseable falls back to 1.0 rather than to free.
        assert_eq!(resolution_factor_from_vf(""), 1.0);
        assert_eq!(resolution_factor_from_vf("scale=nonsense"), 1.0);
    }

    #[test]
    fn codec_and_encryption_factors() {
        assert_eq!(codec_factor("av1_nvenc"), 1.5);
        assert_eq!(codec_factor("hevc_nvenc"), 1.2);
        assert_eq!(codec_factor("h265"), 1.2);
        assert_eq!(codec_factor("h264"), 1.0);
        assert_eq!(codec_factor(""), 1.0);
        assert_eq!(calculate_transcode_units(10.0, 1.0, 1.0, true), 11.0);
        assert_eq!(calculate_transcode_units(10.0, 1.0, 1.0, false), 10.0);
    }

    /// The worked example quoted to the Fabstir v2 team on 2026-08-21 while they
    /// sized their deposits: a 60s clip in three AV1 renditions. If this figure
    /// moves, their deposits stop covering their transcodes, so the number is
    /// pinned here rather than left to be recomputed from the factors.
    #[test]
    fn sixty_second_three_rendition_av1_clip_costs_what_we_quoted() {
        let per_rendition = |height_vf: &str, encrypted: bool| {
            calculate_transcode_units(
                60.0,
                resolution_factor_from_vf(height_vf),
                codec_factor("av1_nvenc"),
                encrypted,
            )
        };
        let renditions = ["scale=854x480", "scale=1280x720", "scale=1920x1080"];

        let plain: f64 = renditions.iter().map(|vf| per_rendition(vf, false)).sum();
        assert_eq!(plain, 157.5);
        let tokens = (plain * 1000.0).ceil() as u64;
        assert_eq!(tokens, 157_500);
        // At pricePerToken 5000: tokens * 5000 / 1000 micro-USDC.
        assert_eq!(tokens * 5000 / 1000, 787_500); // $0.79

        let encrypted: f64 = renditions.iter().map(|vf| per_rendition(vf, true)).sum();
        let enc_tokens = (encrypted * 1000.0).ceil() as u64;
        assert_eq!(enc_tokens, 173_251);
        assert_eq!(enc_tokens * 5000 / 1000, 866_255); // $0.87

        // And the point that matters for sizing: BOTH exceed a 0.5 USDC deposit.
        assert!(787_500 > 500_000);
        assert!(866_255 > 500_000);
    }

    #[tokio::test]
    async fn tracker_accumulates_units_across_renditions() {
        let tracker = TranscodingTracker::new();
        tracker.track(42, Some("s1".into()), 22.5).await;
        tracker.track(42, Some("s1".into()), 45.0).await;
        tracker.track(42, Some("s1".into()), 90.0).await;
        let info = tracker.get_job_info(42).await.unwrap();
        assert_eq!(info.total_units, 157.5);
        assert_eq!(info.format_count, 3);
    }
}
