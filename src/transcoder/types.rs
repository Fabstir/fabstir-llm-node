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
    #[serde(rename = "b:v", skip_serializing_if = "Option::is_none")]
    pub b_v: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ar: Option<u32>,
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

/// A single transcoded output result (parsed from metadata JSON).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscodedFormatResult {
    pub format_index: usize,
    pub cid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption_key: Option<String>,
    pub dest: String,
}

/// Transcoding task lifecycle state.
#[derive(Debug, Clone)]
pub enum TranscodeTaskState {
    Pending,
    InProgress {
        progress: i32,
    },
    Completed {
        metadata: Vec<TranscodedFormatResult>,
        duration: f64,
    },
    Failed {
        error: String,
    },
}
