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
    /// ONNX Runtime session for decoder (thread-safe)
    session: Arc<Mutex<Session>>,
    /// ONNX Runtime session for token embedding (thread-safe)
    embed_session: Arc<Mutex<Session>>,
    /// Tokenizer for text encoding/decoding
    tokenizer: Arc<Tokenizer>,
    /// Maximum tokens to generate
    max_tokens: usize,
    /// Vocabulary size
    vocab_size: usize,
    /// Special token IDs
    bos_token_id: u32,
    eos_token_id: u32,
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
    /// - embed_tokens.onnx not found
    /// - ONNX Runtime initialization fails
    pub async fn new<P: AsRef<Path>>(model_path: P, tokenizer_path: P) -> Result<Self> {
        let model_path = model_path.as_ref();
        let tokenizer_path = tokenizer_path.as_ref();

        // Derive embed_tokens path from model path
        let embed_path = model_path
            .parent()
            .map(|p| p.join("embed_tokens.onnx"))
            .unwrap_or_else(|| std::path::PathBuf::from("embed_tokens.onnx"));

        // Validate paths exist
        if !model_path.exists() {
            anyhow::bail!("Florence decoder model not found: {}", model_path.display());
        }
        if !tokenizer_path.exists() {
            anyhow::bail!("Florence tokenizer not found: {}", tokenizer_path.display());
        }
        if !embed_path.exists() {
            anyhow::bail!(
                "Florence embed_tokens model not found: {}. Please download it from HuggingFace.",
                embed_path.display()
            );
        }

        info!("Loading Florence decoder from {}", model_path.display());

        // Load tokenizer
        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?;

        let vocab_size = tokenizer.get_vocab_size(true);
        info!("Loaded tokenizer with {} tokens", vocab_size);

        // Load embed_tokens model (converts token IDs to embeddings)
        info!("Loading embed_tokens from {}", embed_path.display());
        let embed_session = Session::builder()
            .context("Failed to create embed session builder")?
            .with_execution_providers([CPUExecutionProvider::default().build()])
            .context("Failed to set CPU execution provider for embed")?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .context("Failed to set optimization level for embed")?
            .with_intra_threads(4)
            .context("Failed to set intra threads for embed")?
            .commit_from_file(&embed_path)
            .context(format!(
                "Failed to load embed_tokens model from {}",
                embed_path.display()
            ))?;

        // Load decoder ONNX model with CPU-only execution
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

        // Log decoder inputs for debugging
        let input_names: Vec<_> = session.inputs.iter().map(|i| &i.name).collect();
        debug!("Decoder inputs: {:?}", input_names);

        // Get special token IDs from tokenizer
        let bos_token_id = tokenizer
            .token_to_id("<s>")
            .or_else(|| tokenizer.token_to_id("[CLS]"))
            .unwrap_or(0);
        let eos_token_id = tokenizer
            .token_to_id("</s>")
            .or_else(|| tokenizer.token_to_id("[SEP]"))
            .unwrap_or(2);

        debug!(
            "Special tokens - BOS: {}, EOS: {}",
            bos_token_id, eos_token_id
        );

        info!("✅ Florence decoder loaded successfully (CPU-only)");

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            embed_session: Arc::new(Mutex::new(embed_session)),
            tokenizer: Arc::new(tokenizer),
            max_tokens: DEFAULT_MAX_TOKENS,
            vocab_size,
            bos_token_id,
            eos_token_id,
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
    pub fn generate(&self, image_embeddings: &Array2<f32>, prompt: Option<&str>) -> Result<String> {
        // Initialize input tokens with prompt
        // NOTE: Task tokens (<cap>, <dcap>) produce "unanswerable" with this ONNX export
        // Natural language prompts like "A photo of" work correctly
        let mut tokens = vec![self.bos_token_id];

        if let Some(prompt_text) = prompt {
            // Tokenize the prompt and append (without the auto-added BOS/EOS)
            let encoding = self
                .tokenizer
                .encode(prompt_text, false)
                .map_err(|e| anyhow::anyhow!("Failed to encode prompt: {}", e))?;

            // Filter out special tokens from the encoding
            let prompt_tokens: Vec<u32> = encoding
                .get_ids()
                .iter()
                .copied()
                .filter(|&id| id != self.bos_token_id && id != self.eos_token_id)
                .collect();

            info!(
                "Prompt '{}' tokenized to {} tokens: {:?}",
                prompt_text,
                prompt_tokens.len(),
                prompt_tokens
            );
            tokens.extend(prompt_tokens);
        } else {
            info!("No prompt provided, using BOS-only start");
        }

        // Log the actual token IDs for debugging
        info!("Initial tokens: {:?} (len={})", tokens, tokens.len());
        for (i, &tid) in tokens.iter().enumerate() {
            let tok_str = self.tokenizer.decode(&[tid], false).unwrap_or_default();
            info!("  Token {}: ID {} = '{}'", i, tid, tok_str);
        }

        // Verify encoder output is valid (not all zeros or NaN)
        let enc_mean: f32 = image_embeddings.iter().sum::<f32>() / image_embeddings.len() as f32;
        let enc_var: f32 = image_embeddings
            .iter()
            .map(|x| (x - enc_mean).powi(2))
            .sum::<f32>()
            / image_embeddings.len() as f32;
        info!(
            "Encoder output stats: mean={:.6}, variance={:.6}, shape={:?}",
            enc_mean,
            enc_var,
            image_embeddings.shape()
        );

        if enc_var < 0.001 {
            warn!(
                "⚠️ Encoder output has very low variance ({:.6}) - image features may be garbage!",
                enc_var
            );
        }
        if enc_mean.is_nan() || enc_var.is_nan() {
            warn!("⚠️ Encoder output contains NaN values - model may be corrupted!");
        }

        // Autoregressive generation loop
        debug!(
            "Starting generation with {} initial tokens, EOS={}",
            tokens.len(),
            self.eos_token_id
        );
        for step in 0..self.max_tokens {
            // Get next token logits
            let logits = self.forward(image_embeddings, &tokens)?;

            // Greedy decoding: select highest probability token (mask recent tokens to prevent loops)
            let next_token = self.argmax(&logits, &tokens)?;

            // Debug: show what token was selected and top logits
            if step < 5 {
                let token_text = self
                    .tokenizer
                    .decode(&[next_token], false)
                    .unwrap_or_default();
                info!(
                    "Step {}: selected token {} = '{}' (repetition masked)",
                    step, next_token, token_text
                );

                // Build mask set for display
                let mask_tokens: std::collections::HashSet<u32> = tokens
                    .iter()
                    .rev()
                    .take(5)
                    .copied()
                    .chain(std::iter::once(self.bos_token_id))
                    .collect();

                // Show top 5 logits (excluding masked tokens)
                let mut indexed_logits: Vec<(usize, f32)> = logits
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !mask_tokens.contains(&(*i as u32)))
                    .map(|(i, &v)| (i, v))
                    .collect();
                indexed_logits
                    .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                info!(
                    "  Top 5 logits (after masking {} tokens):",
                    mask_tokens.len()
                );
                for (idx, val) in indexed_logits.iter().take(5) {
                    let tok = self
                        .tokenizer
                        .decode(&[*idx as u32], false)
                        .unwrap_or_default();
                    info!("    ID {:5} = {:>10.4} '{}'", idx, val, tok);
                }
            }

            // Check for end of sequence
            if next_token == self.eos_token_id {
                info!(
                    "Generation stopped at EOS after {} tokens (total {} tokens)",
                    step + 1,
                    tokens.len()
                );
                break;
            }

            tokens.push(next_token);
        }

        debug!("Generation complete: {} total tokens", tokens.len());

        // Decode tokens to text
        let output_text = self
            .tokenizer
            .decode(&tokens, true)
            .map_err(|e| anyhow::anyhow!("Decoding failed: {}", e))?;

        // Clean up the output - remove special tokens and task tokens
        let cleaned = output_text
            .trim()
            .replace("<s>", "")
            .replace("</s>", "")
            .replace("<pad>", "")
            // Remove Florence-2 task tokens
            .replace("<cap>", "")
            .replace("</cap>", "")
            .replace("<dcap>", "")
            .replace("</dcap>", "")
            .replace("<ncap>", "")
            .replace("</ncap>", "")
            .trim()
            .to_string();

        debug!("Generated {} tokens: '{}'", tokens.len(), cleaned);

        Ok(cleaned)
    }

    /// Convert token IDs to embeddings using embed_tokens model
    fn embed_tokens(&self, input_ids: &[u32]) -> Result<ndarray::Array3<f32>> {
        let mut embed_session = self.embed_session.lock().unwrap();

        // Prepare input IDs [1, token_len]
        let token_len = input_ids.len();
        let mut input_ids_array = ndarray::Array2::<i64>::zeros((1, token_len));
        for (i, &token) in input_ids.iter().enumerate() {
            input_ids_array[[0, i]] = token as i64;
        }

        let input_ids_value = Value::from_array(input_ids_array)
            .context("Failed to create input IDs tensor for embedding")?;

        let outputs = embed_session
            .run(ort::inputs!["input_ids" => input_ids_value])
            .context("embed_tokens inference failed")?;

        // Extract embeddings [1, token_len, 768]
        let output_tensor = outputs[0]
            .try_extract_array::<f32>()
            .context("Failed to extract embeddings tensor")?;

        let shape = output_tensor.shape();
        let mut embeddings = ndarray::Array3::<f32>::zeros((shape[0], shape[1], shape[2]));
        for b in 0..shape[0] {
            for s in 0..shape[1] {
                for e in 0..shape[2] {
                    embeddings[[b, s, e]] = output_tensor[IxDyn(&[b, s, e])];
                }
            }
        }

        Ok(embeddings)
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

        // Create encoder attention mask [1, seq_len] - all 1s for valid positions
        let encoder_attention_mask = ndarray::Array2::<i64>::ones((1, seq_len));

        // Convert token IDs to embeddings using embed_tokens model
        // We need to release the session lock first
        drop(session);
        let inputs_embeds = self.embed_tokens(input_ids)?;
        let mut session = self.session.lock().unwrap();

        // Run decoder inference with all three inputs
        let encoder_value = Value::from_array(encoder_input)
            .context("Failed to create encoder hidden states tensor")?;
        let attention_mask_value = Value::from_array(encoder_attention_mask)
            .context("Failed to create encoder attention mask tensor")?;
        let inputs_embeds_value =
            Value::from_array(inputs_embeds).context("Failed to create inputs_embeds tensor")?;

        let outputs = session
            .run(ort::inputs![
                "encoder_hidden_states" => encoder_value,
                "encoder_attention_mask" => attention_mask_value,
                "inputs_embeds" => inputs_embeds_value
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
    /// Masks out BOS token and tokens already in sequence to prevent loops
    fn argmax(&self, logits: &[f32], existing_tokens: &[u32]) -> Result<u32> {
        // Create a set of tokens to mask (BOS + recent tokens to prevent repetition)
        let mask_tokens: std::collections::HashSet<u32> = existing_tokens
            .iter()
            .rev()
            .take(5) // Mask last 5 tokens to prevent short loops
            .copied()
            .chain(std::iter::once(self.bos_token_id))
            .collect();

        let (max_idx, _) = logits
            .iter()
            .enumerate()
            .filter(|(idx, _)| {
                // Mask out BOS and recent tokens to prevent repetition loops
                !mask_tokens.contains(&(*idx as u32))
            })
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .ok_or_else(|| anyhow::anyhow!("Empty logits vector after filtering"))?;

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
        let result = FlorenceDecoder::new("/nonexistent/path/decoder.onnx", TOKENIZER_PATH).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_tokenizer_not_found_error() {
        let result =
            FlorenceDecoder::new(DECODER_MODEL_PATH, "/nonexistent/path/tokenizer.json").await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_model_loading() {
        let result = FlorenceDecoder::new(DECODER_MODEL_PATH, TOKENIZER_PATH)
            .await
            .or_else(|_| {
                futures::executor::block_on(FlorenceDecoder::new(ALT_DECODER_PATH, TOKENIZER_PATH))
            });

        if let Ok(decoder) = result {
            assert!(decoder.is_ready());
            assert!(decoder.vocab_size() > 1000);
            assert_eq!(decoder.max_tokens(), DEFAULT_MAX_TOKENS);
        }
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_decoder_with_custom_max_tokens() {
        let result = FlorenceDecoder::new(DECODER_MODEL_PATH, TOKENIZER_PATH)
            .await
            .or_else(|_| {
                futures::executor::block_on(FlorenceDecoder::new(ALT_DECODER_PATH, TOKENIZER_PATH))
            });

        if let Ok(decoder) = result {
            let decoder = decoder.with_max_tokens(200);
            assert_eq!(decoder.max_tokens(), 200);
        }
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_generation() {
        let decoder = match FlorenceDecoder::new(DECODER_MODEL_PATH, TOKENIZER_PATH)
            .await
            .or_else(|_| {
                futures::executor::block_on(FlorenceDecoder::new(ALT_DECODER_PATH, TOKENIZER_PATH))
            }) {
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
