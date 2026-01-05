#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1

# Download PaddleOCR ONNX models for CPU-based OCR
# This script downloads detection and recognition models for text extraction

set -e  # Exit on error

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
MODEL_NAME="paddleocr"
# Get script directory and set model dir relative to project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "${SCRIPT_DIR}")"
MODEL_DIR="${PROJECT_ROOT}/models/${MODEL_NAME}-onnx"
# Using HuggingFace mirror of PaddleOCR ONNX models
HUGGINGFACE_BASE="https://huggingface.co/tomaarsen/paddleocr-onnx/resolve/main"

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}Fabstir LLM Node - PaddleOCR Model Setup${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "This script downloads PaddleOCR ONNX models"
echo "for CPU-based text extraction from images."
echo ""
echo "Models:"
echo "  - Detection model (PP-OCRv4) ~3MB"
echo "  - Recognition model (PP-OCRv4) ~10MB"
echo "  - Character dictionary"
echo ""
echo "Target: ${MODEL_DIR}"
echo ""

# Create model directory
echo -e "${YELLOW}Creating model directory...${NC}"
mkdir -p "${MODEL_DIR}"

# Check if model already exists
if [ -f "${MODEL_DIR}/det_model.onnx" ] && \
   [ -f "${MODEL_DIR}/rec_model.onnx" ] && \
   [ -f "${MODEL_DIR}/ppocr_keys_v1.txt" ]; then
    echo -e "${GREEN}Model files already exist${NC}"
    echo ""
    echo "To re-download, remove the directory first:"
    echo "  rm -rf ${MODEL_DIR}"
    echo ""
    exit 0
fi

# Function to download with retry
download_file() {
    local url="$1"
    local output="$2"
    local description="$3"

    echo -e "${YELLOW}Downloading ${description}...${NC}"

    if command -v wget &> /dev/null; then
        wget -q --show-progress "${url}" -O "${output}"
    elif command -v curl &> /dev/null; then
        curl -L "${url}" -o "${output}" --progress-bar
    else
        echo -e "${RED}Error: Neither wget nor curl found. Please install one of them.${NC}"
        exit 1
    fi

    if [ ! -f "${output}" ]; then
        echo -e "${RED}Error: Failed to download ${description}${NC}"
        exit 1
    fi
    echo -e "${GREEN}Downloaded ${description}${NC}"
}

# Download detection model
download_file \
    "${HUGGINGFACE_BASE}/ch_PP-OCRv4_det_infer.onnx" \
    "${MODEL_DIR}/det_model.onnx" \
    "detection model (~3MB)"

# Download recognition model
download_file \
    "${HUGGINGFACE_BASE}/ch_PP-OCRv4_rec_infer.onnx" \
    "${MODEL_DIR}/rec_model.onnx" \
    "recognition model (~10MB)"

# Download character dictionary
download_file \
    "${HUGGINGFACE_BASE}/ppocr_keys_v1.txt" \
    "${MODEL_DIR}/ppocr_keys_v1.txt" \
    "character dictionary"

# Verify file sizes
echo -e "${YELLOW}Verifying downloads...${NC}"

DET_SIZE=$(stat -f%z "${MODEL_DIR}/det_model.onnx" 2>/dev/null || stat -c%s "${MODEL_DIR}/det_model.onnx" 2>/dev/null)
REC_SIZE=$(stat -f%z "${MODEL_DIR}/rec_model.onnx" 2>/dev/null || stat -c%s "${MODEL_DIR}/rec_model.onnx" 2>/dev/null)
DICT_SIZE=$(stat -f%z "${MODEL_DIR}/ppocr_keys_v1.txt" 2>/dev/null || stat -c%s "${MODEL_DIR}/ppocr_keys_v1.txt" 2>/dev/null)

if [ "$DET_SIZE" -lt 1000000 ]; then
    echo -e "${RED}Warning: Detection model seems too small (${DET_SIZE} bytes)${NC}"
fi

if [ "$REC_SIZE" -lt 5000000 ]; then
    echo -e "${RED}Warning: Recognition model seems too small (${REC_SIZE} bytes)${NC}"
fi

if [ "$DICT_SIZE" -lt 10000 ]; then
    echo -e "${RED}Warning: Dictionary file seems too small (${DICT_SIZE} bytes)${NC}"
fi

echo -e "${GREEN}File sizes:${NC}"
echo "  det_model.onnx: $(numfmt --to=iec-i --suffix=B $DET_SIZE 2>/dev/null || echo "${DET_SIZE} bytes")"
echo "  rec_model.onnx: $(numfmt --to=iec-i --suffix=B $REC_SIZE 2>/dev/null || echo "${REC_SIZE} bytes")"
echo "  ppocr_keys_v1.txt: $(numfmt --to=iec-i --suffix=B $DICT_SIZE 2>/dev/null || echo "${DICT_SIZE} bytes")"

# Create version file
cat > "${MODEL_DIR}/VERSION" << EOF
PaddleOCR ONNX Models
Version: PP-OCRv4
Detection: ch_PP-OCRv4_det_infer.onnx
Recognition: ch_PP-OCRv4_rec_infer.onnx
Dictionary: ppocr_keys_v1.txt (Chinese + English)
Downloaded: $(date)
Source: https://huggingface.co/tomaarsen/paddleocr-onnx
EOF

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}PaddleOCR model setup complete!${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "Model files installed at:"
echo "  ${MODEL_DIR}/"
echo ""
echo "Next steps:"
echo "  1. Start the fabstir-llm-node"
echo "  2. The /v1/ocr endpoint will be available"
echo "  3. OCR uses CPU only - no GPU VRAM required!"
echo ""
echo "To verify the installation:"
echo "  ls -lh ${MODEL_DIR}/"
echo ""
