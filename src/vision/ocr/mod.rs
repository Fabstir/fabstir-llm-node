// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! PaddleOCR integration for text extraction from images
//!
//! This module provides CPU-based OCR using PaddleOCR ONNX models.
//!
//! Components:
//! - `detection` - Text region detection (PP-OCRv4)
//! - `recognition` - Text recognition from detected regions
//! - `preprocessing` - Image preprocessing for models
//! - `model` - Combined OCR pipeline

pub mod detection;
pub mod model;
pub mod preprocessing;
pub mod recognition;

pub use detection::{OcrDetectionModel, TextBox};
pub use model::{BoundingBox, OcrResult, PaddleOcrModel, TextRegion};
pub use recognition::{OcrRecognitionModel, RecognizedText};
