#!/bin/bash

# Script to run Phase 4.1.3 integration tests
# This verifies we're in the RED phase of TDD

echo "========================================="
echo "Phase 4.1.3: Integration with Both Mocks"
echo "========================================="
echo ""
echo "Running TDD RED phase verification..."
echo ""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to run a test and check if it fails (as expected in RED phase)
run_test() {
    local test_name=$1
    echo -e "${YELLOW}Running test: ${test_name}${NC}"
    
    # Run the test and capture the exit code
    cargo test --test integration_tests $test_name -- --nocapture 2>&1
    local exit_code=$?
    
    if [ $exit_code -ne 0 ]; then
        echo -e "${RED}✓ Test FAILED as expected (RED phase)${NC}"
        return 0
    else
        echo -e "${GREEN}✗ Test PASSED unexpectedly (should be failing in RED phase)${NC}"
        return 1
    fi
}

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ]; then
    echo -e "${RED}Error: Not in fabstir-llm-node directory${NC}"
    echo "Please run from: docker exec -it fabstir-llm-marketplace-node-dev-1 bash"
    echo "Then navigate to: cd /workspace"
    exit 1
fi

# Ensure test files exist
echo "Checking test files..."
if [ ! -f "tests/integration/mock/test_e2e_workflow.rs" ]; then
    echo -e "${YELLOW}Creating test directory structure...${NC}"
    mkdir -p tests/integration/mock
    echo -e "${RED}Error: test_e2e_workflow.rs not found${NC}"
    echo "Please create the test file first"
    exit 1
fi

if [ ! -f "tests/integration/mock/test_cache_flow.rs" ]; then
    echo -e "${RED}Error: test_cache_flow.rs not found${NC}"
    echo "Please create the test file first"
    exit 1
fi

echo -e "${GREEN}✓ Test files found${NC}"
echo ""

# Run individual tests from test_e2e_workflow.rs
echo "========================================="
echo "E2E Workflow Tests"
echo "========================================="

tests_e2e=(
    "test_store_model_in_enhanced_s5"
    "test_generate_embeddings_for_model"
    "test_store_embeddings_in_vector_db"
    "test_semantic_search_for_similar_models"
    "test_complete_workflow_integration"
    "test_model_discovery_by_capability"
    "test_model_versioning_workflow"
)

failed_count=0
for test in "${tests_e2e[@]}"; do
    run_test $test
    if [ $? -ne 0 ]; then
        ((failed_count++))
    fi
    echo ""
done

# Run individual tests from test_cache_flow.rs
echo "========================================="
echo "Cache Flow Tests"
echo "========================================="

tests_cache=(
    "test_hash_prompts_for_cache_lookup"
    "test_search_vector_db_for_similar_prompts"
    "test_retrieve_cached_results_from_s5"
    "test_measure_cache_hit_rates"
    "test_complete_cache_flow"
    "test_cache_expiration_and_cleanup"
    "test_cache_performance_metrics"
)

for test in "${tests_cache[@]}"; do
    run_test $test
    if [ $? -ne 0 ]; then
        ((failed_count++))
    fi
    echo ""
done

# Summary
echo "========================================="
echo "TDD RED Phase Verification Summary"
echo "========================================="
total_tests=$((${#tests_e2e[@]} + ${#tests_cache[@]}))
echo -e "Total tests: ${total_tests}"
echo -e "Expected to fail: ${total_tests}"
echo -e "Actually failed: $((total_tests - failed_count))"
echo ""

if [ $failed_count -eq 0 ]; then
    echo -e "${GREEN}✅ Perfect! All tests are failing as expected.${NC}"
    echo -e "${GREEN}We are correctly in the RED phase of TDD.${NC}"
    echo ""
    echo "Next steps:"
    echo "1. Implement the missing modules one by one:"
    echo "   - src/embeddings/mod.rs (EmbeddingGenerator)"
    echo "   - src/cache/mod.rs (PromptCache)"
    echo "   - Update src/storage/enhanced_s5_client.rs"
    echo "   - Update src/vector/vector_db_client.rs"
    echo ""
    echo "2. Run tests again after each implementation"
    echo "3. Keep implementing until all tests pass (GREEN phase)"
    echo "4. Refactor if needed while keeping tests green"
else
    echo -e "${YELLOW}⚠️ Warning: Some tests may already have implementations${NC}"
    echo -e "${YELLOW}Check if modules are partially implemented${NC}"
fi

echo ""
echo "To run all integration tests after implementation:"
echo "  cargo test --test integration_tests -- --nocapture"
echo ""
echo "To run specific test:"
echo "  cargo test --test integration_tests test_name -- --nocapture"