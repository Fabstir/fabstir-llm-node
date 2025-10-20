#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1

# Script to run EZKL tests for Phase 3.1

echo "Running EZKL Integration Tests - Phase 3.1"
echo "=========================================="

# Create test directories if they don't exist
mkdir -p test_data/srs
mkdir -p test_data/circuits
mkdir -p test_data/vk
mkdir -p test_data/pk
mkdir -p test_data/models
mkdir -p test_data/witness

# Create mock model file for tests
echo "Creating mock model file..."
echo "mock_onnx_model_data" > test_data/models/tiny-llama.onnx
echo "complex_mock_model_data" > test_data/models/complex-model.onnx

# Run individual test suites
echo -e "\n1. Testing EZKL Integration..."
cargo test --test test_integration -- --nocapture

echo -e "\n2. Testing Proof Creation..."
cargo test --test test_proof_creation -- --nocapture

echo -e "\n3. Testing Batch Proofs..."
cargo test --test test_batch_proofs -- --nocapture

echo -e "\n4. Testing Verification..."
cargo test --test test_verification -- --nocapture

# Run all EZKL tests together
echo -e "\n5. Running all EZKL tests..."
cargo test ezkl:: -- --nocapture

# Count results
echo -e "\n=========================================="
echo "Test Summary:"
cargo test ezkl:: 2>&1 | grep -E "test result:|passed|failed"

echo -e "\nNote: All tests should pass with the mock implementation."
echo "Real EZKL integration will be added in Phase 4."