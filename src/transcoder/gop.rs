// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! GOP (Group of Pictures) tracking and estimation for transcode progress.

use super::types::GOPInfo;

/// Default keyframe interval in seconds when not specified.
const DEFAULT_KEYFRAME_INTERVAL: f64 = 2.0;

/// Estimate total GOP count from video duration and keyframe interval.
pub fn estimate_total_gops(duration: f64, keyframe_interval: f64) -> u32 {
    if keyframe_interval <= 0.0 || duration <= 0.0 {
        return 0;
    }
    (duration / keyframe_interval).ceil() as u32
}

/// Estimate current GOP index from progress percentage.
pub fn estimate_current_gop(progress: i32, total_gops: u32) -> u32 {
    ((progress.max(0) as u64 * total_gops as u64) / 100) as u32
}

/// Build GOPInfo from progress, duration, and elapsed time.
pub fn gop_info_from_progress(progress: i32, duration: f64, elapsed: f64) -> GOPInfo {
    let total_gops = estimate_total_gops(duration, DEFAULT_KEYFRAME_INTERVAL);
    let current_gop = estimate_current_gop(progress, total_gops);
    GOPInfo {
        current_gop,
        total_gops,
        elapsed_seconds: elapsed,
    }
}
