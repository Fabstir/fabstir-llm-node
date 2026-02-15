// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Image generation via SGLang Diffusion sidecar with content safety pipeline

pub mod billing;
pub mod client;
pub mod output_safety;
pub mod prompt_safety;
pub mod rate_limiter;
pub mod safety;

pub use client::{DiffusionClient, DiffusionResult, ImageGenerationRequest, ImageSize};
pub use output_safety::OutputSafetyClassifier;
pub use prompt_safety::PromptSafetyClassifier;
pub use rate_limiter::ImageGenerationRateLimiter;
pub use safety::{SafetyAttestation, SafetyCategory, SafetyConfig, SafetyLevel, SafetyResult};
