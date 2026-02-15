// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for image generation billing (Phase 5.1)

use fabstir_llm_node::diffusion::billing::{calculate_generation_units, ImageGenerationTracker};

#[test]
fn test_calculate_units_1024x1024_20steps_1x() {
    let units = calculate_generation_units(1024, 1024, 20, 1.0);
    assert!((units - 1.0).abs() < 0.001);
}

#[test]
fn test_calculate_units_512x512_20steps_1x() {
    let units = calculate_generation_units(512, 512, 20, 1.0);
    assert!((units - 0.25).abs() < 0.001);
}

#[test]
fn test_calculate_units_1024x1024_4steps_1x() {
    // FLUX Klein default: 4 steps
    let units = calculate_generation_units(1024, 1024, 4, 1.0);
    assert!((units - 0.2).abs() < 0.001);
}

#[test]
fn test_calculate_units_1024x1024_50steps_1x() {
    let units = calculate_generation_units(1024, 1024, 50, 1.0);
    assert!((units - 2.5).abs() < 0.001);
}

#[test]
fn test_calculate_units_1024x1024_20steps_2x() {
    // Premium model multiplier
    let units = calculate_generation_units(1024, 1024, 20, 2.0);
    assert!((units - 2.0).abs() < 0.001);
}

#[tokio::test]
async fn test_tracker_creates_new_job_entry() {
    let tracker = ImageGenerationTracker::new();
    tracker.track(1001, Some("session-1"), 0.5).await;
    let info = tracker.get_job_info(1001).await;
    assert!(info.is_some());
    let info = info.unwrap();
    assert_eq!(info.job_id, 1001);
    assert!((info.total_units - 0.5).abs() < 0.001);
    assert_eq!(info.generation_count, 1);
}

#[tokio::test]
async fn test_tracker_accumulates_units() {
    let tracker = ImageGenerationTracker::new();
    tracker.track(2002, Some("session-2"), 0.25).await;
    tracker.track(2002, Some("session-2"), 0.75).await;
    tracker.track(2002, Some("session-2"), 1.0).await;
    let info = tracker.get_job_info(2002).await.unwrap();
    assert!((info.total_units - 2.0).abs() < 0.001);
    assert_eq!(info.generation_count, 3);
}

#[tokio::test]
async fn test_tracker_get_nonexistent_job() {
    let tracker = ImageGenerationTracker::new();
    assert!(tracker.get_job_info(9999).await.is_none());
}

// v8.16.1: Image generation units → token-equivalent conversion for checkpoint system
#[test]
fn test_image_generation_units_to_tokens_conversion() {
    // Conversion factor: units * 1000, ceil → token-equivalent for checkpoint system
    let units_512 = calculate_generation_units(512, 512, 4, 1.0); // 0.05
    let tokens_512 = (units_512 * 1000.0).ceil() as u64;
    assert_eq!(tokens_512, 50);

    let units_1024 = calculate_generation_units(1024, 1024, 4, 1.0); // 0.20
    let tokens_1024 = (units_1024 * 1000.0).ceil() as u64;
    assert_eq!(tokens_1024, 200);

    let units_1024_20 = calculate_generation_units(1024, 1024, 20, 1.0); // 1.00
    let tokens_1024_20 = (units_1024_20 * 1000.0).ceil() as u64;
    assert_eq!(tokens_1024_20, 1000);

    let units_256 = calculate_generation_units(256, 256, 4, 1.0); // ~0.013
    let tokens_256 = (units_256 * 1000.0).ceil() as u64;
    assert_eq!(tokens_256, 13);
}
