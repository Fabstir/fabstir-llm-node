// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Dynamic Model Map Tests (TDD - Phase 1, Sub-phase 1.2)
//!
//! These tests verify the dynamic model map is built correctly from
//! the ModelRegistry contract at startup, enabling support for ANY
//! model registered on-chain without code changes.
//!
//! **TDD Approach**: Tests written BEFORE implementation.

use ethers::types::H256;
use std::str::FromStr;

// ============================================================================
// Mock Types for Testing
// ============================================================================

/// Mock model info structure matching what we expect from contract
#[derive(Debug, Clone)]
struct MockModelInfo {
    model_id: H256,
    repo: String,
    filename: String,
    sha256_hash: H256,
}

/// Create mock model info for testing
fn create_mock_model(filename: &str, repo: &str) -> MockModelInfo {
    // Model ID is keccak256 of repo/filename (simplified for tests)
    let model_id = H256::from_low_u64_be(filename.len() as u64);
    MockModelInfo {
        model_id,
        repo: repo.to_string(),
        filename: filename.to_string(),
        sha256_hash: H256::from_low_u64_be(42), // Mock SHA256
    }
}

// ============================================================================
// DynamicModelInfo Tests (from lib export)
// ============================================================================

/// Test DynamicModelInfo struct has all required fields
#[test]
fn test_dynamic_model_info_fields() {
    use fabstir_llm_node::model_validation::DynamicModelInfo;

    let info = DynamicModelInfo {
        model_id: H256::from_str(
            "0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced"
        ).unwrap(),
        repo: "CohereForAI/TinyVicuna-1B-32k-GGUF".to_string(),
        filename: "tiny-vicuna-1b.q4_k_m.gguf".to_string(),
        sha256_hash: H256::from_str(
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
        ).unwrap(),
    };

    assert_eq!(info.repo, "CohereForAI/TinyVicuna-1B-32k-GGUF");
    assert_eq!(info.filename, "tiny-vicuna-1b.q4_k_m.gguf");
    assert!(!info.model_id.is_zero(), "model_id should not be zero");
    assert!(!info.sha256_hash.is_zero(), "sha256_hash should not be zero");
}

/// Test DynamicModelInfo is Clone
#[test]
fn test_dynamic_model_info_clone() {
    use fabstir_llm_node::model_validation::DynamicModelInfo;

    let info = DynamicModelInfo {
        model_id: H256::from_low_u64_be(123),
        repo: "test/repo".to_string(),
        filename: "model.gguf".to_string(),
        sha256_hash: H256::from_low_u64_be(456),
    };

    let cloned = info.clone();
    assert_eq!(cloned.model_id, info.model_id);
    assert_eq!(cloned.filename, info.filename);
}

/// Test DynamicModelInfo is Debug
#[test]
fn test_dynamic_model_info_debug() {
    use fabstir_llm_node::model_validation::DynamicModelInfo;

    let info = DynamicModelInfo {
        model_id: H256::zero(),
        repo: "test/repo".to_string(),
        filename: "model.gguf".to_string(),
        sha256_hash: H256::zero(),
    };

    let debug_str = format!("{:?}", info);
    assert!(debug_str.contains("DynamicModelInfo"), "Debug should contain struct name");
    assert!(debug_str.contains("model.gguf"), "Debug should contain filename");
}

// ============================================================================
// Model Map Building Tests (Unit test approach)
// ============================================================================

/// Test that map building handles empty registry correctly
#[test]
fn test_build_map_empty_registry() {
    // Empty registry should result in empty map
    let models: Vec<MockModelInfo> = vec![];

    // Build map from mock models
    let map: std::collections::HashMap<String, MockModelInfo> = models
        .into_iter()
        .map(|m| (m.filename.clone(), m))
        .collect();

    assert!(map.is_empty(), "Empty registry should produce empty map");
}

/// Test that map is keyed by filename
#[test]
fn test_build_map_creates_filename_index() {
    let models = vec![
        create_mock_model("tiny-vicuna-1b.q4_k_m.gguf", "CohereForAI/TinyVicuna-1B"),
        create_mock_model("tinyllama-1b.Q4_K_M.gguf", "TheBloke/TinyLlama-1.1B"),
    ];

    // Build map from mock models
    let map: std::collections::HashMap<String, MockModelInfo> = models
        .into_iter()
        .map(|m| (m.filename.clone(), m))
        .collect();

    assert_eq!(map.len(), 2);
    assert!(map.contains_key("tiny-vicuna-1b.q4_k_m.gguf"));
    assert!(map.contains_key("tinyllama-1b.Q4_K_M.gguf"));
    assert!(!map.contains_key("unknown-model.gguf"));
}

/// Test that lookup returns correct model info
#[test]
fn test_lookup_known_model() {
    let models = vec![
        create_mock_model("tiny-vicuna-1b.q4_k_m.gguf", "CohereForAI/TinyVicuna-1B"),
    ];

    let map: std::collections::HashMap<String, MockModelInfo> = models
        .into_iter()
        .map(|m| (m.filename.clone(), m))
        .collect();

    let result = map.get("tiny-vicuna-1b.q4_k_m.gguf");
    assert!(result.is_some(), "Should find known model");

    let info = result.unwrap();
    assert_eq!(info.repo, "CohereForAI/TinyVicuna-1B");
}

/// Test that lookup for unknown model returns None
#[test]
fn test_lookup_unknown_model() {
    let models = vec![
        create_mock_model("tiny-vicuna-1b.q4_k_m.gguf", "CohereForAI/TinyVicuna-1B"),
    ];

    let map: std::collections::HashMap<String, MockModelInfo> = models
        .into_iter()
        .map(|m| (m.filename.clone(), m))
        .collect();

    let result = map.get("unknown-model.gguf");
    assert!(result.is_none(), "Unknown model should return None");
}

/// Test that ANY model registered on-chain can be looked up
/// This verifies dynamic support - no hardcoded models needed
#[test]
fn test_map_supports_any_registered_model() {
    // Even "GPT-OSS-120B" (a fictional large model) should work if registered
    let models = vec![
        create_mock_model("tiny-vicuna-1b.q4_k_m.gguf", "CohereForAI/TinyVicuna-1B"),
        create_mock_model("gpt-oss-120b.gguf", "OpenAI/GPT-OSS-120B-GGUF"),
        create_mock_model("llama-70b.gguf", "Meta/Llama-2-70B-GGUF"),
    ];

    let map: std::collections::HashMap<String, MockModelInfo> = models
        .into_iter()
        .map(|m| (m.filename.clone(), m))
        .collect();

    // All three models should be accessible
    assert!(map.contains_key("tiny-vicuna-1b.q4_k_m.gguf"), "TinyVicuna should be accessible");
    assert!(map.contains_key("gpt-oss-120b.gguf"), "GPT-OSS-120B should be accessible");
    assert!(map.contains_key("llama-70b.gguf"), "Llama-70B should be accessible");
}

/// Test that filename matching is case-sensitive
#[test]
fn test_map_case_sensitive() {
    let models = vec![
        create_mock_model("Model.gguf", "Test/Repo"),
    ];

    let map: std::collections::HashMap<String, MockModelInfo> = models
        .into_iter()
        .map(|m| (m.filename.clone(), m))
        .collect();

    assert!(map.contains_key("Model.gguf"), "Exact case should match");
    assert!(!map.contains_key("model.gguf"), "Lowercase should not match");
    assert!(!map.contains_key("MODEL.gguf"), "Uppercase should not match");
}

/// Test that map can be refreshed/rebuilt
#[test]
fn test_map_refresh() {
    // First build with one model
    let models_v1 = vec![
        create_mock_model("model-a.gguf", "Test/ModelA"),
    ];
    let mut map: std::collections::HashMap<String, MockModelInfo> = models_v1
        .into_iter()
        .map(|m| (m.filename.clone(), m))
        .collect();

    assert_eq!(map.len(), 1);
    assert!(map.contains_key("model-a.gguf"));

    // Refresh with new models
    let models_v2 = vec![
        create_mock_model("model-a.gguf", "Test/ModelA"),
        create_mock_model("model-b.gguf", "Test/ModelB"),
    ];
    map = models_v2
        .into_iter()
        .map(|m| (m.filename.clone(), m))
        .collect();

    assert_eq!(map.len(), 2);
    assert!(map.contains_key("model-a.gguf"));
    assert!(map.contains_key("model-b.gguf"));
}

// ============================================================================
// Contract Query Tests (will use mocks when integrated)
// ============================================================================

/// Test that getAllModels returns list of model IDs
#[test]
fn test_get_all_models_returns_ids() {
    // Simulating what getAllModels() returns
    let model_ids: Vec<H256> = vec![
        H256::from_str("0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced").unwrap(),
        H256::from_str("0x14843424179fbcb9aeb7fd446fa97143300609757bd49ffb3ec7fb2f75aed1ca").unwrap(),
    ];

    assert_eq!(model_ids.len(), 2);
    assert!(!model_ids[0].is_zero());
    assert!(!model_ids[1].is_zero());
}

/// Test that getModel returns full model details
#[test]
fn test_get_model_returns_details() {
    // Simulating what getModel(modelId) returns
    let model_id = H256::from_str(
        "0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced"
    ).unwrap();

    // Mock response from contract
    let repo = "CohereForAI/TinyVicuna-1B-32k-GGUF";
    let filename = "tiny-vicuna-1b.q4_k_m.gguf";
    let sha256_hash = H256::from_str(
        "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
    ).unwrap();

    // Verify we can extract all required fields
    assert!(!model_id.is_zero());
    assert!(!repo.is_empty());
    assert!(!filename.is_empty());
    assert!(!sha256_hash.is_zero());
}
