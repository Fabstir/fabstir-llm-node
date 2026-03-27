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
