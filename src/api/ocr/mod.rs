// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! OCR API endpoint module
//!
//! Provides POST /v1/ocr for extracting text from images.

pub mod handler;
pub mod request;
pub mod response;

pub use handler::ocr_handler;
pub use request::OcrRequest;
pub use response::{OcrResponse, TextRegion};
