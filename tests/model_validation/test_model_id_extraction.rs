// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Model ID Extraction Tests (TDD - Phase 1, Sub-phase 1.4)
//!
//! These tests verify model ID extraction from MODEL_PATH filename
//! using the dynamic map built from ModelRegistry at startup.
//!
//! **Key Design**: Uses dynamic map (no hardcoded models) - any model
//! registered on-chain is automatically supported.
//!
//! **TDD Approach**: Tests written BEFORE implementation.

use ethers::types::H256;
use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

use fabstir_llm_node::model_validation::{DynamicModelInfo, ModelValidationError};

// ============================================================================
// Path Extraction Tests (Unit tests for filename extraction)
// ============================================================================

/// Test extracting filename from various path formats
#[test]
fn test_extract_filename_from_path() {
    // Unix-style paths
    let path = Path::new("/models/tiny-vicuna-1b.q4_k_m.gguf");
    assert_eq!(path.file_name().unwrap().to_str().unwrap(), "tiny-vicuna-1b.q4_k_m.gguf");

    // Relative path
    let path = Path::new("./models/model.gguf");
    assert_eq!(path.file_name().unwrap().to_str().unwrap(), "model.gguf");

    // Just filename
    let path = Path::new("model.gguf");
    assert_eq!(path.file_name().unwrap().to_str().unwrap(), "model.gguf");
}

/// Test path with no filename returns None
#[test]
fn test_extract_filename_empty_path() {
    let path = Path::new("/");
    // Root path has no file_name
    assert!(path.file_name().is_none());
}

/// Test path ending in directory returns None
#[test]
fn test_extract_filename_directory_path() {
    let path = Path::new("/models/");
    // file_name() on dir path is implementation-dependent
    // Path::new("/models/").file_name() returns Some("models") on Unix
    // So we need to check for the file extension or use different logic
    let filename = path.file_name().and_then(|n| n.to_str());
    // If we have a filename without .gguf extension, it's invalid
    if let Some(name) = filename {
        assert!(!name.ends_with(".gguf"), "Directory should not be treated as model file");
    }
}

// ============================================================================
// Dynamic Map Lookup Tests
// ============================================================================

/// Test looking up a registered model in dynamic map
#[test]
fn test_lookup_registered_model_in_map() {
    let model_id = H256::from_str(
        "0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced"
    ).unwrap();

    let mut map: HashMap<String, DynamicModelInfo> = HashMap::new();
    map.insert(
        "tiny-vicuna-1b.q4_k_m.gguf".to_string(),
        DynamicModelInfo {
            model_id,
            repo: "CohereForAI/TinyVicuna-1B-32k-GGUF".to_string(),
            filename: "tiny-vicuna-1b.q4_k_m.gguf".to_string(),
            sha256_hash: H256::zero(),
        },
    );

    // Extract filename from path
    let path = Path::new("/models/tiny-vicuna-1b.q4_k_m.gguf");
    let filename = path.file_name().unwrap().to_str().unwrap();

    // Lookup in map
    let result = map.get(filename);
    assert!(result.is_some(), "Registered model should be found");
    assert_eq!(result.unwrap().model_id, model_id);
}

/// Test looking up unregistered model returns None
#[test]
fn test_lookup_unregistered_model_in_map() {
    let model_id = H256::from_str(
        "0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced"
    ).unwrap();

    let mut map: HashMap<String, DynamicModelInfo> = HashMap::new();
    map.insert(
        "tiny-vicuna-1b.q4_k_m.gguf".to_string(),
        DynamicModelInfo {
            model_id,
            repo: "CohereForAI/TinyVicuna-1B-32k-GGUF".to_string(),
            filename: "tiny-vicuna-1b.q4_k_m.gguf".to_string(),
            sha256_hash: H256::zero(),
        },
    );

    // Try to find unregistered model
    let path = Path::new("/models/unknown-model.gguf");
    let filename = path.file_name().unwrap().to_str().unwrap();

    let result = map.get(filename);
    assert!(result.is_none(), "Unregistered model should return None");
}

/// Test any model registered on-chain works (dynamic support)
#[test]
fn test_any_registered_model_works() {
    // Even "GPT-OSS-120B" works if registered
    let gpt_model_id = H256::from_str(
        "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
    ).unwrap();

    let mut map: HashMap<String, DynamicModelInfo> = HashMap::new();
    map.insert(
        "gpt-oss-120b.gguf".to_string(),
        DynamicModelInfo {
            model_id: gpt_model_id,
            repo: "OpenAI/GPT-OSS-120B-GGUF".to_string(),
            filename: "gpt-oss-120b.gguf".to_string(),
            sha256_hash: H256::zero(),
        },
    );

    let path = Path::new("/models/gpt-oss-120b.gguf");
    let filename = path.file_name().unwrap().to_str().unwrap();

    let result = map.get(filename);
    assert!(result.is_some(), "GPT-OSS-120B should be found if registered");
    assert_eq!(result.unwrap().model_id, gpt_model_id);
}

/// Test production model (Llama 70B) works if registered
#[test]
fn test_production_model_works() {
    let llama_model_id = H256::from_str(
        "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
    ).unwrap();

    let mut map: HashMap<String, DynamicModelInfo> = HashMap::new();
    map.insert(
        "llama-2-70b.Q4_K_M.gguf".to_string(),
        DynamicModelInfo {
            model_id: llama_model_id,
            repo: "Meta-Llama/Llama-2-70b-GGUF".to_string(),
            filename: "llama-2-70b.Q4_K_M.gguf".to_string(),
            sha256_hash: H256::zero(),
        },
    );

    let path = Path::new("/models/llama-2-70b.Q4_K_M.gguf");
    let filename = path.file_name().unwrap().to_str().unwrap();

    let result = map.get(filename);
    assert!(result.is_some(), "Llama 70B should be found if registered");
    assert_eq!(result.unwrap().model_id, llama_model_id);
}

// ============================================================================
// Error Case Tests
// ============================================================================

/// Test ModelNotRegistered error for unknown model
#[test]
fn test_error_model_not_registered() {
    let err = ModelValidationError::ModelNotRegistered(
        "Model file 'unknown-model.gguf' is not registered in ModelRegistry.".to_string()
    );

    let msg = format!("{}", err);
    assert!(msg.contains("not registered"), "Error should mention 'not registered'");
    assert!(msg.contains("unknown-model.gguf"), "Error should contain filename");
}

/// Test InvalidModelPath error for paths with no filename
#[test]
fn test_error_invalid_model_path() {
    let err = ModelValidationError::InvalidModelPath(
        "Cannot extract filename from path: /".to_string()
    );

    let msg = format!("{}", err);
    assert!(msg.contains("Invalid") || msg.contains("invalid"), "Error should indicate invalid path");
}

/// Test case sensitivity - filename must match exactly
#[test]
fn test_case_sensitivity() {
    let model_id = H256::from_str(
        "0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced"
    ).unwrap();

    let mut map: HashMap<String, DynamicModelInfo> = HashMap::new();
    map.insert(
        "TinyVicuna.gguf".to_string(),
        DynamicModelInfo {
            model_id,
            repo: "Test/Repo".to_string(),
            filename: "TinyVicuna.gguf".to_string(),
            sha256_hash: H256::zero(),
        },
    );

    // Exact case - should work
    assert!(map.get("TinyVicuna.gguf").is_some());

    // Different case - should NOT work
    assert!(map.get("tinyvicuna.gguf").is_none());
    assert!(map.get("TINYVICUNA.GGUF").is_none());
}
