#!/bin/bash
# Final solution to run Model Management tests

echo "Running Model Management Tests - Phase 3.2 (Final Solution)"
echo "=========================================================="
echo ""

# Method 1: Use temporary directory for build artifacts
export CARGO_TARGET_DIR=/tmp/model-tests-target
mkdir -p $CARGO_TARGET_DIR
chmod -R 777 $CARGO_TARGET_DIR

# Method 2: Disable all caching mechanisms
export RUSTC_WRAPPER=""
export GGML_CCACHE=OFF
export CMAKE_DISABLE_FIND_PACKAGE_sccache=TRUE
export CMAKE_C_COMPILER_LAUNCHER=""
export CMAKE_CXX_COMPILER_LAUNCHER=""
export CCACHE_DISABLE=1
export SCCACHE_DISABLE=1

# Method 3: Remove sccache from PATH
export PATH=$(echo $PATH | tr ':' '\n' | grep -v sccache | tr '\n' ':' | sed 's/:$//')

# Backup Cargo.toml
cp Cargo.toml Cargo.toml.backup

# Remove the cuda feature from llama-cpp-2
sed -i 's/llama-cpp-2 = { version = "0.1.55", features = \["cuda"\] }/llama-cpp-2 = { version = "0.1.55" }/' Cargo.toml

# Clean previous artifacts
echo "Cleaning build artifacts..."
rm -rf $CARGO_TARGET_DIR/*

# Create test directories
mkdir -p test_data/models
mkdir -p test_data/cache
mkdir -p test_data/cache_persist
mkdir -p test_data/updates

# Create mock model files
echo "Creating mock model files..."
echo "mock_gguf_data" > test_data/models/test_model.gguf
echo "mock_onnx_data" > test_data/models/test_model.onnx
echo "mock_safetensors_data" > test_data/models/test_model.safetensors
echo "corrupted_data_!@#$" > test_data/models/corrupted_model.gguf
echo "model_v1_data" > test_data/models/model_v1.gguf
echo "model_v2_data" > test_data/models/model_v2.gguf
echo "original_model_data" > test_data/models/original.gguf

for i in {0..10}; do
    echo "model_${i}_data" > test_data/models/model_${i}.gguf
done

# Run tests
echo -e "\nRunning tests with all fixes applied..."
echo "Build directory: $CARGO_TARGET_DIR"
echo ""

cargo test --test models_tests -- --nocapture

# Capture the exit code
TEST_EXIT_CODE=$?

# Restore Cargo.toml
echo ""
echo "Restoring original Cargo.toml..."
mv Cargo.toml.backup Cargo.toml

# Cleanup temporary directory
rm -rf $CARGO_TARGET_DIR

# Results
echo -e "\n=========================================================="
echo "Test Summary:"
if [ $TEST_EXIT_CODE -eq 0 ]; then
    echo -e "\n✅ All tests passed!"
    echo ""
    echo "Your model management implementation is working correctly."
else
    echo -e "\n❌ Some tests failed. Exit code: $TEST_EXIT_CODE"
fi

exit $TEST_EXIT_CODE