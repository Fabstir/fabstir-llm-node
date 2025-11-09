#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1

# Test script for GPT-OSS-20B with v8.3.13 CORRECT Harmony format

set -e

echo "========================================="
echo "GPT-OSS-20B Test - v8.3.13 (Harmony)"
echo "========================================="

source .env.local.test

# Verify binary version
echo ""
echo "Binary version check:"
strings target/release/fabstir-llm-node | grep "v8\.3\.13" | head -1

# Stop and remove old containers
echo ""
echo "Stopping old containers..."
docker stop llm-node-prod-1 2>/dev/null || true
docker rm llm-node-prod-1 2>/dev/null || true

# Get absolute path to models directory
MODELS_DIR="$(pwd)/models"
echo "Models directory: $MODELS_DIR"

# Start node with MXFP4 model (recommended quantization)
echo ""
echo "Starting test node with GPT-OSS-20B (MXFP4 - recommended)..."
docker run -d \
  --name llm-node-prod-1 \
  -p 9001-9003:9001-9003 \
  -p 8080:8080 \
  -v "$MODELS_DIR:/models:ro" \
  -e P2P_PORT=9001 \
  -e API_PORT=8080 \
  -e MODEL_PATH=/models/openai_gpt-oss-20b-MXFP4.gguf \
  -e MODEL_CHAT_TEMPLATE=harmony \
  -e HOST_PRIVATE_KEY="${TEST_HOST_1_PRIVATE_KEY}" \
  -e RPC_URL="https://base-sepolia.g.alchemy.com/v2/1pZoccdtgU8CMyxXzE3l_ghnBBaJABMR" \
  -e CONTRACT_JOB_MARKETPLACE="${CONTRACT_JOB_MARKETPLACE}" \
  -e CONTRACT_HOST_EARNINGS="${CONTRACT_HOST_EARNINGS}" \
  -e CONTRACT_NODE_REGISTRY="${CONTRACT_NODE_REGISTRY}" \
  -e CONTRACT_PROOF_SYSTEM="${CONTRACT_PROOF_SYSTEM}" \
  -e TREASURY_FEE_PERCENTAGE="${TREASURY_FEE_PERCENTAGE}" \
  -e HOST_EARNINGS_PERCENTAGE="${HOST_EARNINGS_PERCENTAGE}" \
  -e RUST_LOG=info,fabstir_llm_node=debug \
  --gpus all \
  --add-host host.docker.internal:host-gateway \
  llm-node-prod:latest

# Wait for model to load
echo ""
echo "Waiting 15 seconds for model to load..."
sleep 15

# Verify deployment
echo ""
echo "========================================="
echo "VERIFICATION"
echo "========================================="
docker logs llm-node-prod-1 2>&1 | grep -E "BUILD VERSION" | head -2
echo ""
echo "Checking for Harmony template with channels..."
docker logs llm-node-prod-1 2>&1 | grep "🎨" | head -3
echo ""
echo "Verifying <|channel|>final<|message|> in prompt..."
docker logs llm-node-prod-1 2>&1 | grep "channel" | head -3
echo ""
echo "Verifying encryption is enabled..."
docker logs llm-node-prod-1 2>&1 | grep -i "encryption\|HOST_PRIVATE_KEY" | head -5

echo ""
echo "========================================="
echo "Node is ready for testing!"
echo "========================================="
echo ""
echo "Run these curl tests:"
echo ""
echo "TEST 1: What is 2+2?"
echo "curl -s -X POST http://localhost:8080/v1/inference \\"
echo "  -H 'Content-Type: application/json' \\"
echo "  -d '{\"model\": \"gpt-oss-20b\", \"prompt\": \"What is 2+2?\", \"max_tokens\": 20, \"temperature\": 0.1, \"chain_id\": 84532}' | jq ."
echo ""
echo "TEST 2: What is the capital of France?"
echo "curl -s -X POST http://localhost:8080/v1/inference \\"
echo "  -H 'Content-Type: application/json' \\"
echo "  -d '{\"model\": \"gpt-oss-20b\", \"prompt\": \"What is the capital of France?\", \"max_tokens\": 20, \"temperature\": 0.1, \"chain_id\": 84532}' | jq ."