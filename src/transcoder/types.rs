// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Types mirroring the fabstir-transcoder REST API.

use serde::{Deserialize, Serialize};

/// A single media format specification sent to the transcoder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFormat {
    pub id: u32,
    pub ext: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vcodec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acodec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ch: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vf: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b_v: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ar: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b_a: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c_a: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minrate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maxrate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bufsize: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpu: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compression_level: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypt: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trim_percent: Option<u32>,
}

/// Response from POST /transcode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscodeSubmitResponse {
    pub status_code: i32,
    pub message: String,
    pub task_id: String,
}

/// Response from GET /get_transcoded/{task_id}.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscodeStatusResponse {
    pub status_code: i32,
    pub metadata: String,
    pub progress: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
}

/// Transcoding task lifecycle state.
#[derive(Debug, Clone)]
pub enum TranscodeTaskState {
    Pending,
    InProgress {
        progress: i32,
    },
    Completed {
        metadata: serde_json::Value,
        duration: f64,
    },
    Failed {
        error: String,
    },
}

// --- Trustless verification types (v8.26.0) ---

/// Quality metrics from ffmpeg PSNR/SSIM measurement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityMetrics {
    pub psnr_db: f64,
    pub ssim: Option<f64>,
    pub actual_bitrate: u64,
}

/// Proof for a single GOP (Group of Pictures).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GOPProof {
    pub gop_index: u32,
    pub input_gop_hash: String,
    pub output_gop_hash: String,
    pub psnr_db: f64,
    pub ssim: Option<f64>,
    pub actual_bitrate: u64,
    pub stark_proof_hash: String,
}

/// Merkle tree over all GOP proofs for a transcode job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscodeProofTree {
    pub root_hash: String,
    pub gop_count: u32,
    pub tree_cid: String,
    pub spot_check_hashes: Vec<String>,
}

/// GOP-level progress info for streaming progress messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GOPInfo {
    pub current_gop: u32,
    pub total_gops: u32,
    pub elapsed_seconds: f64,
}

/// Quality threshold constants for transcode verification.
pub const PSNR_STANDARD_THRESHOLD: f64 = 38.0;
pub const PSNR_HIGH_THRESHOLD: f64 = 42.0;
pub const SSIM_STANDARD_THRESHOLD: f64 = 0.90;
pub const SSIM_HIGH_THRESHOLD: f64 = 0.95;
