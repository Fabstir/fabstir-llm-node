// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Florence-2 integration for image description
//!
//! This module provides CPU-based image captioning using Florence-2 ONNX models.
//!
//! Components:
//! - `encoder` - Vision encoder for image feature extraction
//! - `decoder` - Language decoder for text generation
//! - `model` - Combined Florence-2 pipeline
//! - `preprocessing` - Image preprocessing for encoder input

pub mod decoder;
pub mod encoder;
pub mod model;
pub mod preprocessing;

pub use decoder::FlorenceDecoder;
pub use encoder::FlorenceEncoder;
pub use model::{DescriptionResult, DetectedObject, FlorenceModel, ImageAnalysis};
