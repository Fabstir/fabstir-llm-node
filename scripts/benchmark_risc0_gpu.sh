#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1

# Risc0 GPU Benchmarking Script for Phase 5.2
#
# This script benchmarks Risc0 zkVM proof generation with CUDA GPU acceleration
# and compares performance against CPU-only baseline from Phase 5.1.
#
# Prerequisites:
# - CUDA toolkit installed (nvcc in PATH)
# - NVIDIA GPU with CUDA support
# - Rust toolchain with CUDA support compiled

set -e

echo "=================================================="
echo "  Risc0 zkVM GPU Performance Benchmarking"
echo "  Phase 5.2: Performance Benchmarking"
echo "=================================================="
echo ""

# Check CUDA toolkit
echo "🔍 Checking CUDA toolkit..."
if ! command -v nvcc &> /dev/null; then
    echo "❌ ERROR: CUDA toolkit not found (nvcc not in PATH)"
    echo "   Install CUDA toolkit: https://developer.nvidia.com/cuda-downloads"
    exit 1
fi

CUDA_VERSION=$(nvcc --version | grep "release" | awk '{print $5}' | sed 's/,//')
echo "✅ CUDA toolkit found: version $CUDA_VERSION"
echo ""

# Check GPU availability
echo "🔍 Checking NVIDIA GPU..."
if ! command -v nvidia-smi &> /dev/null; then
    echo "❌ ERROR: nvidia-smi not found (NVIDIA drivers not installed)"
    exit 1
fi

echo "📊 GPU Information:"
nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader | while read line; do
    echo "   $line"
done
echo ""

# Display baseline (CPU results from Phase 5.1)
echo "📊 Baseline Performance (CPU - Phase 5.1):"
echo "   - Single Proof: ~4.4 seconds"
echo "   - Proof Size: ~221KB"
echo "   - Verification: <1 second"
echo ""

# Build with CUDA support
echo "🔨 Building with CUDA support..."
echo "   Command: RUSTFLAGS=\"-C target-cpu=native\" cargo build --release --features real-ezkl"
echo ""

RUSTFLAGS="-C target-cpu=native" cargo build --release --features real-ezkl

if [ $? -ne 0 ]; then
    echo "❌ Build failed"
    exit 1
fi

echo "✅ Build successful"
echo ""

# Note about first run
echo "⚠️  NOTE: First GPU run may take 2-5 minutes for JIT kernel compilation"
echo "   Subsequent runs will be faster (kernels cached)"
echo ""

# Benchmark 1: Single Proof Generation
echo "=================================================="
echo "  Benchmark 1: Single Proof Generation (GPU)"
echo "=================================================="
echo ""
echo "Running: cargo test --release --features real-ezkl test_e2e_single_job_complete_flow -- --exact --nocapture"
echo ""

time cargo test --release --features real-ezkl test_e2e_single_job_complete_flow -- --exact --nocapture

echo ""
echo "✅ Single proof benchmark complete"
echo ""

# Benchmark 2: Sequential Proof Generation (if test exists)
echo "=================================================="
echo "  Benchmark 2: Multiple Sequential Proofs"
echo "=================================================="
echo ""

if cargo test --list --features real-ezkl 2>/dev/null | grep -q "test_load_sequential_proof_generation"; then
    echo "Running: cargo test --release --features real-ezkl test_load_sequential_proof_generation -- --exact --nocapture"
    echo ""
    cargo test --release --features real-ezkl test_load_sequential_proof_generation -- --exact --nocapture
    echo ""
    echo "✅ Sequential proof benchmark complete"
else
    echo "⚠️  test_load_sequential_proof_generation not found, skipping"
    echo "   Creating manual 10-proof sequential test..."
    echo ""

    # Run single test 10 times
    START_TIME=$(date +%s)
    for i in {1..10}; do
        echo "Proof $i/10..."
        cargo test --release --features real-ezkl test_e2e_single_job_complete_flow -- --exact --nocapture 2>&1 | tail -n 5
    done
    END_TIME=$(date +%s)
    ELAPSED=$((END_TIME - START_TIME))
    echo ""
    echo "📊 10 sequential proofs completed in: ${ELAPSED}s (~$((ELAPSED / 10))s per proof avg)"
fi

echo ""

# Benchmark 3: Memory Usage
echo "=================================================="
echo "  Benchmark 3: Memory Usage During Proof Generation"
echo "=================================================="
echo ""
echo "Running: /usr/bin/time -v cargo test --release --features real-ezkl test_e2e_cleanup_workflow -- --exact"
echo ""

if command -v /usr/bin/time &> /dev/null; then
    /usr/bin/time -v cargo test --release --features real-ezkl test_e2e_cleanup_workflow -- --exact 2>&1 | \
        grep -E "(Maximum resident|Elapsed time|CPU this job got)"
    echo ""
    echo "✅ Memory benchmark complete"
else
    echo "⚠️  /usr/bin/time not available, skipping memory measurement"
fi

echo ""

# Summary
echo "=================================================="
echo "  Benchmarking Complete"
echo "=================================================="
echo ""
echo "✅ All GPU benchmarks completed"
echo ""
echo "📋 Next Steps:"
echo "   1. Review benchmark results above"
echo "   2. Calculate GPU speedup vs CPU baseline (4.4s)"
echo "   3. Document results in docs/IMPLEMENTATION-RISC0.md Phase 5.2"
echo "   4. Update performance table with actual metrics"
echo ""
echo "💡 Tip: Run this script multiple times and average results for accuracy"
echo ""
