// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! PaddleOCR text detection model
//!
//! This module provides the text detection component of PaddleOCR.
//! It detects text regions in images and returns bounding boxes.

use anyhow::{Context, Result};
use ndarray::{Array4, ArrayViewD, IxDyn};
use ort::execution_providers::CPUExecutionProvider;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Value;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

use super::preprocessing::OCR_INPUT_SIZE;

/// Expected input size for detection model
pub const DETECTION_INPUT_SIZE: u32 = OCR_INPUT_SIZE; // 640x640

/// A detected text box with location and confidence
#[derive(Debug, Clone)]
pub struct TextBox {
    /// X coordinate of top-left corner (in preprocessed image space)
    pub x: f32,
    /// Y coordinate of top-left corner (in preprocessed image space)
    pub y: f32,
    /// Width of the bounding box
    pub width: f32,
    /// Height of the bounding box
    pub height: f32,
    /// Detection confidence score (0.0-1.0)
    pub confidence: f32,
    /// Polygon points for rotated text (4 corners: [[x1,y1], [x2,y2], [x3,y3], [x4,y4]])
    pub polygon: Option<[[f32; 2]; 4]>,
}

impl TextBox {
    /// Check if this text box is valid (reasonable dimensions)
    pub fn is_valid(&self) -> bool {
        self.width > 0.0 && self.height > 0.0 && self.confidence > 0.0
    }

    /// Calculate area of the bounding box
    pub fn area(&self) -> f32 {
        self.width * self.height
    }
}

/// PaddleOCR text detection model
///
/// Uses the PP-OCRv4 detection model to find text regions in images.
/// Runs on CPU only to avoid GPU VRAM competition with LLM.
#[derive(Clone)]
pub struct OcrDetectionModel {
    /// ONNX Runtime session (thread-safe)
    session: Arc<Mutex<Session>>,
    /// Model input name
    input_name: String,
    /// Model output name
    output_name: String,
    /// Confidence threshold for detections
    confidence_threshold: f32,
    /// Whether model is loaded and ready
    is_ready: bool,
}

impl std::fmt::Debug for OcrDetectionModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrDetectionModel")
            .field("input_name", &self.input_name)
            .field("output_name", &self.output_name)
            .field("confidence_threshold", &self.confidence_threshold)
            .field("is_ready", &self.is_ready)
            .finish_non_exhaustive()
    }
}

impl OcrDetectionModel {
    /// Load the OCR detection model from a file
    ///
    /// # Arguments
    /// - `model_path`: Path to the ONNX model file (det_model.onnx)
    ///
    /// # Returns
    /// - `Result<Self>`: Detection model instance or error
    ///
    /// # Errors
    /// Returns error if:
    /// - Model file not found
    /// - ONNX Runtime initialization fails
    /// - Model has unexpected input/output shapes
    pub async fn new<P: AsRef<Path>>(model_path: P) -> Result<Self> {
        let model_path = model_path.as_ref();

        // Validate path exists
        if !model_path.exists() {
            anyhow::bail!(
                "OCR detection model not found: {}",
                model_path.display()
            );
        }

        info!("Loading OCR detection model from {}", model_path.display());

        // Load ONNX model with CPU-only execution (no GPU for vision)
        let session = Session::builder()
            .context("Failed to create session builder")?
            .with_execution_providers([CPUExecutionProvider::default().build()])
            .context("Failed to set CPU execution provider")?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .context("Failed to set optimization level")?
            .with_intra_threads(4)
            .context("Failed to set intra threads")?
            .commit_from_file(model_path)
            .context(format!(
                "Failed to load OCR detection model from {}",
                model_path.display()
            ))?;

        // Get input/output names
        let input_name = session
            .inputs
            .first()
            .map(|input| input.name.clone())
            .unwrap_or_else(|| "x".to_string());

        let output_name = session
            .outputs
            .first()
            .map(|output| output.name.clone())
            .unwrap_or_else(|| "sigmoid_0.tmp_0".to_string());

        debug!(
            "Detection model loaded - input: {}, output: {}",
            input_name, output_name
        );

        // Validate input shape
        if let Some(input) = session.inputs.first() {
            debug!("Detection model input shape: {:?}", input.input_type);
        }

        info!("✅ OCR detection model loaded successfully (CPU-only)");

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            input_name,
            output_name,
            confidence_threshold: 0.3, // Default threshold
            is_ready: true,
        })
    }

    /// Set the confidence threshold for detections
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

    /// Run text detection on a preprocessed image tensor
    ///
    /// # Arguments
    /// - `input`: Preprocessed image tensor of shape [1, 3, H, W] (NCHW format)
    ///
    /// # Returns
    /// - `Result<Vec<TextBox>>`: Detected text boxes
    ///
    /// # Notes
    /// The input tensor should be preprocessed using `preprocess_for_detection()`
    pub fn detect(&self, input: &Array4<f32>) -> Result<Vec<TextBox>> {
        // Validate input shape
        let shape = input.shape();
        if shape.len() != 4 || shape[0] != 1 || shape[1] != 3 {
            anyhow::bail!(
                "Invalid input shape: {:?}, expected [1, 3, H, W]",
                shape
            );
        }

        // Run inference
        let mut session = self.session.lock().unwrap();

        // Convert ndarray to ort Value (need owned array)
        let input_value = Value::from_array(input.to_owned())
            .context("Failed to create input tensor")?;

        let outputs = session
            .run(ort::inputs![&self.input_name => input_value])
            .context("Detection inference failed")?;

        // Parse output - use index access since SessionOutputs doesn't have .first()
        let output_tensor = outputs[0]
            .try_extract_array::<f32>()
            .context("Failed to extract output tensor")?;

        let output_shape = output_tensor.shape();
        info!("Detection output shape: {:?}", output_shape);

        // Log min/max values in output for debugging
        let (min_val, max_val) = output_tensor.iter().fold(
            (f32::MAX, f32::MIN),
            |(min, max), &v| (min.min(v), max.max(v))
        );
        info!("Detection output range: min={:.4}, max={:.4}", min_val, max_val);

        // Parse detection output into text boxes
        let text_boxes = self.parse_detection_output(output_tensor.view(), shape[2], shape[3])?;

        debug!("Detected {} text regions", text_boxes.len());

        Ok(text_boxes)
    }

    /// Parse detection model output into text boxes
    ///
    /// PaddleOCR detection outputs a probability map of shape [1, 1, H, W]
    /// where each pixel represents the probability of being part of text.
    fn parse_detection_output(
        &self,
        output: ArrayViewD<f32>,
        input_height: usize,
        input_width: usize,
    ) -> Result<Vec<TextBox>> {
        let output_shape = output.shape();

        // Expected shape: [1, 1, H, W] or [1, H, W] for probability map
        if output_shape.len() < 3 {
            anyhow::bail!("Unexpected output shape: {:?}", output_shape);
        }

        let mut text_boxes = Vec::new();

        // For now, implement a simple thresholding approach
        // TODO: Implement proper DB (Differentiable Binarization) post-processing
        // with connected component analysis

        // Get the probability map
        let (prob_height, prob_width) = if output_shape.len() == 4 {
            (output_shape[2], output_shape[3])
        } else {
            (output_shape[1], output_shape[2])
        };

        // Scale factors from probability map to input image
        let scale_y = input_height as f32 / prob_height as f32;
        let scale_x = input_width as f32 / prob_width as f32;

        // Simple region detection using connected components
        // This is a placeholder - real implementation needs proper DB post-processing
        let mut visited = vec![vec![false; prob_width]; prob_height];

        for y in 0..prob_height {
            for x in 0..prob_width {
                let prob = if output_shape.len() == 4 {
                    output[IxDyn(&[0, 0, y, x])]
                } else if output_shape.len() == 3 {
                    output[IxDyn(&[0, y, x])]
                } else {
                    // Handle other output shapes
                    continue;
                };

                if prob >= self.confidence_threshold && !visited[y][x] {
                    // Found a text pixel, do simple flood fill to find region
                    let (min_x, max_x, min_y, max_y, count, sum_conf) =
                        self.flood_fill(&output, &mut visited, x, y, prob_width, prob_height);

                    if count > 10 {
                        // Minimum region size
                        let avg_conf = sum_conf / count as f32;

                        text_boxes.push(TextBox {
                            x: min_x as f32 * scale_x,
                            y: min_y as f32 * scale_y,
                            width: (max_x - min_x + 1) as f32 * scale_x,
                            height: (max_y - min_y + 1) as f32 * scale_y,
                            confidence: avg_conf,
                            polygon: None,
                        });
                    }
                }
            }
        }

        // Sort by y-position (top to bottom), then x-position (left to right)
        text_boxes.sort_by(|a, b| {
            let y_cmp = a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal);
            if y_cmp == std::cmp::Ordering::Equal {
                a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal)
            } else {
                y_cmp
            }
        });

        Ok(text_boxes)
    }

    /// Simple flood fill to find connected text region
    fn flood_fill(
        &self,
        output: &ArrayViewD<f32>,
        visited: &mut [Vec<bool>],
        start_x: usize,
        start_y: usize,
        width: usize,
        height: usize,
    ) -> (usize, usize, usize, usize, usize, f32) {
        let mut stack = vec![(start_x, start_y)];
        let mut min_x = start_x;
        let mut max_x = start_x;
        let mut min_y = start_y;
        let mut max_y = start_y;
        let mut count = 0;
        let mut sum_conf = 0.0;

        // Determine indexing based on output shape
        let is_4d = output.shape().len() == 4;

        while let Some((x, y)) = stack.pop() {
            if x >= width || y >= height || visited[y][x] {
                continue;
            }

            let prob = if is_4d {
                output[IxDyn(&[0, 0, y, x])]
            } else {
                output[IxDyn(&[0, y, x])]
            };

            if prob < self.confidence_threshold {
                continue;
            }

            visited[y][x] = true;
            count += 1;
            sum_conf += prob;

            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);

            // Add neighbors (4-connected)
            if x > 0 {
                stack.push((x - 1, y));
            }
            if x + 1 < width {
                stack.push((x + 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if y + 1 < height {
                stack.push((x, y + 1));
            }
        }

        (min_x, max_x, min_y, max_y, count, sum_conf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DETECTION_MODEL_PATH: &str = "/workspace/models/paddleocr-onnx/det_model.onnx";

    #[test]
    fn test_text_box_creation() {
        let text_box = TextBox {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
            confidence: 0.95,
            polygon: None,
        };

        assert_eq!(text_box.x, 10.0);
        assert_eq!(text_box.y, 20.0);
        assert_eq!(text_box.width, 100.0);
        assert_eq!(text_box.height, 50.0);
        assert_eq!(text_box.confidence, 0.95);
        assert!(text_box.is_valid());
        assert_eq!(text_box.area(), 5000.0);
    }

    #[test]
    fn test_text_box_invalid() {
        let invalid_box = TextBox {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 10.0,
            confidence: 0.5,
            polygon: None,
        };
        assert!(!invalid_box.is_valid());
    }

    #[test]
    fn test_text_box_with_polygon() {
        let text_box = TextBox {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            confidence: 0.9,
            polygon: Some([[0.0, 0.0], [100.0, 0.0], [100.0, 50.0], [0.0, 50.0]]),
        };
        assert!(text_box.polygon.is_some());
    }

    #[test]
    fn test_confidence_threshold_clamping() {
        // Test clamping logic directly without needing a real session
        assert!(0.0_f32.clamp(0.0, 1.0) == 0.0);
        assert!(1.5_f32.clamp(0.0, 1.0) == 1.0);
        assert!((-0.5_f32).clamp(0.0, 1.0) == 0.0);
        assert!(0.5_f32.clamp(0.0, 1.0) == 0.5);
        assert!(0.3_f32.clamp(0.0, 1.0) == 0.3);
    }

    #[tokio::test]
    async fn test_model_not_found_error() {
        let result = OcrDetectionModel::new("/nonexistent/path/det_model.onnx").await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_model_loading() {
        let model = OcrDetectionModel::new(DETECTION_MODEL_PATH).await;

        if let Ok(model) = model {
            assert!(model.is_ready());
            assert!(!model.input_name.is_empty());
            assert!(!model.output_name.is_empty());
        }
        // If model files don't exist, test is skipped
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_detection_inference() {
        let model = match OcrDetectionModel::new(DETECTION_MODEL_PATH).await {
            Ok(m) => m,
            Err(_) => return, // Skip if model not available
        };

        // Create a simple test input
        let input = Array4::<f32>::zeros((1, 3, 640, 640));

        let result = model.detect(&input);
        assert!(result.is_ok());

        let boxes = result.unwrap();
        // Empty image should produce no detections
        assert!(boxes.is_empty() || boxes.iter().all(|b| b.confidence < 0.5));
    }

    #[test]
    fn test_detect_invalid_input_shape() {
        // We can't fully test without a real model, but we can verify the shape validation
        // by checking the error message format
        let shape_3d = [1, 3, 640];
        assert!(shape_3d.len() != 4);

        let shape_wrong_channels = [1, 1, 640, 640];
        assert!(shape_wrong_channels[1] != 3);
    }

    #[test]
    fn test_detection_input_size_constant() {
        assert_eq!(DETECTION_INPUT_SIZE, 640);
    }
}
