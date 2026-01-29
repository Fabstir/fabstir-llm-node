#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Comprehensive test suite for GPT-OSS-120B deployment

set -e

echo "========================================="
echo "GPT-OSS-120B Testing Suite"
echo "========================================="
echo ""

API_URL="http://localhost:8080"
CHAIN_ID=84532

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Function to run a test
run_test() {
    local test_name="$1"
    local test_command="$2"
    local expected_pattern="$3"

    echo "----------------------------------------"
    echo "Test: $test_name"
    echo "----------------------------------------"

    # Run the test and capture output
    if output=$(eval "$test_command" 2>&1); then
        # Check if output matches expected pattern
        if echo "$output" | grep -qi "$expected_pattern"; then
            echo -e "${GREEN}✅ PASS${NC}"
            TESTS_PASSED=$((TESTS_PASSED + 1))
        else
            echo -e "${RED}❌ FAIL - Expected pattern not found: $expected_pattern${NC}"
            echo "Output: $output"
            TESTS_FAILED=$((TESTS_FAILED + 1))
        fi
    else
        echo -e "${RED}❌ FAIL - Command failed${NC}"
        echo "Output: $output"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
    echo ""
}

# Check if node is running
echo "Checking if node is running..."
if ! docker ps | grep -q llm-node-prod-1; then
    echo -e "${RED}❌ Error: llm-node-prod-1 container is not running${NC}"
    echo "Start the node first with: ./restart-and-deploy-openai.sh"
    exit 1
fi
echo -e "${GREEN}✅ Node is running${NC}"
echo ""

# Check Docker logs for model loading
echo "========================================="
echo "HEALTH CHECKS"
echo "========================================="
echo ""

echo "Checking model loading in logs..."
if docker logs llm-node-prod-1 2>&1 | grep -q "Model loaded"; then
    echo -e "${GREEN}✅ Model loaded successfully${NC}"
    docker logs llm-node-prod-1 2>&1 | grep "Model loaded" | tail -1
else
    echo -e "${YELLOW}⚠️  Model loading message not found in logs${NC}"
fi
echo ""

echo "Checking GPU layers configuration..."
if docker logs llm-node-prod-1 2>&1 | grep -qi "gpu"; then
    echo -e "${GREEN}✅ GPU configuration found${NC}"
    docker logs llm-node-prod-1 2>&1 | grep -i "gpu" | tail -3
else
    echo -e "${YELLOW}⚠️  GPU configuration not found in logs${NC}"
fi
echo ""

echo "Checking Harmony chat template..."
if docker logs llm-node-prod-1 2>&1 | grep -q "channel"; then
    echo -e "${GREEN}✅ Harmony template active (contains <|channel|> tags)${NC}"
else
    echo -e "${YELLOW}⚠️  Harmony template markers not found${NC}"
fi
echo ""

# GPU Memory Check
echo "========================================="
echo "GPU MEMORY USAGE"
echo "========================================="
if command -v nvidia-smi >/dev/null 2>&1; then
    nvidia-smi --query-gpu=memory.used,memory.total --format=csv
    echo ""
    VRAM_USED=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits | head -1)
    echo "Current VRAM usage: ${VRAM_USED}MB"
    if [ "$VRAM_USED" -gt 90000 ]; then
        echo -e "${YELLOW}⚠️  Warning: VRAM usage is high (>90GB)${NC}"
    elif [ "$VRAM_USED" -gt 60000 ]; then
        echo -e "${GREEN}✅ VRAM usage looks good (~65-90GB expected for 120B)${NC}"
    else
        echo -e "${YELLOW}⚠️  VRAM usage seems low (<60GB) - model may not be fully loaded${NC}"
    fi
else
    echo -e "${YELLOW}⚠️  nvidia-smi not available - cannot check GPU memory${NC}"
fi
echo ""

# Functional Tests
echo "========================================="
echo "FUNCTIONAL TESTS"
echo "========================================="
echo ""

# Test 1: Simple math
run_test "Simple Math (2+2)" \
    "curl -s -X POST ${API_URL}/v1/inference -H 'Content-Type: application/json' -d '{\"model\": \"gpt-oss-120b\", \"prompt\": \"What is 2+2?\", \"max_tokens\": 20, \"temperature\": 0.1, \"chain_id\": ${CHAIN_ID}}'" \
    "4"

# Test 2: Capital city
run_test "Capital City Knowledge" \
    "curl -s -X POST ${API_URL}/v1/inference -H 'Content-Type: application/json' -d '{\"model\": \"gpt-oss-120b\", \"prompt\": \"What is the capital of France?\", \"max_tokens\": 50, \"temperature\": 0.1, \"chain_id\": ${CHAIN_ID}}'" \
    "Paris"

# Test 3: Longer generation
run_test "Longer Generation (100 tokens)" \
    "curl -s -X POST ${API_URL}/v1/inference -H 'Content-Type: application/json' -d '{\"model\": \"gpt-oss-120b\", \"prompt\": \"Write a short poem about computers\", \"max_tokens\": 100, \"temperature\": 0.8, \"chain_id\": ${CHAIN_ID}}'" \
    "content"

# Test 4: Streaming
echo "----------------------------------------"
echo "Test: Streaming Inference"
echo "----------------------------------------"
STREAM_OUTPUT=$(curl -s -X POST ${API_URL}/v1/inference \
    -H 'Content-Type: application/json' \
    -d "{\"model\": \"gpt-oss-120b\", \"prompt\": \"Count to 5\", \"max_tokens\": 50, \"temperature\": 0.5, \"stream\": true, \"chain_id\": ${CHAIN_ID}}" \
    | head -20)

if echo "$STREAM_OUTPUT" | grep -q "data:"; then
    echo -e "${GREEN}✅ PASS - Streaming working${NC}"
    TESTS_PASSED=$((TESTS_PASSED + 1))
else
    echo -e "${RED}❌ FAIL - Streaming not working${NC}"
    TESTS_FAILED=$((TESTS_FAILED + 1))
fi
echo ""

# Performance Test
echo "========================================="
echo "PERFORMANCE TEST"
echo "========================================="
echo ""
echo "Running 100-token generation benchmark..."
START_TIME=$(date +%s)
RESULT=$(curl -s -X POST ${API_URL}/v1/inference \
    -H 'Content-Type: application/json' \
    -d "{\"model\": \"gpt-oss-120b\", \"prompt\": \"Explain quantum computing in detail\", \"max_tokens\": 100, \"temperature\": 0.7, \"chain_id\": ${CHAIN_ID}}")
END_TIME=$(date +%s)
DURATION=$((END_TIME - START_TIME))

if echo "$RESULT" | grep -q "content"; then
    echo -e "${GREEN}✅ Inference successful${NC}"
    echo "Time taken: ${DURATION} seconds"

    # Calculate approximate tokens/sec (rough estimate)
    if [ "$DURATION" -gt 0 ]; then
        TOKENS_PER_SEC=$((100 / DURATION))
        echo "Estimated throughput: ~${TOKENS_PER_SEC} tokens/second"

        if [ "$TOKENS_PER_SEC" -ge 5 ]; then
            echo -e "${GREEN}✅ Performance acceptable (>=5 tokens/sec)${NC}"
        else
            echo -e "${YELLOW}⚠️  Performance lower than expected (<5 tokens/sec)${NC}"
        fi
    fi
else
    echo -e "${RED}❌ Inference failed${NC}"
    echo "Result: $RESULT"
fi
echo ""

# Final Summary
echo "========================================="
echo "TEST SUMMARY"
echo "========================================="
echo ""
echo -e "Tests Passed: ${GREEN}${TESTS_PASSED}${NC}"
echo -e "Tests Failed: ${RED}${TESTS_FAILED}${NC}"
echo ""

if [ "$TESTS_FAILED" -eq 0 ]; then
    echo -e "${GREEN}✅ All tests passed!${NC}"
    echo ""
    echo "Node is ready for production use with GPT-OSS-120B (131K context)"
    exit 0
else
    echo -e "${RED}❌ Some tests failed${NC}"
    echo ""
    echo "Check Docker logs for details:"
    echo "  docker logs llm-node-prod-1"
    exit 1
fi
