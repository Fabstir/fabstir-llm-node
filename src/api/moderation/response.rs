// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Response DTOs for the moderation HTTP endpoints (B8).

use serde::{Deserialize, Serialize};

use crate::moderation::types::ModerationResult;

/// `POST /v1/moderate/asset` response. Mirrors the node-side `ModerationResult`
/// (`reason` is a category/rule id only — never raw matched content).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerateAssetResponse {
    /// `"cleared"` | `"blocked"` | `"flagged"`.
    pub verdict: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(rename = "reportId", skip_serializing_if = "Option::is_none")]
    pub report_id: Option<String>,
}

impl From<ModerationResult> for ModerateAssetResponse {
    fn from(r: ModerationResult) -> Self {
        Self {
            verdict: r.verdict.as_str().to_string(),
            reason: r.reason,
            report_id: r.report_id,
        }
    }
}
