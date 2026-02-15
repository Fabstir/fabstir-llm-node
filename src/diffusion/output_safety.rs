// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Output safety classifier using VLM sidecar for generated image classification

use crate::diffusion::safety::{SafetyCategory, SafetyConfig, SafetyResult};
use crate::vision::vlm_client::VlmClient;

/// Classifier that checks generated images for safety violations via VLM sidecar.
///
/// Uses the existing VlmClient to send classification requests to the Qwen3-VL
/// vision model. If the VLM is unavailable, conservatively returns unsafe.
pub struct OutputSafetyClassifier {
    config: SafetyConfig,
}

impl OutputSafetyClassifier {
    pub fn new(config: SafetyConfig) -> Self {
        Self { config }
    }

    /// Build the VLM classification prompt for image safety analysis
    pub fn build_classification_prompt(&self) -> String {
        "Analyze this image for content safety. Determine if it contains any unsafe content \
         including: violence, sexual/explicit content, hate symbols, self-harm depictions, \
         illegal activities, or deceptive content.\n\n\
         Respond with a JSON object:\n\
         {\"is_safe\": true/false, \"category\": \"violence\"|\"sexual\"|\"hate\"|\"self_harm\"|\"illegal\"|\"deceptive\"|\"other\"|null, \"reason\": \"...\" or null}\n\n\
         Only respond with the JSON object, no extra text."
            .to_string()
    }

    /// Parse the VLM safety classification response. Malformed output defaults to unsafe.
    pub fn parse_vlm_safety_response(&self, vlm_output: &str) -> SafetyResult {
        match serde_json::from_str::<SafetyResult>(vlm_output) {
            Ok(result) => result,
            Err(_) => SafetyResult {
                is_safe: false,
                category: Some(SafetyCategory::Other),
                reason: Some(
                    "Failed to parse VLM safety response; defaulting to unsafe".to_string(),
                ),
                confidence: 0.0,
            },
        }
    }

    /// Classify a generated image for safety. If `vlm_client` is None, returns unsafe.
    pub async fn classify_image(
        &self,
        base64_image: &str,
        format: &str,
        vlm_client: Option<&VlmClient>,
    ) -> SafetyResult {
        let vlm = match vlm_client {
            Some(c) => c,
            None => {
                return SafetyResult {
                    is_safe: false,
                    category: None,
                    reason: Some("VLM sidecar unavailable; cannot verify image safety".to_string()),
                    confidence: 0.0,
                };
            }
        };

        let prompt = self.build_classification_prompt();
        match vlm
            .describe(base64_image, format, "detailed", Some(&prompt))
            .await
        {
            Ok(result) => self.parse_vlm_safety_response(&result.description),
            Err(e) => SafetyResult {
                is_safe: false,
                category: None,
                reason: Some(format!("VLM classification failed: {}", e)),
                confidence: 0.0,
            },
        }
    }
}
