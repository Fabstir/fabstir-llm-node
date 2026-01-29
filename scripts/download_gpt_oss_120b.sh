#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Download and verify GPT-OSS-120B model (MXFP4, 3-part split, ~65GB)
# For NVIDIA RTX Pro 6000 Blackwell GPU with 97GB VRAM

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
MODELS_DIR="${PROJECT_ROOT}/models"
MODEL_DIR="${MODELS_DIR}/gpt-oss-120b-GGUF"

echo "========================================="
echo "GPT-OSS-120B Model Download"
echo "========================================="
echo ""
echo "Model: gpt-oss-120b-GGUF (MXFP4 quantization)"
echo "Size: ~65GB (3 parts)"
echo "Context: 131,072 tokens"
echo "Target GPU: NVIDIA RTX Pro 6000 Blackwell (97GB VRAM)"
echo ""

# Create models directory
mkdir -p "$MODEL_DIR"
cd "$MODEL_DIR"

echo "Download directory: $MODEL_DIR"
echo ""

# Check if huggingface-cli is available
if command -v huggingface-cli >/dev/null 2>&1; then
    echo "✅ Using huggingface-cli (recommended method)"
    echo ""

    # Download using huggingface-cli
    huggingface-cli download ggml-org/gpt-oss-120b-GGUF \
        --local-dir . \
        --include "gpt-oss-120b-mxfp4-*.gguf"

else
    echo "⚠️  huggingface-cli not found, using wget"
    echo "Installing huggingface-cli is recommended: pip install -U 'huggingface_hub[cli]'"
    echo ""

    # Base URL for Hugging Face downloads
    BASE_URL="https://huggingface.co/ggml-org/gpt-oss-120b-GGUF/resolve/main"

    # Download each part
    for i in 1 2 3; do
        PART=$(printf "%05d" $i)
        FILENAME="gpt-oss-120b-mxfp4-${PART}-of-00003.gguf"

        if [ -f "$FILENAME" ]; then
            echo "✅ $FILENAME already exists, skipping download"
        else
            echo "📥 Downloading part $i of 3: $FILENAME"
            wget -c "${BASE_URL}/${FILENAME}" -O "$FILENAME"
        fi
        echo ""
    done
fi

echo ""
echo "========================================="
echo "VERIFICATION"
echo "========================================="

# Check file sizes
echo ""
echo "File sizes:"
ls -lh gpt-oss-120b-mxfp4-*.gguf

# Calculate total size
TOTAL_SIZE=$(du -sh . | cut -f1)
echo ""
echo "Total size: $TOTAL_SIZE"

# Compute individual SHA256 hashes
echo ""
echo "Computing SHA256 hashes (this may take several minutes)..."
echo ""

HASH1=$(sha256sum gpt-oss-120b-mxfp4-00001-of-00003.gguf | cut -d' ' -f1)
HASH2=$(sha256sum gpt-oss-120b-mxfp4-00002-of-00003.gguf | cut -d' ' -f1)
HASH3=$(sha256sum gpt-oss-120b-mxfp4-00003-of-00003.gguf | cut -d' ' -f1)

echo "Individual file hashes:"
echo "Part 1: $HASH1"
echo "Part 2: $HASH2"
echo "Part 3: $HASH3"
echo ""

# Compute composite hash for ModelRegistry
COMPOSITE=$(echo -n "${HASH1}${HASH2}${HASH3}" | sha256sum | cut -d' ' -f1)

echo "========================================="
echo "COMPOSITE HASH FOR MODELREGISTRY"
echo "========================================="
echo ""
echo "$COMPOSITE"
echo ""
echo "This composite hash should be registered in the ModelRegistry contract."
echo "It provides cryptographic proof of integrity for all 3 parts."
echo ""

# Save composite hash to file
echo "$COMPOSITE" > gpt-oss-120b-composite-hash.txt
echo "✅ Composite hash saved to: gpt-oss-120b-composite-hash.txt"

# Calculate model ID for contract
MODEL_ID=$(echo -n "ggml-org/gpt-oss-120b-GGUF/gpt-oss-120b-mxfp4-00001-of-00003.gguf" | sha256sum | cut -d' ' -f1)
echo ""
echo "Model ID (for contract queries): 0x${MODEL_ID}"
echo ""

echo "========================================="
echo "NEXT STEPS"
echo "========================================="
echo ""
echo "1. Update .env.prod with:"
echo "   MODEL_PATH=/models/gpt-oss-120b-GGUF/gpt-oss-120b-mxfp4-00001-of-00003.gguf"
echo "   MAX_CONTEXT_LENGTH=131072"
echo "   GPU_LAYERS=99"
echo ""
echo "2. Update deployment scripts (restart-and-deploy-openai.sh)"
echo ""
echo "3. Register model in ModelRegistry (requires contract owner):"
echo "   See plan for registration commands"
echo ""
echo "4. Restart node with new configuration"
echo ""
echo "✅ Download and verification complete!"
