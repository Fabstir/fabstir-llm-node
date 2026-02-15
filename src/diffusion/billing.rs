// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Image generation billing calculation and per-job tracking

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

/// Calculate generation units for billing.
///
/// Formula: `(width * height / 1_048_576) * (steps / 20) * model_multiplier`
/// - 1_048_576 = 1024*1024 (1 megapixel baseline)
/// - 20 = baseline steps
/// - model_multiplier = 1.0 for FLUX Klein
pub fn calculate_generation_units(
    width: u32,
    height: u32,
    steps: u32,
    model_multiplier: f64,
) -> f64 {
    let megapixels = (width as f64 * height as f64) / 1_048_576.0;
    let step_factor = steps as f64 / 20.0;
    megapixels * step_factor * model_multiplier
}

/// Per-job image generation tracking info
#[derive(Debug, Clone)]
pub struct ImageJobInfo {
    pub job_id: u64,
    pub session_id: Option<String>,
    pub total_units: f64,
    pub generation_count: u32,
    pub last_generation: Instant,
}

/// Tracks image generation units per job for billing
pub struct ImageGenerationTracker {
    jobs: Arc<RwLock<HashMap<u64, ImageJobInfo>>>,
}

impl ImageGenerationTracker {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Record a generation for a job
    pub async fn track(&self, job_id: u64, session_id: Option<&str>, units: f64) {
        let mut jobs = self.jobs.write().await;
        let entry = jobs.entry(job_id).or_insert_with(|| ImageJobInfo {
            job_id,
            session_id: session_id.map(|s| s.to_string()),
            total_units: 0.0,
            generation_count: 0,
            last_generation: Instant::now(),
        });
        entry.total_units += units;
        entry.generation_count += 1;
        entry.last_generation = Instant::now();
    }

    /// Get tracking info for a job
    pub async fn get_job_info(&self, job_id: u64) -> Option<ImageJobInfo> {
        self.jobs.read().await.get(&job_id).cloned()
    }
}

impl Default for ImageGenerationTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Content hashes for image generation proof witness
///
/// Binds prompt, output, and safety attestation into a single
/// hash for inclusion in STARK proofs.
pub struct ImageContentHashes {
    pub prompt_hash: [u8; 32],
    pub output_hash: [u8; 32],
    pub safety_attestation_hash: [u8; 32],
    pub seed: u64,
    pub generation_units: f64,
}

impl ImageContentHashes {
    /// Compute a combined SHA-256 hash of all fields
    pub fn compute_data_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.prompt_hash);
        hasher.update(self.output_hash);
        hasher.update(self.safety_attestation_hash);
        hasher.update(self.seed.to_le_bytes());
        hasher.update(self.generation_units.to_le_bytes());
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }

    /// Serialize to bytes for proof witness
    pub fn to_witness_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(32 * 3 + 8 + 8);
        bytes.extend_from_slice(&self.prompt_hash);
        bytes.extend_from_slice(&self.output_hash);
        bytes.extend_from_slice(&self.safety_attestation_hash);
        bytes.extend_from_slice(&self.seed.to_le_bytes());
        bytes.extend_from_slice(&self.generation_units.to_le_bytes());
        bytes
    }
}
