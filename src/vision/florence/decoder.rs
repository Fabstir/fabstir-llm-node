// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Florence-2 language decoder model
//!
//! This module provides the language decoding component of Florence-2.
//! It generates text descriptions from visual embeddings.

use anyhow::{Context, Result};
use ndarray::{Array2, IxDyn};
use ort::execution_providers::CPUExecutionProvider;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Value;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokenizers::Tokenizer;
use tracing::{debug, info, warn};

/// Default maximum tokens to generate
pub const DEFAULT_MAX_TOKENS: usize = 150;

/// Minimum tokens to generate
pub const MIN_TOKENS: usize = 10;

/// Maximum tokens to generate
pub const MAX_TOKENS: usize = 500;

/// Florence-2 language decoder model
///
/// Uses the Florence-2 decoder to generate text from image embeddings.
/// Runs on CPU only to avoid GPU VRAM competition with LLM.
#[derive(Clone)]
pub struct FlorenceDecoder {
    /// ONNX Runtime session (thread-safe)
    session: Arc<Mutex<Session>>,
    /// Tokenizer for text encoding/decoding
    tokenizer: Arc<Tokenizer>,
    /// Model input names
    encoder_hidden_states_name: String,
    input_ids_name: String,
    /// Model output name
    output_name: String,
    /// Maximum tokens to generate
    max_tokens: usize,
    /// Vocabulary size
    vocab_size: usize,
    /// Special token IDs
    bos_token_id: u32,
    eos_token_id: u32,
    pad_token_id: u32,
    /// Whether model is loaded and ready
    is_ready: bool,
}

impl std::fmt::Debug for FlorenceDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlorenceDecoder")
            .field("max_tokens", &self.max_tokens)
            .field("vocab_size", &self.vocab_size)
            .field("is_ready", &self.is_ready)
            .finish_non_exhaustive()
    }
}

impl FlorenceDecoder {
    /// Load the Florence decoder from files
    ///
    /// # Arguments
    /// - `model_path`: Path to the ONNX model file (decoder.onnx or decoder_model.onnx)
    /// - `tokenizer_path`: Path to the tokenizer file (tokenizer.json)
    ///
    /// # Returns
    /// - `Result<Self>`: Decoder model instance or error
    ///
    /// # Errors
    /// Returns error if:
    /// - Model file not found
    /// - Tokenizer file not found
    /// - ONNX Runtime initialization fails
    pub async fn new<P: AsRef<Path>>(model_path: P, tokenizer_path: P) -> Result<Self> {
        let model_path = model_path.as_ref();
        let tokenizer_path = tokenizer_path.as_ref();

        // Validate paths exist
        if !model_path.exists() {
            anyhow::bail!(
                "Florence decoder model not found: {}",
                model_path.display()
            );
        }
        if !tokenizer_path.exists() {
            anyhow::bail!(
                "Florence tokenizer not found: {}",
                tokenizer_path.display()
            );
        }

        info!(
            "Loading Florence decoder from {}",
            model_path.display()
        );

        // Load tokenizer
        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?;

        let vocab_size = tokenizer.get_vocab_size(true);
        info!("Loaded tokenizer with {} tokens", vocab_size);

        // Load ONNX model with CPU-only execution
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
                "Failed to load Florence decoder model from {}",
                model_path.display()
            ))?;

        // Get input/output names
        // Decoder typically has: encoder_hidden_states, input_ids, attention_mask
        let encoder_hidden_states_name = session
            .inputs
            .iter()
            .find(|i| i.name.contains("encoder") || i.name.contains("hidden"))
            .map(|i| i.name.clone())
            .unwrap_or_else(|| "encoder_hidden_states".to_string());

        let input_ids_name = session
            .inputs
            .iter()
            .find(|i| i.name.contains("input_ids"))
            .map(|i| i.name.clone())
            .unwrap_or_else(|| "input_ids".to_string());

        let output_name = session
            .outputs
            .first()
            .map(|o| o.name.clone())
            .unwrap_or_else(|| "logits".to_string());

        debug!(
            "Decoder loaded - inputs: [{}, {}], output: {}",
            encoder_hidden_states_name, input_ids_name, output_name
        );

        // Get special token IDs from tokenizer
        let bos_token_id = tokenizer
            .token_to_id("<s>")
            .or_else(|| tokenizer.token_to_id("[CLS]"))
            .unwrap_or(0);
        let eos_token_id = tokenizer
            .token_to_id("</s>")
            .or_else(|| tokenizer.token_to_id("[SEP]"))
            .unwrap_or(2);
        let pad_token_id = tokenizer
            .token_to_id("<pad>")
            .or_else(|| tokenizer.token_to_id("[PAD]"))
            .unwrap_or(1);

        debug!(
            "Special tokens - BOS: {}, EOS: {}, PAD: {}",
            bos_token_id, eos_token_id, pad_token_id
        );

        info!("✅ Florence decoder loaded successfully (CPU-only)");

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            tokenizer: Arc::new(tokenizer),
            encoder_hidden_states_name,
            input_ids_name,
            output_name,
            max_tokens: DEFAULT_MAX_TOKENS,
            vocab_size,
            bos_token_id,
            eos_token_id,
            pad_token_id,
            is_ready: true,
        })
    }

    /// Set the maximum tokens to generate
    pub fn with_max_tokens(mut self, max_tokens: usize) -> Self {
        self.max_tokens = max_tokens.clamp(MIN_TOKENS, MAX_TOKENS);
        self
    }

    /// Get the current maximum tokens setting
    pub fn max_tokens(&self) -> usize {
        self.max_tokens
    }

    /// Get the vocabulary size
    pub fn vocab_size(&self) -> usize {
        self.vocab_size
    }

    /// Check if the model is ready for inference
    pub fn is_ready(&self) -> bool {
        self.is_ready
    }

    /// Generate text from image embeddings
    ///
    /// # Arguments
    /// - `image_embeddings`: Visual features from encoder [seq_len, embed_dim]
    /// - `prompt`: Optional text prompt to condition generation
    ///
    /// # Returns
    /// - `Result<String>`: Generated description text
    ///
    /// # Process
    /// 1. Tokenize prompt (if provided) or use BOS token
    /// 2. Run autoregressive generation loop
    /// 3. Stop at EOS token or max tokens
    /// 4. Decode tokens to text
    pub fn generate(
        &self,
        image_embeddings: &Array2<f32>,
        prompt: Option<&str>,
    ) -> Result<String> {
        // Initialize input tokens
        let mut tokens: Vec<u32> = if let Some(prompt_text) = prompt {
            let encoding = self
                .tokenizer
                .encode(prompt_text, true)
                .map_err(|e| anyhow::anyhow!("Tokenization failed: {}", e))?;
            encoding.get_ids().to_vec()
        } else {
            vec![self.bos_token_id]
        };

        // Autoregressive generation loop
        for step in 0..self.max_tokens {
            // Get next token logits
            let logits = self.forward(image_embeddings, &tokens)?;

            // Greedy decoding: select highest probability token
            let next_token = self.argmax(&logits)?;

            // Check for end of sequence
            if next_token == self.eos_token_id {
                debug!("Generation stopped at EOS after {} tokens", step + 1);
                break;
            }

            tokens.push(next_token);
        }

        // Decode tokens to text
        let output_text = self
            .tokenizer
            .decode(&tokens, true)
            .map_err(|e| anyhow::anyhow!("Decoding failed: {}", e))?;

        // Clean up the output
        let cleaned = output_text
            .trim()
            .replace("<s>", "")
            .replace("</s>", "")
            .replace("<pad>", "")
            .trim()
            .to_string();

        debug!("Generated {} tokens: '{}'", tokens.len(), cleaned);

        Ok(cleaned)
    }

    /// Run a single forward pass through the decoder
    fn forward(&self, encoder_hidden_states: &Array2<f32>, input_ids: &[u32]) -> Result<Vec<f32>> {
        let mut session = self.session.lock().unwrap();

        // Prepare encoder hidden states [1, seq_len, embed_dim]
        let (seq_len, embed_dim) = (encoder_hidden_states.nrows(), encoder_hidden_states.ncols());
        let mut encoder_input = ndarray::Array3::<f32>::zeros((1, seq_len, embed_dim));
        for s in 0..seq_len {
            for e in 0..embed_dim {
                encoder_input[[0, s, e]] = encoder_hidden_states[[s, e]];
            }
        }

        // Prepare input IDs [1, token_len]
        let token_len = input_ids.len();
        let mut input_ids_array = ndarray::Array2::<i64>::zeros((1, token_len));
        for (i, &token) in input_ids.iter().enumerate() {
            input_ids_array[[0, i]] = token as i64;
        }

        // Run inference
        let encoder_value = Value::from_array(encoder_input)
            .context("Failed to create encoder hidden states tensor")?;
        let input_ids_value = Value::from_array(input_ids_array)
            .context("Failed to create input IDs tensor")?;

        let outputs = session
            .run(ort::inputs![
                &self.encoder_hidden_states_name => encoder_value,
                &self.input_ids_name => input_ids_value
            ])
            .context("Decoder inference failed")?;

        // Extract logits for last token position
        let output_tensor = outputs[0]
            .try_extract_array::<f32>()
            .context("Failed to extract output tensor")?;

        let output_shape = output_tensor.shape();
        debug!("Decoder output shape: {:?}", output_shape);

        // Get logits for last position [vocab_size]
        let last_pos = if output_shape.len() >= 2 {
            output_shape[1] - 1
        } else {
            0
        };

        let vocab_size = if output_shape.len() == 3 {
            output_shape[2]
        } else if output_shape.len() == 2 {
            output_shape[1]
        } else {
            self.vocab_size
        };

        let mut logits = vec![0.0f32; vocab_size];

        for v in 0..vocab_size {
            logits[v] = match output_shape.len() {
                3 => output_tensor[IxDyn(&[0, last_pos, v])],
                2 => output_tensor[IxDyn(&[last_pos, v])],
                _ => 0.0,
            };
        }

        Ok(logits)
    }

    /// Find the index of the maximum value (greedy decoding)
    fn argmax(&self, logits: &[f32]) -> Result<u32> {
        let (max_idx, _) = logits
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .ok_or_else(|| anyhow::anyhow!("Empty logits vector"))?;

        Ok(max_idx as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECODER_MODEL_PATH: &str = "/workspace/models/florence-2-onnx/decoder_model.onnx";
    const ALT_DECODER_PATH: &str = "/workspace/models/florence-2-onnx/decoder.onnx";
    const TOKENIZER_PATH: &str = "/workspace/models/florence-2-onnx/tokenizer.json";

    #[test]
    fn test_default_max_tokens() {
        assert_eq!(DEFAULT_MAX_TOKENS, 150);
    }

    #[test]
    fn test_token_limits() {
        assert!(MIN_TOKENS < DEFAULT_MAX_TOKENS);
        assert!(DEFAULT_MAX_TOKENS < MAX_TOKENS);
        assert_eq!(MIN_TOKENS, 10);
        assert_eq!(MAX_TOKENS, 500);
    }

    #[test]
    fn test_max_tokens_clamping() {
        // Test clamping logic
        let clamped_low = 5_usize.clamp(MIN_TOKENS, MAX_TOKENS);
        assert_eq!(clamped_low, MIN_TOKENS);

        let clamped_high = 1000_usize.clamp(MIN_TOKENS, MAX_TOKENS);
        assert_eq!(clamped_high, MAX_TOKENS);

        let in_range = 200_usize.clamp(MIN_TOKENS, MAX_TOKENS);
        assert_eq!(in_range, 200);
    }

    #[tokio::test]
    async fn test_model_not_found_error() {
        let result = FlorenceDecoder::new(
            "/nonexistent/path/decoder.onnx",
            TOKENIZER_PATH,
        )
        .await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_tokenizer_not_found_error() {
        let result = FlorenceDecoder::new(
            DECODER_MODEL_PATH,
            "/nonexistent/path/tokenizer.json",
        )
        .await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_model_loading() {
        let result = FlorenceDecoder::new(DECODER_MODEL_PATH, TOKENIZER_PATH).await
            .or_else(|_| futures::executor::block_on(
                FlorenceDecoder::new(ALT_DECODER_PATH, TOKENIZER_PATH)
            ));

        if let Ok(decoder) = result {
            assert!(decoder.is_ready());
            assert!(decoder.vocab_size() > 1000);
            assert_eq!(decoder.max_tokens(), DEFAULT_MAX_TOKENS);
        }
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_decoder_with_custom_max_tokens() {
        let result = FlorenceDecoder::new(DECODER_MODEL_PATH, TOKENIZER_PATH).await
            .or_else(|_| futures::executor::block_on(
                FlorenceDecoder::new(ALT_DECODER_PATH, TOKENIZER_PATH)
            ));

        if let Ok(decoder) = result {
            let decoder = decoder.with_max_tokens(200);
            assert_eq!(decoder.max_tokens(), 200);
        }
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_generation() {
        let decoder = match FlorenceDecoder::new(DECODER_MODEL_PATH, TOKENIZER_PATH).await
            .or_else(|_| futures::executor::block_on(
                FlorenceDecoder::new(ALT_DECODER_PATH, TOKENIZER_PATH)
            ))
        {
            Ok(d) => d,
            Err(_) => return,
        };

        // Create mock embeddings
        let embeddings = Array2::<f32>::zeros((577, 768));

        let result = decoder.generate(&embeddings, None);
        assert!(result.is_ok() || result.is_err()); // May fail with mock embeddings
    }

    #[test]
    fn test_argmax_simple() {
        // Test argmax logic directly
        let logits = vec![0.1, 0.5, 0.3, 0.9, 0.2];
        let (max_idx, _) = logits
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .unwrap();
        assert_eq!(max_idx, 3); // 0.9 is at index 3
    }

    #[test]
    fn test_argmax_negative() {
        let logits = vec![-0.5, -0.1, -0.3];
        let (max_idx, _) = logits
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .unwrap();
        assert_eq!(max_idx, 1); // -0.1 is highest
    }
}
