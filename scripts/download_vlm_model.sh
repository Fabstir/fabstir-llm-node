#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1

# Download Qwen3-VL GGUF model for VLM sidecar (llama-server)
# This model enables high-quality OCR and image description via GPU
# Requires BOTH the language model AND vision encoder (mmproj) files

set -e  # Exit on error

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
MODEL_NAME="qwen3-vl"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "${SCRIPT_DIR}")"
MODEL_DIR="${PROJECT_ROOT}/models/${MODEL_NAME}"

# HuggingFace official Qwen repo
HF_REPO="Qwen/Qwen3-VL-8B-Instruct-GGUF"
HF_BASE="https://huggingface.co/${HF_REPO}/resolve/main"

# Language model (Q4_K_M quantization, ~5GB)
MODEL_FILE="Qwen3VL-8B-Instruct-Q4_K_M.gguf"
MODEL_URL="${HF_BASE}/${MODEL_FILE}"
EXPECTED_MODEL_SIZE_MB=4500  # ~5GB for Q4_K_M

# Vision encoder (mmproj - required for image understanding)
MMPROJ_FILE="mmproj-Qwen3VL-8B-Instruct-F16.gguf"
MMPROJ_URL="${HF_BASE}/${MMPROJ_FILE}"
EXPECTED_MMPROJ_SIZE_MB=100  # Vision encoder is much smaller

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}Fabstir LLM Node - VLM Model Setup${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "This script downloads Qwen3-VL-8B-Instruct (Q4_K_M) GGUF model"
echo "for GPU-based OCR and image description via llama-server."
echo ""
echo -e "${BLUE}Qwen3-VL is a vision-language model that can:${NC}"
echo "  - Extract text from images (OCR)"
echo "  - Describe images in detail"
echo "  - Answer questions about images"
echo ""
echo "Files to download:"
echo "  1. ${MODEL_FILE} (~5GB) - Language model"
echo "  2. ${MMPROJ_FILE} - Vision encoder"
echo "Target: ${MODEL_DIR}/"
echo ""

# Check for required tools
if ! command -v curl &> /dev/null; then
    echo -e "${RED}Error: curl is required but not installed${NC}"
    exit 1
fi

# Create model directory
mkdir -p "${MODEL_DIR}"

# --- Download language model ---

download_file() {
    local file_name="$1"
    local file_url="$2"
    local min_size_mb="$3"
    local description="$4"

    if [ -f "${MODEL_DIR}/${file_name}" ]; then
        ACTUAL_SIZE=$(du -m "${MODEL_DIR}/${file_name}" | cut -f1)
        if [ "${ACTUAL_SIZE}" -ge "${min_size_mb}" ]; then
            echo -e "${YELLOW}${description} already exists: ${file_name} (${ACTUAL_SIZE}MB)${NC}"
            read -r -p "Re-download? [y/N] " response
            if [[ ! "$response" =~ ^[Yy]$ ]]; then
                echo -e "${GREEN}Using existing file.${NC}"
                echo ""
                return 0
            fi
        else
            echo -e "${YELLOW}${description} exists but seems too small (${ACTUAL_SIZE}MB < ${min_size_mb}MB), re-downloading...${NC}"
        fi
    fi

    echo -e "${BLUE}Downloading ${description}: ${file_name}...${NC}"
    echo "URL: ${file_url}"
    echo ""

    curl -L --progress-bar \
        -o "${MODEL_DIR}/${file_name}" \
        "${file_url}"

    # Verify download
    if [ ! -f "${MODEL_DIR}/${file_name}" ]; then
        echo -e "${RED}Error: Download failed - file not found${NC}"
        exit 1
    fi

    ACTUAL_SIZE=$(du -m "${MODEL_DIR}/${file_name}" | cut -f1)
    echo ""
    echo -e "${GREEN}Download complete: ${file_name} (${ACTUAL_SIZE}MB)${NC}"

    # Size sanity check
    if [ "${ACTUAL_SIZE}" -lt "${min_size_mb}" ]; then
        echo -e "${RED}Warning: File seems too small (${ACTUAL_SIZE}MB < ${min_size_mb}MB)${NC}"
        echo -e "${RED}The download may have failed or the URL may be incorrect.${NC}"
        echo -e "${RED}Try downloading manually from: https://huggingface.co/${HF_REPO}${NC}"
        exit 1
    fi
    echo ""
}

echo -e "${BLUE}--- Step 1/2: Language Model ---${NC}"
echo ""
download_file "${MODEL_FILE}" "${MODEL_URL}" "${EXPECTED_MODEL_SIZE_MB}" "Language model"

echo -e "${BLUE}--- Step 2/2: Vision Encoder ---${NC}"
echo ""
download_file "${MMPROJ_FILE}" "${MMPROJ_URL}" "${EXPECTED_MMPROJ_SIZE_MB}" "Vision encoder"

# Create VERSION file
echo "qwen3-vl-8b-instruct-q4_k_m" > "${MODEL_DIR}/VERSION"

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}VLM Model Setup Complete${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "Model location: ${MODEL_DIR}/"
echo "  Language model:  ${MODEL_FILE}"
echo "  Vision encoder:  ${MMPROJ_FILE}"
echo ""
echo "To use with llama-server:"
echo "  llama-server --model ${MODEL_DIR}/${MODEL_FILE} \\"
echo "    --mmproj ${MODEL_DIR}/${MMPROJ_FILE} \\"
echo "    --host 0.0.0.0 --port 8081 \\"
echo "    --ctx-size 4096 --n-gpu-layers 99"
echo ""
echo "Then set in your .env:"
echo "  VLM_ENDPOINT=http://localhost:8081"
echo "  VLM_MODEL_NAME=qwen3-vl"
echo ""
