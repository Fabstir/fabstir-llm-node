// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Image generation API endpoint module
//!
//! Provides POST /v1/images/generate for text-to-image generation.

pub mod handler;
pub mod request;
pub mod response;

pub use handler::generate_image_handler;
pub use request::GenerateImageRequest;
pub use response::{BillingInfo, GenerateImageResponse, SafetyInfo};
