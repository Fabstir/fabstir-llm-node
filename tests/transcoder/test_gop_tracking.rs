// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for GOP estimation and tracking functions.

use fabstir_llm_node::transcoder::gop::{
    estimate_current_gop, estimate_total_gops, gop_info_from_progress,
};

#[test]
fn test_estimate_total_gops() {
    // 120s video, 2s keyframe interval → 60 GOPs
    assert_eq!(estimate_total_gops(120.0, 2.0), 60);
}

#[test]
fn test_estimate_total_gops_short_video() {
    // 5s video, 2s interval → ceil(5/2) = 3 GOPs
    assert_eq!(estimate_total_gops(5.0, 2.0), 3);
}

#[test]
fn test_estimate_current_gop_from_progress() {
    // 50% progress with 60 total GOPs → current GOP = 30
    assert_eq!(estimate_current_gop(50, 60), 30);
}

#[test]
fn test_gop_info_from_progress_and_duration() {
    let info = gop_info_from_progress(50, 120.0, 25.0);
    assert_eq!(info.current_gop, 30);
    assert_eq!(info.total_gops, 60);
    assert_eq!(info.elapsed_seconds, 25.0);
}
