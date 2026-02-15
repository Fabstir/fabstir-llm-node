// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Image generation request types and validation

use serde::{Deserialize, Serialize};

use crate::diffusion::client::ALLOWED_SIZES;

/// Request for image generation via POST /v1/images/generate
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateImageRequest {
    /// Text prompt describing the desired image
    pub prompt: String,

    /// Model name (optional; defaults to sidecar's configured model)
    #[serde(default)]
    pub model: Option<String>,

    /// Output image size (e.g., "1024x1024")
    #[serde(default)]
    pub size: Option<String>,

    /// Number of inference steps
    #[serde(default)]
    pub steps: Option<u32>,

    /// Random seed for reproducibility
    #[serde(default)]
    pub seed: Option<u64>,

    /// Negative prompt to guide away from
    #[serde(default)]
    pub negative_prompt: Option<String>,

    /// Classifier-free guidance scale
    #[serde(default)]
    pub guidance_scale: Option<f32>,

    /// Safety level: strict, moderate, permissive
    #[serde(default)]
    pub safety_level: Option<String>,

    /// Chain ID for pricing context
    #[serde(default)]
    pub chain_id: Option<u64>,

    /// Session ID for rate limiting and tracking
    #[serde(default)]
    pub session_id: Option<String>,

    /// Job ID for billing integration
    #[serde(default)]
    pub job_id: Option<u64>,
}

impl GenerateImageRequest {
    /// Validate the image generation request
    pub fn validate(&self) -> Result<(), String> {
        // Validate prompt is not empty
        if self.prompt.trim().is_empty() {
            return Err("prompt must not be empty".to_string());
        }

        // Validate size if provided
        if let Some(ref size) = self.size {
            if !ALLOWED_SIZES.contains(&size.as_str()) {
                return Err(format!(
                    "invalid size '{}'; allowed: {}",
                    size,
                    ALLOWED_SIZES.join(", ")
                ));
            }
        }

        // Validate steps if provided
        if let Some(steps) = self.steps {
            if steps == 0 || steps > 100 {
                return Err(format!("steps must be between 1 and 100, got {}", steps));
            }
        }

        Ok(())
    }
}
