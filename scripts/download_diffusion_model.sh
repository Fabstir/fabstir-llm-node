#!/bin/bash
# Download FLUX.2 Klein 4B model for SGLang Diffusion sidecar
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
set -e

MODEL_DIR="${1:-./models/flux2-klein-4b}"
REPO="black-forest-labs/FLUX.2-klein-4B"
HF_BASE="https://huggingface.co/${REPO}/resolve/main"

echo "FLUX.2 Klein 4B Model Downloader"
echo "Target: ${MODEL_DIR}"
echo "Source: ${REPO}"

mkdir -p "${MODEL_DIR}"

if command -v huggingface-cli > /dev/null 2>&1; then
    echo "Using huggingface-cli for download..."
    huggingface-cli download "${REPO}" --local-dir "${MODEL_DIR}"
else
    echo "huggingface-cli not found. Install it with:"
    echo "  pip install huggingface_hub"
    echo ""
    echo "Downloading config files with curl as fallback..."
    curl -L -o "${MODEL_DIR}/config.json" "${HF_BASE}/config.json" --progress-bar
    curl -L -o "${MODEL_DIR}/model_index.json" "${HF_BASE}/model_index.json" --progress-bar
    echo ""
    echo "For full model weights, run:"
    echo "  pip install huggingface_hub"
    echo "  huggingface-cli download ${REPO} --local-dir ${MODEL_DIR}"
fi

echo "FLUX.2-Klein-4B" > "${MODEL_DIR}/VERSION"
echo "Done. Model directory: ${MODEL_DIR}"
ls -lah "${MODEL_DIR}"
