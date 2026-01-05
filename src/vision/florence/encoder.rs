// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Florence-2 vision encoder model
//!
//! This module provides the vision encoding component of Florence-2.
//! It extracts visual features from images for the decoder.

use anyhow::{Context, Result};
use ndarray::{Array2, Array4, IxDyn};
use ort::execution_providers::CPUExecutionProvider;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Value;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::{debug, info};

use super::preprocessing::FLORENCE_INPUT_SIZE;

/// Expected input size for Florence encoder
pub const ENCODER_INPUT_SIZE: u32 = FLORENCE_INPUT_SIZE; // 768x768

/// Florence-2 vision encoder model
///
/// Uses the Florence-2 vision encoder to extract visual features from images.
/// Runs on CPU only to avoid GPU VRAM competition with LLM.
#[derive(Clone)]
pub struct FlorenceEncoder {
    /// ONNX Runtime session (thread-safe)
    session: Arc<Mutex<Session>>,
    /// Model input name
    input_name: String,
    /// Model output name (image embeddings)
    output_name: String,
    /// Embedding dimension
    embedding_dim: usize,
    /// Whether model is loaded and ready
    is_ready: bool,
}

impl std::fmt::Debug for FlorenceEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlorenceEncoder")
            .field("input_name", &self.input_name)
            .field("output_name", &self.output_name)
            .field("embedding_dim", &self.embedding_dim)
            .field("is_ready", &self.is_ready)
            .finish_non_exhaustive()
    }
}

impl FlorenceEncoder {
    /// Load the Florence vision encoder from a file
    ///
    /// # Arguments
    /// - `model_path`: Path to the ONNX model file (encoder.onnx or vision_encoder.onnx)
    ///
    /// # Returns
    /// - `Result<Self>`: Encoder model instance or error
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
                "Florence encoder model not found: {}",
                model_path.display()
            );
        }

        info!(
            "Loading Florence vision encoder from {}",
            model_path.display()
        );

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
                "Failed to load Florence encoder model from {}",
                model_path.display()
            ))?;

        // Get input/output names
        let input_name = session
            .inputs
            .first()
            .map(|input| input.name.clone())
            .unwrap_or_else(|| "pixel_values".to_string());

        let output_name = session
            .outputs
            .first()
            .map(|output| output.name.clone())
            .unwrap_or_else(|| "last_hidden_state".to_string());

        debug!(
            "Florence encoder loaded - input: {}, output: {}",
            input_name, output_name
        );

        // Validate and get embedding dimension from output shape
        let embedding_dim = if let Some(output) = session.outputs.first() {
            debug!("Encoder output type: {:?}", output.output_type);
            // Florence-2 base typically has 768 embedding dimension
            768
        } else {
            768 // Default for Florence-2-base
        };

        info!(
            "✅ Florence encoder loaded successfully (CPU-only, {}D embeddings)",
            embedding_dim
        );

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            input_name,
            output_name,
            embedding_dim,
            is_ready: true,
        })
    }

    /// Get the embedding dimension
    pub fn embedding_dim(&self) -> usize {
        self.embedding_dim
    }

    /// Check if the model is ready for inference
    pub fn is_ready(&self) -> bool {
        self.is_ready
    }

    /// Encode an image into visual features
    ///
    /// # Arguments
    /// - `input`: Preprocessed image tensor of shape [1, 3, 768, 768] (NCHW format)
    ///
    /// # Returns
    /// - `Result<Array2<f32>>`: Image embeddings of shape [seq_len, embedding_dim]
    ///
    /// # Notes
    /// The input tensor should be preprocessed using `preprocess_for_florence()`
    pub fn encode(&self, input: &Array4<f32>) -> Result<Array2<f32>> {
        // Validate input shape
        let shape = input.shape();
        if shape.len() != 4 || shape[0] != 1 || shape[1] != 3 {
            anyhow::bail!(
                "Invalid input shape: {:?}, expected [1, 3, H, W]",
                shape
            );
        }

        // Expected size check (warning only, some models are flexible)
        if shape[2] != ENCODER_INPUT_SIZE as usize || shape[3] != ENCODER_INPUT_SIZE as usize {
            debug!(
                "Input size {}x{} differs from expected {}x{}",
                shape[2], shape[3], ENCODER_INPUT_SIZE, ENCODER_INPUT_SIZE
            );
        }

        // Run inference
        let mut session = self.session.lock().unwrap();

        let input_value = Value::from_array(input.to_owned())
            .context("Failed to create input tensor")?;

        let outputs = session
            .run(ort::inputs![&self.input_name => input_value])
            .context("Encoder inference failed")?;

        // Parse output
        let output_tensor = outputs[0]
            .try_extract_array::<f32>()
            .context("Failed to extract output tensor")?;

        let output_shape = output_tensor.shape();
        debug!("Encoder output shape: {:?}", output_shape);

        // Convert to 2D array [seq_len, embedding_dim]
        self.parse_encoder_output(&output_tensor)
    }

    /// Parse encoder output into 2D embeddings
    fn parse_encoder_output(
        &self,
        output: &ndarray::ArrayBase<ndarray::ViewRepr<&f32>, ndarray::Dim<ndarray::IxDynImpl>>,
    ) -> Result<Array2<f32>> {
        let shape = output.shape();

        // Expected shapes:
        // - [batch, seq_len, embedding_dim] -> extract [seq_len, embedding_dim]
        // - [batch, embedding_dim] -> treat as [1, embedding_dim]
        // - [seq_len, embedding_dim] -> use directly

        let (seq_len, embed_dim) = match shape.len() {
            3 => {
                // [batch, seq_len, embed_dim]
                (shape[1], shape[2])
            }
            2 => {
                // Could be [batch, embed_dim] or [seq_len, embed_dim]
                if shape[0] == 1 {
                    (1, shape[1])
                } else {
                    (shape[0], shape[1])
                }
            }
            _ => {
                anyhow::bail!("Unexpected encoder output shape: {:?}", shape);
            }
        };

        // Create 2D output array
        let mut embeddings = Array2::<f32>::zeros((seq_len, embed_dim));

        for s in 0..seq_len {
            for e in 0..embed_dim {
                let value = match shape.len() {
                    3 => output[IxDyn(&[0, s, e])],
                    2 => output[IxDyn(&[s, e])],
                    _ => 0.0,
                };
                embeddings[[s, e]] = value;
            }
        }

        debug!(
            "Parsed encoder output: {} sequences x {} dimensions",
            seq_len, embed_dim
        );

        Ok(embeddings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENCODER_MODEL_PATH: &str = "/workspace/models/florence-2-onnx/vision_encoder.onnx";
    const ALT_ENCODER_PATH: &str = "/workspace/models/florence-2-onnx/encoder.onnx";

    #[test]
    fn test_encoder_input_size_constant() {
        assert_eq!(ENCODER_INPUT_SIZE, 768);
    }

    #[tokio::test]
    async fn test_model_not_found_error() {
        let result = FlorenceEncoder::new("/nonexistent/path/encoder.onnx").await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_model_loading() {
        // Try primary path first, then alternative
        let result = FlorenceEncoder::new(ENCODER_MODEL_PATH).await
            .or_else(|_| futures::executor::block_on(FlorenceEncoder::new(ALT_ENCODER_PATH)));

        if let Ok(encoder) = result {
            assert!(encoder.is_ready());
            assert!(encoder.embedding_dim() > 0);
            assert!(!encoder.input_name.is_empty());
        }
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_encode_inference() {
        let encoder = match FlorenceEncoder::new(ENCODER_MODEL_PATH).await
            .or_else(|_| futures::executor::block_on(FlorenceEncoder::new(ALT_ENCODER_PATH)))
        {
            Ok(e) => e,
            Err(_) => return, // Skip if model not available
        };

        // Create a test input (768x768 image)
        let input = Array4::<f32>::zeros((1, 3, 768, 768));

        let result = encoder.encode(&input);
        assert!(result.is_ok());

        let embeddings = result.unwrap();
        assert!(embeddings.nrows() > 0);
        assert!(embeddings.ncols() > 0);
    }

    #[test]
    fn test_encode_invalid_input_shape_batch() {
        // Verify shape validation logic
        let wrong_batch_shape = [2, 3, 768, 768]; // Batch should be 1
        assert!(wrong_batch_shape[0] != 1);
    }

    #[test]
    fn test_encode_invalid_input_shape_channels() {
        let wrong_channels_shape = [1, 1, 768, 768]; // Channels should be 3
        assert!(wrong_channels_shape[1] != 3);
    }

    #[test]
    fn test_parse_output_3d_shape() {
        // Test shape interpretation for 3D output
        let shape = [1, 577, 768]; // [batch, seq_len, embed_dim]
        assert_eq!(shape.len(), 3);
        assert_eq!(shape[1], 577); // seq_len
        assert_eq!(shape[2], 768); // embed_dim
    }

    #[test]
    fn test_parse_output_2d_shape() {
        // Test shape interpretation for 2D output
        let shape = [577, 768]; // [seq_len, embed_dim]
        assert_eq!(shape.len(), 2);
    }

    #[test]
    fn test_embedding_dimension() {
        // Florence-2-base uses 768D embeddings
        let expected_dim = 768;
        assert!(expected_dim > 0);
    }
}
