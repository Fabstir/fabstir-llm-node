// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Request DTOs for the moderation HTTP endpoints (B8).

use serde::{Deserialize, Serialize};

/// `POST /v1/moderate/asset` body: an asset kind + base64-encoded bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModerateAssetRequest {
    /// `"image"` | `"subtitle"` | `"video_keyframe"`.
    pub kind: String,
    /// Base64 (standard alphabet) encoded asset bytes.
    pub data: String,
}
