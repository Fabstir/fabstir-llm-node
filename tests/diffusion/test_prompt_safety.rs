// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for prompt safety classifier (Sub-phase 2.2)

use fabstir_llm_node::diffusion::prompt_safety::PromptSafetyClassifier;
use fabstir_llm_node::diffusion::safety::{SafetyCategory, SafetyConfig, SafetyLevel};

#[test]
fn test_keyword_blocklist_detects_unsafe_terms() {
    let classifier = PromptSafetyClassifier::new(SafetyConfig::default());
    let result = classifier.check_keywords("generate a nude image of a person");
    assert!(!result.is_safe, "Should detect unsafe keyword");
    assert!(result.category.is_some());
}

#[test]
fn test_benign_prompt_passes_keyword_check() {
    let classifier = PromptSafetyClassifier::new(SafetyConfig::default());
    let result = classifier.check_keywords("a beautiful sunset over the ocean");
    assert!(result.is_safe, "Benign prompt should pass keyword check");
    assert!(result.category.is_none());
}

#[test]
fn test_classification_prompt_format_is_valid() {
    let classifier = PromptSafetyClassifier::new(SafetyConfig::default());
    let prompt = classifier.build_classification_prompt("a cat sitting on a windowsill");
    assert!(prompt.contains("cat sitting on a windowsill"));
    assert!(prompt.contains("safe") || prompt.contains("unsafe"));
    // Should contain JSON format instruction
    assert!(prompt.contains("JSON") || prompt.contains("json"));
}

#[test]
fn test_parse_safety_response_safe() {
    let classifier = PromptSafetyClassifier::new(SafetyConfig::default());
    let llm_output = r#"{"is_safe": true, "category": null, "reason": null}"#;
    let result = classifier.parse_safety_response(llm_output);
    assert!(result.is_safe);
    assert!(result.category.is_none());
}

#[test]
fn test_parse_safety_response_unsafe() {
    let classifier = PromptSafetyClassifier::new(SafetyConfig::default());
    let llm_output =
        r#"{"is_safe": false, "category": "violence", "reason": "depicts graphic violence"}"#;
    let result = classifier.parse_safety_response(llm_output);
    assert!(!result.is_safe);
    assert_eq!(result.category.unwrap(), SafetyCategory::Violence);
    assert!(result.reason.unwrap().contains("violence"));
}

#[test]
fn test_parse_safety_response_malformed_defaults_unsafe() {
    let classifier = PromptSafetyClassifier::new(SafetyConfig::default());
    let result = classifier.parse_safety_response("this is not valid json at all");
    assert!(
        !result.is_safe,
        "Malformed response should default to unsafe"
    );
}

#[test]
fn test_strict_blocks_more_than_moderate() {
    let strict = PromptSafetyClassifier::new(SafetyConfig {
        level: SafetyLevel::Strict,
        blocked_categories: vec![
            SafetyCategory::Sexual,
            SafetyCategory::Violence,
            SafetyCategory::Illegal,
            SafetyCategory::SelfHarm,
            SafetyCategory::Hate,
            SafetyCategory::Deceptive,
        ],
        custom_blocked_terms: Vec::new(),
    });
    let moderate = PromptSafetyClassifier::new(SafetyConfig {
        level: SafetyLevel::Moderate,
        blocked_categories: vec![
            SafetyCategory::Sexual,
            SafetyCategory::Violence,
            SafetyCategory::Illegal,
            SafetyCategory::SelfHarm,
        ],
        custom_blocked_terms: Vec::new(),
    });

    // Strict should block a category that moderate does not
    let hate_response = r#"{"is_safe": false, "category": "hate", "reason": "hate speech"}"#;
    let strict_result = strict.classify(hate_response);
    let moderate_result = moderate.classify(hate_response);

    assert!(!strict_result.is_safe, "Strict should block hate category");
    // Moderate doesn't have hate in blocked_categories, so it still flags it from LLM
    // but the classify method should respect blocked_categories
    assert!(
        moderate_result.is_safe,
        "Moderate should allow hate category (not in blocked list)"
    );
}

#[test]
fn test_permissive_only_blocks_illegal_and_self_harm() {
    let permissive = PromptSafetyClassifier::new(SafetyConfig {
        level: SafetyLevel::Permissive,
        blocked_categories: vec![SafetyCategory::Illegal, SafetyCategory::SelfHarm],
        custom_blocked_terms: Vec::new(),
    });

    // Violence should be allowed in permissive mode
    let violence_response =
        r#"{"is_safe": false, "category": "violence", "reason": "action scene"}"#;
    let result = permissive.classify(violence_response);
    assert!(
        result.is_safe,
        "Permissive should allow violence (not in blocked list)"
    );

    // Illegal should still be blocked
    let illegal_response =
        r#"{"is_safe": false, "category": "illegal", "reason": "illegal activity"}"#;
    let result = permissive.classify(illegal_response);
    assert!(!result.is_safe, "Permissive should still block illegal");
}
