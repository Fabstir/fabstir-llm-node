#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1

# Load Testing Script for Embedding API (Sub-phase 9.3)
#
# This script performs load testing on the /v1/embed endpoint with realistic traffic patterns.
# It tests various scenarios: single requests, batch requests, concurrent requests.

set -e

# Configuration
API_URL="${API_URL:-http://localhost:8080}"
EMBED_ENDPOINT="$API_URL/v1/embed"
OUTPUT_DIR="/tmp/embed_load_test_$(date +%Y%m%d_%H%M%S)"
mkdir -p "$OUTPUT_DIR"

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Test results
TOTAL_TESTS=0
PASSED_TESTS=0
FAILED_TESTS=0

log() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if server is running
check_server() {
    log "Checking if server is running at $API_URL..."
    if ! curl -s -f "$API_URL/health" > /dev/null 2>&1; then
        error "Server is not running at $API_URL"
        error "Please start the server first: cargo run --release"
        exit 1
    fi
    log "✓ Server is running"
}

# Test 1: Single text embedding (baseline)
test_single_text() {
    log "Test 1: Single text embedding (baseline)"
    TOTAL_TESTS=$((TOTAL_TESTS + 1))

    local request='{"texts":["Hello world"],"model":"all-MiniLM-L6-v2","chainId":84532}'
    local response_file="$OUTPUT_DIR/test1_response.json"
    local timing_file="$OUTPUT_DIR/test1_timing.txt"

    # Send request and capture timing
    if curl -s -w "%{http_code}\n%{time_total}\n" -o "$response_file" \
        -X POST "$EMBED_ENDPOINT" \
        -H "Content-Type: application/json" \
        -d "$request" > "$timing_file"; then

        local http_code=$(head -n1 "$timing_file")
        local time_total=$(tail -n1 "$timing_file")

        if [ "$http_code" == "200" ]; then
            log "✓ Single text test passed (${time_total}s)"
            PASSED_TESTS=$((PASSED_TESTS + 1))
        else
            error "✗ Single text test failed (HTTP $http_code)"
            FAILED_TESTS=$((FAILED_TESTS + 1))
        fi
    else
        error "✗ Single text test failed (curl error)"
        FAILED_TESTS=$((FAILED_TESTS + 1))
    fi
}

# Test 2: Batch embedding (10 texts)
test_batch_10() {
    log "Test 2: Batch embedding (10 texts)"
    TOTAL_TESTS=$((TOTAL_TESTS + 1))

    local request='{"texts":["Text 1","Text 2","Text 3","Text 4","Text 5","Text 6","Text 7","Text 8","Text 9","Text 10"],"model":"all-MiniLM-L6-v2","chainId":84532}'
    local response_file="$OUTPUT_DIR/test2_response.json"
    local timing_file="$OUTPUT_DIR/test2_timing.txt"

    if curl -s -w "%{http_code}\n%{time_total}\n" -o "$response_file" \
        -X POST "$EMBED_ENDPOINT" \
        -H "Content-Type: application/json" \
        -d "$request" > "$timing_file"; then

        local http_code=$(head -n1 "$timing_file")
        local time_total=$(tail -n1 "$timing_file")

        if [ "$http_code" == "200" ]; then
            log "✓ Batch 10 test passed (${time_total}s)"
            PASSED_TESTS=$((PASSED_TESTS + 1))
        else
            error "✗ Batch 10 test failed (HTTP $http_code)"
            FAILED_TESTS=$((FAILED_TESTS + 1))
        fi
    else
        error "✗ Batch 10 test failed (curl error)"
        FAILED_TESTS=$((FAILED_TESTS + 1))
    fi
}

# Test 3: Large batch (50 texts)
test_batch_50() {
    log "Test 3: Large batch (50 texts)"
    TOTAL_TESTS=$((TOTAL_TESTS + 1))

    # Generate 50 texts
    local texts='['
    for i in $(seq 1 50); do
        texts+="\"Sample text number $i\""
        if [ $i -lt 50 ]; then
            texts+=','
        fi
    done
    texts+=']'

    local request="{\"texts\":$texts,\"model\":\"all-MiniLM-L6-v2\",\"chainId\":84532}"
    local response_file="$OUTPUT_DIR/test3_response.json"
    local timing_file="$OUTPUT_DIR/test3_timing.txt"

    if curl -s -w "%{http_code}\n%{time_total}\n" -o "$response_file" \
        -X POST "$EMBED_ENDPOINT" \
        -H "Content-Type: application/json" \
        -d "$request" > "$timing_file"; then

        local http_code=$(head -n1 "$timing_file")
        local time_total=$(tail -n1 "$timing_file")

        if [ "$http_code" == "200" ]; then
            log "✓ Batch 50 test passed (${time_total}s)"
            PASSED_TESTS=$((PASSED_TESTS + 1))
        else
            error "✗ Batch 50 test failed (HTTP $http_code)"
            FAILED_TESTS=$((FAILED_TESTS + 1))
        fi
    else
        error "✗ Batch 50 test failed (curl error)"
        FAILED_TESTS=$((FAILED_TESTS + 1))
    fi
}

# Test 4: Maximum batch (96 texts)
test_batch_96() {
    log "Test 4: Maximum batch (96 texts - limit)"
    TOTAL_TESTS=$((TOTAL_TESTS + 1))

    # Generate 96 texts
    local texts='['
    for i in $(seq 1 96); do
        texts+="\"Text $i\""
        if [ $i -lt 96 ]; then
            texts+=','
        fi
    done
    texts+=']'

    local request="{\"texts\":$texts,\"model\":\"all-MiniLM-L6-v2\",\"chainId\":84532}"
    local response_file="$OUTPUT_DIR/test4_response.json"
    local timing_file="$OUTPUT_DIR/test4_timing.txt"

    if curl -s -w "%{http_code}\n%{time_total}\n" -o "$response_file" \
        -X POST "$EMBED_ENDPOINT" \
        -H "Content-Type: application/json" \
        -d "$request" > "$timing_file"; then

        local http_code=$(head -n1 "$timing_file")
        local time_total=$(tail -n1 "$timing_file")

        if [ "$http_code" == "200" ]; then
            log "✓ Batch 96 test passed (${time_total}s)"
            PASSED_TESTS=$((PASSED_TESTS + 1))
        else
            error "✗ Batch 96 test failed (HTTP $http_code)"
            FAILED_TESTS=$((FAILED_TESTS + 1))
        fi
    else
        error "✗ Batch 96 test failed (curl error)"
        FAILED_TESTS=$((FAILED_TESTS + 1))
    fi
}

# Test 5: Concurrent requests (10 parallel)
test_concurrent() {
    log "Test 5: Concurrent requests (10 parallel)"
    TOTAL_TESTS=$((TOTAL_TESTS + 1))

    local request='{"texts":["Concurrent test"],"model":"all-MiniLM-L6-v2","chainId":84532}'
    local start_time=$(date +%s.%N)
    local pids=()

    # Launch 10 concurrent requests
    for i in $(seq 1 10); do
        curl -s -o "$OUTPUT_DIR/test5_response_$i.json" \
            -X POST "$EMBED_ENDPOINT" \
            -H "Content-Type: application/json" \
            -d "$request" &
        pids+=($!)
    done

    # Wait for all requests to complete
    local failed=0
    for pid in "${pids[@]}"; do
        if ! wait $pid; then
            failed=$((failed + 1))
        fi
    done

    local end_time=$(date +%s.%N)
    local duration=$(echo "$end_time - $start_time" | bc)

    if [ $failed -eq 0 ]; then
        log "✓ Concurrent test passed (10 requests in ${duration}s)"
        PASSED_TESTS=$((PASSED_TESTS + 1))
    else
        error "✗ Concurrent test failed ($failed/$10 requests failed)"
        FAILED_TESTS=$((FAILED_TESTS + 1))
    fi
}

# Test 6: Long text (stress test)
test_long_text() {
    log "Test 6: Long text (4000 characters)"
    TOTAL_TESTS=$((TOTAL_TESTS + 1))

    # Generate 4000 character text
    local long_text=$(python3 -c "print('word ' * 800)")
    local request="{\"texts\":[\"$long_text\"],\"model\":\"all-MiniLM-L6-v2\",\"chainId\":84532}"
    local response_file="$OUTPUT_DIR/test6_response.json"
    local timing_file="$OUTPUT_DIR/test6_timing.txt"

    if curl -s -w "%{http_code}\n%{time_total}\n" -o "$response_file" \
        -X POST "$EMBED_ENDPOINT" \
        -H "Content-Type: application/json" \
        -d "$request" > "$timing_file"; then

        local http_code=$(head -n1 "$timing_file")
        local time_total=$(tail -n1 "$timing_file")

        if [ "$http_code" == "200" ]; then
            log "✓ Long text test passed (${time_total}s)"
            PASSED_TESTS=$((PASSED_TESTS + 1))
        else
            error "✗ Long text test failed (HTTP $http_code)"
            FAILED_TESTS=$((FAILED_TESTS + 1))
        fi
    else
        error "✗ Long text test failed (curl error)"
        FAILED_TESTS=$((FAILED_TESTS + 1))
    fi
}

# Test 7: Sustained load (50 sequential requests)
test_sustained_load() {
    log "Test 7: Sustained load (50 sequential requests)"
    TOTAL_TESTS=$((TOTAL_TESTS + 1))

    local request='{"texts":["Load test"],"model":"all-MiniLM-L6-v2","chainId":84532}'
    local start_time=$(date +%s.%N)
    local success_count=0
    local total_time=0

    for i in $(seq 1 50); do
        local timing_file="$OUTPUT_DIR/test7_timing_$i.txt"

        if curl -s -w "%{time_total}\n" -o /dev/null \
            -X POST "$EMBED_ENDPOINT" \
            -H "Content-Type: application/json" \
            -d "$request" > "$timing_file"; then
            success_count=$((success_count + 1))
            local time=$(cat "$timing_file")
            total_time=$(echo "$total_time + $time" | bc)
        fi
    done

    local end_time=$(date +%s.%N)
    local total_duration=$(echo "$end_time - $start_time" | bc)
    local avg_time=$(echo "scale=3; $total_time / 50" | bc)
    local throughput=$(echo "scale=2; 50 / $total_duration" | bc)

    if [ $success_count -eq 50 ]; then
        log "✓ Sustained load test passed"
        log "  Total time: ${total_duration}s"
        log "  Average request time: ${avg_time}s"
        log "  Throughput: ${throughput} req/s"
        PASSED_TESTS=$((PASSED_TESTS + 1))
    else
        error "✗ Sustained load test failed ($success_count/50 succeeded)"
        FAILED_TESTS=$((FAILED_TESTS + 1))
    fi
}

# Generate report
generate_report() {
    local report_file="$OUTPUT_DIR/load_test_report.txt"

    cat > "$report_file" <<EOF
# Embedding API Load Test Report
Generated: $(date)
API URL: $API_URL
Output Directory: $OUTPUT_DIR

## Test Summary
Total Tests: $TOTAL_TESTS
Passed: $PASSED_TESTS
Failed: $FAILED_TESTS
Success Rate: $(echo "scale=2; $PASSED_TESTS * 100 / $TOTAL_TESTS" | bc)%

## Test Results

1. Single text embedding: $(grep -l "test1" "$OUTPUT_DIR"/*.txt >/dev/null 2>&1 && echo "✓ Pass" || echo "✗ Fail")
2. Batch 10 texts: $(grep -l "test2" "$OUTPUT_DIR"/*.txt >/dev/null 2>&1 && echo "✓ Pass" || echo "✗ Fail")
3. Batch 50 texts: $(grep -l "test3" "$OUTPUT_DIR"/*.txt >/dev/null 2>&1 && echo "✓ Pass" || echo "✗ Fail")
4. Batch 96 texts (max): $(grep -l "test4" "$OUTPUT_DIR"/*.txt >/dev/null 2>&1 && echo "✓ Pass" || echo "✗ Fail")
5. Concurrent requests (10): $([ -f "$OUTPUT_DIR/test5_response_1.json" ] && echo "✓ Pass" || echo "✗ Fail")
6. Long text (4000 chars): $(grep -l "test6" "$OUTPUT_DIR"/*.txt >/dev/null 2>&1 && echo "✓ Pass" || echo "✗ Fail")
7. Sustained load (50 req): $([ -f "$OUTPUT_DIR/test7_timing_50.txt" ] && echo "✓ Pass" || echo "✗ Fail")

## Performance Metrics

See individual test timing files in $OUTPUT_DIR for detailed timing information.

## Files Generated
- Response files: *_response*.json
- Timing files: *_timing*.txt
- This report: load_test_report.txt
EOF

    log "Report saved to: $report_file"
    cat "$report_file"
}

# Main execution
main() {
    log "========================================="
    log "Embedding API Load Testing Suite"
    log "========================================="
    log ""

    check_server
    log ""

    test_single_text
    log ""

    test_batch_10
    log ""

    test_batch_50
    log ""

    test_batch_96
    log ""

    test_concurrent
    log ""

    test_long_text
    log ""

    test_sustained_load
    log ""

    log "========================================="
    log "Load Testing Complete"
    log "========================================="
    log ""

    generate_report

    log ""
    log "Results saved to: $OUTPUT_DIR"
    log ""

    if [ $FAILED_TESTS -gt 0 ]; then
        error "Some tests failed. Check the output above for details."
        exit 1
    else
        log "All tests passed!"
        exit 0
    fi
}

# Run main function
main "$@"
