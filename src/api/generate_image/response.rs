// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Image generation response types

use serde::{Deserialize, Serialize};

/// Response from image generation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateImageResponse {
    /// Base64-encoded generated image
    pub image: String,
    /// Model used for generation
    pub model: String,
    /// Output size (e.g., "1024x1024")
    pub size: String,
    /// Inference steps used
    pub steps: u32,
    /// Random seed used
    pub seed: u64,
    /// Processing time in milliseconds
    pub processing_time_ms: u64,
    /// Safety classification info
    pub safety: SafetyInfo,
    /// Provider (always "host")
    pub provider: String,
    /// Chain ID
    pub chain_id: u64,
    /// Chain name (e.g., "Base Sepolia")
    pub chain_name: String,
    /// Native token symbol (e.g., "ETH")
    pub native_token: String,
    /// Billing information
    pub billing: BillingInfo,
}

/// Safety classification info included in responses
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafetyInfo {
    /// Whether the prompt passed safety checks
    pub prompt_safe: bool,
    /// Whether the output image passed safety checks
    pub output_safe: bool,
    /// Safety level used for classification
    pub safety_level: String,
}

/// Billing information for image generation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillingInfo {
    /// Generation units consumed (megapixels * step_factor * model_multiplier)
    pub generation_units: f64,
    /// Model-specific billing multiplier
    pub model_multiplier: f64,
    /// Megapixels of the output image
    pub megapixels: f64,
    /// Number of inference steps
    pub steps: u32,
}

impl GenerateImageResponse {
    /// Create a response with chain context automatically resolved
    pub fn with_chain_context(
        image: String,
        model: String,
        size: String,
        steps: u32,
        seed: u64,
        processing_time_ms: u64,
        safety: SafetyInfo,
        billing: BillingInfo,
        chain_id: u64,
    ) -> Self {
        let (chain_name, native_token) = match chain_id {
            84532 => ("Base Sepolia", "ETH"),
            5611 => ("opBNB Testnet", "BNB"),
            _ => ("Base Sepolia", "ETH"),
        };

        Self {
            image,
            model,
            size,
            steps,
            seed,
            processing_time_ms,
            safety,
            provider: "host".to_string(),
            chain_id,
            chain_name: chain_name.to_string(),
            native_token: native_token.to_string(),
            billing,
        }
    }
}
