// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! HTTP request types for transcoding endpoints.

use crate::transcoder::types::VideoFormat;
use serde::Deserialize;

/// POST /v1/transcode request body.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscodeHttpRequest {
    pub source_cid: String,
    pub media_formats: Vec<VideoFormat>,
    #[serde(default = "default_true")]
    pub is_encrypted: bool,
    #[serde(default = "default_true")]
    pub is_gpu: bool,
    pub chain_id: Option<u64>,
    pub session_id: Option<String>,
    pub job_id: Option<u64>,
}

fn default_true() -> bool {
    true
}
