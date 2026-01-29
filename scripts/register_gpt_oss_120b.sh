#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Register GPT-OSS-120B model in ModelRegistry contract
# Requires contract owner private key

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
MODEL_DIR="${PROJECT_ROOT}/models/gpt-oss-120b-GGUF"

echo "========================================="
echo "ModelRegistry Registration - GPT-OSS-120B"
echo "========================================="
echo ""

# Check if .env.local.test exists
if [ ! -f "${PROJECT_ROOT}/.env.local.test" ]; then
    echo "❌ Error: .env.local.test not found"
    echo "This file should contain contract addresses and RPC URL"
    exit 1
fi

# Source environment variables
source "${PROJECT_ROOT}/.env.local.test"

# Check if composite hash file exists
if [ ! -f "${MODEL_DIR}/gpt-oss-120b-composite-hash.txt" ]; then
    echo "❌ Error: Composite hash file not found"
    echo "Expected: ${MODEL_DIR}/gpt-oss-120b-composite-hash.txt"
    echo ""
    echo "Run the download script first:"
    echo "  ./scripts/download_gpt_oss_120b.sh"
    exit 1
fi

# Load composite hash
COMPOSITE_HASH=$(cat "${MODEL_DIR}/gpt-oss-120b-composite-hash.txt")
BYTES32_HASH="0x${COMPOSITE_HASH}"

echo "Model Information:"
echo "  Repository: ggml-org/gpt-oss-120b-GGUF"
echo "  File: gpt-oss-120b-mxfp4-00001-of-00003.gguf"
echo "  Composite Hash: $COMPOSITE_HASH"
echo ""

# Calculate model ID
MODEL_ID=$(cast keccak "ggml-org/gpt-oss-120b-GGUF/gpt-oss-120b-mxfp4-00001-of-00003.gguf")
echo "Model ID (keccak256): $MODEL_ID"
echo ""

# Check if already registered
echo "Checking if model is already registered..."
IS_APPROVED=$(cast call $CONTRACT_MODEL_REGISTRY \
    "isModelApproved(bytes32)" \
    "$MODEL_ID" \
    --rpc-url "$BASE_SEPOLIA_RPC_URL" 2>/dev/null || echo "0x0")

if [ "$IS_APPROVED" = "0x0000000000000000000000000000000000000000000000000000000000000001" ]; then
    echo "✅ Model is already registered and approved!"
    echo ""
    echo "Model Details:"
    cast call $CONTRACT_MODEL_REGISTRY \
        "getModel(bytes32)" \
        "$MODEL_ID" \
        --rpc-url "$BASE_SEPOLIA_RPC_URL"
    exit 0
fi

echo "⚠️  Model not registered. Proceeding with registration..."
echo ""

# Check if OWNER_PRIVATE_KEY is set
if [ -z "$OWNER_PRIVATE_KEY" ]; then
    echo "❌ Error: OWNER_PRIVATE_KEY not set in environment"
    echo ""
    echo "Only the contract owner can register trusted models."
    echo "Set OWNER_PRIVATE_KEY in .env.local.test or export it:"
    echo "  export OWNER_PRIVATE_KEY=0x..."
    exit 1
fi

# Register the model
echo "Registering model in ModelRegistry..."
echo "This requires contract owner privileges."
echo ""

TX_HASH=$(cast send $CONTRACT_MODEL_REGISTRY \
    "addTrustedModel(string,string,bytes32)" \
    "ggml-org/gpt-oss-120b-GGUF" \
    "gpt-oss-120b-mxfp4-00001-of-00003.gguf" \
    "$BYTES32_HASH" \
    --rpc-url "$BASE_SEPOLIA_RPC_URL" \
    --private-key "$OWNER_PRIVATE_KEY" \
    --legacy 2>&1 | grep -o "0x[a-fA-F0-9]\{64\}" | head -1 || echo "")

if [ -z "$TX_HASH" ]; then
    echo "❌ Registration transaction failed"
    echo "Check that:"
    echo "  1. OWNER_PRIVATE_KEY is the contract owner"
    echo "  2. Account has sufficient gas on Base Sepolia"
    echo "  3. RPC URL is correct and accessible"
    exit 1
fi

echo "✅ Registration transaction sent: $TX_HASH"
echo ""
echo "Waiting for confirmation (15 seconds)..."
sleep 15

# Verify registration
echo ""
echo "Verifying registration..."
IS_APPROVED=$(cast call $CONTRACT_MODEL_REGISTRY \
    "isModelApproved(bytes32)" \
    "$MODEL_ID" \
    --rpc-url "$BASE_SEPOLIA_RPC_URL")

if [ "$IS_APPROVED" = "0x0000000000000000000000000000000000000000000000000000000000000001" ]; then
    echo "✅ Model successfully registered in ModelRegistry!"
    echo ""
    echo "Model Details:"
    cast call $CONTRACT_MODEL_REGISTRY \
        "getModel(bytes32)" \
        "$MODEL_ID" \
        --rpc-url "$BASE_SEPOLIA_RPC_URL"
    echo ""
    echo "========================================="
    echo "NEXT STEPS"
    echo "========================================="
    echo ""
    echo "1. Update your node registration to support this model:"
    echo "   cast send $CONTRACT_NODE_REGISTRY \\"
    echo "     \"updateSupportedModels(bytes32[])\" \\"
    echo "     \"[$MODEL_ID]\" \\"
    echo "     --rpc-url $BASE_SEPOLIA_RPC_URL \\"
    echo "     --private-key \$HOST_PRIVATE_KEY \\"
    echo "     --legacy"
    echo ""
    echo "2. Document the hashes for other hosts (see plan)"
    echo ""
    echo "3. Deploy the node with updated configuration"
else
    echo "❌ Model registration verification failed"
    echo "The transaction was sent but approval check returned false"
    exit 1
fi
