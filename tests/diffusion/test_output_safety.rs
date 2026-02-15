// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for output safety classifier (Sub-phase 2.3)

use fabstir_llm_node::diffusion::output_safety::OutputSafetyClassifier;
use fabstir_llm_node::diffusion::safety::{SafetyCategory, SafetyConfig, SafetyLevel};

#[test]
fn test_build_classification_prompt_contains_safety_instructions() {
    let classifier = OutputSafetyClassifier::new(SafetyConfig::default());
    let prompt = classifier.build_classification_prompt();
    assert!(prompt.contains("safe") || prompt.contains("unsafe"));
    assert!(prompt.contains("JSON") || prompt.contains("json"));
}

#[test]
fn test_build_classification_prompt_covers_categories() {
    let classifier = OutputSafetyClassifier::new(SafetyConfig::default());
    let prompt = classifier.build_classification_prompt();
    // Should mention the categories being checked
    assert!(
        prompt.to_lowercase().contains("violen")
            || prompt.to_lowercase().contains("sexual")
            || prompt.to_lowercase().contains("safe")
    );
}

#[test]
fn test_parse_vlm_safety_response_safe() {
    let classifier = OutputSafetyClassifier::new(SafetyConfig::default());
    let vlm_output = r#"{"is_safe": true, "category": null, "reason": null}"#;
    let result = classifier.parse_vlm_safety_response(vlm_output);
    assert!(result.is_safe);
    assert!(result.category.is_none());
}

#[test]
fn test_parse_vlm_safety_response_unsafe_with_category() {
    let classifier = OutputSafetyClassifier::new(SafetyConfig::default());
    let vlm_output =
        r#"{"is_safe": false, "category": "sexual", "reason": "explicit content detected"}"#;
    let result = classifier.parse_vlm_safety_response(vlm_output);
    assert!(!result.is_safe);
    assert_eq!(result.category.unwrap(), SafetyCategory::Sexual);
    assert!(result.reason.unwrap().contains("explicit"));
}

#[test]
fn test_parse_vlm_safety_response_malformed_defaults_unsafe() {
    let classifier = OutputSafetyClassifier::new(SafetyConfig::default());
    let result = classifier.parse_vlm_safety_response("completely garbage text");
    assert!(
        !result.is_safe,
        "Malformed VLM response should default to unsafe"
    );
}

#[tokio::test]
async fn test_classify_image_returns_unsafe_when_vlm_unavailable() {
    let classifier = OutputSafetyClassifier::new(SafetyConfig::default());
    // Pass None for VLM client — should conservatively return unsafe
    let result = classifier.classify_image("base64data", "png", None).await;
    assert!(
        !result.is_safe,
        "Should return unsafe when VLM is unavailable"
    );
    assert!(result.reason.is_some());
}
