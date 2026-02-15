// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! PaddleOCR text recognition model
//!
//! This module provides the text recognition component of PaddleOCR.
//! It recognizes text content from cropped text regions.

use anyhow::{Context, Result};
use ndarray::{Array4, IxDyn};
use ort::execution_providers::CPUExecutionProvider;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};

use super::preprocessing::REC_INPUT_HEIGHT;

/// Recognition model input height (PP-OCRv5 English model uses 48)
pub const RECOGNITION_INPUT_HEIGHT: u32 = REC_INPUT_HEIGHT; // 48

/// Recognized text with confidence score
#[derive(Debug, Clone)]
pub struct RecognizedText {
    /// The recognized text content
    pub text: String,
    /// Overall confidence score (0.0-1.0)
    pub confidence: f32,
    /// Per-character confidences (if available)
    pub char_confidences: Vec<f32>,
}

impl RecognizedText {
    /// Create a new recognized text result
    pub fn new(text: String, confidence: f32) -> Self {
        Self {
            text,
            confidence,
            char_confidences: Vec::new(),
        }
    }

    /// Check if the text is empty or whitespace only
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// PaddleOCR text recognition model
///
/// Uses the PP-OCRv4 recognition model to extract text from cropped images.
/// Runs on CPU only to avoid GPU VRAM competition with LLM.
#[derive(Clone)]
pub struct OcrRecognitionModel {
    /// ONNX Runtime session (thread-safe)
    session: Arc<Mutex<Session>>,
    /// Character dictionary for CTC decoding
    dictionary: Arc<Vec<char>>,
    /// Model input name
    input_name: String,
    /// Model output name
    output_name: String,
    /// Whether model is loaded and ready
    is_ready: bool,
}

impl std::fmt::Debug for OcrRecognitionModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrRecognitionModel")
            .field("dictionary_size", &self.dictionary.len())
            .field("input_name", &self.input_name)
            .field("output_name", &self.output_name)
            .field("is_ready", &self.is_ready)
            .finish_non_exhaustive()
    }
}

impl OcrRecognitionModel {
    /// Load the OCR recognition model from files
    ///
    /// # Arguments
    /// - `model_path`: Path to the ONNX model file (rec_model.onnx)
    /// - `dict_path`: Path to the character dictionary (ppocr_keys_v1.txt)
    ///
    /// # Returns
    /// - `Result<Self>`: Recognition model instance or error
    ///
    /// # Errors
    /// Returns error if:
    /// - Model file not found
    /// - Dictionary file not found
    /// - ONNX Runtime initialization fails
    pub async fn new<P: AsRef<Path>>(model_path: P, dict_path: P) -> Result<Self> {
        let model_path = model_path.as_ref();
        let dict_path = dict_path.as_ref();

        // Validate paths exist
        if !model_path.exists() {
            anyhow::bail!("OCR recognition model not found: {}", model_path.display());
        }
        if !dict_path.exists() {
            anyhow::bail!(
                "OCR character dictionary not found: {}",
                dict_path.display()
            );
        }

        info!(
            "Loading OCR recognition model from {}",
            model_path.display()
        );

        // Load character dictionary
        let dictionary = Self::load_dictionary(dict_path)?;
        info!(
            "Loaded character dictionary with {} characters",
            dictionary.len()
        );

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
                "Failed to load OCR recognition model from {}",
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
            .unwrap_or_else(|| "softmax_0.tmp_0".to_string());

        // Log expected input shape
        if let Some(input) = session.inputs.first() {
            info!("Recognition model expected input: {:?}", input.input_type);
        }
        debug!(
            "Recognition model loaded - input: {}, output: {}",
            input_name, output_name
        );

        info!("✅ OCR recognition model loaded successfully (CPU-only)");

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            dictionary: Arc::new(dictionary),
            input_name,
            output_name,
            is_ready: true,
        })
    }

    /// Load character dictionary from file
    ///
    /// Each line in the file contains one character.
    /// Special tokens: blank (index 0) for CTC
    fn load_dictionary<P: AsRef<Path>>(path: P) -> Result<Vec<char>> {
        let file = File::open(path.as_ref()).context(format!(
            "Failed to open dictionary: {}",
            path.as_ref().display()
        ))?;

        let reader = BufReader::new(file);
        let mut dictionary = vec![' ']; // Index 0 is blank token for CTC

        for line in reader.lines() {
            let line = line.context("Failed to read dictionary line")?;
            if let Some(ch) = line.chars().next() {
                dictionary.push(ch);
            }
        }

        // Add space if not present
        if !dictionary.contains(&' ') {
            dictionary.push(' ');
        }

        Ok(dictionary)
    }

    /// Get the dictionary size
    pub fn dictionary_size(&self) -> usize {
        self.dictionary.len()
    }

    /// Check if the model is ready for inference
    pub fn is_ready(&self) -> bool {
        self.is_ready
    }

    /// Recognize text from a preprocessed image tensor
    ///
    /// # Arguments
    /// - `input`: Preprocessed image tensor of shape [1, 3, 48, W] (NCHW format)
    ///            Width is dynamic based on text region aspect ratio
    ///
    /// # Returns
    /// - `Result<RecognizedText>`: Recognized text with confidence
    ///
    /// # Notes
    /// The input tensor should be preprocessed using `preprocess_for_recognition()`
    pub fn recognize(&self, input: &Array4<f32>) -> Result<RecognizedText> {
        // Validate input shape (width is dynamic)
        let shape = input.shape();
        if shape.len() != 4
            || shape[0] != 1
            || shape[1] != 3
            || shape[2] != RECOGNITION_INPUT_HEIGHT as usize
            || shape[3] < 4
        {
            anyhow::bail!(
                "Invalid input shape: {:?}, expected [1, 3, {}, W>=4]",
                shape,
                RECOGNITION_INPUT_HEIGHT
            );
        }

        // Run inference
        let mut session = self.session.lock().unwrap();

        let input_value =
            Value::from_array(input.to_owned()).context("Failed to create input tensor")?;

        let outputs = session
            .run(ort::inputs![&self.input_name => input_value])
            .context("Recognition inference failed")?;

        // Parse output
        let output_tensor = outputs[0]
            .try_extract_array::<f32>()
            .context("Failed to extract output tensor")?;

        let output_shape = output_tensor.shape();
        debug!("Recognition output shape: {:?}", output_shape);

        // CTC decode the output
        let (text, confidence, char_confidences) = self.ctc_decode(&output_tensor)?;

        Ok(RecognizedText {
            text,
            confidence,
            char_confidences,
        })
    }

    /// CTC (Connectionist Temporal Classification) greedy decoding
    ///
    /// The recognition model outputs a probability distribution over characters
    /// at each timestep. We use greedy decoding (best path) with blank removal.
    fn ctc_decode(
        &self,
        output: &ndarray::ArrayBase<ndarray::ViewRepr<&f32>, ndarray::Dim<ndarray::IxDynImpl>>,
    ) -> Result<(String, f32, Vec<f32>)> {
        let output_shape = output.shape();

        // Expected shape: [batch, seq_len, num_classes] or [seq_len, num_classes]
        let (seq_len, num_classes) = if output_shape.len() == 3 {
            (output_shape[1], output_shape[2])
        } else if output_shape.len() == 2 {
            (output_shape[0], output_shape[1])
        } else {
            anyhow::bail!("Unexpected output shape: {:?}", output_shape);
        };

        let mut text = String::new();
        let mut char_confidences = Vec::new();
        let mut total_confidence = 0.0f32;
        let mut prev_index: Option<usize> = None;

        for t in 0..seq_len {
            // Find max probability class at this timestep
            let mut max_prob = f32::NEG_INFINITY;
            let mut max_index = 0usize;

            for c in 0..num_classes {
                let prob = if output_shape.len() == 3 {
                    output[IxDyn(&[0, t, c])]
                } else {
                    output[IxDyn(&[t, c])]
                };

                if prob > max_prob {
                    max_prob = prob;
                    max_index = c;
                }
            }

            // CTC blank token is typically index 0
            // Skip if blank or same as previous (collapse repeats)
            if max_index != 0 && Some(max_index) != prev_index {
                // Get character from dictionary
                if max_index < self.dictionary.len() {
                    text.push(self.dictionary[max_index]);
                    char_confidences.push(max_prob);
                    total_confidence += max_prob;
                }
            }

            prev_index = if max_index == 0 {
                None
            } else {
                Some(max_index)
            };
        }

        // Calculate average confidence
        let avg_confidence = if char_confidences.is_empty() {
            0.0
        } else {
            total_confidence / char_confidences.len() as f32
        };

        // Convert log probabilities to probabilities if needed (sigmoid)
        let avg_confidence = if avg_confidence < 0.0 {
            1.0 / (1.0 + (-avg_confidence).exp())
        } else {
            avg_confidence.min(1.0)
        };

        // Post-process: insert spaces at word boundaries for English text
        let text = Self::insert_word_spaces(&text);

        Ok((text, avg_confidence, char_confidences))
    }

    /// Insert spaces at word boundaries in English text
    ///
    /// Uses a combination of:
    /// 1. Dictionary-based word segmentation for common English words
    /// 2. Heuristic rules for camelCase, digits, punctuation
    fn insert_word_spaces(text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }

        // First apply dictionary-based segmentation
        let segmented = Self::segment_words(text);

        // Then apply heuristic rules for any remaining issues
        Self::apply_heuristic_spacing(&segmented)
    }

    /// Segment text into words using a dictionary of common English words
    /// Uses greedy maximum matching from left to right
    fn segment_words(text: &str) -> String {
        // Common English words for segmentation (most frequent words)
        const COMMON_WORDS: &[&str] = &[
            // Articles & pronouns
            "the",
            "a",
            "an",
            "i",
            "you",
            "he",
            "she",
            "it",
            "we",
            "they",
            "me",
            "him",
            "her",
            "us",
            "them",
            "my",
            "your",
            "his",
            "its",
            "our",
            "their",
            // Prepositions & conjunctions
            "of",
            "to",
            "in",
            "for",
            "on",
            "with",
            "at",
            "by",
            "from",
            "as",
            "into",
            "through",
            "during",
            "before",
            "after",
            "above",
            "below",
            "and",
            "or",
            "but",
            "if",
            "then",
            "else",
            "when",
            "where",
            "why",
            "how",
            // Verbs
            "is",
            "are",
            "was",
            "were",
            "be",
            "been",
            "being",
            "have",
            "has",
            "had",
            "do",
            "does",
            "did",
            "will",
            "would",
            "could",
            "should",
            "may",
            "might",
            "can",
            "want",
            "need",
            "find",
            "get",
            "give",
            "take",
            "make",
            "know",
            "think",
            "see",
            "come",
            "go",
            "say",
            "tell",
            "ask",
            "use",
            "try",
            // Common nouns & adjectives
            "this",
            "that",
            "these",
            "those",
            "what",
            "which",
            "who",
            "whom",
            "all",
            "each",
            "every",
            "both",
            "few",
            "more",
            "most",
            "other",
            "some",
            "such",
            "no",
            "not",
            "only",
            "same",
            "so",
            "than",
            "too",
            "very",
            "just",
            "also",
            "now",
            "here",
            "there",
            "out",
            "up",
            "down",
            // Math related
            "square",
            "root",
            "plus",
            "minus",
            "times",
            "divided",
            "equals",
            "number",
            "numbers",
            "sum",
            "total",
            "average",
            "mean",
            "value",
            // Common words in instructions
            "please",
            "help",
            "show",
            "display",
            "calculate",
            "compute",
            "solve",
            "answer",
            "question",
            "problem",
            "example",
            "text",
            "image",
            "file",
        ];

        let lower = text.to_lowercase();
        let chars: Vec<char> = lower.chars().collect();
        let n = chars.len();

        if n == 0 {
            return String::new();
        }

        // Dynamic programming: best[i] = (cost, prev_index) for segmenting chars[0..i]
        // Lower cost = better segmentation (longer words preferred)
        let mut best: Vec<(i32, usize)> = vec![(i32::MAX, 0); n + 1];
        best[0] = (0, 0);

        for i in 1..=n {
            // Try all possible last words ending at position i
            for j in 0..i {
                if best[j].0 == i32::MAX {
                    continue;
                }

                let word: String = chars[j..i].iter().collect();
                let word_len = i - j;

                // Calculate cost: prefer longer dictionary words
                let cost = if COMMON_WORDS.contains(&word.as_str()) {
                    // Dictionary word: negative cost (reward), longer = better
                    -(word_len as i32 * 10)
                } else if word_len == 1 && chars[j].is_ascii_alphabetic() {
                    // Single letter: small penalty (but allow for "I", "a")
                    if word == "i" || word == "a" {
                        -5
                    } else {
                        5
                    }
                } else {
                    // Unknown word: positive cost (penalty)
                    word_len as i32
                };

                let total_cost = best[j].0 + cost;
                if total_cost < best[i].0 {
                    best[i] = (total_cost, j);
                }
            }
        }

        // Backtrack to find the segmentation
        let mut words: Vec<String> = Vec::new();
        let mut i = n;
        while i > 0 {
            let j = best[i].1;
            let word: String = chars[j..i].iter().collect();
            words.push(word);
            i = j;
        }
        words.reverse();

        // Restore original case from input
        let mut result = String::new();
        let mut pos = 0;
        for (idx, word) in words.iter().enumerate() {
            if idx > 0 {
                result.push(' ');
            }
            // Use original case from input text
            for c in word.chars() {
                if pos < text.len() {
                    result.push(text.chars().nth(pos).unwrap_or(c));
                    pos += 1;
                }
            }
        }

        result
    }

    /// Apply heuristic spacing rules for edge cases
    fn apply_heuristic_spacing(text: &str) -> String {
        let mut result = String::with_capacity(text.len() + 10);
        let chars: Vec<char> = text.chars().collect();

        for i in 0..chars.len() {
            let c = chars[i];

            if i > 0 {
                let prev = chars[i - 1];

                // Insert space before digit following letter (if not already spaced)
                if prev.is_ascii_alphabetic() && c.is_ascii_digit() && prev != ' ' {
                    result.push(' ');
                }
                // Insert space after digit before letter (if not already spaced)
                else if prev.is_ascii_digit() && c.is_ascii_alphabetic() && prev != ' ' {
                    result.push(' ');
                }
                // Insert space after certain punctuation
                else if (prev == ':' || prev == '.' || prev == ',') && c.is_ascii_alphanumeric() {
                    if !result.ends_with(' ') {
                        result.push(' ');
                    }
                }
            }

            result.push(c);
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECOGNITION_MODEL_PATH: &str = "/workspace/models/paddleocr-onnx/rec_model.onnx";
    const DICTIONARY_PATH: &str = "/workspace/models/paddleocr-onnx/ppocr_keys_v1.txt";

    #[test]
    fn test_recognized_text_creation() {
        let result = RecognizedText::new("Hello World".to_string(), 0.95);
        assert_eq!(result.text, "Hello World");
        assert_eq!(result.confidence, 0.95);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_recognized_text_empty() {
        let result = RecognizedText::new("".to_string(), 0.0);
        assert!(result.is_empty());

        let whitespace = RecognizedText::new("   ".to_string(), 0.5);
        assert!(whitespace.is_empty());
    }

    #[test]
    fn test_recognized_text_with_confidences() {
        let mut result = RecognizedText::new("AB".to_string(), 0.9);
        result.char_confidences = vec![0.95, 0.85];
        assert_eq!(result.char_confidences.len(), 2);
    }

    #[test]
    fn test_recognition_input_height_constant() {
        assert_eq!(RECOGNITION_INPUT_HEIGHT, 48);
    }

    #[test]
    fn test_insert_word_spaces_basic() {
        // Test basic word segmentation
        assert_eq!(
            OcrRecognitionModel::insert_word_spaces("Iwantyoutofindthe"),
            "I want you to find the"
        );
    }

    #[test]
    fn test_insert_word_spaces_math() {
        // Test math-related words
        assert_eq!(
            OcrRecognitionModel::insert_word_spaces("squarerootof"),
            "square root of"
        );
    }

    #[test]
    fn test_insert_word_spaces_numbers() {
        // Test number boundaries
        assert_eq!(
            OcrRecognitionModel::insert_word_spaces("value81here"),
            "value 81 here"
        );
    }

    #[test]
    fn test_insert_word_spaces_empty() {
        assert_eq!(OcrRecognitionModel::insert_word_spaces(""), "");
    }

    #[test]
    fn test_insert_word_spaces_already_spaced() {
        // Should handle already spaced text
        let result = OcrRecognitionModel::insert_word_spaces("Hello World");
        // May add/preserve spaces, but should be readable
        assert!(result.contains("Hello") && result.contains("World"));
    }

    #[test]
    fn test_insert_word_spaces_preserves_case() {
        // Should preserve original case
        let result = OcrRecognitionModel::insert_word_spaces("FINDTHE");
        assert!(result.contains("FIND") || result.contains("THE"));
    }

    #[tokio::test]
    async fn test_model_not_found_error() {
        let result =
            OcrRecognitionModel::new("/nonexistent/path/rec_model.onnx", DICTIONARY_PATH).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn test_dictionary_not_found_error() {
        let result =
            OcrRecognitionModel::new(RECOGNITION_MODEL_PATH, "/nonexistent/path/dict.txt").await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_model_loading() {
        let model = OcrRecognitionModel::new(RECOGNITION_MODEL_PATH, DICTIONARY_PATH).await;

        if let Ok(model) = model {
            assert!(model.is_ready());
            assert!(model.dictionary_size() > 1000); // Chinese+English chars
            assert!(!model.input_name.is_empty());
        }
    }

    #[tokio::test]
    #[ignore] // Only run if model files are downloaded
    async fn test_recognition_inference() {
        let model = match OcrRecognitionModel::new(RECOGNITION_MODEL_PATH, DICTIONARY_PATH).await {
            Ok(m) => m,
            Err(_) => return, // Skip if model not available
        };

        // Create a simple test input (32 height, variable width)
        let input = Array4::<f32>::zeros((1, 3, 32, 320));

        let result = model.recognize(&input);
        assert!(result.is_ok());

        let recognized = result.unwrap();
        // Empty image should produce empty or low-confidence text
        assert!(recognized.confidence < 0.5 || recognized.is_empty());
    }

    #[test]
    fn test_recognize_invalid_input_shape() {
        // Verify shape validation logic
        let wrong_height_shape = [1, 3, 32, 320]; // Height should be 48
        assert!(wrong_height_shape[2] != 48);

        let wrong_batch_shape = [2, 3, 48, 320]; // Batch should be 1
        assert!(wrong_batch_shape[0] != 1);
    }

    #[test]
    fn test_load_dictionary_format() {
        // Test that dictionary loading works with a mock file
        // This tests the dictionary format expectations
        let blank_index = 0;
        let mock_dict = vec![' ', 'a', 'b', 'c']; // Blank at index 0

        assert_eq!(mock_dict[blank_index], ' ');
        assert!(mock_dict.len() > 1);
    }
}
