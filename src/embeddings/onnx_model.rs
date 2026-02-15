// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1

//! ONNX Embedding Model Wrapper (Sub-phase 3.1, GPU support in 8.2)
//!
//! This module provides a wrapper around ONNX Runtime for running
//! the all-MiniLM-L6-v2 sentence transformer model.
//!
//! Features:
//! - ONNX model loading from disk
//! - GPU acceleration via CUDA (with automatic CPU fallback)
//! - BERT tokenization with padding/truncation
//! - Single and batch embedding generation
//! - Mean pooling over token embeddings
//! - 384-dimensional output vectors

use anyhow::{Context, Result};
use ndarray::{Array2, Axis};
use ort::execution_providers::{CPUExecutionProvider, CUDAExecutionProvider};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Value;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokenizers::Tokenizer;
use tracing::{info, warn};

/// ONNX-based embedding model (all-MiniLM-L6-v2)
///
/// This struct wraps ONNX Runtime to provide 384-dimensional embeddings.
/// The model uses a sentence transformer architecture with:
/// - BERT-based tokenizer
/// - Mean pooling over token embeddings
/// - L2 normalization (applied by the ONNX model)
///
/// # Model Details
/// - Input: Text strings (up to 256 tokens)
/// - Output: 384-dimensional f32 vectors
/// - Provider: CPU (ONNX Runtime)
///
/// # Thread Safety
/// All fields are wrapped in Arc for cheap cloning and thread-safe sharing.
#[derive(Clone)]
pub struct OnnxEmbeddingModel {
    /// ONNX Runtime session (wrapped in Arc<Mutex> for thread-safe shared access)
    session: Arc<Mutex<Session>>,

    /// BERT tokenizer
    tokenizer: Arc<Tokenizer>,

    /// Model name (e.g., "all-MiniLM-L6-v2")
    model_name: String,

    /// Output dimension (384 for all-MiniLM-L6-v2)
    dimension: usize,

    /// Maximum sequence length (256 for all-MiniLM-L6-v2)
    max_length: usize,
}

impl std::fmt::Debug for OnnxEmbeddingModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OnnxEmbeddingModel")
            .field("model_name", &self.model_name)
            .field("dimension", &self.dimension)
            .field("max_length", &self.max_length)
            .finish_non_exhaustive()
    }
}

impl OnnxEmbeddingModel {
    /// Creates a new ONNX embedding model from disk paths
    ///
    /// # Arguments
    /// - `model_path`: Path to ONNX model file (model.onnx)
    /// - `tokenizer_path`: Path to tokenizer JSON file (tokenizer.json)
    ///
    /// # Returns
    /// - `Result<Self>`: Model instance or error
    ///
    /// # Errors
    /// Returns error if:
    /// - Model file not found or invalid
    /// - Tokenizer file not found or invalid
    /// - ONNX Runtime initialization fails
    /// - Model doesn't output 384 dimensions
    ///
    /// # Example
    /// ```ignore
    /// let model = OnnxEmbeddingModel::new(
    ///     "all-MiniLM-L6-v2",
    ///     "/workspace/models/all-MiniLM-L6-v2-onnx/model.onnx",
    ///     "/workspace/models/all-MiniLM-L6-v2-onnx/tokenizer.json"
    /// ).await?;
    /// ```
    pub async fn new<P: AsRef<Path>>(
        model_name: impl Into<String>,
        model_path: P,
        tokenizer_path: P,
    ) -> Result<Self> {
        let model_name = model_name.into();
        let model_path = model_path.as_ref();
        let tokenizer_path = tokenizer_path.as_ref();

        // Validate paths exist
        if !model_path.exists() {
            anyhow::bail!("ONNX model file not found: {}", model_path.display());
        }
        if !tokenizer_path.exists() {
            anyhow::bail!("Tokenizer file not found: {}", tokenizer_path.display());
        }

        // Load ONNX model session with GPU support (Sub-phase 8.2)
        // Try CUDA first, fall back to CPU if unavailable
        info!("🚀 Initializing ONNX embedding model with GPU support");

        // Try CUDA-only first to detect if CUDA is actually available
        info!("   Attempting CUDA execution provider...");
        let cuda_result = Session::builder()
            .context("Failed to create session builder")?
            .with_execution_providers([CUDAExecutionProvider::default().build()])
            .context("Failed to set CUDA execution provider")?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .context("Failed to set optimization level")?
            .with_intra_threads(4)
            .context("Failed to set intra threads")?
            .commit_from_file(model_path);

        let mut session = match cuda_result {
            Ok(s) => {
                info!("✅ CUDA execution provider initialized successfully!");
                s
            }
            Err(e) => {
                warn!("⚠️  CUDA execution provider failed: {}", e);
                warn!("   Falling back to CPU execution provider");
                Session::builder()
                    .context("Failed to create session builder")?
                    .with_execution_providers([CPUExecutionProvider::default().build()])
                    .context("Failed to set CPU execution provider")?
                    .with_optimization_level(GraphOptimizationLevel::Level3)
                    .context("Failed to set optimization level")?
                    .with_intra_threads(4)
                    .context("Failed to set intra threads")?
                    .commit_from_file(model_path)
                    .context(format!(
                        "Failed to load ONNX model from {}",
                        model_path.display()
                    ))?
            }
        };

        info!("✅ ONNX embedding model loaded successfully");

        // Load tokenizer
        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?;

        // Validate model outputs 384 dimensions by running a test inference
        // Wrap in a block to ensure outputs are dropped before moving session
        {
            let test_text = "validation test";
            let test_encoding = tokenizer
                .encode(test_text, true)
                .map_err(|e| anyhow::anyhow!("Tokenizer validation failed: {}", e))?;

            let input_ids: Vec<i64> = test_encoding
                .get_ids()
                .iter()
                .map(|&id| id as i64)
                .collect();
            let attention_mask: Vec<i64> = test_encoding
                .get_attention_mask()
                .iter()
                .map(|&m| m as i64)
                .collect();
            let token_type_ids: Vec<i64> = vec![0i64; input_ids.len()]; // All zeros for simple embedding

            let input_ids_array = Array2::from_shape_vec((1, input_ids.len()), input_ids)
                .context("Failed to create input_ids array")?;
            let attention_mask_array =
                Array2::from_shape_vec((1, attention_mask.len()), attention_mask)
                    .context("Failed to create attention_mask array")?;
            let token_type_ids_array =
                Array2::from_shape_vec((1, token_type_ids.len()), token_type_ids)
                    .context("Failed to create token_type_ids array")?;

            // Run validation inference (ort 2.0 API)
            let outputs = session.run(ort::inputs![
                "input_ids" => Value::from_array(input_ids_array)?,
                "attention_mask" => Value::from_array(attention_mask_array)?,
                "token_type_ids" => Value::from_array(token_type_ids_array)?
            ])?;

            // Extract output dimensions (ort 2.0 API)
            // Use index [0] instead of name since different models may have different output names
            let output_tensor = outputs[0]
                .try_extract_array::<f32>()
                .context("Failed to extract output tensor")?;
            let output_shape = output_tensor.shape();

            // Model outputs token-level embeddings: [batch, seq_len, hidden_dim]
            // We need to apply mean pooling to get sentence embeddings: [batch, hidden_dim]
            if output_shape.len() != 3 || output_shape[2] != 384 {
                anyhow::bail!(
                    "Model outputs unexpected dimensions: {:?} (expected [batch, seq_len, 384])",
                    output_shape
                );
            }
        } // outputs dropped here

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            tokenizer: Arc::new(tokenizer),
            model_name,
            dimension: 384,
            max_length: 256,
        })
    }

    /// Generates embedding for a single text
    ///
    /// # Arguments
    /// - `text`: Input text string
    ///
    /// # Returns
    /// - `Result<Vec<f32>>`: 384-dimensional embedding vector
    ///
    /// # Implementation
    /// 1. Tokenize input with BERT tokenizer (padding/truncation to max_length)
    /// 2. Run ONNX inference
    /// 3. Mean pooling applied by ONNX model
    /// 4. L2 normalization applied by ONNX model
    ///
    /// # Example
    /// ```ignore
    /// let embedding = model.embed("Hello world").await?;
    /// assert_eq!(embedding.len(), 384);
    /// ```
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // Tokenize input
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("Tokenization failed: {}", e))?;

        let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect();
        let token_type_ids: Vec<i64> = vec![0i64; input_ids.len()]; // All zeros for simple embedding

        // Keep a copy of attention_mask for mean pooling
        let attention_mask_for_pooling = attention_mask.clone();

        // Create input tensors
        let input_ids_array = Array2::from_shape_vec((1, input_ids.len()), input_ids)
            .context("Failed to create input_ids array")?;
        let attention_mask_array =
            Array2::from_shape_vec((1, attention_mask.len()), attention_mask)
                .context("Failed to create attention_mask array")?;
        let token_type_ids_array =
            Array2::from_shape_vec((1, token_type_ids.len()), token_type_ids)
                .context("Failed to create token_type_ids array")?;

        // Run inference (ort 2.0 API) - lock session for thread-safe access
        let mut session_guard = self.session.lock().unwrap();
        let outputs = session_guard.run(ort::inputs![
            "input_ids" => Value::from_array(input_ids_array)?,
            "attention_mask" => Value::from_array(attention_mask_array)?,
            "token_type_ids" => Value::from_array(token_type_ids_array)?
        ])?;

        // Extract output tensor (ort 2.0 API)
        // Use index [0] instead of name since different models may have different output names
        let output_array = outputs[0]
            .try_extract_array::<f32>()
            .context("Failed to extract output tensor")?;

        // Model outputs token-level embeddings: [batch, seq_len, hidden_dim]
        // Apply mean pooling over sequence dimension to get sentence embedding
        let batch_0 = output_array.index_axis(Axis(0), 0); // [seq_len, hidden_dim]

        // Mean pooling: average over sequence length dimension
        // Weight by attention mask to ignore padding tokens
        let seq_len = batch_0.shape()[0];
        let hidden_dim = batch_0.shape()[1];

        let mut pooled = vec![0.0f32; hidden_dim];
        let mut sum_mask = 0.0f32;

        for i in 0..seq_len {
            let mask_value = attention_mask_for_pooling[i] as f32;
            sum_mask += mask_value;
            for j in 0..hidden_dim {
                pooled[j] += batch_0[[i, j]] * mask_value;
            }
        }

        // Normalize by sum of mask (number of non-padding tokens)
        for val in &mut pooled {
            *val /= sum_mask.max(1e-9); // Avoid division by zero
        }

        let embedding = pooled;

        if embedding.len() != self.dimension {
            anyhow::bail!(
                "Unexpected embedding dimension: {} (expected {})",
                embedding.len(),
                self.dimension
            );
        }

        Ok(embedding)
    }

    /// Generates embeddings for multiple texts in a batch
    ///
    /// # Arguments
    /// - `texts`: Array of input text strings
    ///
    /// # Returns
    /// - `Result<Vec<Vec<f32>>>`: Array of 384-dimensional embeddings
    ///
    /// # Implementation
    /// Tokenizes all texts, pads to same length, and runs batch inference.
    /// More efficient than calling embed() multiple times for large batches.
    ///
    /// # Example
    /// ```ignore
    /// let texts = vec!["Hello".to_string(), "World".to_string()];
    /// let embeddings = model.embed_batch(&texts).await?;
    /// assert_eq!(embeddings.len(), 2);
    /// ```
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        // Tokenize all texts
        let encodings: Vec<_> = texts
            .iter()
            .map(|text| {
                self.tokenizer
                    .encode(text.as_str(), true)
                    .map_err(|e| anyhow::anyhow!("Tokenization failed: {}", e))
            })
            .collect::<Result<Vec<_>>>()?;

        // Find max length in batch for padding
        let max_len = encodings
            .iter()
            .map(|enc| enc.get_ids().len())
            .max()
            .unwrap_or(0);

        // Prepare batch tensors (pad all sequences to same length)
        let mut input_ids_batch = Vec::with_capacity(texts.len() * max_len);
        let mut attention_mask_batch = Vec::with_capacity(texts.len() * max_len);
        let mut token_type_ids_batch = Vec::with_capacity(texts.len() * max_len);

        for encoding in &encodings {
            let ids = encoding.get_ids();
            let mask = encoding.get_attention_mask();

            // Add tokens
            input_ids_batch.extend(ids.iter().map(|&id| id as i64));
            attention_mask_batch.extend(mask.iter().map(|&m| m as i64));
            token_type_ids_batch.extend(std::iter::repeat(0i64).take(ids.len())); // All zeros

            // Pad to max_len
            let padding_needed = max_len - ids.len();
            input_ids_batch.extend(std::iter::repeat(0i64).take(padding_needed));
            attention_mask_batch.extend(std::iter::repeat(0i64).take(padding_needed));
            token_type_ids_batch.extend(std::iter::repeat(0i64).take(padding_needed));
        }

        // Keep a copy of attention_mask_batch for mean pooling
        let attention_mask_for_pooling = attention_mask_batch.clone();

        // Create batch tensors
        let input_ids_array = Array2::from_shape_vec((texts.len(), max_len), input_ids_batch)
            .context("Failed to create batch input_ids array")?;
        let attention_mask_array =
            Array2::from_shape_vec((texts.len(), max_len), attention_mask_batch)
                .context("Failed to create batch attention_mask array")?;
        let token_type_ids_array =
            Array2::from_shape_vec((texts.len(), max_len), token_type_ids_batch)
                .context("Failed to create batch token_type_ids array")?;

        // Run batch inference (ort 2.0 API) - lock session for thread-safe access
        let mut session_guard = self.session.lock().unwrap();
        let outputs = session_guard.run(ort::inputs![
            "input_ids" => Value::from_array(input_ids_array)?,
            "attention_mask" => Value::from_array(attention_mask_array)?,
            "token_type_ids" => Value::from_array(token_type_ids_array)?
        ])?;

        // Extract output tensor (ort 2.0 API)
        // Use index [0] instead of name since different models may have different output names
        let output_array = outputs[0]
            .try_extract_array::<f32>()
            .context("Failed to extract output tensor")?;

        // Model outputs token-level embeddings: [batch, seq_len, hidden_dim]
        // Apply mean pooling over sequence dimension for each item in batch
        let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(texts.len());

        for batch_idx in 0..texts.len() {
            let batch_item = output_array.index_axis(Axis(0), batch_idx); // [seq_len, hidden_dim]
            let seq_len = batch_item.shape()[0];
            let hidden_dim = batch_item.shape()[1];

            // Get attention mask for this batch item
            let mask_start = batch_idx * max_len;
            let mask_end = mask_start + max_len;
            let item_mask = &attention_mask_for_pooling[mask_start..mask_end];

            // Mean pooling with attention mask
            let mut pooled = vec![0.0f32; hidden_dim];
            let mut sum_mask = 0.0f32;

            for i in 0..seq_len {
                let mask_value = item_mask[i] as f32;
                sum_mask += mask_value;
                for j in 0..hidden_dim {
                    pooled[j] += batch_item[[i, j]] * mask_value;
                }
            }

            // Normalize
            for val in &mut pooled {
                *val /= sum_mask.max(1e-9);
            }

            embeddings.push(pooled);
        }

        // Validate all embeddings are correct dimension
        for (i, emb) in embeddings.iter().enumerate() {
            if emb.len() != self.dimension {
                anyhow::bail!(
                    "Unexpected embedding dimension at index {}: {} (expected {})",
                    i,
                    emb.len(),
                    self.dimension
                );
            }
        }

        Ok(embeddings)
    }

    /// Counts tokens in a text string
    ///
    /// # Arguments
    /// - `text`: Input text string
    ///
    /// # Returns
    /// - `Result<usize>`: Number of tokens (including special tokens)
    ///
    /// # Example
    /// ```ignore
    /// let count = model.count_tokens("Hello world").await?;
    /// assert!(count >= 2); // At least 2 tokens
    /// ```
    pub async fn count_tokens(&self, text: &str) -> Result<usize> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!("Tokenization failed: {}", e))?;

        // Count only non-padding tokens by summing attention mask
        // (attention_mask is 1 for real tokens, 0 for padding)
        let attention_mask = encoding.get_attention_mask();
        let token_count: usize = attention_mask.iter().map(|&m| m as usize).sum();

        Ok(token_count)
    }

    /// Returns the output dimension of this model
    pub fn dimension(&self) -> usize {
        self.dimension
    }

    /// Returns the model name
    pub fn model_name(&self) -> &str {
        &self.model_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These inline tests are kept minimal.
    // Comprehensive TDD tests are in tests/embeddings/test_onnx_model.rs

    const MODEL_PATH: &str = "/workspace/models/all-MiniLM-L6-v2-onnx/model.onnx";
    const TOKENIZER_PATH: &str = "/workspace/models/all-MiniLM-L6-v2-onnx/tokenizer.json";

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_model_creation() {
        let model = OnnxEmbeddingModel::new("all-MiniLM-L6-v2", MODEL_PATH, TOKENIZER_PATH)
            .await
            .unwrap();
        assert_eq!(model.dimension(), 384);
        assert_eq!(model.model_name(), "all-MiniLM-L6-v2");
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_embed_basic() {
        let model = OnnxEmbeddingModel::new("all-MiniLM-L6-v2", MODEL_PATH, TOKENIZER_PATH)
            .await
            .unwrap();
        let embedding = model.embed("test").await.unwrap();
        assert_eq!(embedding.len(), 384);
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_embed_batch_basic() {
        let model = OnnxEmbeddingModel::new("all-MiniLM-L6-v2", MODEL_PATH, TOKENIZER_PATH)
            .await
            .unwrap();
        let texts = vec!["test1".to_string(), "test2".to_string()];
        let embeddings = model.embed_batch(&texts).await.unwrap();
        assert_eq!(embeddings.len(), 2);
        assert_eq!(embeddings[0].len(), 384);
        assert_eq!(embeddings[1].len(), 384);
    }
}
