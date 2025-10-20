#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1


echo "Fixing import paths in performance tests..."

# Fix all three test files
for file in tests/performance/connected/*.rs; do
    echo "Fixing $file"
    sed -i 's/use crate::vector::/use fabstir_llm_node::vector::/' "$file"
done

echo "Done! Now running test to verify..."
cargo test test_baseline_throughput -- --nocapture
