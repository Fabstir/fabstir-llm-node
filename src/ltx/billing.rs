// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Megapixel-frame billing for the LTX sidecar: cost estimation and per-job
//! token/cost tracking. Mirrors `transcoder::billing::TranscodingTracker`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use ethers::types::U256;
use tokio::sync::RwLock;

use crate::ltx::submit::ltx_tokens;
use crate::ltx::types::LtxJob;

/// Minimum billable tokens floor (mirrors `checkpoint_manager::MIN_PROVEN_TOKENS`).
/// Any real clip clears this by orders of magnitude (121×1280×720 = 111,514).
pub const MIN_PROVEN_TOKENS: u64 = 100;

/// Estimate the on-chain cost of a job: `tokens × price_per_token`.
///
/// Straight multiply with no PRICE_PRECISION scaling — `price_per_token` already
/// carries the precision (see `pricing_constants`), exactly as the on-chain
/// `host_payment = (tokens × price_per_token) / PRICE_PRECISION` formula treats it.
/// `tokens` is the pinned megapixel-frame count from `submit::ltx_tokens`.
pub fn estimate_cost(job: &LtxJob, price_per_token: U256) -> U256 {
    let tokens = ltx_tokens(job.frames, job.resolution.w, job.resolution.h);
    U256::from(tokens) * price_per_token
}

/// Per-job LTX billing info (mirrors `transcoder::billing::TranscodingJobInfo`).
#[derive(Debug, Clone)]
pub struct LtxJobInfo {
    pub job_id: u64,
    pub session_id: Option<String>,
    pub total_tokens: u64,
    pub total_cost: U256,
    pub generation_count: u32,
    pub last_update: Instant,
}

/// Tracks LTX generation tokens/cost per job for billing.
pub struct LtxTracker {
    jobs: Arc<RwLock<HashMap<u64, LtxJobInfo>>>,
}

impl LtxTracker {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Record tokens and cost for a job, accumulating across generations.
    pub async fn track(&self, job_id: u64, session_id: Option<String>, tokens: u64, cost: U256) {
        let mut jobs = self.jobs.write().await;
        let entry = jobs.entry(job_id).or_insert_with(|| LtxJobInfo {
            job_id,
            session_id: session_id.clone(),
            total_tokens: 0,
            total_cost: U256::zero(),
            generation_count: 0,
            last_update: Instant::now(),
        });
        entry.total_tokens += tokens;
        entry.total_cost += cost;
        entry.generation_count += 1;
        entry.last_update = Instant::now();
    }

    /// Get tracking info for a job.
    pub async fn get_job_info(&self, job_id: u64) -> Option<LtxJobInfo> {
        self.jobs.read().await.get(&job_id).cloned()
    }
}

impl Default for LtxTracker {
    fn default() -> Self {
        Self::new()
    }
}
