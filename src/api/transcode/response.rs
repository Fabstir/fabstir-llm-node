// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! HTTP response types for transcoding endpoints.

use serde::Serialize;

/// Response from POST /v1/transcode.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscodeHttpResponse {
    pub task_id: String,
    pub status: String,
    pub message: String,
}

/// Billing info included in status responses.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscodeBillingInfo {
    pub units: f64,
    pub tokens: u64,
}

/// Response from GET /v1/transcode/:task_id.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscodeStatusHttpResponse {
    pub task_id: String,
    pub progress: i32,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing: Option<TranscodeBillingInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
}
