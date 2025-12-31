#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1

# Download Florence-2 ONNX model for CPU-based image description
# This script downloads encoder and decoder models for vision-language tasks

set -e  # Exit on error

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
MODEL_NAME="florence-2"
# Get script directory and set model dir relative to project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "${SCRIPT_DIR}")"
MODEL_DIR="${PROJECT_ROOT}/models/${MODEL_NAME}-onnx"
# Using HuggingFace ONNX community models
HUGGINGFACE_BASE="https://huggingface.co/onnx-community/Florence-2-base-ft/resolve/main"

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}Fabstir LLM Node - Florence-2 Model Setup${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "This script downloads Florence-2 ONNX models"
echo "for CPU-based image description and analysis."
echo ""
echo -e "${BLUE}Florence-2 is a vision-language model that can:${NC}"
echo "  - Generate image captions"
echo "  - Describe objects and scenes"
echo "  - Answer questions about images"
echo ""
echo "Models:"
echo "  - Vision encoder (~450MB)"
echo "  - Language decoder (~1.2GB)"
echo "  - Tokenizer (~2MB)"
echo ""
echo -e "${YELLOW}Note: Total download ~1.7GB${NC}"
echo "Target: ${MODEL_DIR}"
echo ""

# Create model directory
echo -e "${YELLOW}Creating model directory...${NC}"
mkdir -p "${MODEL_DIR}"

# Check if model already exists
if [ -f "${MODEL_DIR}/vision_encoder.onnx" ] && \
   [ -f "${MODEL_DIR}/decoder_model.onnx" ] && \
   [ -f "${MODEL_DIR}/tokenizer.json" ]; then
    echo -e "${GREEN}Model files already exist${NC}"
    echo ""
    echo "To re-download, remove the directory first:"
    echo "  rm -rf ${MODEL_DIR}"
    echo ""
    exit 0
fi

# Function to download with retry and progress
download_file() {
    local url="$1"
    local output="$2"
    local description="$3"

    echo -e "${YELLOW}Downloading ${description}...${NC}"
    echo "  URL: ${url}"

    if command -v wget &> /dev/null; then
        wget -q --show-progress "${url}" -O "${output}"
    elif command -v curl &> /dev/null; then
        curl -L "${url}" -o "${output}" --progress-bar
    else
        echo -e "${RED}Error: Neither wget nor curl found. Please install one of them.${NC}"
        exit 1
    fi

    if [ ! -f "${output}" ] || [ ! -s "${output}" ]; then
        echo -e "${RED}Error: Failed to download ${description}${NC}"
        exit 1
    fi
    echo -e "${GREEN}Downloaded ${description}${NC}"
}

# Download vision encoder
download_file \
    "${HUGGINGFACE_BASE}/onnx/vision_encoder.onnx" \
    "${MODEL_DIR}/vision_encoder.onnx" \
    "vision encoder (~450MB)"

# Download decoder model
download_file \
    "${HUGGINGFACE_BASE}/onnx/decoder_model.onnx" \
    "${MODEL_DIR}/decoder_model.onnx" \
    "language decoder (~1.2GB)"

# Download encoder-decoder combined model (if available, for single-pass inference)
echo -e "${YELLOW}Attempting to download encoder-decoder model (optional)...${NC}"
if command -v wget &> /dev/null; then
    wget -q "${HUGGINGFACE_BASE}/onnx/encoder_decoder_model.onnx" \
        -O "${MODEL_DIR}/encoder_decoder_model.onnx" 2>/dev/null || true
elif command -v curl &> /dev/null; then
    curl -sL "${HUGGINGFACE_BASE}/onnx/encoder_decoder_model.onnx" \
        -o "${MODEL_DIR}/encoder_decoder_model.onnx" 2>/dev/null || true
fi

if [ -f "${MODEL_DIR}/encoder_decoder_model.onnx" ] && [ -s "${MODEL_DIR}/encoder_decoder_model.onnx" ]; then
    echo -e "${GREEN}Downloaded encoder-decoder combined model${NC}"
else
    rm -f "${MODEL_DIR}/encoder_decoder_model.onnx"
    echo -e "${YELLOW}Encoder-decoder combined model not available (using separate models)${NC}"
fi

# Download tokenizer
download_file \
    "${HUGGINGFACE_BASE}/tokenizer.json" \
    "${MODEL_DIR}/tokenizer.json" \
    "tokenizer (~2MB)"

# Download tokenizer config
echo -e "${YELLOW}Downloading tokenizer config...${NC}"
if command -v wget &> /dev/null; then
    wget -q "${HUGGINGFACE_BASE}/tokenizer_config.json" \
        -O "${MODEL_DIR}/tokenizer_config.json" 2>/dev/null || true
elif command -v curl &> /dev/null; then
    curl -sL "${HUGGINGFACE_BASE}/tokenizer_config.json" \
        -o "${MODEL_DIR}/tokenizer_config.json" 2>/dev/null || true
fi

if [ -f "${MODEL_DIR}/tokenizer_config.json" ]; then
    echo -e "${GREEN}Downloaded tokenizer config${NC}"
fi

# Download special tokens map
echo -e "${YELLOW}Downloading special tokens map...${NC}"
if command -v wget &> /dev/null; then
    wget -q "${HUGGINGFACE_BASE}/special_tokens_map.json" \
        -O "${MODEL_DIR}/special_tokens_map.json" 2>/dev/null || true
elif command -v curl &> /dev/null; then
    curl -sL "${HUGGINGFACE_BASE}/special_tokens_map.json" \
        -o "${MODEL_DIR}/special_tokens_map.json" 2>/dev/null || true
fi

if [ -f "${MODEL_DIR}/special_tokens_map.json" ]; then
    echo -e "${GREEN}Downloaded special tokens map${NC}"
fi

# Verify file sizes
echo ""
echo -e "${YELLOW}Verifying downloads...${NC}"

ENCODER_SIZE=$(stat -f%z "${MODEL_DIR}/vision_encoder.onnx" 2>/dev/null || stat -c%s "${MODEL_DIR}/vision_encoder.onnx" 2>/dev/null)
DECODER_SIZE=$(stat -f%z "${MODEL_DIR}/decoder_model.onnx" 2>/dev/null || stat -c%s "${MODEL_DIR}/decoder_model.onnx" 2>/dev/null)
TOKENIZER_SIZE=$(stat -f%z "${MODEL_DIR}/tokenizer.json" 2>/dev/null || stat -c%s "${MODEL_DIR}/tokenizer.json" 2>/dev/null)

# Minimum expected sizes (in bytes)
MIN_ENCODER_SIZE=100000000   # 100MB minimum
MIN_DECODER_SIZE=500000000   # 500MB minimum
MIN_TOKENIZER_SIZE=100000    # 100KB minimum

if [ "$ENCODER_SIZE" -lt "$MIN_ENCODER_SIZE" ]; then
    echo -e "${RED}Warning: Vision encoder seems too small (${ENCODER_SIZE} bytes)${NC}"
    echo -e "${YELLOW}Expected at least 100MB${NC}"
fi

if [ "$DECODER_SIZE" -lt "$MIN_DECODER_SIZE" ]; then
    echo -e "${RED}Warning: Decoder model seems too small (${DECODER_SIZE} bytes)${NC}"
    echo -e "${YELLOW}Expected at least 500MB${NC}"
fi

if [ "$TOKENIZER_SIZE" -lt "$MIN_TOKENIZER_SIZE" ]; then
    echo -e "${RED}Warning: Tokenizer seems too small (${TOKENIZER_SIZE} bytes)${NC}"
fi

echo -e "${GREEN}File sizes:${NC}"
echo "  vision_encoder.onnx: $(numfmt --to=iec-i --suffix=B $ENCODER_SIZE 2>/dev/null || echo "${ENCODER_SIZE} bytes")"
echo "  decoder_model.onnx: $(numfmt --to=iec-i --suffix=B $DECODER_SIZE 2>/dev/null || echo "${DECODER_SIZE} bytes")"
echo "  tokenizer.json: $(numfmt --to=iec-i --suffix=B $TOKENIZER_SIZE 2>/dev/null || echo "${TOKENIZER_SIZE} bytes")"

# Calculate total size
TOTAL_SIZE=$((ENCODER_SIZE + DECODER_SIZE + TOKENIZER_SIZE))
echo "  Total: $(numfmt --to=iec-i --suffix=B $TOTAL_SIZE 2>/dev/null || echo "${TOTAL_SIZE} bytes")"

# Create version file
cat > "${MODEL_DIR}/VERSION" << EOF
Florence-2 ONNX Models
Model: Florence-2-base-ft
Version: ONNX Community Export
Vision Encoder: vision_encoder.onnx
Language Decoder: decoder_model.onnx
Tokenizer: tokenizer.json
Input Size: 768x768
Downloaded: $(date)
Source: https://huggingface.co/onnx-community/Florence-2-base-ft
EOF

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}Florence-2 model setup complete!${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "Model files installed at:"
echo "  ${MODEL_DIR}/"
echo ""
echo "Next steps:"
echo "  1. Start the fabstir-llm-node"
echo "  2. The /v1/describe-image endpoint will be available"
echo "  3. Vision uses CPU only - no GPU VRAM required!"
echo ""
echo -e "${BLUE}Supported tasks:${NC}"
echo "  - <CAPTION>: Brief image caption"
echo "  - <DETAILED_CAPTION>: Detailed description"
echo "  - <MORE_DETAILED_CAPTION>: Comprehensive analysis"
echo "  - <OD>: Object detection"
echo ""
echo "To verify the installation:"
echo "  ls -lh ${MODEL_DIR}/"
echo ""
