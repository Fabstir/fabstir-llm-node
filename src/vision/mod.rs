// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Vision processing module for CPU-based image analysis
//!
//! This module provides:
//! - OCR (Optical Character Recognition) via PaddleOCR
//! - Image description via Florence-2
//!
//! Both run on CPU only to avoid competing with LLM for GPU VRAM.

pub mod florence;
pub mod image_utils;
pub mod model_manager;
pub mod ocr;

pub use image_utils::{decode_base64_image, decode_image_bytes, detect_format, ImageError, ImageInfo};
pub use model_manager::{VisionModelConfig, VisionModelInfo, VisionModelManager};
