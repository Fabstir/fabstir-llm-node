#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Start Fabstir LLM Node with Enhanced S5.js Bridge
#
# This script:
# 1. Starts the Enhanced S5.js bridge service
# 2. Waits for bridge health check
# 3. Starts the Rust node

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
BRIDGE_URL="${BRIDGE_URL:-http://localhost:5522}"
BRIDGE_DIR="services/s5-bridge"
MAX_HEALTH_CHECKS=30
HEALTH_CHECK_INTERVAL=2

echo "╔════════════════════════════════════════════════════╗"
echo "║  Fabstir LLM Node with Enhanced S5.js Bridge      ║"
echo "╚════════════════════════════════════════════════════╝"
echo ""

# Check if bridge directory exists
if [ ! -d "$BRIDGE_DIR" ]; then
    echo -e "${RED}❌ Bridge directory not found: $BRIDGE_DIR${NC}"
    exit 1
fi

# Check if S5_SEED_PHRASE is set
if [ -z "$S5_SEED_PHRASE" ]; then
    echo -e "${RED}❌ S5_SEED_PHRASE environment variable not set${NC}"
    echo ""
    echo "Generate a seed phrase with:"
    echo "  cd $BRIDGE_DIR"
    echo "  node -e \"import('@julesl23/s5js').then(({S5}) => S5.generateSeedPhrase().then(console.log))\""
    echo ""
    echo "Then export it:"
    echo "  export S5_SEED_PHRASE=\"your twelve word phrase here\""
    exit 1
fi

# Function to check bridge health
check_bridge_health() {
    curl -sf "$BRIDGE_URL/health" > /dev/null 2>&1
    return $?
}

# Function to cleanup on exit
cleanup() {
    echo ""
    echo -e "${YELLOW}🛑 Shutting down services...${NC}"

    # Kill bridge if we started it
    if [ ! -z "$BRIDGE_PID" ] && ps -p "$BRIDGE_PID" > /dev/null 2>&1; then
        echo "   Stopping bridge service (PID: $BRIDGE_PID)"
        kill "$BRIDGE_PID" 2>/dev/null || true
        wait "$BRIDGE_PID" 2>/dev/null || true
    fi

    # Kill node if we started it
    if [ ! -z "$NODE_PID" ] && ps -p "$NODE_PID" > /dev/null 2>&1; then
        echo "   Stopping Rust node (PID: $NODE_PID)"
        kill "$NODE_PID" 2>/dev/null || true
        wait "$NODE_PID" 2>/dev/null || true
    fi

    echo -e "${GREEN}✅ Cleanup complete${NC}"
}

# Trap signals for cleanup
trap cleanup EXIT INT TERM

# Step 1: Check if bridge is already running
echo "🔍 Checking if bridge is already running..."
if check_bridge_health; then
    echo -e "${GREEN}✅ Bridge is already running${NC}"
else
    # Step 2: Start bridge service
    echo "🚀 Starting Enhanced S5.js bridge service..."
    cd "$BRIDGE_DIR"

    # Check if node_modules exists
    if [ ! -d "node_modules" ]; then
        echo "   Installing dependencies..."
        npm install
    fi

    # Start bridge in background
    npm start > /tmp/s5-bridge.log 2>&1 &
    BRIDGE_PID=$!
    cd - > /dev/null

    echo -e "${GREEN}   Bridge started (PID: $BRIDGE_PID)${NC}"
    echo "   Logs: /tmp/s5-bridge.log"

    # Step 3: Wait for bridge health check
    echo ""
    echo "⏳ Waiting for bridge to be ready..."
    attempt=0
    while [ $attempt -lt $MAX_HEALTH_CHECKS ]; do
        if check_bridge_health; then
            echo -e "${GREEN}✅ Bridge is healthy!${NC}"
            break
        fi

        attempt=$((attempt + 1))
        echo "   Attempt $attempt/$MAX_HEALTH_CHECKS..."
        sleep $HEALTH_CHECK_INTERVAL

        # Check if bridge process is still running
        if ! ps -p "$BRIDGE_PID" > /dev/null 2>&1; then
            echo -e "${RED}❌ Bridge process died unexpectedly${NC}"
            echo ""
            echo "Last 20 lines of bridge log:"
            tail -20 /tmp/s5-bridge.log
            exit 1
        fi
    done

    if [ $attempt -eq $MAX_HEALTH_CHECKS ]; then
        echo -e "${RED}❌ Bridge failed to become healthy after ${MAX_HEALTH_CHECKS} attempts${NC}"
        echo ""
        echo "Last 20 lines of bridge log:"
        tail -20 /tmp/s5-bridge.log
        exit 1
    fi
fi

# Get bridge status
echo ""
echo "📊 Bridge Status:"
curl -s "$BRIDGE_URL/health" | jq '.' 2>/dev/null || echo "   (jq not installed, raw output)"

# Step 4: Start Rust node
echo ""
echo "🚀 Starting Fabstir LLM Node..."
echo ""

# Use release build if available, otherwise dev build
if [ -f "target/release/fabstir-llm-node" ]; then
    RUST_NODE="target/release/fabstir-llm-node"
else
    RUST_NODE="cargo run --release"
fi

# Start node in foreground (or background with --daemon flag)
if [ "$1" = "--daemon" ]; then
    $RUST_NODE > /tmp/fabstir-node.log 2>&1 &
    NODE_PID=$!
    echo -e "${GREEN}✅ Node started in daemon mode (PID: $NODE_PID)${NC}"
    echo "   Logs: /tmp/fabstir-node.log"
    echo ""
    echo "To stop services, run:"
    echo "  kill $BRIDGE_PID $NODE_PID"
else
    # Run in foreground
    exec $RUST_NODE
fi
