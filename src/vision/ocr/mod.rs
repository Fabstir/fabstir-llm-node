// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! PaddleOCR integration for text extraction from images
//!
//! This module provides CPU-based OCR using PaddleOCR ONNX models.

pub mod model;
pub mod preprocessing;

pub use model::{BoundingBox, OcrResult, PaddleOcrModel, TextRegion};
