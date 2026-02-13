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
pub mod vlm_client;

pub use image_utils::{decode_base64_image, decode_image_bytes, detect_format, ImageError, ImageInfo};
pub use model_manager::{VisionModelConfig, VisionModelInfo, VisionModelManager};
pub use vlm_client::{VlmClient, VlmDescribeResult, VlmOcrResult};

/// Augment a user prompt with vision analysis context (v8.15.4+)
///
/// Injects image analysis **inline** into the last user message rather than as
/// a separate block. This prevents two issues:
/// 1. The LLM ignoring a separate [Image Analysis] block (e.g., missing OCR text)
/// 2. The LLM echoing the raw [Image Analysis] markers in its response
///
/// For multi-turn conversations, explicitly instructs the LLM to use ONLY the
/// new image analysis, not any previous image descriptions from conversation
/// history.
///
/// Returns the original prompt unchanged if no descriptions are provided.
pub fn augment_prompt_with_vision(descriptions: &[String], user_prompt: &str) -> String {
    if descriptions.is_empty() {
        return user_prompt.to_string();
    }
    let vision_context = descriptions.join("\n");

    // Detect multi-turn: if history contains a previous assistant response
    let has_history = user_prompt.contains("\nAssistant:");
    let override_instruction = if has_history {
        "A NEW image has been attached. IGNORE any previous image descriptions from the conversation. Use ONLY the following new analysis:\n"
    } else {
        "The attached image contains the following:\n"
    };

    // Find the last "User:" turn and inject the image context into it
    if let Some(last_user_pos) = user_prompt.rfind("\nUser:") {
        // Find the actual message content after "User: "
        let after_prefix = last_user_pos + "\nUser:".len();
        let user_msg = user_prompt[after_prefix..].trim_start();

        let mut result = String::with_capacity(user_prompt.len() + vision_context.len() + 200);
        result.push_str(&user_prompt[..last_user_pos + 1]); // history up to \n
        result.push_str("User: ");
        result.push_str(override_instruction);
        result.push_str(&vision_context);
        result.push_str("\n\nUser's question: ");
        result.push_str(user_msg);
        return result;
    }

    // Fallback for single-turn (no "User:" prefix in prompt)
    format!(
        "The attached image contains the following:\n{}\n\nUser's question: {}",
        vision_context, user_prompt
    )
}
