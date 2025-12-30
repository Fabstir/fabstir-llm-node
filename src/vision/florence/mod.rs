// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Florence-2 integration for image description
//!
//! This module provides CPU-based image captioning using Florence-2 ONNX models.

pub mod model;
pub mod preprocessing;

pub use model::{DescriptionResult, DetectedObject, FlorenceModel, ImageAnalysis};
