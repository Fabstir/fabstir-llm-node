// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// tests/integration_tests.rs
// Main integration test entry point

// Import the fabstir-llm-node library
use fabstir_llm_node;

// Include all integration test modules
mod integration;

// Re-export test modules
#[cfg(test)]
pub use integration::mock::*;
