#!/bin/bash
# test-watcher.sh - Common test watcher for all projects

set -e

echo "👀 Test Watcher Active"

# Detect project type
if [ -f "foundry.toml" ]; then
    PROJECT_TYPE="contracts"
    TEST_CMD="forge test"
    TEST_PATTERN="*.t.sol"
elif [ -f "Cargo.toml" ]; then
    PROJECT_TYPE="rust"
    TEST_CMD="cargo test"
    TEST_PATTERN="*.rs"
elif [ -f "package.json" ]; then
    PROJECT_TYPE="node"
    TEST_CMD="pnpm test"
    TEST_PATTERN="*.spec.ts"
else
    echo "❌ Unknown project type"
    exit 1
fi

echo "📁 Detected $PROJECT_TYPE project"

# Keep container running and watch for tests
while true; do
    sleep 5
done