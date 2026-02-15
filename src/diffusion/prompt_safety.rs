// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Prompt safety classifier using keyword blocklist and LLM-based classification

use crate::diffusion::safety::{SafetyCategory, SafetyConfig, SafetyResult};

/// Blocked keyword entries: (keyword, associated category)
const KEYWORD_BLOCKLIST: &[(&str, SafetyCategory)] = &[
    ("nude", SafetyCategory::Sexual),
    ("naked", SafetyCategory::Sexual),
    ("pornographic", SafetyCategory::Sexual),
    ("explicit sexual", SafetyCategory::Sexual),
    ("gore", SafetyCategory::Violence),
    ("dismember", SafetyCategory::Violence),
    ("graphic violence", SafetyCategory::Violence),
    ("suicide", SafetyCategory::SelfHarm),
    ("self-harm", SafetyCategory::SelfHarm),
    ("bomb making", SafetyCategory::Illegal),
    ("illegal drugs", SafetyCategory::Illegal),
    ("child exploitation", SafetyCategory::Illegal),
];

/// Classifier that checks image generation prompts for safety violations.
///
/// Two-stage pipeline:
/// 1. Fast keyword check (`check_keywords`) — no external calls
/// 2. LLM-based classification — caller sends the prompt from `build_classification_prompt`
///    to the LLM, then passes the response to `parse_safety_response` / `classify`
pub struct PromptSafetyClassifier {
    config: SafetyConfig,
}

impl PromptSafetyClassifier {
    pub fn new(config: SafetyConfig) -> Self {
        Self { config }
    }

    /// Fast keyword-based check. Returns a safe result if no blocked keywords are found.
    pub fn check_keywords(&self, prompt: &str) -> SafetyResult {
        let lower = prompt.to_lowercase();
        for &(keyword, category) in KEYWORD_BLOCKLIST {
            if lower.contains(keyword) && self.config.blocked_categories.contains(&category) {
                return SafetyResult {
                    is_safe: false,
                    category: Some(category),
                    reason: Some(format!("Blocked keyword detected: {}", keyword)),
                    confidence: 1.0,
                };
            }
        }
        // Also check custom blocked terms
        for term in &self.config.custom_blocked_terms {
            if lower.contains(&term.to_lowercase()) {
                return SafetyResult {
                    is_safe: false,
                    category: Some(SafetyCategory::Other),
                    reason: Some(format!("Custom blocked term detected: {}", term)),
                    confidence: 1.0,
                };
            }
        }
        SafetyResult {
            is_safe: true,
            category: None,
            reason: None,
            confidence: 1.0,
        }
    }

    /// Build an LLM classification prompt. The caller sends this to the LLM
    /// and passes the response back to `parse_safety_response`.
    pub fn build_classification_prompt(&self, user_prompt: &str) -> String {
        format!(
            "You are a content safety classifier for image generation prompts. \
             Analyze the following prompt and determine if it is safe or unsafe \
             for image generation.\n\n\
             Prompt: \"{}\"\n\n\
             Respond with a JSON object:\n\
             {{\"is_safe\": true/false, \"category\": \"violence\"|\"sexual\"|\"hate\"|\"self_harm\"|\"illegal\"|\"deceptive\"|\"other\"|null, \"reason\": \"...\" or null}}\n\n\
             Categories to check: violence, sexual, hate, self_harm, illegal, deceptive, other.\n\
             Only respond with the JSON object, no extra text.",
            user_prompt
        )
    }

    /// Parse an LLM safety response. Malformed output defaults to unsafe.
    pub fn parse_safety_response(&self, llm_output: &str) -> SafetyResult {
        match serde_json::from_str::<SafetyResult>(llm_output) {
            Ok(result) => result,
            Err(_) => SafetyResult {
                is_safe: false,
                category: Some(SafetyCategory::Other),
                reason: Some("Failed to parse safety response; defaulting to unsafe".to_string()),
                confidence: 0.0,
            },
        }
    }

    /// Classify an LLM response, applying the blocked_categories filter.
    /// If the LLM flags a category that is NOT in `blocked_categories`, the
    /// result is overridden to safe (the category is allowed at this safety level).
    pub fn classify(&self, llm_output: &str) -> SafetyResult {
        let mut result = self.parse_safety_response(llm_output);
        if !result.is_safe {
            if let Some(cat) = result.category {
                if !self.config.blocked_categories.contains(&cat) {
                    // Category not blocked at this safety level — override to safe
                    result.is_safe = true;
                    result.reason = None;
                }
            }
        }
        result
    }
}
