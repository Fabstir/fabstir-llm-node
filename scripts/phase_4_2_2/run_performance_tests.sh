#!/bin/bash

echo "Running Performance Tests Individually to Avoid Timeouts"
echo "========================================================"

# Function to run a single test module
run_test_module() {
    local module=$1
    echo -e "\n\n=== Running $module tests ==="
    echo "================================"
    
    # Run with timeout and capture output
    timeout 300 cargo test --test performance_tests performance::$module -- --nocapture --test-threads=1 2>&1 | tee ${module}_results.log
    
    # Check exit code
    if [ ${PIPESTATUS[0]} -eq 124 ]; then
        echo "❌ $module tests TIMED OUT after 5 minutes"
    elif [ ${PIPESTATUS[0]} -eq 0 ]; then
        echo "✅ $module tests PASSED"
    else
        echo "❌ $module tests FAILED"
    fi
    
    # Extract summary
    echo -e "\nSummary for $module:"
    grep -E "test result:|passed|failed" ${module}_results.log | tail -n 1
}

# Run each module separately
run_test_module "test_gpu_management"
run_test_module "test_batching"
run_test_module "test_caching"
run_test_module "test_load_balancing"

echo -e "\n\n=== FINAL SUMMARY ==="
echo "===================="
for module in test_gpu_management test_batching test_caching test_load_balancing; do
    echo -n "$module: "
    grep -E "test result:" ${module}_results.log | tail -n 1 || echo "No results found"
done

# Also try running just the hanging test to debug
echo -e "\n\n=== Debug: Running semantic_caching test alone ==="
timeout 60 cargo test --test performance_tests test_semantic_caching -- --nocapture --test-threads=1