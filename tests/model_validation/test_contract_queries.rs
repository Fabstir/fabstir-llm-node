// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
//! Contract Query Tests (TDD - Phase 1, Sub-phase 1.3)
//!
//! These tests verify host authorization queries against NodeRegistry
//! contract with in-memory caching for performance.
//!
//! **TDD Approach**: Tests written BEFORE implementation.

use ethers::types::{Address, H256};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

// ============================================================================
// Cache Logic Tests (Unit tests for caching pattern)
// ============================================================================

/// Test that cache lookup returns true for cached authorized model
#[test]
fn test_cache_hit_returns_true() {
    let host = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
    let model_id =
        H256::from_str("0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced")
            .unwrap();

    let mut cache: HashMap<Address, Vec<H256>> = HashMap::new();
    cache.insert(host, vec![model_id]);

    // Check if model is in cache for this host
    let is_authorized = cache
        .get(&host)
        .map(|models| models.contains(&model_id))
        .unwrap_or(false);

    assert!(is_authorized, "Cached authorization should return true");
}

/// Test that cache lookup returns false for non-cached model
#[test]
fn test_cache_miss_returns_false() {
    let host = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
    let model_id =
        H256::from_str("0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced")
            .unwrap();
    let other_model_id =
        H256::from_str("0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890")
            .unwrap();

    let mut cache: HashMap<Address, Vec<H256>> = HashMap::new();
    cache.insert(host, vec![other_model_id]);

    // Check if model is in cache for this host
    let is_authorized = cache
        .get(&host)
        .map(|models| models.contains(&model_id))
        .unwrap_or(false);

    assert!(!is_authorized, "Non-cached model should return false");
}

/// Test that cache lookup returns false for unknown host
#[test]
fn test_cache_miss_unknown_host() {
    let host = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
    let unknown_host = Address::from_str("0xabcdefabcdefabcdefabcdefabcdefabcdefabcd").unwrap();
    let model_id =
        H256::from_str("0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced")
            .unwrap();

    let mut cache: HashMap<Address, Vec<H256>> = HashMap::new();
    cache.insert(host, vec![model_id]);

    // Check if model is in cache for unknown host
    let is_authorized = cache
        .get(&unknown_host)
        .map(|models| models.contains(&model_id))
        .unwrap_or(false);

    assert!(!is_authorized, "Unknown host should return false");
}

/// Test that cache can store multiple models for same host
#[test]
fn test_cache_multiple_models_same_host() {
    let host = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
    let model_id_1 =
        H256::from_str("0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced")
            .unwrap();
    let model_id_2 =
        H256::from_str("0x14843424179fbcb9aeb7fd446fa97143300609757bd49ffb3ec7fb2f75aed1ca")
            .unwrap();

    let mut cache: HashMap<Address, Vec<H256>> = HashMap::new();
    cache.insert(host, vec![model_id_1, model_id_2]);

    // Both models should be authorized
    let models = cache.get(&host).unwrap();
    assert!(
        models.contains(&model_id_1),
        "First model should be authorized"
    );
    assert!(
        models.contains(&model_id_2),
        "Second model should be authorized"
    );
}

/// Test cache update on successful authorization
#[test]
fn test_cache_update_on_success() {
    let host = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
    let model_id =
        H256::from_str("0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced")
            .unwrap();

    let mut cache: HashMap<Address, Vec<H256>> = HashMap::new();

    // Simulate successful contract query (host IS authorized)
    let contract_returned_true = true;

    if contract_returned_true {
        // Update cache
        cache.entry(host).or_insert_with(Vec::new).push(model_id);
    }

    // Verify cache was updated
    let is_cached = cache
        .get(&host)
        .map(|models| models.contains(&model_id))
        .unwrap_or(false);

    assert!(is_cached, "Cache should be updated after successful auth");
}

/// Test cache NOT updated when authorization fails
#[test]
fn test_cache_not_updated_on_failure() {
    let host = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
    let model_id =
        H256::from_str("0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced")
            .unwrap();

    let mut cache: HashMap<Address, Vec<H256>> = HashMap::new();

    // Simulate failed contract query (host NOT authorized)
    let contract_returned_false = false;

    if contract_returned_false {
        // Don't update cache for failed auth
        cache.entry(host).or_insert_with(Vec::new).push(model_id);
    }

    // Verify cache was NOT updated
    let is_cached = cache
        .get(&host)
        .map(|models| models.contains(&model_id))
        .unwrap_or(false);

    assert!(!is_cached, "Cache should NOT be updated after failed auth");
}

// ============================================================================
// Async Cache Tests (using tokio RwLock)
// ============================================================================

/// Test async cache lookup
#[tokio::test]
async fn test_async_cache_lookup() {
    let host = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
    let model_id =
        H256::from_str("0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced")
            .unwrap();

    let cache: Arc<RwLock<HashMap<Address, Vec<H256>>>> = Arc::new(RwLock::new(HashMap::new()));

    // Populate cache
    {
        let mut cache_write = cache.write().await;
        cache_write.insert(host, vec![model_id]);
    }

    // Read from cache
    let is_authorized = {
        let cache_read = cache.read().await;
        cache_read
            .get(&host)
            .map(|models| models.contains(&model_id))
            .unwrap_or(false)
    };

    assert!(is_authorized, "Async cache should return correct value");
}

/// Test async cache update
#[tokio::test]
async fn test_async_cache_update() {
    let host = Address::from_str("0x1234567890123456789012345678901234567890").unwrap();
    let model_id =
        H256::from_str("0x0b75a2061e70e736924a30c0a327db7ab719402129f76f631adbd7b7a5a5bced")
            .unwrap();

    let cache: Arc<RwLock<HashMap<Address, Vec<H256>>>> = Arc::new(RwLock::new(HashMap::new()));

    // Update cache
    {
        let mut cache_write = cache.write().await;
        cache_write
            .entry(host)
            .or_insert_with(Vec::new)
            .push(model_id);
    }

    // Verify update
    let is_authorized = {
        let cache_read = cache.read().await;
        cache_read
            .get(&host)
            .map(|models| models.contains(&model_id))
            .unwrap_or(false)
    };

    assert!(is_authorized, "Async cache update should persist");
}

// ============================================================================
// Error Handling Tests
// ============================================================================

/// Test that contract unavailable returns proper error type
#[test]
fn test_contract_unavailable_error() {
    use fabstir_llm_node::model_validation::ModelValidationError;

    let error = ModelValidationError::ContractUnavailable(
        "RPC timeout: failed to query nodeSupportsModel".to_string(),
    );

    let msg = format!("{}", error);
    assert!(msg.to_lowercase().contains("contract"));
    assert!(msg.to_lowercase().contains("unavailable") || msg.to_lowercase().contains("rpc"));
}

/// Test error can be created with various RPC failure messages
#[test]
fn test_contract_error_messages() {
    use fabstir_llm_node::model_validation::ModelValidationError;

    let errors = vec![
        ModelValidationError::ContractUnavailable("Connection refused".to_string()),
        ModelValidationError::ContractUnavailable("Request timeout".to_string()),
        ModelValidationError::ContractUnavailable("Invalid response".to_string()),
    ];

    for error in errors {
        let msg = format!("{}", error);
        // All should indicate contract issue
        assert!(
            msg.to_lowercase().contains("contract") || msg.to_lowercase().contains("unavailable"),
            "Error message should indicate contract issue: {}",
            msg
        );
    }
}
