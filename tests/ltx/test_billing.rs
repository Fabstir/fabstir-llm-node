// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Phase 7 billing tests for the LTX sidecar: the megapixel-frame token vector,
//! `estimate_cost`, the `MIN_PROVEN_TOKENS` floor, and `LtxTracker` accumulation.

use ethers::types::U256;
use fabstir_llm_node::ltx::billing::{estimate_cost, LtxTracker, MIN_PROVEN_TOKENS};
use fabstir_llm_node::ltx::submit::ltx_tokens;
use fabstir_llm_node::ltx::{LtxJob, OutputKind, Resolution};

fn sample_job() -> LtxJob {
    LtxJob {
        template_id: "ltx-t2v-hdr".to_string(),
        template_hash: "0x9f2c".to_string(),
        prompt: "a derelict spaceship corridor".to_string(),
        seed: "4815162342".to_string(),
        frames: 121,
        fps: 24,
        resolution: Resolution { w: 1280, h: 720 },
        lora: "ltx-iclora-hdr@v1".to_string(),
        output: OutputKind::ExrSequence,
        images: None,
    }
}

#[test]
fn test_token_count_vector() {
    // Worked example from the interface seam: 121 × 1280 × 720 → 111,514.
    assert_eq!(ltx_tokens(121, 1280, 720), 111_514);
}

#[test]
fn test_estimate_cost_is_tokens_times_price() {
    let price = U256::from(5_000u64); // pricePerToken (with PRICE_PRECISION)
    let cost = estimate_cost(&sample_job(), price);
    assert_eq!(cost, U256::from(111_514u64) * price);
}

#[test]
fn test_real_clip_clears_floor() {
    assert_eq!(MIN_PROVEN_TOKENS, 100);
    let tokens = ltx_tokens(121, 1280, 720);
    assert!(
        tokens >= MIN_PROVEN_TOKENS,
        "a real clip ({tokens}) must clear the floor"
    );
}

#[tokio::test]
async fn test_tracker_accumulates_across_records() {
    let tracker = LtxTracker::new();
    let price = U256::from(10u64);
    tracker
        .track(
            1,
            Some("session-1".into()),
            111_514,
            U256::from(111_514u64) * price,
        )
        .await;
    tracker
        .track(
            1,
            Some("session-1".into()),
            50_000,
            U256::from(50_000u64) * price,
        )
        .await;
    let info = tracker.get_job_info(1).await.unwrap();
    assert_eq!(info.job_id, 1);
    assert_eq!(info.total_tokens, 161_514);
    assert_eq!(info.total_cost, U256::from(161_514u64) * price);
    assert_eq!(info.generation_count, 2);
}
