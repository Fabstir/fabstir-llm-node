// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Tests for safety types and configuration (Sub-phase 2.1)

use fabstir_llm_node::diffusion::safety::{
    SafetyAttestation, SafetyCategory, SafetyConfig, SafetyLevel, SafetyResult,
};

#[test]
fn test_safety_level_serde_roundtrip() {
    for level in [
        SafetyLevel::Strict,
        SafetyLevel::Moderate,
        SafetyLevel::Permissive,
    ] {
        let json = serde_json::to_string(&level).unwrap();
        let deserialized: SafetyLevel = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, level);
    }
}

#[test]
fn test_safety_level_serde_snake_case() {
    assert_eq!(
        serde_json::to_string(&SafetyLevel::Strict).unwrap(),
        "\"strict\""
    );
    assert_eq!(
        serde_json::to_string(&SafetyLevel::Moderate).unwrap(),
        "\"moderate\""
    );
    assert_eq!(
        serde_json::to_string(&SafetyLevel::Permissive).unwrap(),
        "\"permissive\""
    );
}

#[test]
fn test_safety_level_default_is_strict() {
    let level = SafetyLevel::default();
    assert_eq!(level, SafetyLevel::Strict);
}

#[test]
fn test_safety_category_serde_roundtrip() {
    let categories = [
        SafetyCategory::Violence,
        SafetyCategory::Sexual,
        SafetyCategory::Hate,
        SafetyCategory::SelfHarm,
        SafetyCategory::Illegal,
        SafetyCategory::Deceptive,
        SafetyCategory::Other,
    ];
    for cat in categories {
        let json = serde_json::to_string(&cat).unwrap();
        let deserialized: SafetyCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, cat);
    }
}

#[test]
fn test_safety_config_with_blocked_categories() {
    let config = SafetyConfig {
        level: SafetyLevel::Moderate,
        blocked_categories: vec![SafetyCategory::Violence, SafetyCategory::Hate],
        custom_blocked_terms: vec!["badword".to_string()],
    };
    assert_eq!(config.level, SafetyLevel::Moderate);
    assert_eq!(config.blocked_categories.len(), 2);
    assert_eq!(config.custom_blocked_terms.len(), 1);
}

#[test]
fn test_safety_config_default_blocks_expected() {
    let config = SafetyConfig::default();
    assert_eq!(config.level, SafetyLevel::Strict);
    assert!(config.blocked_categories.contains(&SafetyCategory::Sexual));
    assert!(config
        .blocked_categories
        .contains(&SafetyCategory::Violence));
    assert!(config.blocked_categories.contains(&SafetyCategory::Illegal));
    assert!(config
        .blocked_categories
        .contains(&SafetyCategory::SelfHarm));
    assert_eq!(config.blocked_categories.len(), 4);
    assert!(config.custom_blocked_terms.is_empty());
}

#[test]
fn test_safety_result_safe_case() {
    let result = SafetyResult {
        is_safe: true,
        category: None,
        reason: None,
        confidence: 0.95,
    };
    assert!(result.is_safe);
    assert!(result.category.is_none());
    assert!(result.reason.is_none());
    assert!(result.confidence > 0.9);
}

#[test]
fn test_safety_result_unsafe_case() {
    let result = SafetyResult {
        is_safe: false,
        category: Some(SafetyCategory::Violence),
        reason: Some("Contains violent imagery request".to_string()),
        confidence: 0.87,
    };
    assert!(!result.is_safe);
    assert_eq!(result.category.unwrap(), SafetyCategory::Violence);
    assert!(result.reason.unwrap().contains("violent"));
    assert!((result.confidence - 0.87).abs() < f32::EPSILON);
}

#[test]
fn test_safety_attestation_compute_hash_returns_32_bytes() {
    let attestation = SafetyAttestation {
        prompt_hash: [0u8; 32],
        prompt_safe: true,
        output_hash: None,
        output_safe: None,
        safety_level: SafetyLevel::Strict,
        timestamp: 1700000000,
    };
    let hash = attestation.compute_hash();
    assert_eq!(hash.len(), 32);
    // Hash should not be all zeros
    assert!(hash.iter().any(|&b| b != 0));
}

#[test]
fn test_safety_attestation_compute_hash_is_deterministic() {
    let attestation = SafetyAttestation {
        prompt_hash: [42u8; 32],
        prompt_safe: false,
        output_hash: Some([1u8; 32]),
        output_safe: Some(true),
        safety_level: SafetyLevel::Moderate,
        timestamp: 1700000000,
    };
    let hash1 = attestation.compute_hash();
    let hash2 = attestation.compute_hash();
    assert_eq!(hash1, hash2);
}

#[test]
fn test_safety_attestation_to_bytes() {
    let attestation = SafetyAttestation {
        prompt_hash: [7u8; 32],
        prompt_safe: true,
        output_hash: None,
        output_safe: None,
        safety_level: SafetyLevel::Permissive,
        timestamp: 1700000000,
    };
    let bytes = attestation.to_bytes();
    assert!(!bytes.is_empty());
    // Should be deserializable back
    let deserialized: SafetyAttestation = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(deserialized.prompt_safe, true);
    assert_eq!(deserialized.timestamp, 1700000000);
    assert_eq!(deserialized.safety_level, SafetyLevel::Permissive);
}
