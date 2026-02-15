// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! PaddleOCR model wrapper for text detection and recognition
//!
//! This module provides the complete OCR pipeline combining:
//! - Text detection (finding text regions in images)
//! - Text recognition (reading text from detected regions)

use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView};
use std::error::Error;
use std::path::Path;
use std::time::Instant;
use tracing::{debug, info, warn};

use super::detection::{OcrDetectionModel, TextBox};
use super::preprocessing::{
    preprocess_for_detection, preprocess_for_recognition, PreprocessInfo, OCR_INPUT_SIZE,
};
use super::recognition::OcrRecognitionModel;

/// Bounding box for detected text (in original image coordinates)
#[derive(Debug, Clone)]
pub struct BoundingBox {
    /// X coordinate of top-left corner
    pub x: u32,
    /// Y coordinate of top-left corner
    pub y: u32,
    /// Width of the bounding box
    pub width: u32,
    /// Height of the bounding box
    pub height: u32,
}

impl From<&TextBox> for BoundingBox {
    fn from(text_box: &TextBox) -> Self {
        Self {
            x: text_box.x.max(0.0) as u32,
            y: text_box.y.max(0.0) as u32,
            width: text_box.width.max(1.0) as u32,
            height: text_box.height.max(1.0) as u32,
        }
    }
}

/// A detected text region with bounding box
#[derive(Debug, Clone)]
pub struct TextRegion {
    /// Extracted text content
    pub text: String,
    /// Confidence score (0.0-1.0)
    pub confidence: f32,
    /// Bounding box location (in original image coordinates)
    pub bounding_box: BoundingBox,
}

/// Result of OCR processing
#[derive(Debug, Clone)]
pub struct OcrResult {
    /// Full extracted text (all regions combined)
    pub text: String,
    /// Average confidence score
    pub confidence: f32,
    /// Individual text regions
    pub regions: Vec<TextRegion>,
    /// Processing time in milliseconds
    pub processing_time_ms: u64,
}

impl OcrResult {
    /// Create an empty OCR result
    pub fn empty(processing_time_ms: u64) -> Self {
        Self {
            text: String::new(),
            confidence: 0.0,
            regions: Vec::new(),
            processing_time_ms,
        }
    }
}

/// PaddleOCR model for text extraction
///
/// Combines text detection and recognition models for end-to-end OCR.
/// Runs on CPU only to avoid GPU VRAM competition with LLM.
#[derive(Clone)]
pub struct PaddleOcrModel {
    /// Text detection model (finds text regions)
    detector: OcrDetectionModel,
    /// Text recognition model (reads text from regions)
    recognizer: OcrRecognitionModel,
    /// Minimum confidence threshold for detections
    confidence_threshold: f32,
    /// Model directory path
    model_dir: String,
    /// Whether the model is ready for inference
    is_ready: bool,
}

impl std::fmt::Debug for PaddleOcrModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaddleOcrModel")
            .field("model_dir", &self.model_dir)
            .field("confidence_threshold", &self.confidence_threshold)
            .field("is_ready", &self.is_ready)
            .finish_non_exhaustive()
    }
}

impl PaddleOcrModel {
    /// Load PaddleOCR models from the specified directory
    ///
    /// Expected files:
    /// - det_model.onnx (text detection)
    /// - rec_model.onnx (text recognition)
    /// - ppocr_keys_v1.txt (character dictionary)
    ///
    /// # Arguments
    /// - `model_dir`: Directory containing the model files
    ///
    /// # Returns
    /// - `Result<Self>`: OCR model instance or error
    ///
    /// # Errors
    /// Returns error if:
    /// - Model directory doesn't exist
    /// - Required model files are missing
    /// - ONNX Runtime initialization fails
    pub async fn new<P: AsRef<Path>>(model_dir: P) -> Result<Self> {
        let model_dir = model_dir.as_ref();

        // Validate directory exists
        if !model_dir.exists() {
            anyhow::bail!(
                "PaddleOCR model directory not found: {}",
                model_dir.display()
            );
        }

        let det_path = model_dir.join("det_model.onnx");
        let rec_path = model_dir.join("rec_model.onnx");

        // Try to find dictionary file (English or Chinese)
        let dict_path = if model_dir.join("en_dict.txt").exists() {
            model_dir.join("en_dict.txt")
        } else {
            model_dir.join("ppocr_keys_v1.txt")
        };

        info!("Loading PaddleOCR models from {}", model_dir.display());
        info!("Using dictionary: {}", dict_path.display());

        // Load detection model
        let detector = OcrDetectionModel::new(&det_path)
            .await
            .context("Failed to load OCR detection model")?;

        // Load recognition model
        let recognizer = OcrRecognitionModel::new(&rec_path, &dict_path)
            .await
            .context("Failed to load OCR recognition model")?;

        info!("✅ PaddleOCR pipeline ready (CPU-only)");

        Ok(Self {
            detector,
            recognizer,
            confidence_threshold: 0.5,
            model_dir: model_dir.to_string_lossy().to_string(),
            is_ready: true,
        })
    }

    /// Set the confidence threshold for filtering detections
    pub fn with_confidence_threshold(mut self, threshold: f32) -> Self {
        self.confidence_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Get current confidence threshold
    pub fn confidence_threshold(&self) -> f32 {
        self.confidence_threshold
    }

    /// Check if the model is ready for inference
    pub fn is_ready(&self) -> bool {
        self.is_ready
    }

    /// Process an image and extract text
    ///
    /// # Arguments
    /// - `image`: Image to process
    ///
    /// # Returns
    /// - `Result<OcrResult>`: OCR result with detected text regions
    ///
    /// # Process
    /// 1. Preprocess image for detection (resize, normalize)
    /// 2. Detect text regions
    /// 3. For each region above confidence threshold:
    ///    a. Crop the text box from original image
    ///    b. Preprocess for recognition
    ///    c. Recognize text
    /// 4. Aggregate results
    pub fn process(&self, image: &DynamicImage) -> Result<OcrResult> {
        let start = Instant::now();

        // Get preprocessing info for coordinate mapping
        let preprocess_info = PreprocessInfo::new(image, OCR_INPUT_SIZE);

        // 1. Preprocess image for detection
        let det_input = preprocess_for_detection(image);
        debug!("Detection input shape: {:?}", det_input.shape());

        // 2. Detect text boxes
        let text_boxes = self.detector.detect(&det_input)?;
        info!("🔍 Detection found {} text regions", text_boxes.len());
        for (i, tb) in text_boxes.iter().enumerate() {
            info!(
                "  Region {}: x={:.0}, y={:.0}, w={:.0}, h={:.0}, conf={:.2}%",
                i,
                tb.x,
                tb.y,
                tb.width,
                tb.height,
                tb.confidence * 100.0
            );
        }

        // 3. For each text box, crop and recognize
        let mut regions = Vec::new();

        for text_box in &text_boxes {
            // Filter by confidence
            if text_box.confidence < self.confidence_threshold {
                debug!(
                    "Skipping low-confidence region: {:.2}%",
                    text_box.confidence * 100.0
                );
                continue;
            }

            // Map coordinates back to original image space
            let (orig_x, orig_y) = preprocess_info.map_to_original(text_box.x, text_box.y);
            let (orig_x2, orig_y2) = preprocess_info
                .map_to_original(text_box.x + text_box.width, text_box.y + text_box.height);

            // Calculate original dimensions
            let orig_width = (orig_x2 - orig_x).abs();
            let orig_height = (orig_y2 - orig_y).abs();

            // Skip if region is too small
            if orig_width < 2.0 || orig_height < 2.0 {
                debug!("Skipping tiny region: {:.1}x{:.1}", orig_width, orig_height);
                continue;
            }

            // Crop the text box from original image
            let cropped = crop_text_box(
                image,
                orig_x.max(0.0) as u32,
                orig_y.max(0.0) as u32,
                orig_width.max(1.0) as u32,
                orig_height.max(1.0) as u32,
            );

            // Preprocess for recognition
            let rec_input = preprocess_for_recognition(&cropped);

            // Recognize text
            info!("🔤 Recognition input shape: {:?}", rec_input.shape());
            match self.recognizer.recognize(&rec_input) {
                Ok(recognized) => {
                    // Skip empty results
                    if recognized.is_empty() {
                        debug!("Skipping empty recognition result");
                        continue;
                    }

                    debug!(
                        "Recognized: '{}' (confidence: {:.2}%)",
                        recognized.text,
                        recognized.confidence * 100.0
                    );

                    regions.push(TextRegion {
                        text: recognized.text,
                        confidence: recognized.confidence,
                        bounding_box: BoundingBox {
                            x: orig_x.max(0.0) as u32,
                            y: orig_y.max(0.0) as u32,
                            width: orig_width.max(1.0) as u32,
                            height: orig_height.max(1.0) as u32,
                        },
                    });
                }
                Err(e) => {
                    warn!("Recognition failed for region: {:?}", e);
                    // Log the full error chain
                    let mut source = e.source();
                    while let Some(s) = source {
                        warn!("  Caused by: {}", s);
                        source = s.source();
                    }
                    continue;
                }
            }
        }

        // 4. Aggregate results
        let processing_time_ms = start.elapsed().as_millis() as u64;

        if regions.is_empty() {
            debug!("No text detected in image");
            return Ok(OcrResult::empty(processing_time_ms));
        }

        // Sort regions by position (top-to-bottom, left-to-right)
        regions.sort_by(|a, b| {
            let y_cmp = a.bounding_box.y.cmp(&b.bounding_box.y);
            if y_cmp == std::cmp::Ordering::Equal {
                a.bounding_box.x.cmp(&b.bounding_box.x)
            } else {
                y_cmp
            }
        });

        // Combine text from all regions
        let full_text = regions
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        // Calculate average confidence
        let total_confidence: f32 = regions.iter().map(|r| r.confidence).sum();
        let avg_confidence = total_confidence / regions.len() as f32;

        info!(
            "OCR complete: {} regions, {} chars, {:.2}% confidence, {}ms",
            regions.len(),
            full_text.len(),
            avg_confidence * 100.0,
            processing_time_ms
        );

        Ok(OcrResult {
            text: full_text,
            confidence: avg_confidence,
            regions,
            processing_time_ms,
        })
    }
}

/// Crop a text box region from an image
///
/// Handles edge cases where the box extends beyond image boundaries.
fn crop_text_box(image: &DynamicImage, x: u32, y: u32, width: u32, height: u32) -> DynamicImage {
    let (img_w, img_h) = image.dimensions();

    // Clamp coordinates to image boundaries
    let x = x.min(img_w.saturating_sub(1));
    let y = y.min(img_h.saturating_sub(1));
    let width = width.min(img_w.saturating_sub(x)).max(1);
    let height = height.min(img_h.saturating_sub(y)).max(1);

    image.crop_imm(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    const MODEL_DIR: &str = "/workspace/models/paddleocr-onnx";

    #[test]
    fn test_bounding_box() {
        let bbox = BoundingBox {
            x: 10,
            y: 20,
            width: 100,
            height: 50,
        };
        assert_eq!(bbox.x, 10);
        assert_eq!(bbox.width, 100);
    }

    #[test]
    fn test_bounding_box_from_text_box() {
        let text_box = TextBox {
            x: 10.5,
            y: 20.7,
            width: 100.3,
            height: 50.8,
            confidence: 0.95,
            polygon: None,
        };

        let bbox = BoundingBox::from(&text_box);
        assert_eq!(bbox.x, 10);
        assert_eq!(bbox.y, 20);
        assert_eq!(bbox.width, 100);
        assert_eq!(bbox.height, 50);
    }

    #[test]
    fn test_bounding_box_negative_values() {
        let text_box = TextBox {
            x: -5.0,
            y: -3.0,
            width: 100.0,
            height: 50.0,
            confidence: 0.9,
            polygon: None,
        };

        let bbox = BoundingBox::from(&text_box);
        assert_eq!(bbox.x, 0); // Clamped to 0
        assert_eq!(bbox.y, 0);
    }

    #[test]
    fn test_text_region() {
        let region = TextRegion {
            text: "Hello".to_string(),
            confidence: 0.95,
            bounding_box: BoundingBox {
                x: 0,
                y: 0,
                width: 50,
                height: 20,
            },
        };
        assert_eq!(region.text, "Hello");
        assert!(region.confidence > 0.9);
    }

    #[test]
    fn test_ocr_result() {
        let result = OcrResult {
            text: "Hello World".to_string(),
            confidence: 0.92,
            regions: vec![],
            processing_time_ms: 150,
        };
        assert_eq!(result.text, "Hello World");
        assert_eq!(result.processing_time_ms, 150);
    }

    #[test]
    fn test_ocr_result_empty() {
        let result = OcrResult::empty(100);
        assert!(result.text.is_empty());
        assert_eq!(result.confidence, 0.0);
        assert!(result.regions.is_empty());
        assert_eq!(result.processing_time_ms, 100);
    }

    #[test]
    fn test_crop_text_box_normal() {
        let img = DynamicImage::new_rgb8(100, 100);
        let cropped = crop_text_box(&img, 10, 10, 50, 50);
        assert_eq!(cropped.dimensions(), (50, 50));
    }

    #[test]
    fn test_crop_text_box_at_edge() {
        let img = DynamicImage::new_rgb8(100, 100);
        let cropped = crop_text_box(&img, 80, 80, 50, 50);
        // Should be clamped to image boundaries
        assert!(cropped.width() <= 20);
        assert!(cropped.height() <= 20);
    }

    #[test]
    fn test_crop_text_box_beyond_bounds() {
        let img = DynamicImage::new_rgb8(100, 100);
        let cropped = crop_text_box(&img, 200, 200, 50, 50);
        // Should still return a valid image (edge case handling)
        assert!(cropped.width() >= 1);
        assert!(cropped.height() >= 1);
    }

    #[test]
    fn test_confidence_threshold_builder() {
        // Test clamping
        let threshold = 0.7_f32.clamp(0.0, 1.0);
        assert_eq!(threshold, 0.7);

        let clamped_high = 1.5_f32.clamp(0.0, 1.0);
        assert_eq!(clamped_high, 1.0);

        let clamped_low = (-0.5_f32).clamp(0.0, 1.0);
        assert_eq!(clamped_low, 0.0);
    }

    #[tokio::test]
    async fn test_model_dir_not_found() {
        let result = PaddleOcrModel::new("/nonexistent/path").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_model_loading() {
        let model = PaddleOcrModel::new(MODEL_DIR).await;

        if let Ok(model) = model {
            assert!(model.is_ready());
            assert_eq!(model.confidence_threshold(), 0.5);
        }
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_model_with_custom_threshold() {
        let model = match PaddleOcrModel::new(MODEL_DIR).await {
            Ok(m) => m,
            Err(_) => return,
        };

        let model = model.with_confidence_threshold(0.7);
        assert_eq!(model.confidence_threshold(), 0.7);
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_process_empty_image() {
        let model = match PaddleOcrModel::new(MODEL_DIR).await {
            Ok(m) => m,
            Err(_) => return,
        };

        // Create a blank image
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(640, 640, Rgb([255, 255, 255])));

        let result = model.process(&img);
        assert!(result.is_ok());

        let ocr_result = result.unwrap();
        // Empty image should have no or very low confidence text
        assert!(ocr_result.regions.is_empty() || ocr_result.confidence < 0.5);
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_process_returns_timing() {
        let model = match PaddleOcrModel::new(MODEL_DIR).await {
            Ok(m) => m,
            Err(_) => return,
        };

        let img = DynamicImage::new_rgb8(100, 100);
        let result = model.process(&img);
        assert!(result.is_ok());

        let ocr_result = result.unwrap();
        // Processing time should be recorded
        assert!(ocr_result.processing_time_ms > 0);
    }

    #[test]
    fn test_region_sorting() {
        // Test that regions are sorted top-to-bottom, left-to-right
        let mut regions = vec![
            TextRegion {
                text: "C".to_string(),
                confidence: 0.9,
                bounding_box: BoundingBox {
                    x: 0,
                    y: 100,
                    width: 50,
                    height: 20,
                },
            },
            TextRegion {
                text: "A".to_string(),
                confidence: 0.9,
                bounding_box: BoundingBox {
                    x: 0,
                    y: 0,
                    width: 50,
                    height: 20,
                },
            },
            TextRegion {
                text: "B".to_string(),
                confidence: 0.9,
                bounding_box: BoundingBox {
                    x: 100,
                    y: 0,
                    width: 50,
                    height: 20,
                },
            },
        ];

        regions.sort_by(|a, b| {
            let y_cmp = a.bounding_box.y.cmp(&b.bounding_box.y);
            if y_cmp == std::cmp::Ordering::Equal {
                a.bounding_box.x.cmp(&b.bounding_box.x)
            } else {
                y_cmp
            }
        });

        assert_eq!(regions[0].text, "A"); // y=0, x=0
        assert_eq!(regions[1].text, "B"); // y=0, x=100
        assert_eq!(regions[2].text, "C"); // y=100, x=0
    }
}
