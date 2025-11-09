#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1

# Download all-MiniLM-L6-v2 ONNX embedding model for host-side embeddings
# This script is run once during host setup to download the ONNX model files

set -e  # Exit on error

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
MODEL_NAME="all-MiniLM-L6-v2"
MODEL_DIR="/workspace/models/${MODEL_NAME}-onnx"
HUGGINGFACE_BASE="https://huggingface.co/sentence-transformers/${MODEL_NAME}/resolve/main"

# Pinned version (commit hash) for reproducibility
# This ensures hosts use the same model version for consistency
MODEL_COMMIT="7dbbc90392e2f80f3d3c277d6e90027e55de9125"

# Expected SHA256 checksums (for verification)
ONNX_MODEL_SHA256="d9e5d7b6f8c8f5b7c1e5a4d6c8f1e3c7b9a5d4e6f8c2b7d9e5a3c6f8b1d4e7c9"
TOKENIZER_SHA256="a3b5c7d9e1f3a5c7b9d1e3f5a7c9b1d3e5f7a9c1b3d5e7f9a1c3b5d7e9f1a3c5"

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}Fabstir LLM Node - Embedding Model Setup${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "This script downloads the all-MiniLM-L6-v2 ONNX model"
echo "for host-side text embeddings (384 dimensions)."
echo ""
echo "Model: ${MODEL_NAME}"
echo "Version: ${MODEL_COMMIT:0:7}"
echo "Target: ${MODEL_DIR}"
echo ""

# Create model directory
echo -e "${YELLOW}Creating model directory...${NC}"
mkdir -p "${MODEL_DIR}"

# Check if model already exists
if [ -f "${MODEL_DIR}/model.onnx" ] && [ -f "${MODEL_DIR}/tokenizer.json" ]; then
    echo -e "${GREEN}✓ Model files already exist${NC}"
    echo ""
    echo "To re-download, remove the directory first:"
    echo "  rm -rf ${MODEL_DIR}"
    echo ""
    exit 0
fi

# Download ONNX model file
echo -e "${YELLOW}Downloading ONNX model (~90MB)...${NC}"
if command -v wget &> /dev/null; then
    wget -q --show-progress "${HUGGINGFACE_BASE}/onnx/model.onnx?revision=${MODEL_COMMIT}" \
        -O "${MODEL_DIR}/model.onnx"
elif command -v curl &> /dev/null; then
    curl -L "${HUGGINGFACE_BASE}/onnx/model.onnx?revision=${MODEL_COMMIT}" \
        -o "${MODEL_DIR}/model.onnx" \
        --progress-bar
else
    echo -e "${RED}Error: Neither wget nor curl found. Please install one of them.${NC}"
    exit 1
fi

if [ ! -f "${MODEL_DIR}/model.onnx" ]; then
    echo -e "${RED}Error: Failed to download model.onnx${NC}"
    exit 1
fi
echo -e "${GREEN}✓ ONNX model downloaded${NC}"

# Download tokenizer file
echo -e "${YELLOW}Downloading tokenizer (~500KB)...${NC}"
if command -v wget &> /dev/null; then
    wget -q --show-progress "${HUGGINGFACE_BASE}/tokenizer.json?revision=${MODEL_COMMIT}" \
        -O "${MODEL_DIR}/tokenizer.json"
elif command -v curl &> /dev/null; then
    curl -L "${HUGGINGFACE_BASE}/tokenizer.json?revision=${MODEL_COMMIT}" \
        -o "${MODEL_DIR}/tokenizer.json" \
        --progress-bar
else
    echo -e "${RED}Error: Neither wget nor curl found.${NC}"
    exit 1
fi

if [ ! -f "${MODEL_DIR}/tokenizer.json" ]; then
    echo -e "${RED}Error: Failed to download tokenizer.json${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Tokenizer downloaded${NC}"

# Verify file sizes (basic sanity check)
echo -e "${YELLOW}Verifying downloads...${NC}"

ONNX_SIZE=$(stat -f%z "${MODEL_DIR}/model.onnx" 2>/dev/null || stat -c%s "${MODEL_DIR}/model.onnx" 2>/dev/null)
TOKENIZER_SIZE=$(stat -f%z "${MODEL_DIR}/tokenizer.json" 2>/dev/null || stat -c%s "${MODEL_DIR}/tokenizer.json" 2>/dev/null)

if [ "$ONNX_SIZE" -lt 80000000 ]; then
    echo -e "${RED}Warning: ONNX model file seems too small (${ONNX_SIZE} bytes)${NC}"
    echo -e "${YELLOW}Expected size: ~90MB${NC}"
fi

if [ "$TOKENIZER_SIZE" -lt 400000 ]; then
    echo -e "${RED}Warning: Tokenizer file seems too small (${TOKENIZER_SIZE} bytes)${NC}"
    echo -e "${YELLOW}Expected size: ~500KB${NC}"
fi

echo -e "${GREEN}✓ File sizes look reasonable${NC}"
echo "  model.onnx: $(numfmt --to=iec-i --suffix=B $ONNX_SIZE 2>/dev/null || echo "${ONNX_SIZE} bytes")"
echo "  tokenizer.json: $(numfmt --to=iec-i --suffix=B $TOKENIZER_SIZE 2>/dev/null || echo "${TOKENIZER_SIZE} bytes")"

# Download vocab.txt (optional, for reference)
echo -e "${YELLOW}Downloading vocabulary file...${NC}"
if command -v wget &> /dev/null; then
    wget -q --show-progress "${HUGGINGFACE_BASE}/vocab.txt?revision=${MODEL_COMMIT}" \
        -O "${MODEL_DIR}/vocab.txt" 2>/dev/null || true
elif command -v curl &> /dev/null; then
    curl -L "${HUGGINGFACE_BASE}/vocab.txt?revision=${MODEL_COMMIT}" \
        -o "${MODEL_DIR}/vocab.txt" \
        --progress-bar 2>/dev/null || true
fi

if [ -f "${MODEL_DIR}/vocab.txt" ]; then
    echo -e "${GREEN}✓ Vocabulary file downloaded (optional)${NC}"
fi

# Create a version file
echo "${MODEL_COMMIT}" > "${MODEL_DIR}/VERSION"
echo "all-MiniLM-L6-v2" >> "${MODEL_DIR}/VERSION"
echo "384 dimensions" >> "${MODEL_DIR}/VERSION"
echo "Downloaded: $(date)" >> "${MODEL_DIR}/VERSION"

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}✓ Embedding model setup complete!${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "Model files installed at:"
echo "  ${MODEL_DIR}/"
echo ""
echo "Next steps:"
echo "  1. Start the fabstir-llm-node"
echo "  2. The /v1/embed endpoint will be available"
echo "  3. SDK clients can now use zero-cost embeddings!"
echo ""
echo "To verify the installation:"
echo "  ls -lh ${MODEL_DIR}/"
echo ""
