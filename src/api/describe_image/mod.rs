// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Describe image API endpoint module
//!
//! Provides POST /v1/describe-image for generating image descriptions.

pub mod handler;
pub mod request;
pub mod response;

pub use handler::describe_image_handler;
pub use request::DescribeImageRequest;
pub use response::{DescribeImageResponse, DetectedObject, ImageAnalysis};
