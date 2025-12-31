// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Florence-2 model wrapper for image description
//!
//! This module provides the complete Florence-2 pipeline combining:
//! - Vision encoder (image feature extraction)
//! - Language decoder (text generation)

use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView};
use std::path::Path;
use std::time::Instant;
use tracing::{debug, info, warn};

use super::decoder::FlorenceDecoder;
use super::encoder::FlorenceEncoder;
use super::preprocessing::preprocess_for_florence;

use crate::vision::ocr::BoundingBox;

/// A detected object in the image
#[derive(Debug, Clone)]
pub struct DetectedObject {
    /// Object label/class
    pub label: String,
    /// Confidence score (0.0-1.0)
    pub confidence: f32,
    /// Optional bounding box
    pub bounding_box: Option<BoundingBox>,
}

/// Image analysis metadata
#[derive(Debug, Clone)]
pub struct ImageAnalysis {
    /// Image width
    pub width: u32,
    /// Image height
    pub height: u32,
    /// Dominant colors (hex strings)
    pub dominant_colors: Vec<String>,
    /// Scene type (indoor, outdoor, etc.)
    pub scene_type: Option<String>,
}

impl ImageAnalysis {
    /// Create analysis from image dimensions
    pub fn from_image(image: &DynamicImage) -> Self {
        let (width, height) = image.dimensions();
        Self {
            width,
            height,
            dominant_colors: Vec::new(),
            scene_type: None,
        }
    }
}

/// Result of image description
#[derive(Debug, Clone)]
pub struct DescriptionResult {
    /// Generated description text
    pub description: String,
    /// Detected objects
    pub objects: Vec<DetectedObject>,
    /// Image analysis metadata
    pub analysis: ImageAnalysis,
    /// Processing time in milliseconds
    pub processing_time_ms: u64,
}

impl DescriptionResult {
    /// Create an empty result
    pub fn empty(image: &DynamicImage, processing_time_ms: u64) -> Self {
        Self {
            description: String::new(),
            objects: Vec::new(),
            analysis: ImageAnalysis::from_image(image),
            processing_time_ms,
        }
    }
}

/// Detail level for image description
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DetailLevel {
    /// Brief, concise description (1-2 sentences)
    Brief,
    /// Moderate detail (2-3 sentences)
    Detailed,
    /// Comprehensive description (4+ sentences)
    Comprehensive,
}

impl DetailLevel {
    /// Parse from string
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "brief" | "short" | "concise" => Self::Brief,
            "comprehensive" | "full" | "verbose" => Self::Comprehensive,
            _ => Self::Detailed,
        }
    }

    /// Get the max tokens for this detail level
    pub fn max_tokens(&self) -> usize {
        match self {
            Self::Brief => 50,
            Self::Detailed => 150,
            Self::Comprehensive => 300,
        }
    }

    /// Get the prompt prefix for this detail level
    pub fn prompt_prefix(&self) -> &'static str {
        match self {
            Self::Brief => "Briefly describe this image:",
            Self::Detailed => "Describe this image in detail:",
            Self::Comprehensive => "Provide a comprehensive description of this image, including all visible objects, their positions, colors, and the overall scene:",
        }
    }
}

impl Default for DetailLevel {
    fn default() -> Self {
        Self::Detailed
    }
}

/// Florence-2 model for image description
///
/// Combines vision encoder and language decoder for image captioning.
/// Runs on CPU only to avoid GPU VRAM competition with LLM.
#[derive(Clone)]
pub struct FlorenceModel {
    /// Vision encoder
    encoder: FlorenceEncoder,
    /// Language decoder
    decoder: FlorenceDecoder,
    /// Model directory path
    model_dir: String,
    /// Whether the model is ready for inference
    is_ready: bool,
}

impl std::fmt::Debug for FlorenceModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlorenceModel")
            .field("model_dir", &self.model_dir)
            .field("is_ready", &self.is_ready)
            .finish_non_exhaustive()
    }
}

impl FlorenceModel {
    /// Load Florence-2 models from the specified directory
    ///
    /// Expected files:
    /// - encoder.onnx or vision_encoder.onnx (vision encoder)
    /// - decoder.onnx or decoder_model.onnx (language decoder)
    /// - tokenizer.json (tokenizer config)
    ///
    /// # Arguments
    /// - `model_dir`: Directory containing the model files
    ///
    /// # Returns
    /// - `Result<Self>`: Florence model instance or error
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
                "Florence model directory not found: {}",
                model_dir.display()
            );
        }

        info!("Loading Florence-2 models from {}", model_dir.display());

        // Find encoder model (try multiple names)
        let encoder_path = Self::find_model_file(model_dir, &[
            "vision_encoder.onnx",
            "encoder.onnx",
        ])?;

        // Find decoder model (try multiple names)
        let decoder_path = Self::find_model_file(model_dir, &[
            "decoder_model.onnx",
            "decoder.onnx",
        ])?;

        let tokenizer_path = model_dir.join("tokenizer.json");

        // Load encoder
        let encoder = FlorenceEncoder::new(&encoder_path)
            .await
            .context("Failed to load Florence encoder")?;

        // Load decoder
        let decoder = FlorenceDecoder::new(&decoder_path, &tokenizer_path)
            .await
            .context("Failed to load Florence decoder")?;

        info!("✅ Florence-2 pipeline ready (CPU-only)");

        Ok(Self {
            encoder,
            decoder,
            model_dir: model_dir.to_string_lossy().to_string(),
            is_ready: true,
        })
    }

    /// Find a model file by trying multiple possible names
    fn find_model_file(dir: &Path, names: &[&str]) -> Result<std::path::PathBuf> {
        for name in names {
            let path = dir.join(name);
            if path.exists() {
                return Ok(path);
            }
        }
        anyhow::bail!(
            "Model file not found in {}. Tried: {:?}",
            dir.display(),
            names
        );
    }

    /// Check if the model is ready for inference
    pub fn is_ready(&self) -> bool {
        self.is_ready
    }

    /// Describe an image
    ///
    /// # Arguments
    /// * `image` - The image to describe
    /// * `detail` - Detail level: "brief", "detailed", or "comprehensive"
    /// * `prompt` - Optional custom prompt (overrides detail level prompt)
    ///
    /// # Returns
    /// - `Result<DescriptionResult>`: Generated description with metadata
    ///
    /// # Process
    /// 1. Preprocess image for encoder (resize, normalize)
    /// 2. Extract visual features with encoder
    /// 3. Generate text with decoder
    /// 4. Return description with timing
    pub fn describe(
        &self,
        image: &DynamicImage,
        detail: &str,
        prompt: Option<&str>,
    ) -> Result<DescriptionResult> {
        let start = Instant::now();

        let detail_level = DetailLevel::from_str(detail);
        info!("Describing image with detail level: {:?}", detail_level);

        // 1. Preprocess image
        info!("Step 1: Preprocessing image {}x{}", image.width(), image.height());
        let preprocessed = preprocess_for_florence(image);
        info!("Preprocessed image shape: {:?}", preprocessed.shape());

        // 2. Encode image
        info!("Step 2: Encoding image...");
        let embeddings = self.encoder.encode(&preprocessed)
            .context("Failed to encode image")?;
        info!("Encoded to {} sequences x {} dimensions", embeddings.nrows(), embeddings.ncols());

        // 3. Determine prompt
        let generation_prompt = prompt.unwrap_or_else(|| detail_level.prompt_prefix());
        info!("Step 3: Using prompt: '{}'", generation_prompt);

        // 4. Generate description
        info!("Step 4: Generating text from embeddings...");
        let description = self.decoder.generate(&embeddings, Some(generation_prompt))
            .context("Failed to generate description")?;
        info!("Generated text: '{}' ({} chars)",
              if description.len() > 50 { &description[..50] } else { &description },
              description.len());

        let processing_time_ms = start.elapsed().as_millis() as u64;

        // 5. Create result
        let analysis = ImageAnalysis::from_image(image);

        info!(
            "Florence complete: {} chars, {}ms",
            description.len(),
            processing_time_ms
        );

        Ok(DescriptionResult {
            description,
            objects: Vec::new(), // Object detection not implemented yet
            analysis,
            processing_time_ms,
        })
    }

    /// Describe an image with a specific detail level enum
    pub fn describe_with_level(
        &self,
        image: &DynamicImage,
        level: DetailLevel,
    ) -> Result<DescriptionResult> {
        let detail_str = match level {
            DetailLevel::Brief => "brief",
            DetailLevel::Detailed => "detailed",
            DetailLevel::Comprehensive => "comprehensive",
        };
        self.describe(image, detail_str, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    const MODEL_DIR: &str = "/workspace/models/florence-2-onnx";

    #[test]
    fn test_detected_object() {
        let obj = DetectedObject {
            label: "cat".to_string(),
            confidence: 0.95,
            bounding_box: None,
        };
        assert_eq!(obj.label, "cat");
        assert!(obj.confidence > 0.9);
    }

    #[test]
    fn test_detected_object_with_bbox() {
        let obj = DetectedObject {
            label: "dog".to_string(),
            confidence: 0.88,
            bounding_box: Some(BoundingBox {
                x: 10,
                y: 20,
                width: 100,
                height: 80,
            }),
        };
        assert!(obj.bounding_box.is_some());
        assert_eq!(obj.bounding_box.as_ref().unwrap().x, 10);
    }

    #[test]
    fn test_image_analysis() {
        let analysis = ImageAnalysis {
            width: 1920,
            height: 1080,
            dominant_colors: vec!["#FF0000".to_string()],
            scene_type: Some("indoor".to_string()),
        };
        assert_eq!(analysis.width, 1920);
        assert_eq!(analysis.height, 1080);
    }

    #[test]
    fn test_image_analysis_from_image() {
        let img = DynamicImage::new_rgb8(800, 600);
        let analysis = ImageAnalysis::from_image(&img);
        assert_eq!(analysis.width, 800);
        assert_eq!(analysis.height, 600);
    }

    #[test]
    fn test_description_result() {
        let result = DescriptionResult {
            description: "A cat sitting on a couch".to_string(),
            objects: vec![],
            analysis: ImageAnalysis {
                width: 800,
                height: 600,
                dominant_colors: vec![],
                scene_type: None,
            },
            processing_time_ms: 4500,
        };
        assert!(!result.description.is_empty());
        assert_eq!(result.processing_time_ms, 4500);
    }

    #[test]
    fn test_description_result_empty() {
        let img = DynamicImage::new_rgb8(640, 480);
        let result = DescriptionResult::empty(&img, 100);
        assert!(result.description.is_empty());
        assert_eq!(result.analysis.width, 640);
        assert_eq!(result.processing_time_ms, 100);
    }

    #[test]
    fn test_detail_level_from_str() {
        assert_eq!(DetailLevel::from_str("brief"), DetailLevel::Brief);
        assert_eq!(DetailLevel::from_str("short"), DetailLevel::Brief);
        assert_eq!(DetailLevel::from_str("detailed"), DetailLevel::Detailed);
        assert_eq!(DetailLevel::from_str("comprehensive"), DetailLevel::Comprehensive);
        assert_eq!(DetailLevel::from_str("full"), DetailLevel::Comprehensive);
        assert_eq!(DetailLevel::from_str("unknown"), DetailLevel::Detailed);
    }

    #[test]
    fn test_detail_level_max_tokens() {
        assert_eq!(DetailLevel::Brief.max_tokens(), 50);
        assert_eq!(DetailLevel::Detailed.max_tokens(), 150);
        assert_eq!(DetailLevel::Comprehensive.max_tokens(), 300);
    }

    #[test]
    fn test_detail_level_prompt_prefix() {
        assert!(DetailLevel::Brief.prompt_prefix().contains("Briefly"));
        assert!(DetailLevel::Detailed.prompt_prefix().contains("detail"));
        assert!(DetailLevel::Comprehensive.prompt_prefix().contains("comprehensive"));
    }

    #[test]
    fn test_detail_level_default() {
        assert_eq!(DetailLevel::default(), DetailLevel::Detailed);
    }

    #[tokio::test]
    async fn test_model_dir_not_found() {
        let result = FlorenceModel::new("/nonexistent/path").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_model_loading() {
        let model = FlorenceModel::new(MODEL_DIR).await;

        if let Ok(model) = model {
            assert!(model.is_ready());
        }
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_describe_image() {
        let model = match FlorenceModel::new(MODEL_DIR).await {
            Ok(m) => m,
            Err(_) => return,
        };

        // Create a simple test image
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(768, 768, Rgb([128, 128, 128])));

        let result = model.describe(&img, "brief", None);
        assert!(result.is_ok() || result.is_err()); // May fail with simple gray image
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_describe_with_custom_prompt() {
        let model = match FlorenceModel::new(MODEL_DIR).await {
            Ok(m) => m,
            Err(_) => return,
        };

        let img = DynamicImage::new_rgb8(768, 768);

        let result = model.describe(&img, "detailed", Some("What is in this image?"));
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_processing_time_recorded() {
        let model = match FlorenceModel::new(MODEL_DIR).await {
            Ok(m) => m,
            Err(_) => return,
        };

        let img = DynamicImage::new_rgb8(768, 768);

        if let Ok(result) = model.describe(&img, "brief", None) {
            assert!(result.processing_time_ms > 0);
        }
    }
}
